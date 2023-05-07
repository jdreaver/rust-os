use core::fmt;
use core::fmt::Write;

use x86_64::PhysAddr;

use crate::strings::IndentWriter;

const MAX_PCI_BUS: u8 = 255;
const MAX_PCI_BUS_DEVICE: u8 = 31;
const MAX_PCI_BUS_DEVICE_FUNCTION: u8 = 7;

/// <https://wiki.osdev.org/PCI#.22Brute_Force.22_Scan>
///
/// NOTE: I think this would miss devices that are behind a PCI bridge, except
/// we are enumerating all buses, so maybe it is fine?
pub fn for_pci_devices_brute_force<F>(base_addr: PhysAddr, mut f: F)
where
    F: FnMut(PCIeDeviceConfig),
{
    for bus in 0..=MAX_PCI_BUS {
        for slot in 0..=MAX_PCI_BUS_DEVICE {
            for function in 0..=MAX_PCI_BUS_DEVICE_FUNCTION {
                let device = unsafe { PCIeDeviceConfig::new(base_addr, bus, slot, function) };
                let Some(device) = device else { continue; };
                f(device);
            }
        }
    }
}

/// Interface into a PCI Express device's configuration space. See:
/// - <https://wiki.osdev.org/PCI_Express#Configuration_Space>
/// - Section 7.5 "7.5 PCI and PCIe Capabilities Required by the Base Spec for all Ports" of the PCI Express Base Specification
/// - <https://wiki.osdev.org/PCI>, which is the legacy interface, but is still a good explanation.
pub struct PCIeDeviceConfig {
    /// Physical address where the PCI Express extended configuration mechanism
    /// memory region starts for this device.
    physical_address: PhysAddr,

    /// Which PCIe bus this device is on.
    bus_number: u8,

    /// Device number of the device within the bus.
    device_number: u8,

    /// Function number of the device if the device is a multifunction device.
    function_number: u8,

    /// All PCI/PCIe devices have a common header field that lives at the base
    /// of the device's configuration space.
    header: PCIDeviceConfigHeader,
}

impl PCIeDeviceConfig {
    /// Returns `Some` if a device exists at the given location.
    ///
    /// # Safety
    ///
    /// Caller must ensure that `base_address` is a valid pointer to a PCI
    /// Express extended configuration mechanism memory region.
    unsafe fn new(
        enhanced_config_region_address: PhysAddr,
        bus_number: u8,
        device_number: u8,
        function_number: u8,
    ) -> Option<Self> {
        let bus = u64::from(bus_number);
        let device = u64::from(device_number);
        let function = u64::from(function_number);
        let physical_address =
            enhanced_config_region_address + ((bus << 20) | (device << 15) | (function << 12));

        let header = PCIDeviceConfigHeader::new(physical_address);

        if !header.device_exists() {
            return None;
        }

        Some(Self {
            physical_address,
            bus_number,
            device_number,
            function_number,
            header,
        })
    }

    pub fn body(&self) -> Result<PCIDeviceConfigBody, &str> {
        let layout = self.header.header_type().header_layout()?;
        let body = match layout {
            PCIDeviceConfigHeaderLayout::GeneralDevice => {
                PCIDeviceConfigBody::GeneralDevice(unsafe {
                    PCIDeviceConfigBodyType0::from_config_base(self.physical_address)
                })
            }
            PCIDeviceConfigHeaderLayout::PCIToPCIBridge => PCIDeviceConfigBody::PCIToPCIBridge,
        };
        Ok(body)
    }

    pub fn header(&self) -> PCIDeviceConfigHeader {
        self.header
    }

