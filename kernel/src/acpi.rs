use core::cell::SyncUnsafeCell;
use core::ptr::NonNull;

use acpi::mcfg::{Mcfg, McfgEntry};
use acpi::platform::interrupt::Apic;
use acpi::{AcpiHandler, AcpiTable, AcpiTables, HpetInfo, PhysicalMapping};
use x86_64::PhysAddr;

use crate::serial_println;

static ACPI_INFO: SyncUnsafeCell<Option<ACPIInfo>> = SyncUnsafeCell::new(None);

pub(crate) unsafe fn init(rsdp_addr: PhysAddr) {
    ACPI_INFO
        .get()
        .replace(Some(ACPIInfo::from_rsdp(rsdp_addr)));
}

pub(crate) fn acpi_info() -> &'static ACPIInfo {
    unsafe {
        ACPI_INFO
            .get()
            .as_ref()
            .expect("failed to convert ACPI_INFO to reference")
            .as_ref()
            .expect("ACPI_INFO not initialized")
    }
}

/// Holds important Advanced Configuration and Power Interface (ACPI)
/// information needed to start the kernel.
pub(crate) struct ACPIInfo {
    tables: AcpiTables<IdentityMapAcpiHandler>,
}

impl ACPIInfo {
    /// # Safety
    ///
    /// Caller must ensure RSDP address is valid, and that page tables are set
    /// up for identity mapping for any memory that could be used to access ACPI
    /// tables (e.g. identity mapping physical memory).
    unsafe fn from_rsdp(rsdp_addr: PhysAddr) -> Self {
        let handler = IdentityMapAcpiHandler;
        let rsdp_addr = rsdp_addr.as_u64() as usize;
        let tables = unsafe {
            AcpiTables::from_rsdp(handler, rsdp_addr).expect("failed to load ACPI tables from RSDP")
        };
        Self { tables }
    }

    /// Panics if PCI config regions cannot be found, simply because propagating
    /// the error is a PITA.
    pub(crate) fn pci_config_region_base_address(&self) -> PhysAddr {
        let pci_config_regions = acpi::mcfg::PciConfigRegions::new(&self.tables)
            .expect("couldn't get PCI config regions");

        // For some reason, pci_config_regions.regions is a private field, so we
        // have to just probe it.
        let pci_config_region_base_address = pci_config_regions
            .physical_address(0, 0, 0, 0)
            .expect("couldn't get PCI config address");

        PhysAddr::new(pci_config_region_base_address)
    }

    /// Asserts that the interrupt model is APIC, and returns the APIC info data
    /// structure.
    pub(crate) fn apic_info(&self) -> Apic {
        let interrupt_model = self
            .tables
            .platform_info()
            .expect("failed to get platform info for APIC")
            .interrupt_model;
        match interrupt_model {
            acpi::InterruptModel::Unknown => panic!("unknown interrupt model instead of ACPI"),
            acpi::InterruptModel::Apic(apic) => apic,
            _ => panic!("truly unknown interrupt model {interrupt_model:?}"),
        }
    }

    pub(crate) fn hpet_info(&self) -> HpetInfo {
        HpetInfo::new(&self.tables).expect("failed to get HPET info")
    }
}

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

pub(crate) fn print_acpi_info() {
    let info = acpi_info();

    let acpi_tables = &info.tables;
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

    let hpet_info = info.hpet_info();
    serial_println!("HPET info: {:#x?}", hpet_info);

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
        serial_println!("  MCFG entry {i}: {entry:#x?}");
    }
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
