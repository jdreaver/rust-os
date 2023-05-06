use core::fmt;
use core::fmt::Write;

use crate::strings::IndentWriter;

const MAX_PCI_BUS: u8 = 255;
const MAX_PCI_BUS_DEVICE: u8 = 31;
const MAX_PCI_BUS_DEVICE_FUNCTION: u8 = 7;

/// <https://wiki.osdev.org/PCI#.22Brute_Force.22_Scan>
///
/// NOTE: I think this would miss devices that are behind a PCI bridge, except
/// we are enumerating all buses, so maybe it is fine?
pub fn for_pci_devices_brute_force<F>(base_addr: u64, f: F)
where
    F: Fn(PCIeDeviceConfig),
{
    for bus in 0..=MAX_PCI_BUS {
        for slot in 0..=MAX_PCI_BUS_DEVICE {
            for function in 0..=MAX_PCI_BUS_DEVICE_FUNCTION {
                let device =
                    unsafe { PCIeDeviceConfig::new(base_addr as *const u8, bus, slot, function) };
                let Some(device) = device else { continue; };
                f(device);
            }
        }
    }
}

#[allow(clippy::doc_markdown)] // Clippy doesn't like PCIe
/// Interface into a PCI Express device's configuration space. See:
/// - <https://wiki.osdev.org/PCI_Express#Configuration_Space>
/// - Section 7.5 "7.5 PCI and PCIe Capabilities Required by the Base Spec for all Ports" of the PCI Express Base Specification
/// - <https://wiki.osdev.org/PCI>, which is the legacy interface, but is still a good explanation.
pub struct PCIeDeviceConfig {
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
    fn physical_address(&self) -> *mut u8 {
        self.header.0.cast::<u8>()
    }

    fn read_body(&self) -> Result<PCIDeviceConfigBody, &str> {
        let layout = self.header.header_type().header_layout()?;
        let body = match layout {
            PCIDeviceConfigHeaderLayout::GeneralDevice => {
                PCIDeviceConfigBody::GeneralDevice(unsafe {
                    PCIDeviceConfigBodyType0Ptr::from_config_base(self.physical_address())
                })
            }
            PCIDeviceConfigHeaderLayout::PCIToPCIBridge => PCIDeviceConfigBody::PCIToPCIBridge,
        };
        Ok(body)
    }

