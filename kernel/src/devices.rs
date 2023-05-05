use core::ptr::NonNull;

use acpi::mcfg::{Mcfg, McfgEntry};
use acpi::{AcpiHandler, AcpiTable, AcpiTables, PhysicalMapping};
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
    // TODO: Figure out how to read memory mapped PCIe config
    let base_device = unsafe { *(pci_config_region_base_address as *mut PCIDeviceConfigHeader) };
    serial_println!("Base PCI device: {:#x?}", base_device);

    brute_force_search_pci(pci_config_region_base_address);
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

/// <https://wiki.osdev.org/PCI#.22Brute_Force.22_Scan>
///
/// NOTE: I think this would miss devices that are behind a PCI bridge, except
/// we are enumerating all buses, so maybe it is fine?
fn brute_force_search_pci(base_addr: u64) {
    for bus in 0..MAX_PCI_BUSES {
        for slot in 0..MAX_PCI_BUS_DEVICES {
            for function in 0..MAX_PCI_BUS_DEVICE_FUNCTIONS {
                let addr = base_addr | (bus << 20) | (slot << 15) | (function << 12);
                let header = PCIDeviceConfigHeader::from_ptr(addr as *mut u8)
                    .expect("failed to read PCI device header");
                if let Some(header) = header {
                    serial_println!(
                        "PCI device found at {:#x} (bus: {:x}, slot: {:x}, function: {:x}):\n{:#x?}",
                        addr,
                        bus,
                        slot,
                        function,
                        header
                    );
                }
            }
        }
    }
}

/// See <https://wiki.osdev.org/PCI#Common_Header_Fields>
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct PCIDeviceConfigHeader {
    vendor_id: u16,
    device_id: u16,
    command: u16,
    status: u16,
    revision_id: u8,
    class: PCIDeviceClass,
    cache_line_size: u8,
    latency_timer: u8,
    header_type: PCIDeviceConfigHeaderType,
    multiple_functions: bool,
    bist: u8,
}

impl PCIDeviceConfigHeader {
    fn from_ptr(ptr: *mut u8) -> Result<Option<Self>, &'static str> {
        let bytes = unsafe { *ptr.cast::<[u8; 16]>() };
        Self::from_bytes(&bytes)
    }

    fn from_bytes(bytes: &[u8; 16]) -> Result<Option<Self>, &'static str> {
        let raw = RawPCIDeviceConfigHeader::from_bytes(bytes);

        // If the vendor ID is 0xffff, then the device doesn't exist
        if raw.vendor_id == 0xffff {
            return Ok(None);
        }

        let header_type = match raw.header_type & 0xF {
            0x0 => PCIDeviceConfigHeaderType::GeneralDevice,
            0x1 => PCIDeviceConfigHeaderType::PCIToPCIBridge,
            0x2 => PCIDeviceConfigHeaderType::PCIToCardBusBridge,
            _ => return Err("invalid PCI header type"),
        };

        let multiple_functions = raw.header_type & 0x80 != 0;

        let class = PCIDeviceClass::from_bytes(raw.class, raw.subclass, raw.prog_if)?;

        Ok(Some(Self {
            vendor_id: raw.vendor_id,
            device_id: raw.device_id,
            command: raw.command,
            status: raw.status,
            revision_id: raw.revision_id,
            class,
            cache_line_size: raw.cache_line_size,
            latency_timer: raw.latency_timer,
            header_type,
            multiple_functions,
            bist: raw.bist,
        }))
    }
}

