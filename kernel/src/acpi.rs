use core::ptr::NonNull;

use acpi::mcfg::{Mcfg, McfgEntry};
use acpi::platform::interrupt::Apic;
use acpi::{AcpiHandler, AcpiTable, AcpiTables, PhysicalMapping};
use x86_64::PhysAddr;

use crate::register_struct;
use crate::registers::{RegisterRO, RegisterRW, RegisterWO};
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

/// Holds important Advanced Configuration and Power Interface (ACPI)
/// information needed to start the kernel.
pub struct ACPIInfo {
    tables: AcpiTables<IdentityMapAcpiHandler>,
}

impl ACPIInfo {
    /// # Safety
    ///
    /// Caller must ensure RSDP address is valid, and that page tables are set
    /// up for identity mapping for any memory that could be used to access ACPI
    /// tables (e.g. identity mapping physical memory).
    pub(crate) unsafe fn from_rsdp(rsdp_addr: PhysAddr) -> Self {
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
    pub(crate) fn apic(&self) -> Apic {
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
}

register_struct!(
    /// See "11.4.1 The Local APIC Block Diagram", specifically "Table 11-1. Local
    /// APIC Register Address Map" in the Intel 64 Manual Volume 3. Also see
    /// <https://wiki.osdev.org/APIC>.
    pub(crate) LocalAPICRegisters {
        0x20 => local_apic_id: RegisterRW<u32>,
        0x30 => local_apic_version: RegisterRO<u32>,
        0x80 => task_priority: RegisterRW<u32>,
        0x90 => arbitration_priority: RegisterRO<u32>,
        0xa0 => processor_priority: RegisterRO<u32>,
        0xb0 => end_of_interrupt: RegisterWO<u32>,
        0xc0 => remote_read: RegisterRO<u32>,
        0xd0 => logical_destination: RegisterRW<u32>,
        0xe0 => destination_format: RegisterRW<u32>,
        0xf0 => spurious_interrupt_vector: RegisterRW<u32>,

        0x100 => in_service_0: RegisterRO<u32>,
        0x110 => in_service_1: RegisterRO<u32>,
        0x120 => in_service_2: RegisterRO<u32>,
        0x130 => in_service_3: RegisterRO<u32>,
        0x140 => in_service_4: RegisterRO<u32>,
        0x150 => in_service_5: RegisterRO<u32>,
        0x160 => in_service_6: RegisterRO<u32>,
        0x170 => in_service_7: RegisterRO<u32>,

        0x180 => trigger_mode_0: RegisterRO<u32>,
        0x190 => trigger_mode_1: RegisterRO<u32>,
        0x1a0 => trigger_mode_2: RegisterRO<u32>,
        0x1b0 => trigger_mode_3: RegisterRO<u32>,
        0x1c0 => trigger_mode_4: RegisterRO<u32>,
        0x1d0 => trigger_mode_5: RegisterRO<u32>,
        0x1e0 => trigger_mode_6: RegisterRO<u32>,
        0x1f0 => trigger_mode_7: RegisterRO<u32>,

        0x200 => interrupt_request_0: RegisterRO<u32>,
        0x210 => interrupt_request_1: RegisterRO<u32>,
        0x220 => interrupt_request_2: RegisterRO<u32>,
        0x230 => interrupt_request_3: RegisterRO<u32>,
        0x240 => interrupt_request_4: RegisterRO<u32>,
        0x250 => interrupt_request_5: RegisterRO<u32>,
        0x260 => interrupt_request_6: RegisterRO<u32>,
        0x270 => interrupt_request_7: RegisterRO<u32>,

        0x280 => error_status: RegisterRO<u32>,
        0x2f0 => lvt_corrected_machine_check_interrupt: RegisterRW<u32>,
        0x300 => interrupt_command_low_bits: RegisterRW<u32>,
        0x310 => interrupt_command_high_bits: RegisterRW<u32>,
        0x320 => lvt_timer: RegisterRW<u32>,
        0x330 => lvt_thermal_sensor: RegisterRW<u32>,
        0x340 => lvt_performance_monitoring_counters: RegisterRW<u32>,
        0x350 => lvt_lint0: RegisterRW<u32>,
        0x360 => lvt_lint1: RegisterRW<u32>,
        0x370 => lvt_error: RegisterRW<u32>,
        0x380 => initial_count: RegisterRW<u32>,
        0x398 => current_count: RegisterRO<u32>,
        0x3e0 => divide_configuration: RegisterRW<u32>,
    }
);

pub(crate) fn print_acpi_info(info: &ACPIInfo) {
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

    let apic = info.apic();
    serial_println!("ACPI APIC: {:#x?}", apic);

    let local_apic_reg =
        unsafe { LocalAPICRegisters::from_address(apic.local_apic_address as usize) };
    serial_println!("Local APIC Registers: {:#x?}", local_apic_reg);

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
