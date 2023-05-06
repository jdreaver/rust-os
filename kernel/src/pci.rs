use core::fmt;

use crate::serial;

const MAX_PCI_BUS: u8 = 255;
const MAX_PCI_BUS_DEVICE: u8 = 31;
const MAX_PCI_BUS_DEVICE_FUNCTION: u8 = 7;

/// <https://wiki.osdev.org/PCI#.22Brute_Force.22_Scan>
///
/// NOTE: I think this would miss devices that are behind a PCI bridge, except
/// we are enumerating all buses, so maybe it is fine?
pub fn brute_force_search_pci(base_addr: u64) {
    for bus in 0..=MAX_PCI_BUS {
        for slot in 0..=MAX_PCI_BUS_DEVICE {
            for function in 0..=MAX_PCI_BUS_DEVICE_FUNCTION {
                let device =
                    unsafe { PCIeDeviceConfig::new(base_addr as *const u8, bus, slot, function) };
                let Some(device) = device else { continue; };
                device
                    .print(serial::serial1_writer())
                    .expect("failed to print device config");
            }
        }
    }
}

#[allow(clippy::doc_markdown)] // Clippy doesn't like PCIe
/// Interface into a PCI Express device's configuration space. See:
/// - <https://wiki.osdev.org/PCI_Express#Configuration_Space>
/// - Section 7.5 "7.5 PCI and PCIe Capabilities Required by the Base Spec for all Ports" of the PCI Express Base Specification
/// - <https://wiki.osdev.org/PCI>, which is the legacy interface, but is still a good explanation.
struct PCIeDeviceConfig {
    /// Base address of the PCI Express extended configuration mechanism memory
    /// region in which this device resides.
    _enhanced_config_region_address: *const u8,

    /// Which PCIe bus this device is on.
    bus_number: u8,

    /// Device number of the device within the bus.
    device_number: u8,

    /// Function number of the device if the device is a multifunction device.
    function_number: u8,

    /// All PCI/PCIe devices have a common header field that lives at the base
    /// of the device's configuration space.
    header: PCIDeviceConfigHeaderPtr,
}

impl PCIeDeviceConfig {
    /// Returns `Some` if a device exists at the given location.
    ///
    /// # Safety
    ///
    /// Caller must ensure that `base_address` is a valid pointer to a PCI
    /// Express extended configuration mechanism memory region.
    unsafe fn new(
        enhanced_config_region_address: *const u8,
        bus_number: u8,
        device_number: u8,
        function_number: u8,
    ) -> Option<Self> {
        let bus = u64::from(bus_number);
        let device = u64::from(device_number);
        let function = u64::from(function_number);
        let base_addr =
            enhanced_config_region_address as u64 | (bus << 20) | (device << 15) | (function << 12);

        let header = PCIDeviceConfigHeaderPtr::new(base_addr as *mut PCIDeviceConfigHeader);

        if !header.device_exists() {
            return None;
        }

        Some(Self {
            _enhanced_config_region_address: enhanced_config_region_address,
            bus_number,
            device_number,
            function_number,
            header,
        })
    }

    /// Computes the physical address of the device's configuration space.
    #[inline]
    fn physical_address(&self) -> u64 {
        self.header.0 as *const _ as u64
    }

    fn read_body(&self) -> Result<PCIDeviceConfigBody, &str> {
        let header_location = self.header.0 as u64;
        let body_ptr_location =
            header_location + core::mem::size_of::<PCIDeviceConfigHeader>() as u64;

        let body = match self.header.header_layout()? {
            PCIDeviceConfigHeaderLayout::GeneralDevice => {
                let ptr = body_ptr_location as *mut PCIDeviceConfigBodyType0;
                PCIDeviceConfigBody::GeneralDevice(PCIDeviceConfigBodyType0Ptr::new(ptr))
            }
            PCIDeviceConfigHeaderLayout::PCIToPCIBridge => PCIDeviceConfigBody::PCIToPCIBridge,
        };
        Ok(body)
    }