#[derive(Debug, Clone, Copy)]
enum PCIDeviceConfigHeaderType {
    GeneralDevice,
    PCIToPCIBridge,
    PCIToCardBusBridge,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum PCIDeviceClass {
    Unclassified {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    MassStorageController(PCIDeviceMassStorageControllerSubclass),
    NetworkController {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    DisplayController {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    MultimediaController {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    MemoryController {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    BridgeDevice {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    SimpleCommunicationController {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    BaseSystemPeripheral {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    InputDeviceController {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    DockingStation {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    Processor {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    SerialBusController {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    WirelessController {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    IntelligentController {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    SatelliteCommunicationController {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    EncryptionController {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    SignalProcessingController {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    ProcessingAccelerator {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    NonEssentialInstrumentation {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    Coprocessor {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
    Unassigned {
        subclass: PCIDeviceUnknownSubclass,
        prog_if: PCIDeviceUnknownProgIF,
    },
}

type PCIDeviceUnknownSubclass = u8;
type PCIDeviceUnknownProgIF = u8;

impl PCIDeviceClass {
    fn from_bytes(class: u8, subclass: u8, prog_if: u8) -> Result<Self, &'static str> {
        match class {
            0x00 => Ok(Self::Unclassified { subclass, prog_if }),
            0x01 => Ok(Self::MassStorageController(
                PCIDeviceMassStorageControllerSubclass::from_bytes(subclass, prog_if)?,
            )),
            0x02 => Ok(Self::NetworkController { subclass, prog_if }),
            0x03 => Ok(Self::DisplayController { subclass, prog_if }),
            0x04 => Ok(Self::MultimediaController { subclass, prog_if }),
            0x05 => Ok(Self::MemoryController { subclass, prog_if }),
            0x06 => Ok(Self::BridgeDevice { subclass, prog_if }),
            0x07 => Ok(Self::SimpleCommunicationController { subclass, prog_if }),
            0x08 => Ok(Self::BaseSystemPeripheral { subclass, prog_if }),
            0x09 => Ok(Self::InputDeviceController { subclass, prog_if }),
            0x0A => Ok(Self::DockingStation { subclass, prog_if }),
            0x0B => Ok(Self::Processor { subclass, prog_if }),
            0x0C => Ok(Self::SerialBusController { subclass, prog_if }),
            0x0D => Ok(Self::WirelessController { subclass, prog_if }),
            0x0E => Ok(Self::IntelligentController { subclass, prog_if }),
            0x0F => Ok(Self::SatelliteCommunicationController { subclass, prog_if }),
            0x10 => Ok(Self::EncryptionController { subclass, prog_if }),
            0x11 => Ok(Self::SignalProcessingController { subclass, prog_if }),
            0x12 => Ok(Self::ProcessingAccelerator { subclass, prog_if }),
            0x13 => Ok(Self::NonEssentialInstrumentation { subclass, prog_if }),
            0x40 => Ok(Self::Coprocessor { subclass, prog_if }),
            0xFF => Ok(Self::Unassigned { subclass, prog_if }),
            _ => Err("invalid PCI device class"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum PCIDeviceMassStorageControllerSubclass {
    SCSI(PCIDeviceUnknownProgIF),
    IDE(PCIDeviceUnknownProgIF),
    FloppyDisk(PCIDeviceUnknownProgIF),
    IPIBus(PCIDeviceUnknownProgIF),
    RAID(PCIDeviceUnknownProgIF),
    ATA(PCIDeviceUnknownProgIF),
    SATA(PCIDeviceUnknownProgIF),
    SAS(PCIDeviceUnknownProgIF),
    NVM(PCIDeviceUnknownProgIF),
    Other(PCIDeviceUnknownProgIF),
}

impl PCIDeviceMassStorageControllerSubclass {
    fn from_bytes(subclass: u8, prog_if: u8) -> Result<Self, &'static str> {
        match subclass {
            0x00 => Ok(Self::SCSI(prog_if)),
            0x01 => Ok(Self::IDE(prog_if)),
            0x02 => Ok(Self::FloppyDisk(prog_if)),
            0x03 => Ok(Self::IPIBus(prog_if)),
            0x04 => Ok(Self::RAID(prog_if)),
            0x05 => Ok(Self::ATA(prog_if)),
            0x06 => Ok(Self::SATA(prog_if)),
            0x07 => Ok(Self::SAS(prog_if)),
            0x08 => Ok(Self::NVM(prog_if)),
            0x80 => Ok(Self::Other(prog_if)),
            _ => Err("invalid PCI device mass storage controller subclass"),
        }
    }
}

/// Just used for IO to corral the bits for `PCIDeviceConfigHeader`.
///
/// See <https://wiki.osdev.org/PCI#Common_Header_Fields>
#[repr(packed)]
#[derive(Debug, Clone, Copy)]
struct RawPCIDeviceConfigHeader {
    vendor_id: u16,
    device_id: u16,
    command: u16,
    status: u16,
    revision_id: u8,
    prog_if: u8,
    subclass: u8,
    class: u8,
    cache_line_size: u8,
    latency_timer: u8,
    header_type: u8,
    bist: u8,
}

impl RawPCIDeviceConfigHeader {
    fn from_bytes(bytes: &[u8; 16]) -> Self {
        unsafe { core::ptr::read(bytes.as_ptr().cast::<Self>()) }
    }
}

const MAX_PCI_BUSES: u64 = 256;
const MAX_PCI_BUS_DEVICES: u64 = 32;
const MAX_PCI_BUS_DEVICE_FUNCTIONS: u64 = 8;
