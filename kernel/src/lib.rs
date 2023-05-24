#![no_std]
#![feature(abi_x86_interrupt)]
#![feature(allocator_api)]
#![feature(naked_functions)]
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
pub(crate) mod hpet;
pub(crate) mod interrupts;
pub(crate) mod ioapic;
pub(crate) mod keyboard;
pub(crate) mod memory;
pub(crate) mod pci;
#[allow(dead_code)] // This could be its own crate
pub(crate) mod registers;
pub(crate) mod scheduler;
pub(crate) mod serial;
pub(crate) mod shell;
pub(crate) mod strings;
pub(crate) mod tests;
pub(crate) mod virtio;

pub fn start() -> ! {
    gdt::init();
    interrupts::init_interrupts();

    let boot_info_data = boot_info::boot_info();
    let limine_usable_memory = boot_info::limine_usable_memory_regions();
    unsafe {
        memory::init(
            boot_info_data.higher_half_direct_map_offset,
            limine_usable_memory,
        );
    };
    heap::init().expect("failed to initialize heap");

    // N.B. Probing ACPI must happen after heap initialization because the Rust
    // `acpi` crate uses alloc. It would be nice to not need that...
    unsafe { acpi::init(boot_info_data.rsdp_physical_addr()) };

    let acpi_info = acpi::acpi_info();
    apic::init_local_apic(acpi_info);
    ioapic::init(acpi_info);

    unsafe {
        hpet::init(acpi_info.hpet_info().base_address);
    };

    keyboard::init_keyboard();

    // Try to init VirtIO RNG device, if it exists
    let pci_config_region_base_address = acpi_info.pci_config_region_base_address();
    pci::for_pci_devices_brute_force(pci_config_region_base_address, |device| {
        let Some(device_config) = virtio::VirtIODeviceConfig::from_pci_config(device) else { return; };
        virtio::try_init_virtio_rng(device_config);
    });

    shell::run_serial_shell();
}

pub fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    serial_println!("PANIC: {info}");
    hlt_loop()
}

pub(crate) fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