    pub fn print<W: Write>(&self, w: &mut W) -> fmt::Result {
        let w = &mut IndentWriter::new(w, 2);

        writeln!(w, "PCIe device config:")?;

        w.indent();
        writeln!(w, "Address: {:#x}", self.physical_address.as_u64())?;
        writeln!(w, "Bus number: {}", self.bus_number)?;
        writeln!(w, "Device number: {}", self.device_number)?;
        writeln!(w, "Function number: {}", self.function_number)?;
        writeln!(w, "Header:")?;

        w.indent();
        self.header.print(w)?;
        w.unindent();

        let body = self.body().expect("failed to read PCI device body");
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
pub enum PCIDeviceConfigBody {
    GeneralDevice(PCIDeviceConfigBodyType0),
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
pub struct PCIDeviceConfigHeaderRaw {
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

#[derive(Debug, Clone, Copy)]
pub struct PCIDeviceConfigHeader {
    address: PhysAddr,
}

impl PCIDeviceConfigHeader {
    fn new(address: PhysAddr) -> Self {
        Self { address }
    }

    /// A device exists if the Vendor ID register is not 0xFFFF.
    fn device_exists(self) -> bool {
        self.as_ref().vendor_id != 0xFFFF
    }

    fn header_type(self) -> PCIDeviceConfigHeaderType {
        self.as_ref().header_type
    }

    pub fn vendor_id(self) -> u16 {
        self.as_ref().vendor_id
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

impl AsRef<PCIDeviceConfigHeaderRaw> for PCIDeviceConfigHeader {
    fn as_ref(&self) -> &PCIDeviceConfigHeaderRaw {
        let ptr = self.address.as_u64() as *const PCIDeviceConfigHeaderRaw;
        unsafe { &*ptr }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PCIDeviceConfigHeaderType(u8);

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
pub enum PCIDeviceConfigHeaderLayout {
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
pub enum PCIDeviceClass {
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
pub struct PCIDeviceConfigBodyType0Raw {
    bars: [u32; 6],
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

#[derive(Debug, Clone, Copy)]
pub struct PCIDeviceConfigBodyType0 {
    /// Address of the PCI device configuration base (not for the body, but for
    /// the base of the whole config).
    config_base_address: PhysAddr,

    /// Address for the device's configuration body (so not including the
    /// header).
    address: PhysAddr,
}

impl PCIDeviceConfigBodyType0 {
    unsafe fn from_config_base(config_base_address: PhysAddr) -> Self {
        let address = config_base_address + core::mem::size_of::<PCIDeviceConfigHeaderRaw>();
        Self {
            config_base_address,
            address,
        }
    }

    pub fn iter_capabilities(&self) -> PCIDeviceCapabilityIterator {
        let body = self.as_ref();
        let cap_ptr = unsafe {
            PCIDeviceCapabilityHeader::new(self.config_base_address, body.capabilities_pointer)
        };
        PCIDeviceCapabilityIterator::new(cap_ptr)
    }

    pub fn bar(self, bar_idx: usize) -> BARAddress {
        let bars = self.as_ref().bars;
        let bar_addresses = bar_addresses(bars);
        let bar_address = bar_addresses
            .get(bar_idx)
            .expect("invalid PCI device BAR index");
        bar_address.unwrap_or_else(|| panic!("failed to get BAR address, perhaps you tried to index the second half of a 64 bit BAR?"))
    }

    fn print<W: Write>(self, w: &mut IndentWriter<'_, W>) -> fmt::Result {
        let body = self.as_ref();

        let bars = body.bars;
        let cardbus_cis_pointer = body.cardbus_cis_pointer;
        let subsystem_vendor_id = body.subsystem_vendor_id;
        let subsystem_id = body.subsystem_id;
        let expansion_rom_base_address = body.expansion_rom_base_address;

        let bar_addresses = bar_addresses(bars);
        for (i, bar_address) in bar_addresses.iter().enumerate() {
            match bar_address {
                Some(BARAddress::Mem32Bit {
                    address,
                    prefetchable,
                }) => {
                    let prefetch = if *prefetchable { " (prefetchable)" } else { "" };
                    writeln!(w, "BAR{i}: 32-bit memory at 0x{address:x}{prefetch}")?;
                }
                Some(BARAddress::Mem64Bit {
                    address,
                    prefetchable,
                }) => {
                    let prefetch = if *prefetchable { " (prefetchable)" } else { "" };
                    writeln!(w, "BAR{i}: 64-bit memory at 0x{address:x}{prefetch}")?;
                }
                Some(BARAddress::IO(address)) => {
                    writeln!(w, "BAR{i}: I/O at 0x{address:x}")?;
                }
                None => {
                    continue;
                }
            }
        }
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

        writeln!(w, "Capability Headers:")?;
        w.indent();
        for (i, capability_header) in self.iter_capabilities().enumerate() {
            let capability_header = capability_header.as_ref();
            let id = capability_header.id;
            let next = capability_header.next;
            writeln!(
                w,
                "Capability Header {i}: id: {id:#x}, next_offset: {next:#x}"
            )?;
        }
        w.unindent();

        Ok(())
    }
}

#[derive(Debug, Copy, Clone)]
pub enum BARAddress {
    /// 32-bit BAR address. Uses a single BAR register.
    Mem32Bit { address: u32, prefetchable: bool },

    /// 64-bit BAR address. Uses a single BAR register.
    Mem64Bit { address: u64, prefetchable: bool },

    /// I/O BAR address. Uses a single BAR register.
    IO(u32),
}

/// Interprets the BAR addresses into `BARAddress`es. This is a bit non-trivial
/// because adjacent BAR addresses can be part of the same 64 bit address, so we
/// can't just look at them 1 by 1.
fn bar_addresses<const N: usize>(bars: [u32; N]) -> [Option<BARAddress>; N] {
    let mut addresses = [None; N];

    let mut i = 0;
    while i < bars.len() {
        let bar = bars[i];
        if bar == 0 {
            // This BAR is not implemented.
            i += 1;
            continue;
        }

        let next_bar = bars.get(i + 1).copied();

        let bit_0 = bar & 0b1;

        let bit_1_2 = (bar >> 1) & 0b11;
        let bit_3 = (bar >> 3) & 0b1;
        match (bit_0, bit_1_2) {
            (0b0, 0b00) => {
                // 32-bit address
                let address = bar & 0xffff_fff0;
                let prefetchable = bit_3 == 0b1;
                addresses[i] = Some(BARAddress::Mem32Bit {
                    address,
                    prefetchable,
                });
                i += 1;
            }
            (0b0, 0b10) => {
                // 64-bit address. Use the next BAR as well for the upper 32 bits.
                let next_bar = next_bar.expect("got 64 bit address BAR, but there is no next BAR");
                let address = (u64::from(next_bar) << 32) | u64::from(bar) & 0xffff_fff0;
                let prefetchable = bit_3 == 0b1;
                addresses[i] = Some(BARAddress::Mem64Bit {
                    address,
                    prefetchable,
                });

                // This address is being used by the 64-bit BAR, so we shouldn't
                // try to interpret it on its own.
                addresses[i + 1] = None;
                i += 2;
            }
            (0b1, _) => {
                // I/O address
                let addr = bar & 0xffff_fffc;
                addresses[i] = Some(BARAddress::IO(addr));
                i += 1;
            }
            _ => panic!("invalid BAR address configuration bits"),
        }
    }

    addresses
}

impl AsRef<PCIDeviceConfigBodyType0Raw> for PCIDeviceConfigBodyType0 {
    fn as_ref(&self) -> &PCIDeviceConfigBodyType0Raw {
        let ptr = self.address.as_u64() as *const PCIDeviceConfigBodyType0Raw;
        unsafe { &*ptr }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PCIDeviceCapabilityHeader {
    config_base_address: PhysAddr,
    address: PhysAddr,
}

impl PCIDeviceCapabilityHeader {
    /// Construct a new `PCIDeviceCapabilityHeaderPtr` from the given
    /// `config_region_base` and `offset`. The offset must be from the device
    /// configuration header. Returns `None` if the offset is 0.
    ///
    /// # Safety
    ///
    /// Both `config_region_base` and `offset` must be valid.
    unsafe fn new(config_base_address: PhysAddr, offset: u8) -> Option<Self> {
        if offset == 0 {
            return None;
        }

        let address = config_base_address + u64::from(offset);

        Some(Self {
            config_base_address,
            address,
        })
    }

    pub fn address(&self) -> PhysAddr {
        self.address
    }

    fn next_capability(&self) -> Option<Self> {
        unsafe { Self::new(self.config_base_address, self.as_ref().next) }
    }
}

impl AsRef<PCIDeviceCapabilityHeaderRaw> for PCIDeviceCapabilityHeader {
    fn as_ref(&self) -> &PCIDeviceCapabilityHeaderRaw {
        let ptr = self.address.as_u64() as *const PCIDeviceCapabilityHeaderRaw;
        unsafe { &*ptr }
    }
}

#[repr(packed)]
#[derive(Debug, Clone, Copy)]
pub struct PCIDeviceCapabilityHeaderRaw {
    id: u8,
    next: u8,
}

#[derive(Debug)]
pub struct PCIDeviceCapabilityIterator {
    ptr: Option<PCIDeviceCapabilityHeader>,
}

impl PCIDeviceCapabilityIterator {
    fn new(ptr: Option<PCIDeviceCapabilityHeader>) -> Self {
        Self { ptr }
    }
}

impl Iterator for PCIDeviceCapabilityIterator {
    type Item = PCIDeviceCapabilityHeader;

    fn next(&mut self) -> Option<Self::Item> {
        self.ptr = self.ptr.and_then(|ptr| ptr.next_capability());
        self.ptr
    }
}
