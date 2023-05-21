use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt::Write;

use uefi::table::{Runtime, SystemTable};
use vesa_framebuffer::{TextBuffer, VESAFramebuffer32Bit};
use x86_64::structures::paging::{Size2MiB, Size4KiB};

use crate::{acpi, boot_info, memory, pci, scheduler, serial_println, virtio};

pub(crate) fn run_tests(
    boot_info_data: &boot_info::BootInfo,
    acpi_info: &acpi::ACPIInfo,
    text_buffer: &'static mut TextBuffer,
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

    writeln!(text_buffer, "Hello!").expect("failed to write to text buffer");
    writeln!(text_buffer, "World!").expect("failed to write to text buffer");

    text_buffer.flush(&mut framebuffer);

    acpi::print_acpi_info(acpi_info);
    let pci_config_region_base_address = acpi_info.pci_config_region_base_address();

    // Iterate over PCI devices
    pci::for_pci_devices_brute_force(pci_config_region_base_address, |device| {
        serial_println!("Found PCI device: {:#x?}", device);
    });

    // Find VirtIO devices
    pci::for_pci_devices_brute_force(pci_config_region_base_address, |device| {
        let Some(device_config) = virtio::VirtIODeviceConfig::from_pci_config(device) else { return; };
        virtio::try_init_virtio_rng(device_config);
    });

    // Request some VirtIO RNG bytes
    virtio::request_random_numbers();
    virtio::request_random_numbers();

    // Print out some test addresses
    let addresses = [
        // the identity-mapped vga buffer page
        0xb8000,
        0xb8000 + boot_info_data.higher_half_direct_map_offset.as_u64(),
        // some code page
        0x0020_1008,
        // some stack page
        0x0100_0020_1a10,
        // virtual address mapped to physical address 0
        boot_info_data.higher_half_direct_map_offset.as_u64(),
    ];

    for &address in &addresses {
        let virt = x86_64::VirtAddr::new(address);
        let phys = memory::translate_addr(virt);
        serial_println!("{:?} -> {:?}", virt, phys);
    }

    serial_println!(
        "next 4KiB page: {:?}",
        memory::allocate_physical_frame::<Size4KiB>()
    );
    serial_println!(
        "next 2MiB page: {:?}",
        memory::allocate_physical_frame::<Size2MiB>()
    );
    serial_println!(
        "next 4KiB page: {:?}",
        memory::allocate_physical_frame::<Size4KiB>()
    );
    serial_println!(
        "next 2MiB page: {:?}",
        memory::allocate_physical_frame::<Size2MiB>()
    );

    for _ in 0..10000 {
        memory::allocate_physical_frame::<Size4KiB>();
    }

    serial_println!(
        "far page: {:?}",
        memory::allocate_physical_frame::<Size4KiB>()
    );

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
    let my_box = Box::new_in(42, &memory::KERNEL_PHYSICAL_ALLOCATOR);
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

    scheduler::push_task("task 1", task_1_test_task);
    scheduler::push_task("task 2", task_2_test_task);

    scheduler::start_multitasking();
}

fn task_1_test_task() {
    loop {
        serial_println!("task 1 is running!");
        scheduler::run_scheduler();
    }
}

fn task_2_test_task() {
    loop {
        serial_println!("task 2 is running!");
        scheduler::run_scheduler();
    }
}
