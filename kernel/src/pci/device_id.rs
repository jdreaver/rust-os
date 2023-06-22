use core::fmt;

use crate::register_struct;
use crate::registers::RegisterRO;

use super::location::PCIDeviceLocation;

#[derive(Clone, Copy)]
pub(crate) struct PCIConfigDeviceID {
    registers: PCIConfigDeviceIDRegisters,
}

register_struct!(
    /// See <https://wiki.osdev.org/PCI#Common_Header_Fields> and "7.5.1.1 Type
    /// 0/1 Common Configuration Space" in spec
    pub(crate) PCIConfigDeviceIDRegisters {
        0x00 => vendor_id: RegisterRO<u16>,
        0x02 => device_id: RegisterRO<u16>,

        0x08 => revision_id: RegisterRO<u8>,
        0x09 => prog_if: RegisterRO<u8>,
        0x0A => subclass: RegisterRO<u8>,
        0x0B => class: RegisterRO<u8>,
    }
);

impl PCIConfigDeviceID {
    pub(crate) unsafe fn new(location: PCIDeviceLocation) -> Self {
        let address = location.device_base_address();
        let registers = PCIConfigDeviceIDRegisters::from_address(address);
        Self { registers }
    }

    pub(crate) fn registers(self) -> PCIConfigDeviceIDRegisters {
        self.registers
    }

    pub(crate) fn vendor_id(self) -> u16 {
        self.registers.vendor_id().read()
    }

    pub(crate) fn device_id(self) -> u16 {
        self.registers.device_id().read()
    }

    pub(crate) fn known_vendor_id(self) -> &'static str {
        let vendor_id = self.registers.vendor_id().read();
        lookup_vendor_id(vendor_id)
    }

    pub(crate) fn known_device_id(self) -> &'static str {
        let vendor_id = self.registers.vendor_id().read();
        let device_id = self.registers.device_id().read();
        lookup_known_device_id(vendor_id, device_id)
    }
}

impl fmt::Debug for PCIConfigDeviceID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let vendor = self.known_vendor_id();
        let device = self.known_device_id();

        let known_device_type = device_type(
            self.registers.class().read(),
            self.registers.subclass().read(),
            self.registers.prog_if().read(),
        )
        .unwrap_or("UNKNOWN");

        f.debug_struct("PCIConfigDeviceClass")
            .field("vendor", &vendor)
            .field("device", &device)
            .field("device_type", &known_device_type)
            .field("registers", &self.registers)
            .finish()
    }
}

/// Reports some known PCI vendor IDs. This is absolutely not exhaustive, but
/// known vendor IDs are useful for debugging.
///
/// Great resource for vendor IDs: <https://www.pcilookup.com>
fn lookup_vendor_id(vendor_id: u16) -> &'static str {
    match vendor_id {
        // If the vendor ID is 0xffff, then the device doesn't exist
        0xFFFF => "INVALID (0xFFFF)",
        0x8086 | 0x8087 => "Intel Corp.",
        0x1af4 => "virtio", // This is actually Red Hat, Inc., but it means virtio
        0x1002 => "Advanced Micro Devices, Inc. [AMD/ATI]",
        _ => "UNKNOWN",
    }
}