    pub fn print<W: Write>(&self, w: &mut W) -> fmt::Result {
        let w = &mut IndentWriter::new(w, 2);

        writeln!(w, "PCIe device config:")?;

        w.indent();
        writeln!(w, "Address: {:#?}", self.physical_address())?;
        writeln!(w, "Bus number: {}", self.bus_number)?;
        writeln!(w, "Device number: {}", self.device_number)?;
        writeln!(w, "Function number: {}", self.function_number)?;
        writeln!(w, "Header:")?;

        w.indent();
        self.header.print(w)?;
        w.unindent();

        let body = self.read_body().expect("failed to read PCI device body");
        match body {
            PCIDeviceConfigBody::GeneralDevice(body) => {
                writeln!(w, "General Device Body:")?;
                w.indent();
                body.print(w)?;
                w.unindent();
            }
            PCIDeviceConfigBody::PCIToPCIBridge => {
                writeln!(w, "Body: PCI to PCI bridge")?;
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
    header_type: PCIDeviceConfigHeaderType,
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

    fn header_type(self) -> PCIDeviceConfigHeaderType {
        self.as_ref().header_type
    }

    fn print<W: Write>(self, w: &mut IndentWriter<'_, W>) -> fmt::Result {
        let header = self.as_ref();

        let header_type = header.header_type;

        let layout = header_type
            .header_layout()
            .expect("couldn't construct header layout")
            .as_str();
        writeln!(w, "layout: {layout}")?;

        let multifunction = header_type.is_multifunction();
        writeln!(w, "multifunction: {multifunction}")?;

        let command = header.command;
        writeln!(w, "command: {command:#016b}")?;

        let status = header.status;
        writeln!(w, "status: {status:#016b}")?;

        let vendor_id = header.vendor_id;
        let vendor = lookup_vendor_id(vendor_id);
        write!(w, "vendor: {vendor_id:#x}")?;
        writeln!(w, " ({})", vendor.unwrap_or("UNKNOWN"))?;

        let device_id = header.device_id;
        let device = lookup_known_device_id(vendor_id, device_id);
        let revision_id = header.revision_id;
        write!(w, "device_id: {device_id:#x}")?;
        write!(w, ", revision_id: {revision_id:#x}")?;
        writeln!(w, " ({device})")?;

        let device = device_type(header.class, header.subclass, header.prog_if)
            .expect("couldn't construct device class");
        writeln!(w, "device:")?;
        w.indent();
        writeln!(w, "name: {device}")?;
        writeln!(w, "class: {:#x}", header.class)?;
        writeln!(w, "subclass: {:#x}", header.subclass,)?;
        writeln!(w, "prog_if: {:#x}", header.prog_if)?;
        w.unindent();

        Ok(())
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct PCIDeviceConfigHeaderType(u8);

impl PCIDeviceConfigHeaderType {
    /// The layout is in the first 7 bits of the Header Type register.
    fn header_layout(self) -> Result<PCIDeviceConfigHeaderLayout, &'static str> {
        match self.0 & 0x7 {
            0x00 => Ok(PCIDeviceConfigHeaderLayout::GeneralDevice),
            0x01 => Ok(PCIDeviceConfigHeaderLayout::PCIToPCIBridge),
            // 0x02 => Ok(PCIDeviceConfigHeaderType::PCIToCardBusBridge),
            _ => Err("invalid PCI device header type"),
        }
    }

    /// If the 8th bit of the Header Type register is set, the device is a
    /// multifunction device.
    fn is_multifunction(self) -> bool {
        self.0 & 0x80 != 0
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
struct PCIDeviceConfigBodyType0Ptr {
    config_base_ptr: *mut u8,
    ptr: *mut PCIDeviceConfigBodyType0,
}

impl PCIDeviceConfigBodyType0Ptr {
    unsafe fn from_config_base(config_base_ptr: *mut u8) -> Self {
        let ptr = config_base_ptr
            .add(core::mem::size_of::<PCIDeviceConfigHeader>())
            .cast::<PCIDeviceConfigBodyType0>();

        Self {
            config_base_ptr,
            ptr,
        }
    }

    fn as_ref(&self) -> &'static PCIDeviceConfigBodyType0 {
        unsafe { &*self.ptr }
    }

    fn print<W: Write>(self, w: &mut IndentWriter<'_, W>) -> fmt::Result {
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

        writeln!(w, "bar0: 0x{bar0:08x}")?;
        writeln!(w, "bar1: 0x{bar1:08x}")?;
        writeln!(w, "bar2: 0x{bar2:08x}")?;
        writeln!(w, "bar3: 0x{bar3:08x}")?;
        writeln!(w, "bar4: 0x{bar4:08x}")?;
        writeln!(w, "bar5: 0x{bar5:08x}")?;
        writeln!(w, "cardbus_cis_pointer: 0x{cardbus_cis_pointer:08x}")?;
        writeln!(w, "subsystem_vendor_id: 0x{subsystem_vendor_id:04x}")?;
        writeln!(w, "subsystem_id: 0x{subsystem_id:04x}")?;
        writeln!(
            w,
            "expansion_rom_base_address: 0x{expansion_rom_base_address:08x}"
        )?;
        writeln!(
            w,
            "capabilities_pointer: 0x{:02x}",
            body.capabilities_pointer,
        )?;
        writeln!(w, "interrupt_line: 0x{:02x}", body.interrupt_line,)?;
        writeln!(w, "interrupt_pin: 0x{:02x}", body.interrupt_pin,)?;
        writeln!(w, "min_grant: 0x{:02x}", body.min_grant,)?;
        writeln!(w, "max_latency: 0x{:02x}", body.max_latency,)?;

        let cap_ptr = unsafe {
            PCIDeviceCapabilityHeaderPtr::new(self.config_base_ptr, body.capabilities_pointer)
        };
        if let Some(cap_ptr) = cap_ptr {
            writeln!(w, "Capability Headers:")?;

            w.indent();
            let cap_iter = PCIDeviceCapabilityIterator::new(cap_ptr);
            for (i, capability_header) in cap_iter.enumerate() {
                let capability_header = capability_header.as_ref();
                let id = capability_header.id;
                let next = capability_header.next;
                writeln!(
                    w,
                    "Capability Header {i}: id: {id:#x}, next_offset: {next:#x}"
                )?;
            }
            w.unindent();
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct PCIDeviceCapabilityHeaderPtr {
    config_region_base: *mut u8,
    ptr: *mut PCIDeviceCapabilityHeader,
}

impl PCIDeviceCapabilityHeaderPtr {
    /// Construct a new `PCIDeviceCapabilityHeaderPtr` from the given
    /// `config_region_base` and `offset`. The offset must be from the device
    /// configuration header. Returns `None` if the offset is 0.
    ///
    /// # Safety
    ///
    /// Both `config_region_base` and `offset` must be valid.
    unsafe fn new(config_region_base: *mut u8, offset: u8) -> Option<Self> {
        if offset == 0 {
            return None;
        }

        Some(Self {
            config_region_base,
            ptr: config_region_base
                .add(offset as usize)
                .cast::<PCIDeviceCapabilityHeader>(),
        })
    }

    fn as_ref(&self) -> &'static PCIDeviceCapabilityHeader {
        unsafe { &*self.ptr }
    }

    fn next_capability(&self) -> Option<Self> {
        unsafe { Self::new(self.config_region_base, self.as_ref().next) }
    }
}

#[repr(packed)]
#[derive(Debug, Clone, Copy)]
struct PCIDeviceCapabilityHeader {
    id: u8,
    next: u8,
}

#[derive(Debug)]
struct PCIDeviceCapabilityIterator {
    ptr: Option<PCIDeviceCapabilityHeaderPtr>,
}

impl PCIDeviceCapabilityIterator {
    fn new(ptr: PCIDeviceCapabilityHeaderPtr) -> Self {
        Self { ptr: Some(ptr) }
    }
}

impl Iterator for PCIDeviceCapabilityIterator {
    type Item = PCIDeviceCapabilityHeaderPtr;

    fn next(&mut self) -> Option<Self::Item> {
        self.ptr = self.ptr.and_then(|ptr| ptr.next_capability());
        self.ptr
    }
}
