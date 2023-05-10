#![no_std]
#![no_main]
#![feature(allocator_api)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use uefi::table::{Runtime, SystemTable};
use vesa_framebuffer::{TextBuffer, VESAFramebuffer32Bit};
use x86_64::structures::paging::{FrameAllocator, OffsetPageTable};

use rust_os::{
    acpi, boot_info, gdt, heap, interrupts, memory, pci, serial, serial_println, virtio,
};

static mut TEXT_BUFFER: TextBuffer = TextBuffer::new();

#[no_mangle]
extern "C" fn _start() -> ! {
    serial::init_serial_writer();

    boot_info::init_boot_info();
    let boot_info_data = boot_info::boot_info();

    gdt::init();
    interrupts::init_idt();

    let mut mapper = unsafe { memory::init(boot_info_data.higher_half_direct_map_offset) };
    let frame_allocator = boot_info::allocator_from_limine_memory_map();
    let mut frame_allocator = memory::LockedNaiveFreeMemoryBlockAllocator::new(frame_allocator);
    heap::init(&mut mapper, &mut frame_allocator).expect("failed to initialize allocator");

    run_tests(boot_info_data, &mut mapper, &mut frame_allocator);

    hlt_loop()
}

#[panic_handler]
fn rust_panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("PANIC: {}", info);
    hlt_loop()
}

fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

fn run_tests(
    boot_info_data: &boot_info::BootInfo,
    mapper: &mut OffsetPageTable,
    frame_allocator: &mut memory::LockedNaiveFreeMemoryBlockAllocator,
) {
    serial_println!("limine boot info:\n{:#x?}", boot_info_data);
    boot_info::print_limine_memory_map();

    if let Some(system_table_addr) = boot_info_data.efi_system_table_address {
        unsafe {
            let system_table = SystemTable::<Runtime>::from_ptr(system_table_addr.as_mut_ptr())
                .expect("failed to create EFI system table");
            serial_println!(
                "EFI runtime services:\n{:#?}",
                system_table.runtime_services()
            );

            for entry in system_table.config_table() {
                if entry.guid == uefi::table::cfg::ACPI2_GUID {
                    // This should match the limine RSDP address
                    serial_println!("EFI config table ACPI2 entry: {:#X?}", entry);
                }
            }
        };
    }

    // Ensure we got a framebuffer.
    let mut framebuffer = unsafe {
        VESAFramebuffer32Bit::from_limine_framebuffer(boot_info_data.framebuffer)
            .expect("failed to create VESAFramebuffer32Bit")
    };
    serial_println!("framebuffer: {:#?}", framebuffer);

    unsafe {
        use core::fmt::Write;

        writeln!(TEXT_BUFFER, "Hello!").expect("failed to write to text buffer");
        writeln!(TEXT_BUFFER, "World!").expect("failed to write to text buffer");

        TEXT_BUFFER.flush(&mut framebuffer);
    };

    let rsdp_physical_addr = boot_info_data
        .rsdp_address
        .map(|addr| {
            x86_64::PhysAddr::new(
                addr.as_u64() - boot_info_data.higher_half_direct_map_offset.as_u64(),
            )
        })
        .expect("failed to get RSDP physical address");
    serial_println!("RSDP physical address: {:?}", rsdp_physical_addr);

    let acpi_info = unsafe { acpi::ACPIInfo::from_rsdp(rsdp_physical_addr) };
    acpi::print_acpi_info(&acpi_info);
    let pci_config_region_base_address = acpi_info.pci_config_region_base_address();

    // Iterate over PCI devices
    pci::for_pci_devices_brute_force(pci_config_region_base_address, |device| {
        serial_println!("Found PCI device: {:#x?}", device);
    });

    // Find VirtIO devices
    pci::for_pci_devices_brute_force(pci_config_region_base_address, |device| {
        let Some(virtio_device) = virtio::VirtIODeviceConfig::from_pci_config(device, mapper, frame_allocator) else { return; };
        serial_println!("Found VirtIO device: {:#x?}", virtio_device);

        virtio_device.initialize(frame_allocator);
        serial_println!(
            "VirtIO device initialized: {:#x?}",
            virtio_device.common_virtio_config()
        );
    });

    // Print out some test addresses
    let addresses = [
        // the identity-mapped vga buffer page
        0xb8000,
        0xb8000 + boot_info_data.higher_half_direct_map_offset.as_u64(),
        // some code page
        0x201008,
        // some stack page
        0x0100_0020_1a10,
        // virtual address mapped to physical address 0
        boot_info_data.higher_half_direct_map_offset.as_u64(),
    ];

    use x86_64::structures::paging::Translate;

    for &address in &addresses {
        let virt = x86_64::VirtAddr::new(address);
        let phys = mapper.translate_addr(virt);
        serial_println!("{:?} -> {:?}", virt, phys);
    }

    use x86_64::structures::paging::{Size2MiB, Size4KiB};

    let alloc_4kib =
        <memory::LockedNaiveFreeMemoryBlockAllocator as FrameAllocator<Size4KiB>>::allocate_frame;
    let alloc_2mib =
        <memory::LockedNaiveFreeMemoryBlockAllocator as FrameAllocator<Size2MiB>>::allocate_frame;

    serial_println!("next 4KiB page: {:?}", alloc_4kib(frame_allocator));
    serial_println!("next 2MiB page: {:?}", alloc_2mib(frame_allocator));
    serial_println!("next 4KiB page: {:?}", alloc_4kib(frame_allocator));
    serial_println!("next 2MiB page: {:?}", alloc_2mib(frame_allocator));

    for _ in 0..10000 {
        alloc_4kib(frame_allocator);
    }

    serial_println!("far page: {:?}", alloc_4kib(frame_allocator));

    // Invoke a breakpoint exception and ensure we continue on
    serial_println!("interrupt");
    x86_64::instructions::interrupts::int3();

    serial_println!("done with interrupt");

    // Allocate a number on the heap
    let heap_value = Box::new(41);
    serial_println!("heap_value at {:p}", heap_value);
    assert_eq!(*heap_value, 41);

    // create a dynamically sized vector
    let mut vec = Vec::new();
    for i in 0..10 {
        vec.push(i);
    }
    serial_println!("vec at {:p}: {:?}", vec.as_slice(), vec);
    assert_eq!(vec.into_iter().sum::<u32>(), 45);

    // Create a Box value with the `Allocator` API
    let my_box = Box::new_in(42, &*frame_allocator);
    serial_println!("Allocator alloc'ed my_box {:?} at {:p}", my_box, my_box);

    // Trigger a page fault, which should trigger a double fault if we don't
    // have a page fault handler.
    // unsafe {
    //     // N.B. Rust panics if we try to use 0xdeadbeef as a pointer (address
    //     // must be a multiple of 0x8), so we use 0xdeadbee0 instead
    //     *(0xdeadbee0 as *mut u64) = 42;
    // };

    serial_println!("Tests passed!");

    // Test custom panic handler
    // panic!("Some panic message");
}
