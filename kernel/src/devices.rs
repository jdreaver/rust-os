use core::ptr::NonNull;

use acpi::{AcpiHandler, AcpiTables, PciConfigRegions, PhysicalMapping};
use x86_64::PhysAddr;

use crate::serial_println;

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

    let pci_config_regions =
        PciConfigRegions::new(&acpi_tables).expect("failed to get PCI config regions");
    serial_println!("ACPI PCI config regions: {:#x?}", pci_config_regions);
}