/// Reports on known PCI device IDs. This is absolutely not exhaustive, but
/// known device IDs are useful for debugging.
///
/// Great resource for device IDs: <https://www.pcilookup.com>
fn lookup_known_device_id(vendor_id: u16, device_id: u16) -> &'static str {
    #[allow(clippy::match_same_arms)]
    match (vendor_id, device_id) {
        (0x8086, 0x10d3) => "82574L Gigabit Network Connection",
        (0x8086, 0x2918) => "82801IB (ICH9) LPC Interface Controller",
        (0x8086, 0x2922) => "82801IR/IO/IH (ICH9R/DO/DH) 6 port SATA Controller [AHCI mode]",
        (0x8086, 0x2930) => "82801I (ICH9 Family) SMBus Controller",
        (0x8086, 0x29c0) => "82G33/G31/P35/P31 Express DRAM Controller",

        // See Section 4.1.2 "PCI Device Discovery" as well as Section 5 "Device
        // Types" of the VirtIO spec.
        //
        // "Devices MUST have the PCI Vendor ID 0x1AF4. Devices MUST either have
        // the PCI Device ID calculated by adding 0x1040 to the Virtio Device
        // ID, as indicated in section 5 or have the Transitional PCI Device ID
        // depending on the device type..."

        // Transitional IDs
        (0x1af4, 0x1000) => "network card",
        (0x1af4, 0x1001) => "block device",
        (0x1af4, 0x1002) => "memory ballooning (traditional)",
        (0x1af4, 0x1003) => "console",
        (0x1af4, 0x1004) => "SCSI host",
        (0x1af4, 0x1005) => "entropy source",
        (0x1af4, 0x1009) => "9P transport",

        // Non transitional IDs. These are device numbers added to 0x1040.
        (0x1af4, 0x1040) => "reserved (invalid)",
        (0x1af4, 0x1041) => "network card",
        (0x1af4, 0x1042) => "block device",
        (0x1af4, 0x1043) => "console",
        (0x1af4, 0x1044) => "entropy source",
        (0x1af4, 0x1045) => "memory ballooning (traditional)",
        (0x1af4, 0x1046) => "ioMemory",
        (0x1af4, 0x1047) => "rpmsg",
        (0x1af4, 0x1048) => "SCSI host",
        (0x1af4, 0x1049) => "9P transport",
        (0x1af4, 0x104A) => "mac80211 wlan",
        (0x1af4, 0x104B) => "rproc serial",
        (0x1af4, 0x104C) => "virtio CAIF",
        (0x1af4, 0x104D) => "memory balloon",
        (0x1af4, 0x1050) => "GPU device",
        (0x1af4, 0x1051) => "Timer/Clock device",
        (0x1af4, 0x1052) => "Input device",
        (0x1af4, 0x1053) => "Socket device",
        (0x1af4, 0x1054) => "Crypto device",
        (0x1af4, 0x1055) => "Signal Distribution Module",
        (0x1af4, 0x1056) => "pstore device",
        (0x1af4, 0x1057) => "IOMMU device",
        (0x1af4, 0x1058) => "Memory device",
        (0x1af4, 0x1059) => "Audio device",
        (0x1af4, 0x105A) => "file system device",
        (0x1af4, 0x105B) => "PMEM device",
        (0x1af4, 0x105C) => "RPMB device",
        (0x1af4, 0x105D) => "mac80211 hwsim wireless simulation device",
        (0x1af4, 0x105E) => "Video encoder device",
        (0x1af4, 0x105F) => "Video decoder device",
        (0x1af4, 0x1060) => "SCMI device",
        (0x1af4, 0x1061) => "NitroSecureModule",
        (0x1af4, 0x1062) => "I2C adapter",
        (0x1af4, 0x1063) => "Watchdog",
        (0x1af4, 0x1064) => "CAN device",
        (0x1af4, 0x1066) => "Parameter Server",
        (0x1af4, 0x1067) => "Audio policy device",
        (0x1af4, 0x1068) => "Bluetooth device",
        (0x1af4, 0x1069) => "GPIO device",
        (0x1af4, 0x106A) => "RDMA device",

        _ => "UNKNOWN",
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum PCIDeviceClass {
    Unclassified,
    MassStorageController,
    NetworkController,
    DisplayController,
    MultimediaController,
    MemoryController,
    BridgeDevice,
    SimpleCommunicationController,
    BaseSystemPeripheral,
    InputDeviceController,
    DockingStation,
    Processor,
    SerialBusController,
    WirelessController,
    IntelligentController,
    SatelliteCommunicationController,
    EncryptionController,
    SignalProcessingController,
    ProcessingAccelerator,
    NonEssentialInstrumentation,
    Coprocessor,
    Unassigned,
}

impl PCIDeviceClass {
    fn from_byte(class: u8) -> Result<Self, &'static str> {
        match class {
            0x00 => Ok(Self::Unclassified),
            0x01 => Ok(Self::MassStorageController),
            0x02 => Ok(Self::NetworkController),
            0x03 => Ok(Self::DisplayController),
            0x04 => Ok(Self::MultimediaController),
            0x05 => Ok(Self::MemoryController),
            0x06 => Ok(Self::BridgeDevice),
            0x07 => Ok(Self::SimpleCommunicationController),
            0x08 => Ok(Self::BaseSystemPeripheral),
            0x09 => Ok(Self::InputDeviceController),
            0x0A => Ok(Self::DockingStation),
            0x0B => Ok(Self::Processor),
            0x0C => Ok(Self::SerialBusController),
            0x0D => Ok(Self::WirelessController),
            0x0E => Ok(Self::IntelligentController),
            0x0F => Ok(Self::SatelliteCommunicationController),
            0x10 => Ok(Self::EncryptionController),
            0x11 => Ok(Self::SignalProcessingController),
            0x12 => Ok(Self::ProcessingAccelerator),
            0x13 => Ok(Self::NonEssentialInstrumentation),
            0x40 => Ok(Self::Coprocessor),
            0xFF => Ok(Self::Unassigned),
            _ => Err("invalid PCI device class"),
        }
    }
}

