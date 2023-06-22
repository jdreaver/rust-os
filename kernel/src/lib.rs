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
pub(crate) mod debug;
pub(crate) mod elf;
pub(crate) mod fs;
pub(crate) mod gdt;
pub(crate) mod graphics;
pub(crate) mod hpet;
pub(crate) mod interrupts;
pub(crate) mod ioapic;
pub(crate) mod keyboard;
pub(crate) mod logging;
pub(crate) mod memory;
pub(crate) mod pci;
pub(crate) mod percpu;
pub(crate) mod qemu;
#[allow(dead_code)] // This could be its own crate
pub(crate) mod registers;
pub(crate) mod sched;
pub(crate) mod serial;
pub(crate) mod shell;
pub(crate) mod strings;
pub(crate) mod sync;
#[cfg(feature = "tests")]
pub(crate) mod tests;
pub(crate) mod tick;
pub(crate) mod transmute;
pub(crate) mod vfs;
pub(crate) mod virtio;

use core::sync::atomic::{AtomicU8, Ordering};

use alloc::string::String;
use apic::ProcessorID;

pub fn start() -> ! {
    serial::init();
    logging::init();

    let boot_info_data = boot_info::boot_info();
    early_per_cpu_setup(
        true,
        ProcessorID(boot_info_data.bootstrap_processor_lapic_id as u8),
    );

    log::info!("kernel cmdline: {}", boot_info_data.kernel_cmdline);
    global_setup(boot_info_data);

    // Finish bootstrapping current CPU
    later_per_cpu_setup();

    // Bootstrap other CPUs
    for mut entry in boot_info::limine_smp_entries() {
        entry.bootstrap_cpu(bootstrap_secondary_cpu);
    }

    // Ensure that all CPUs have finished bootstrapping before continuing
    let cpu_count = boot_info::limine_smp_entries().count() as u8;
    while NUM_CPUS_BOOTSTRAPPED.load(Ordering::Acquire) < cpu_count {
        core::hint::spin_loop();
    }

    tick::global_init();

    // TEST CODE time a bit allocation

    use crate::memory::{allocate_and_map_pages, Page, PageRange, PageSize, PageTableEntryFlags};
    use x86_64::VirtAddr;
    let alloc_size = 100 * 1024 * 1024;

    log::warn!("test start");
    let start_ms = hpet::elapsed_milliseconds();


    let alloc_start_addr = VirtAddr::new(0xffff_a000_0000_0000);
    let alloc_start = Page::containing_address(alloc_start_addr, PageSize::Size4KiB);
    let page_range = PageRange::from_bytes_inclusive(alloc_start, alloc_size);
    let flags = PageTableEntryFlags::PRESENT | PageTableEntryFlags::WRITABLE;
    allocate_and_map_pages(page_range.iter(), flags).expect("allocation failed");

    let end_ms = hpet::elapsed_milliseconds();
    let total_ms = u64::from(end_ms) - u64::from(start_ms);

    log::warn!("took {total_ms}ms to allocate {alloc_size} bytes");

    log::warn!("test x86_64 start");
    let start_ms = hpet::elapsed_milliseconds();

    let alloc_start_addr = VirtAddr::new(0xffff_b000_0000_0000);
    let page_range = PageRange::from_bytes_inclusive(alloc_start, alloc_size);
    let page_range = x86_64::structures::paging::Page::range_inclusive(
        x86_64::structures::paging::Page::containing_address(alloc_start_addr),
        x86_64::structures::paging::Page::containing_address(alloc_start_addr + (alloc_size - 1)),
    );
    use x86_64::structures::paging::PageTableFlags;
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    memory::x86_allocate_and_map_pages(page_range, flags).expect("allocation failed");

    let end_ms = hpet::elapsed_milliseconds();
    let total_ms = u64::from(end_ms) - u64::from(start_ms);

    log::warn!("x86_64 took {total_ms}ms to allocate {alloc_size} bytes");

    // END TEST CODE

    sched::start_multitasking(
        String::from("shell"),
        shell::run_serial_shell,
        core::ptr::null::<()>(),
    );

    panic!("ERROR: ended multi-tasking");
}

/// Records how many CPUs have been bootstrapped. Used as a synchronization
/// point before continuing with init.
static NUM_CPUS_BOOTSTRAPPED: AtomicU8 = AtomicU8::new(0);

fn early_per_cpu_setup(bootstrap_cpu: bool, processor_id: ProcessorID) {
    if bootstrap_cpu {
        gdt::init_bootstrap_gdt();
    } else {
        gdt::init_secondary_cpu_gdt();
    }
    interrupts::init_interrupts();
    percpu::init_current_cpu(processor_id);
    tick::per_cpu_init();
}

fn global_setup(boot_info_data: &boot_info::BootInfo) {
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
        memory::init(boot_info_data, make_memory_map);
    };

    // N.B. Probing ACPI must happen after heap initialization because the Rust
    // `acpi` crate uses alloc. It would be nice to not need that...
    unsafe { acpi::init(boot_info_data.rsdp_address.expect("no rsdp address")) };

    let acpi_info = acpi::acpi_info();
    apic::global_init(acpi_info);
    ioapic::init(acpi_info);
    sched::global_init();

    unsafe {
        hpet::init(acpi_info.hpet_address());
    };

    keyboard::init_keyboard();

    // Initialize VirtIO devices
    let pci_config_region_base_address = acpi_info.pci_config_region_base_address();
    pci::for_pci_devices_brute_force(pci_config_region_base_address, |device| {
        let Some(device_config) = virtio::VirtIODeviceConfig::from_pci_config(device) else { return; };
        virtio::try_init_virtio_rng(device_config);
        virtio::try_init_virtio_block(device_config);
    });

    graphics::init(boot_info_data);
}

fn later_per_cpu_setup() {
    apic::per_cpu_init();
    sched::per_cpu_init();
    NUM_CPUS_BOOTSTRAPPED.fetch_add(1, Ordering::Release);
}

extern "C" fn bootstrap_secondary_cpu(info: *const limine::LimineSmpInfo) -> ! {
    let info = unsafe { &*info };
    let processor_id = ProcessorID(info.lapic_id as u8);
    // log::info!("bootstrapping CPU: {info:#x?}");
    early_per_cpu_setup(false, processor_id);
    later_per_cpu_setup();
    loop {
        x86_64::instructions::hlt();
    }
}

pub fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    logging::force_unlock_logger();
    log::error!("PANIC: {info}");
    debug::print_stack_trace();
    hlt_loop()
}

pub(crate) fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
