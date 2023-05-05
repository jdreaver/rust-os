use crate::serial_println;

const MAX_PCI_BUSES: u64 = 256;
const MAX_PCI_BUS_DEVICES: u64 = 32;
const MAX_PCI_BUS_DEVICE_FUNCTIONS: u64 = 8;

/// <https://wiki.osdev.org/PCI#.22Brute_Force.22_Scan>
///
/// NOTE: I think this would miss devices that are behind a PCI bridge, except
/// we are enumerating all buses, so maybe it is fine?
pub fn brute_force_search_pci(base_addr: u64) {
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
    raw_vendor_id: u16,
    known_vendor_id: PCIDeviceVendorID,
    raw_device_id: u16,
    known_device_id: &'static str,
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

        let Some(known_vendor_id) = PCIDeviceVendorID::from_bytes(raw.vendor_id) else { return Ok(None); };
        let known_device_id = lookup_known_device_id(raw.vendor_id, raw.device_id);

        let header_type = match raw.header_type & 0xF {
            0x0 => PCIDeviceConfigHeaderType::GeneralDevice,
            0x1 => PCIDeviceConfigHeaderType::PCIToPCIBridge,
            0x2 => PCIDeviceConfigHeaderType::PCIToCardBusBridge,
            _ => return Err("invalid PCI header type"),
        };

        let multiple_functions = raw.header_type & 0x80 != 0;

        let class = PCIDeviceClass::from_bytes(raw.class, raw.subclass, raw.prog_if)?;

        Ok(Some(Self {
            raw_vendor_id: raw.vendor_id,
            known_vendor_id,
            raw_device_id: raw.device_id,
            known_device_id,
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

/// Reports some known PCI vendor IDs. This is absolutely not exhaustive, but
/// known vendor IDs are useful for debugging.
///
/// Great resource for vendor IDs: <https://www.pcilookup.com>
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum PCIDeviceVendorID {
    Intel,
    VirtIO,
    AMD,
    Unknown,
}

impl PCIDeviceVendorID {
    fn from_bytes(bytes: u16) -> Option<Self> {
        match bytes {
            // If the vendor ID is 0xffff, then the device doesn't exist
            0xFFFF => None,
            0x8086 | 0x8087 => Some(Self::Intel),
            0x1af4 => Some(Self::VirtIO),
            0x1002 => Some(Self::AMD),
            _ => Some(Self::Unknown),
        }
    }
}

/// Reports on known PCI device IDs. This is absolutely not exhaustive, but
/// known device IDs are useful for debugging.
///
/// Great resource for device IDs: <https://www.pcilookup.com>
fn lookup_known_device_id(vendor_id: u16, device_id: u16) -> &'static str {
    match (vendor_id, device_id) {
        (0x8086, 0x10d3) => "82574L Gigabit Network Connection",
        (0x8086, 0x2918) => "82801IB (ICH9) LPC Interface Controller",
        (0x8086, 0x2922) => "82801IR/IO/IH (ICH9R/DO/DH) 6 port SATA Controller [AHCI mode]",
        (0x8086, 0x2930) => "82801I (ICH9 Family) SMBus Controller",
        (0x8086, 0x29c0) => "82G33/G31/P35/P31 Express DRAM Controller",
        (0x1af4, 0x1050) => "Virtio GPU",
        _ => "UNKNOWN",
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
    MassStorageController {
        subclass: PCIDeviceMassStorageControllerSubclass,
    },
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
            0x01 => Ok(Self::MassStorageController {
                subclass: PCIDeviceMassStorageControllerSubclass::from_bytes(subclass, prog_if)?,
            }),
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
    SCSI {
        prog_if: PCIDeviceUnknownProgIF,
    },
    IDE {
        prog_if: PCIDeviceUnknownProgIF,
    },
    FloppyDisk {
        prog_if: PCIDeviceUnknownProgIF,
    },
    IPIBus {
        prog_if: PCIDeviceUnknownProgIF,
    },
    RAID {
        prog_if: PCIDeviceUnknownProgIF,
    },
    ATA {
        prog_if: PCIDeviceUnknownProgIF,
    },
    SATA {
        prog_if: PCIDeviceMassStorageControllerSATAProgIF,
    },
    SAS {
        prog_if: PCIDeviceUnknownProgIF,
    },
    NVM {
        prog_if: PCIDeviceUnknownProgIF,
    },
    Other {
        prog_if: PCIDeviceUnknownProgIF,
    },
}

impl PCIDeviceMassStorageControllerSubclass {
    fn from_bytes(subclass: u8, prog_if: u8) -> Result<Self, &'static str> {
        match subclass {
            0x00 => Ok(Self::SCSI { prog_if }),
            0x01 => Ok(Self::IDE { prog_if }),
            0x02 => Ok(Self::FloppyDisk { prog_if }),
            0x03 => Ok(Self::IPIBus { prog_if }),
            0x04 => Ok(Self::RAID { prog_if }),
            0x05 => Ok(Self::ATA { prog_if }),
            0x06 => Ok(Self::SATA {
                prog_if: PCIDeviceMassStorageControllerSATAProgIF::from_bytes(prog_if)?,
            }),
            0x07 => Ok(Self::SAS { prog_if }),
            0x08 => Ok(Self::NVM { prog_if }),
            0x80 => Ok(Self::Other { prog_if }),
            _ => Err("invalid PCI device mass storage controller subclass"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum PCIDeviceMassStorageControllerSATAProgIF {
    VendorSpecific,
    AHCI1_0,
    SerialStorageBus,
}

impl PCIDeviceMassStorageControllerSATAProgIF {
    fn from_bytes(prog_if: u8) -> Result<Self, &'static str> {
        match prog_if {
            0x00 => Ok(Self::VendorSpecific),
            0x01 => Ok(Self::AHCI1_0),
            0x02 => Ok(Self::SerialStorageBus),
            _ => Err("invalid PCI device mass storage controller SATA prog_if"),
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
