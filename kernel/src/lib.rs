#![no_std]
#![feature(abi_x86_interrupt)]
#![feature(allocator_api)]
#![feature(asm_const)]
#![feature(cell_leak)]
#![feature(int_roundings)]
#![feature(offset_of)]
#![feature(naked_functions)]
#![feature(pointer_is_aligned)]
#![feature(strict_provenance)]
#![feature(sync_unsafe_cell)]
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
    clippy::non_send_fields_in_send_ty,
    clippy::redundant_pub_crate,
    clippy::suboptimal_flops,
    clippy::upper_case_acronyms,
    clippy::wildcard_imports
)]

#[macro_use] // For format! macro
#[allow(unused_imports)] // format! macro is unused at the time of writing
extern crate alloc;

pub(crate) mod acpi;
pub(crate) mod ansiterm;
pub(crate) mod apic;
pub(crate) mod barrier;
pub(crate) mod block;
pub(crate) mod boot_info;
pub(crate) mod fs;
pub(crate) mod gdt;
pub(crate) mod heap;
pub(crate) mod hpet;
pub(crate) mod interrupts;
pub(crate) mod ioapic;
pub(crate) mod keyboard;
pub(crate) mod logging;
pub(crate) mod memory;
pub(crate) mod pci;
pub(crate) mod percpu;
#[allow(dead_code)] // This could be its own crate
pub(crate) mod registers;
pub(crate) mod sched;
pub(crate) mod serial;
pub(crate) mod shell;
pub(crate) mod strings;
pub(crate) mod sync;
pub(crate) mod tests;
pub(crate) mod tick;
pub(crate) mod vfs;
pub(crate) mod virtio;

pub fn start() -> ! {
    logging::init();

    early_per_cpu_setup(true);
    global_setup();

    // Finish bootstrapping current CPU
    later_per_cpu_setup();

    // Bootstrap other CPUs
    for mut entry in boot_info::limine_smp_entries() {
        entry.bootstrap_cpu(bootstrap_secondary_cpu);
    }

    sched::start_multitasking("shell", shell::run_serial_shell, core::ptr::null::<()>());

    panic!("ERROR: ended multi-tasking");
}

fn early_per_cpu_setup(bootstrap_cpu: bool) {
    if bootstrap_cpu {
        gdt::init_bootstrap_gdt();
    } else {
        gdt::init_secondary_cpu_gdt();
    }
    interrupts::init_interrupts();
}

fn global_setup() {
    let boot_info_data = boot_info::boot_info();

    // KLUDGE: Limine just doesn't report on memory below 0x1000, so we
    // explicitly mark it as reserved. TODO: Perhaps instead of only reserving
    // reserved regions, we should assume all memory is reserved and instead
    // explicitly free the regions limine says are free.
    let make_memory_map = || {
        core::iter::once(bitmap_alloc::MemoryRegion {
            start_address: 0,
            len_bytes: 0x1000,
            free: false,
        })
        .chain(boot_info::limine_memory_regions())
    };

    unsafe {
        memory::init(
            boot_info_data.higher_half_direct_map_offset,
            make_memory_map,
        );
    };
    heap::init().expect("failed to initialize heap");

    // N.B. Probing ACPI must happen after heap initialization because the Rust
    // `acpi` crate uses alloc. It would be nice to not need that...
    unsafe { acpi::init(boot_info_data.rsdp_physical_addr()) };

    let acpi_info = acpi::acpi_info();
    apic::init_local_apic(acpi_info);
    ioapic::init(acpi_info);
    sched::global_init();

    unsafe {
        hpet::init(acpi_info.hpet_info().base_address);
    };

    tick::init();
    keyboard::init_keyboard();

    // Initialize VirtIO devices
    let pci_config_region_base_address = acpi_info.pci_config_region_base_address();
    pci::for_pci_devices_brute_force(pci_config_region_base_address, |device| {
        let Some(device_config) = virtio::VirtIODeviceConfig::from_pci_config(device) else { return; };
        virtio::try_init_virtio_rng(device_config);
        virtio::try_init_virtio_block(device_config);
    });
}

fn later_per_cpu_setup() {
    percpu::init_current_cpu();
    sched::per_cpu_init();
}

extern "C" fn bootstrap_secondary_cpu(_info: *const limine::LimineSmpInfo) -> ! {
    // let info = unsafe { &*info };
    // log::info!("bootstrapping CPU: {info:#x?}");
    early_per_cpu_setup(false);
    later_per_cpu_setup();
    loop {
        x86_64::instructions::hlt();
    }
}

pub fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    logging::force_unlock_logger();
    log::error!("PANIC: {info}");
    hlt_loop()
}

pub(crate) fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