fn device_type(class: u8, subclass: u8, prog_if: u8) -> Result<&'static str, &'static str> {
    let class_name = PCIDeviceClass::from_byte(class)?;
    match class_name {
        PCIDeviceClass::Unclassified => Ok("Unclassified"),
        PCIDeviceClass::MassStorageController => match subclass {
            0x00 => Ok("Mass Storage Controller: SCSI"),
            0x01 => Ok("Mass Storage Controller: IDE"),
            0x02 => Ok("Mass Storage Controller: FloppyDisk"),
            0x03 => Ok("Mass Storage Controller: IPIBus"),
            0x04 => Ok("Mass Storage Controller: RAID"),
            0x05 => Ok("Mass Storage Controller: ATA"),
            0x06 => match prog_if {
                0x00 => Ok("Mass Storage Controller: SATA: VendorSpecific"),
                0x01 => Ok("Mass Storage Controller: SATA: AHCI1_0"),
                0x02 => Ok("Mass Storage Controller: SATA: SerialStorageBus"),
                _ => Err("invalid PCI device mass storage controller SATA prog_if"),
            },
            0x07 => Ok("Mass Storage Controller: SAS"),
            0x08 => Ok("Mass Storage Controller: NVM"),
            0x80 => Ok("Mass Storage Controller: Other"),
            _ => Err("invalid PCI device mass storage controller subclass"),
        },
        PCIDeviceClass::NetworkController => Ok("Network Controller"),
        PCIDeviceClass::DisplayController => Ok("Display Controller"),
        PCIDeviceClass::MultimediaController => Ok("Multimedia Controller"),
        PCIDeviceClass::MemoryController => Ok("Memory Controller"),
        PCIDeviceClass::BridgeDevice => Ok("Bridge Device"),
        PCIDeviceClass::SimpleCommunicationController => Ok("Simple Communication Controller"),
        PCIDeviceClass::BaseSystemPeripheral => Ok("Base System Peripheral"),
        PCIDeviceClass::InputDeviceController => Ok("Input Device Controller"),
        PCIDeviceClass::DockingStation => Ok("Docking Station"),
        PCIDeviceClass::Processor => Ok("Processor"),
        PCIDeviceClass::SerialBusController => Ok("Serial Bus Controller"),
        PCIDeviceClass::WirelessController => Ok("Wireless Controller"),
        PCIDeviceClass::IntelligentController => Ok("Intelligent Controller"),
        PCIDeviceClass::SatelliteCommunicationController => {
            Ok("Satellite Communication Controller")
        }
        PCIDeviceClass::EncryptionController => Ok("Encryption Controller"),
        PCIDeviceClass::SignalProcessingController => Ok("Signal Processing Controller"),
        PCIDeviceClass::ProcessingAccelerator => Ok("Processing Accelerator"),
        PCIDeviceClass::NonEssentialInstrumentation => Ok("Non Essential Instrumentation"),
        PCIDeviceClass::Coprocessor => Ok("Coprocessor"),
        PCIDeviceClass::Unassigned => Ok("Unassigned"),
    }
}