    fn print<W: fmt::Write>(&self, w: &mut W) -> fmt::Result {
        writeln!(w, "PCIe device config:")?;
        writeln!(w, "  Address: {:#x?}", self.physical_address())?;
        writeln!(w, "  Bus number: {}", self.bus_number)?;
        writeln!(w, "  Device number: {}", self.device_number)?;
        writeln!(w, "  Function number: {}", self.function_number)?;
        writeln!(w, "  Header:")?;
        self.header.print(w, 4)?;

        let body = self.read_body().expect("failed to read PCI device body");
        match body {
            PCIDeviceConfigBody::GeneralDevice(body) => {
                writeln!(w, "  General Device Body:")?;
                body.print(w, 4)?;
            }
            PCIDeviceConfigBody::PCIToPCIBridge => {
                writeln!(w, "  Body: PCI to PCI bridge")?;
            }
        };
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum PCIDeviceConfigBody {
    GeneralDevice(PCIDeviceConfigBodyType0Ptr),
    PCIToPCIBridge,
    // N.B. PCIToCardBusBridge doesn't exist any longer in PCI Express. Let's
    // just pretend it never existed.
    // PCIToCardBusBridge,
}

/// Reports some known PCI vendor IDs. This is absolutely not exhaustive, but
/// known vendor IDs are useful for debugging.
///
/// Great resource for vendor IDs: <https://www.pcilookup.com>
fn lookup_vendor_id(vendor_id: u16) -> Option<&'static str> {
    match vendor_id {
        // If the vendor ID is 0xffff, then the device doesn't exist
        0xFFFF => None,
        0x8086 | 0x8087 => Some("Intel Corp."),
        0x1af4 => Some("virtio"), // This is actually Red Hat, Inc., but it means virtio
        0x1002 => Some("Advanced Micro Devices, Inc. [AMD/ATI]"),
        _ => Some("UNKNOWN"),
    }
}

/// See <https://wiki.osdev.org/PCI#Common_Header_Fields>
#[repr(packed)]
#[derive(Clone, Copy)]
struct PCIDeviceConfigHeader {
    vendor_id: u16,
    device_id: u16,
    command: u16,
    status: u16,
    revision_id: u8,
    prog_if: u8,
    subclass: u8,
    class: u8,
    _cache_line_size: u8,
    _latency_timer: u8,
    header_type: u8, // TODO: Replace with wrapper type to get layout vs multifunction
    _bist: u8,
}

#[derive(Clone, Copy)]
struct PCIDeviceConfigHeaderPtr(*mut PCIDeviceConfigHeader);

impl PCIDeviceConfigHeaderPtr {
    fn new(ptr: *mut PCIDeviceConfigHeader) -> Self {
        Self(ptr)
    }

    fn as_ref(self) -> &'static PCIDeviceConfigHeader {
        unsafe { &*self.0 }
    }

    /// A device exists if the Vendor ID register is not 0xFFFF.
    fn device_exists(self) -> bool {
        self.as_ref().vendor_id != 0xFFFF
    }

    /// The layout is in the first 7 bits of the Header Type register.
    fn header_layout(self) -> Result<PCIDeviceConfigHeaderLayout, &'static str> {
        match self.as_ref().header_type & 0x7 {
            0x00 => Ok(PCIDeviceConfigHeaderLayout::GeneralDevice),
            0x01 => Ok(PCIDeviceConfigHeaderLayout::PCIToPCIBridge),
            // 0x02 => Ok(PCIDeviceConfigHeaderType::PCIToCardBusBridge),
            _ => Err("invalid PCI device header type"),
        }
    }

    /// If the 8th bit of the Header Type register is set, the device is a
    /// multifunction device.
    fn is_multifunction(self) -> bool {
        self.as_ref().header_type & 0x80 != 0
    }

    fn print<W: fmt::Write>(self, w: &mut W, indent: usize) -> fmt::Result {
        let header = self.as_ref();

        let layout = self
            .header_layout()
            .expect("couldn't construct header layout")
            .as_str();
        writeln!(w, "{:indent$}layout: {}", "", layout, indent = indent)?;

        let multifunction = self.is_multifunction();
        writeln!(
            w,
            "{:indent$}multifunction: {}",
            "",
            multifunction,
            indent = indent
        )?;

        let command = header.command;
        writeln!(
            w,
            "{:indent$}command: {:#08b}",
            "",
            command,
            indent = indent
        )?;

        let status = header.status;
        writeln!(w, "{:indent$}status: {:#08b}", "", status, indent = indent)?;

        let vendor_id = header.vendor_id;
        let vendor = lookup_vendor_id(vendor_id);
        write!(w, "{:indent$}vendor: {:#x}", "", vendor_id, indent = indent)?;
        writeln!(w, " ({})", vendor.unwrap_or("UNKNOWN"))?;

        let device_id = header.device_id;
        let device = lookup_known_device_id(vendor_id, device_id);
        let revision_id = header.revision_id;
        writeln!(
            w,
            "{:indent$}device_id: {:#x}, revision_id: {:#x}, ({device})",
            "",
            device_id,
            revision_id,
            indent = indent
        )?;

        let device = device_type(header.class, header.subclass, header.prog_if)
            .expect("couldn't construct device class");
        writeln!(w, "{:indent$}device:", "", indent = indent)?;
        writeln!(w, "{:indent$}name: {}", "", device, indent = indent + 2)?;
        writeln!(
            w,
            "{:indent$}class: {:#x}",
            "",
            header.class,
            indent = indent + 2
        )?;
        writeln!(
            w,
            "{:indent$}subclass: {:#x}",
            "",
            header.subclass,
            indent = indent + 2
        )?;
        writeln!(
            w,
            "{:indent$}prog_if: {:#x}",
            "",
            header.prog_if,
            indent = indent + 2
        )?;

        Ok(())
    }
}

