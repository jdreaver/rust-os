#![no_std]
#![feature(abi_x86_interrupt)]
#![feature(allocator_api)]
#![feature(strict_provenance)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cargo_common_metadata,
    clippy::doc_markdown,
    clippy::implicit_hasher,
    clippy::implicit_return,
    clippy::len_without_is_empty,
    clippy::missing_const_for_fn,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::multiple_crate_versions,
    clippy::must_use_candidate,
    clippy::new_without_default,
    clippy::redundant_pub_crate,
    clippy::suboptimal_flops,
    clippy::upper_case_acronyms,
    clippy::wildcard_imports
)]

extern crate alloc;

pub(crate) mod acpi;
pub(crate) mod apic;
pub(crate) mod boot_info;
pub(crate) mod gdt;
pub(crate) mod heap;
pub(crate) mod interrupts;
pub(crate) mod memory;
pub(crate) mod pci;
#[allow(dead_code)] // This could be its own crate
pub(crate) mod registers;
pub(crate) mod serial;
pub(crate) mod strings;
pub(crate) mod tests;
pub(crate) mod virtio;

use vesa_framebuffer::TextBuffer;

static mut TEXT_BUFFER: TextBuffer = TextBuffer::new();

pub fn start() -> ! {
    serial::init_serial_writer();

    boot_info::init_boot_info();
    let boot_info_data = boot_info::boot_info();

    gdt::init();
    interrupts::init_idt();

    let mut mapper = unsafe { memory::init(boot_info_data.higher_half_direct_map_offset) };
    let frame_allocator = boot_info::allocator_from_limine_memory_map();
    let mut frame_allocator = memory::LockedNaiveFreeMemoryBlockAllocator::new(frame_allocator);
    heap::init(&mut mapper, &mut frame_allocator).expect("failed to initialize allocator");

    // N.B. Probing ACPI must happen after heap initialization because the Rust
    // `acpi` crate uses alloc. It would be nice to not need that...
    let acpi_info = unsafe { acpi::ACPIInfo::from_rsdp(boot_info_data.rsdp_physical_addr()) };

    let mut local_apic = apic::LocalAPIC::from_acpi_info(&acpi_info);
    local_apic.enable();
    serial_println!("Local APIC: {:#x?}", local_apic);

    // TODO: Initialize TEXT_BUFFER better so we don't need unsafe.
    let text_buffer = unsafe { &mut TEXT_BUFFER };
    tests::run_tests(
        boot_info_data,
        &acpi_info,
        &mut mapper,
        &mut frame_allocator,
        text_buffer,
    );

    hlt_loop()
}

pub fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    serial_println!("PANIC: {}", info);
    hlt_loop()
}

pub(crate) fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
