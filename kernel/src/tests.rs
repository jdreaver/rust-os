use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt::Write;

use uefi::table::{Runtime, SystemTable};
use vesa_framebuffer::{TextBuffer, VESAFramebuffer32Bit};
use x86_64::structures::paging::{FrameAllocator, OffsetPageTable, Size2MiB, Size4KiB, Translate};

use crate::{acpi, apic, boot_info, memory, pci, serial_println, virtio};

pub(crate) fn run_tests(
    boot_info_data: &boot_info::BootInfo,
    acpi_info: &acpi::ACPIInfo,
    mapper: &mut OffsetPageTable,
    frame_allocator: &mut memory::LockedNaiveFreeMemoryBlockAllocator,
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

    let apic_info = acpi_info.apic_info();
    serial_println!("ACPI APIC: {:#x?}", apic_info);

    let local_apic_reg =
        unsafe { apic::LocalAPICRegisters::from_address(apic_info.local_apic_address as usize) };
    serial_println!("Local APIC Registers: {:#x?}", local_apic_reg);

    // Iterate over PCI devices
    pci::for_pci_devices_brute_force(pci_config_region_base_address, |device| {
        serial_println!("Found PCI device: {:#x?}", device);
    });

    // Find VirtIO devices
    pci::for_pci_devices_brute_force(pci_config_region_base_address, |device| {
        let Some(device_config) = virtio::VirtIODeviceConfig::from_pci_config(device, mapper, frame_allocator) else { return; };
        serial_println!("Found VirtIO device, initializing");

        let initialized_device =
            virtio::VirtIOInitializedDevice::new(device_config, frame_allocator);
        serial_println!("VirtIO device initialized: {:#x?}", initialized_device);

        // Test out the RNG device
        test_rng_virtio_device(initialized_device, frame_allocator);
    });

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
        let phys = mapper.translate_addr(virt);
        serial_println!("{:?} -> {:?}", virt, phys);
    }

    serial_println!(
        "next 4KiB page: {:?}",
        FrameAllocator::<Size4KiB>::allocate_frame(frame_allocator)
    );
    serial_println!(
        "next 2MiB page: {:?}",
        FrameAllocator::<Size2MiB>::allocate_frame(frame_allocator)
    );
    serial_println!(
        "next 4KiB page: {:?}",
        FrameAllocator::<Size4KiB>::allocate_frame(frame_allocator)
    );
    serial_println!(
        "next 2MiB page: {:?}",
        FrameAllocator::<Size2MiB>::allocate_frame(frame_allocator)
    );

    for _ in 0..10000 {
        FrameAllocator::<Size4KiB>::allocate_frame(frame_allocator);
    }

    serial_println!(
        "far page: {:?}",
        FrameAllocator::<Size4KiB>::allocate_frame(frame_allocator)
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

fn test_rng_virtio_device(
    mut device: virtio::VirtIOInitializedDevice,
    frame_allocator: &mut memory::LockedNaiveFreeMemoryBlockAllocator,
) {
    let device_id = device.config().pci_config().device_id();
    if device_id.known_vendor_id() == "virtio" && device_id.known_device_id() == "entropy source" {
        // RNG device only has a single virtq
        let queue_index = 0;
        let buffer_size = 16;

        let buf = memory::allocate_zeroed_buffer(frame_allocator, buffer_size, 16)
            .expect("failed to allocate buffer for entropy virtq");
        let virtq = device.get_virtqueue_mut(queue_index).unwrap();
        let flags = virtio::VirtqDescriptorFlags::new().with_device_write(true);
        virtq.add_buffer(buf, buffer_size as u32, flags);

        serial_println!("Added buffer to virtq: {:#x?}", virtq);
        serial_println!("Waiting for response...");

        // Dummy loop to waste time to wait for response, but apparently it
        // isn't needed? Neat.
        //
        // let mut i = 0;
        // while virtq.used_ring_index() == 0 {
        //     i += 1;
        //     if i > 1_000 {
        //         panic!("timed out")
        //     }
        // }

        let used_index = virtq.used_ring_index() - 1;
        let (used_entry, descriptor) = virtq.get_used_ring_entry(used_index);
        serial_println!("Got used entry: {:#x?}", (used_entry, descriptor));

        // The used entry should be using the exact same buffer we just
        // created, but let's pretend we didn't know that.
        let buffer = unsafe {
            core::slice::from_raw_parts(
                descriptor.addr as *const u8,
                // NOTE: Using the length from the used entry, not the buffer
                // length, b/c the RNG device might not have written the whole
                // thing!
                used_entry.len as usize,
            )
        };
        serial_println!("RNG buffer: {:x?}", buffer);
    }
}
