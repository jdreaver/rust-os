use core::ptr::NonNull;

use acpi::mcfg::{Mcfg, McfgEntry};
use acpi::{AcpiHandler, AcpiTable, AcpiTables, PhysicalMapping};
use x86_64::PhysAddr;

use crate::{pci, serial, serial_println};

/// We need to implement `acpi::AcpiHandler` to use the `acpi` crate. This is
/// needed so we can map physical regions of memory to virtual regions. Luckily,
/// we do identity mapping for the first ~4 GiB of memory thanks to limine, so
/// this is easy; we don't actually have to modify page tables.
#[derive(Debug, Clone)]
pub struct IdentityMapAcpiHandler;

impl AcpiHandler for IdentityMapAcpiHandler {
    /// # Safety
    ///
    /// Caller must ensure page tables are set up for identity mapping for any
    /// memory that could be used to access ACPI tables.
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        let physical_start = physical_address;
        let virtual_start = NonNull::new_unchecked(physical_address as *mut T);
        let region_length = size;
        let mapped_length = size;
        let handler = self.clone();
        PhysicalMapping::new(
            physical_start,
            virtual_start,
            region_length,
            mapped_length,
            handler,
        )
    }

    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {
        // Nothing to do here.
    }
}

fn acpi_tables_from_rsdp(rsdp_addr: PhysAddr) -> acpi::AcpiTables<IdentityMapAcpiHandler> {
    let handler = IdentityMapAcpiHandler;
    let rsdp_addr = rsdp_addr.as_u64() as usize;
    unsafe {
        AcpiTables::from_rsdp(handler, rsdp_addr).expect("failed to load ACPI tables from RSDP")
    }
}

pub fn print_acpi_info(rsdp_addr: PhysAddr) {
    let acpi_tables = acpi_tables_from_rsdp(rsdp_addr);
    let platform_info = acpi_tables
        .platform_info()
        .expect("failed to get platform info");
    if let Some(processor_info) = platform_info.processor_info {
        serial_println!("ACPI processor info: {:?}", processor_info.boot_processor);
        for (i, processor) in processor_info.application_processors.iter().enumerate() {
            serial_println!("  ACPI application processor {}: {:?}", i, processor);
        }
    }
    serial_println!("ACPI power profile: {:?}", platform_info.power_profile);
    serial_println!(
        "ACPI interrupt model:\n{:#x?}",
        platform_info.interrupt_model
    );

    serial_println!("ACPI SDTs:");
    for (signature, sdt) in &acpi_tables.sdts {
        serial_println!(
            "  ACPI SDT: signature: {}, address: {:x}, length: {:x}, validated: {}",
            signature,
            sdt.physical_address,
            sdt.length,
            sdt.validated
        );
    }

    serial_println!("ACPI DSDT: {:#x?}", acpi_tables.dsdt);
    serial_println!("ACPI SSDTs: {:#x?}", acpi_tables.ssdts);

    let pci_config_regions =
        acpi::mcfg::PciConfigRegions::new(&acpi_tables).expect("failed to get PCI config regions");

    // For some reason, pci_config_regions.regions is a private field, so we
    // have to just probe it.
    let pci_config_region_base_address = pci_config_regions
        .physical_address(0, 0, 0, 0)
        .expect("couldn't get PCI config address");
    serial_println!(
        "PCI config region base address: {:#x?}",
        pci_config_region_base_address
    );

    // This is another way of getting pci_config_region_base_address, kinda. I
    // don't know why this isn't exposed from the acpi crate.
    let mcfg = unsafe {
        acpi_tables
            .get_sdt::<Mcfg>(acpi::sdt::Signature::MCFG)
            .expect("failed to get MCFG table")
            .expect("MCFG table is not present")
    };
    let mcfg_entries = mcfg_entries(&mcfg);
    serial_println!("MCFG entries:");
    for (i, entry) in mcfg_entries.iter().enumerate() {
        serial_println!("  MCFG entry {}: {:#x?}", i, entry);
    }

    // Iterate over PCI devices
    pci::for_pci_devices_brute_force(pci_config_region_base_address, |device| {
        device
            .print(serial::serial1_writer())
            .expect("failed to print PCI device");
    });
}

/// For some reason this code is private in the acpi crate. See
/// <https://docs.rs/acpi/4.1.1/src/acpi/mcfg.rs.html#61-74>.
fn mcfg_entries<H: AcpiHandler>(mcfg: &PhysicalMapping<H, Mcfg>) -> &[McfgEntry] {
    let length = mcfg.header().length as usize - core::mem::size_of::<Mcfg>();

    // Intentionally round down in case length isn't an exact multiple of McfgEntry size
    // (see rust-osdev/acpi#58)
    let num_entries = length / core::mem::size_of::<McfgEntry>();

    let start_ptr = mcfg.virtual_start().as_ptr() as *const u8;
    unsafe {
        let pointer = start_ptr
            .add(core::mem::size_of::<Mcfg>())
            .cast::<acpi::mcfg::McfgEntry>();
        core::slice::from_raw_parts(pointer, num_entries)
    }
}