#[derive(Clone, Copy)]
enum PCIDeviceConfigHeaderLayout {
    GeneralDevice,
    PCIToPCIBridge,
    // N.B. PCIToCardBusBridge doesn't exist any longer in PCI Express. Let's
    // just pretend it never existed.
    // PCIToCardBusBridge,
}

impl PCIDeviceConfigHeaderLayout {
    fn as_str(self) -> &'static str {
        match self {
            Self::GeneralDevice => "0x00 (General Device)",
            Self::PCIToPCIBridge => "0x01 (PCI-to-PCI Bridge)",
            // PCIDeviceConfigHeaderLayout::PCIToCardBusBridge => "0x02 (PCI-to-CardBus Bridge)",
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
#[allow(dead_code)]
enum PCIDeviceClass {
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

#[repr(packed)]
#[derive(Clone, Copy)]
struct PCIDeviceConfigBodyType0 {
    bar0: u32,
    bar1: u32,
    bar2: u32,
    bar3: u32,
    bar4: u32,
    bar5: u32,
    cardbus_cis_pointer: u32,
    subsystem_vendor_id: u16,
    subsystem_id: u16,
    expansion_rom_base_address: u32,
    capabilities_pointer: u8,
    _reserved: [u8; 7],
    interrupt_line: u8,
    interrupt_pin: u8,
    min_grant: u8,
    max_latency: u8,
}

#[derive(Clone, Copy)]
struct PCIDeviceConfigBodyType0Ptr(*mut PCIDeviceConfigBodyType0);

impl PCIDeviceConfigBodyType0Ptr {
    fn new(ptr: *mut PCIDeviceConfigBodyType0) -> Self {
        Self(ptr)
    }

    fn as_ref(&self) -> &'static PCIDeviceConfigBodyType0 {
        unsafe { &*self.0 }
    }

    fn print<W: fmt::Write>(self, w: &mut W, indent: usize) -> fmt::Result {
        let body = self.as_ref();

        let bar0 = body.bar0;
        let bar1 = body.bar1;
        let bar2 = body.bar2;
        let bar3 = body.bar3;
        let bar4 = body.bar4;
        let bar5 = body.bar5;
        let cardbus_cis_pointer = body.cardbus_cis_pointer;
        let subsystem_vendor_id = body.subsystem_vendor_id;
        let subsystem_id = body.subsystem_id;
        let expansion_rom_base_address = body.expansion_rom_base_address;

        writeln!(w, "{:indent$}bar0: 0x{:08x}", "", bar0, indent = indent)?;
        writeln!(w, "{:indent$}bar1: 0x{:08x}", "", bar1, indent = indent)?;
        writeln!(w, "{:indent$}bar2: 0x{:08x}", "", bar2, indent = indent)?;
        writeln!(w, "{:indent$}bar3: 0x{:08x}", "", bar3, indent = indent)?;
        writeln!(w, "{:indent$}bar4: 0x{:08x}", "", bar4, indent = indent)?;
        writeln!(w, "{:indent$}bar5: 0x{:08x}", "", bar5, indent = indent)?;
        writeln!(
            w,
            "{:indent$}cardbus_cis_pointer: 0x{:08x}",
            "",
            cardbus_cis_pointer,
            indent = indent
        )?;
        writeln!(
            w,
            "{:indent$}subsystem_vendor_id: 0x{:04x}",
            "",
            subsystem_vendor_id,
            indent = indent
        )?;
        writeln!(
            w,
            "{:indent$}subsystem_id: 0x{:04x}",
            "",
            subsystem_id,
            indent = indent
        )?;
        writeln!(
            w,
            "{:indent$}expansion_rom_base_address: 0x{:08x}",
            "",
            expansion_rom_base_address,
            indent = indent
        )?;
        writeln!(
            w,
            "{:indent$}capabilities_pointer: 0x{:02x}",
            "",
            body.capabilities_pointer,
            indent = indent
        )?;
        writeln!(
            w,
            "{:indent$}interrupt_line: 0x{:02x}",
            "",
            body.interrupt_line,
            indent = indent
        )?;
        writeln!(
            w,
            "{:indent$}interrupt_pin: 0x{:02x}",
            "",
            body.interrupt_pin,
            indent = indent
        )?;
        writeln!(
            w,
            "{:indent$}min_grant: 0x{:02x}",
            "",
            body.min_grant,
            indent = indent
        )?;
        writeln!(
            w,
            "{:indent$}max_latency: 0x{:02x}",
            "",
            body.max_latency,
            indent = indent
        )?;

        Ok(())
    }
}
