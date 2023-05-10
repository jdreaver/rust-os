use core::fmt;

use bitfield_struct::bitfield;
use x86_64::PhysAddr;

use crate::register_struct;
use crate::registers::{RegisterRO, RegisterRW};

const MAX_PCI_BUS: u8 = 255;
const MAX_PCI_BUS_DEVICE: u8 = 31;
const MAX_PCI_BUS_DEVICE_FUNCTION: u8 = 7;

/// <https://wiki.osdev.org/PCI#.22Brute_Force.22_Scan>
///
/// NOTE: I think this would miss devices that are behind a PCI bridge, except
/// we are enumerating all buses, so maybe it is fine?
pub fn for_pci_devices_brute_force<F>(base_addr: PhysAddr, mut f: F)
where
    F: FnMut(PCIDeviceConfig),
{
    for bus in 0..=MAX_PCI_BUS {
        for slot in 0..=MAX_PCI_BUS_DEVICE {
            for function in 0..=MAX_PCI_BUS_DEVICE_FUNCTION {
                let location = PCIDeviceLocation {
                    ecam_base_address: base_addr,
                    bus_number: bus,
                    device_number: slot,
                    function_number: function,
                };
                let config = unsafe { PCIDeviceConfig::new(location) };
                let Some(config) = config else { continue; };
                f(config);
            }
        }
    }
}

/// Location within the PCI Express Enhanced Configuration Mechanism memory
/// region. See "7.2.2 PCI Express Enhanced Configuration Access Mechanism
/// (ECAM)" of the PCI Express Base Specification, as well as
/// <https://wiki.osdev.org/PCI_Express>.
#[derive(Debug, Clone, Copy)]
pub struct PCIDeviceLocation {
    /// Physical address where the PCI Express extended configuration mechanism
    /// memory region starts for this device.
    ecam_base_address: PhysAddr,

    /// Which PCIe bus this device is on.
    bus_number: u8,

    /// Device number of the device within the bus.
    device_number: u8,

    /// Function number of the device if the device is a multifunction device.
    function_number: u8,
}

impl PCIDeviceLocation {
    pub fn device_base_address(&self) -> PhysAddr {
        let bus = u64::from(self.bus_number);
        let device = u64::from(self.device_number);
        let function = u64::from(self.function_number);
        self.ecam_base_address + ((bus << 20) | (device << 15) | (function << 12))
    }
}

register_struct!(
    /// See <https://wiki.osdev.org/PCI#Common_Header_Fields> and "7.5.1.1 Type
    /// 0/1 Common Configuration Space" in spec
    PCIDeviceCommonConfigRegisters {
        // N.B. vendor_id and device_id are in a separate struct.

        0x04 => command: RegisterRW<PCIDeviceConfigCommand>,
        0x06 => status: RegisterRW<PCIDeviceConfigStatus>,

        // N.B. revision_id, class, subclass, and prog_if are in a separate struct.

        0x0C => cache_line_size: RegisterRW<u8>,
        0x0D => latency_timer: RegisterRW<u8>,
        0x0E => header_type: RegisterRO<PCIDeviceConfigHeaderType>,
        0x0F => bist: RegisterRW<u8>,

        // Tons of padding for type-specific fields

        0x34 => capabilities_pointer: RegisterRO<u8>,
        0x3C => interrupt_line: RegisterRW<u8>,
        0x3D => interrupt_pin: RegisterRO<u8>,
    }
);

register_struct!(
    /// See <https://wiki.osdev.org/PCI#Common_Header_Fields> and "7.5.1.1 Type
    /// 0/1 Common Configuration Space" in spec
    PCIConfigDeviceIDRegisters {
        0x00 => vendor_id: RegisterRO<u16>,
        0x02 => device_id: RegisterRO<u16>,

        0x08 => revision_id: RegisterRO<u8>,
        0x09 => prog_if: RegisterRO<u8>,
        0x0A => subclass: RegisterRO<u8>,
        0x0B => class: RegisterRO<u8>,
    }
);

#[derive(Debug, Clone, Copy)]
pub struct PCIDeviceConfig {
    location: PCIDeviceLocation,

    /// All PCI devices share some common configuration. See
    /// <https://wiki.osdev.org/PCI#Common_Header_Fields> and "7.5.1.1 Type 0/1
    /// Common Configuration Space" in spec
    device_id: PCIConfigDeviceID,
    common_registers: PCIDeviceCommonConfigRegisters,
}

impl PCIDeviceConfig {
    /// Returns `Some` if a device exists at the given location.
    ///
    /// # Safety
    ///
    /// Caller must ensure that `base_address` is a valid pointer to a PCI
    /// Express extended configuration mechanism memory region.
    unsafe fn new(location: PCIDeviceLocation) -> Option<Self> {
        let address = location.device_base_address().as_u64() as usize;
        #[allow(unused_unsafe)]
        // If the vendor ID is 0xFFFF, then there is no device at this location.
        let device_id = unsafe { PCIConfigDeviceID::new(location) };
        if device_id.registers.vendor_id().read() == 0xFFFF {
            return None;
        }

        let common_registers = unsafe { PCIDeviceCommonConfigRegisters::from_address(address) };

        Some(Self {
            location,
            device_id,
            common_registers,
        })
    }

    pub fn device_id(self) -> PCIConfigDeviceID {
        self.device_id
    }

    pub fn common_registers(self) -> PCIDeviceCommonConfigRegisters {
        self.common_registers
    }

    pub fn config_type(&self) -> Result<PCIDeviceConfigTypes, &str> {
        let layout = self.common_registers.header_type().read().layout()?;
        let body = match layout {
            PCIDeviceConfigHeaderLayout::GeneralDevice => {
                PCIDeviceConfigTypes::GeneralDevice(unsafe {
                    PCIDeviceConfigType0::from_common_config(*self)
                })
            }
            PCIDeviceConfigHeaderLayout::PCIToPCIBridge => PCIDeviceConfigTypes::PCIToPCIBridge,
        };
        Ok(body)
    }
}

#[derive(Clone, Copy)]
pub struct PCIConfigDeviceID {
    registers: PCIConfigDeviceIDRegisters,
}

impl PCIConfigDeviceID {
    unsafe fn new(location: PCIDeviceLocation) -> Self {
        let address = location.device_base_address().as_u64() as usize;
        let registers = PCIConfigDeviceIDRegisters::from_address(address);
        Self { registers }
    }

    pub fn registers(&self) -> PCIConfigDeviceIDRegisters {
        self.registers
    }

    pub fn known_vendor_id(&self) -> &'static str {
        let vendor_id = self.registers.vendor_id().read();
        lookup_vendor_id(vendor_id)
    }

    pub fn known_device_id(&self) -> &'static str {
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

/// See "7.5.1.1.3 Command Register (Offset 04h)" in the PCI Express Spec
#[bitfield(u16)]
pub struct PCIDeviceConfigCommand {
    io_space_enable: bool,
    memory_space_enable: bool,
    bus_master_enable: bool,
    special_cycle_enable: bool,
    memory_write_and_invalidate: bool,
    vga_palette_snoop: bool,
    parity_error_response: bool,
    idsel_stepping_wait_cycle_control: bool,
    serr_enable: bool,
    fast_back_to_back_transactions_enable: bool,
    interrupt_disable: bool,
    #[bits(5)]
    __: u8,
}

/// See "7.5.1.1.4 Status Register (Offset 06h)" in the PCI Express spec.
#[bitfield(u16)]
pub struct PCIDeviceConfigStatus {
    immediate_readiness: bool,
    #[bits(2)]
    __: u8,
    interrupt_status: bool,
    capabilities_list: bool,
    mhz_66_capable: bool,
    __: bool,
    fast_back_to_back_transactions_capable: bool,
    master_data_parity_error: bool,
    #[bits(2)]
    devsel_timing: u8,
    signaled_target_abort: bool,
    received_target_abort: bool,
    received_master_abort: bool,
    signaled_system_error: bool,
    detected_parity_error: bool,
}

/// 7.5.1.1.9 Header Type Register (Offset 0Eh)
#[bitfield(u8, debug = false)]
pub struct PCIDeviceConfigHeaderType {
    #[bits(2)]
    raw_layout: u8,

    #[bits(5)]
    _reserved: u8,

    multifunction: bool,
}

impl PCIDeviceConfigHeaderType {
    /// The layout is in the first 7 bits of the Header Type register.
    fn layout(self) -> Result<PCIDeviceConfigHeaderLayout, &'static str> {
        match self.0 & 0x7 {
            0x00 => Ok(PCIDeviceConfigHeaderLayout::GeneralDevice),
            0x01 => Ok(PCIDeviceConfigHeaderLayout::PCIToPCIBridge),
            // 0x02 => Ok(PCIDeviceConfigHeaderType::PCIToCardBusBridge),
            _ => Err("invalid PCI device header type"),
        }
    }
}

impl fmt::Debug for PCIDeviceConfigHeaderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let layout = self.layout();
        let layout_str = layout.map_or("INVALID", |layout| layout.as_str());
        let raw_layout = self.raw_layout();

        f.debug_struct("PCIConfigHeaderType")
            .field("layout", &format_args!("{raw_layout:#x} ({layout_str})"))
            .field("multifunction", &self.multifunction())
            .finish()
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

#[derive(Clone, Copy)]
pub enum PCIDeviceConfigTypes {
    GeneralDevice(PCIDeviceConfigType0),
    PCIToPCIBridge,
    // N.B. PCIToCardBusBridge doesn't exist any longer in PCI Express. Let's
    // just pretend it never existed.
    // PCIToCardBusBridge,
}

register_struct!(
    /// 7.5.1.2 Type 0 Configuration Space Header
    PCIDeviceConfigType0Registers {
        // N.B. Base address is for the entire configuration block (that is, the
        // base of the common configuration), not just for the type 0 registers.
        0x10 => raw_bar0: RegisterRW<u32>,
        0x14 => raw_bar1: RegisterRW<u32>,
        0x18 => raw_bar2: RegisterRW<u32>,
        0x1C => raw_bar3: RegisterRW<u32>,
        0x20 => raw_bar4: RegisterRW<u32>,
        0x24 => raw_bar5: RegisterRW<u32>,
        0x28 => cardbus_cis_pointer: RegisterRW<u32>,
        0x2C => subsystem_vendor_id: RegisterRW<u16>,
        0x2E => subsystem_id: RegisterRW<u16>,
        0x30 => expansion_rom_base_address: RegisterRW<u32>,
        0x34 => capabilities_pointer: RegisterRW<u8>,
        // 7 bytes reserved
        0x3C => interrupt_line: RegisterRW<u8>,
        0x3D => interrupt_pin: RegisterRW<u8>,
        0x3E => min_grant: RegisterRW<u8>,
        0x3F => max_latency: RegisterRW<u8>,
    }
);

#[derive(Clone, Copy)]
pub struct PCIDeviceConfigType0 {
    common_config: PCIDeviceConfig,
    registers: PCIDeviceConfigType0Registers,
}

impl PCIDeviceConfigType0 {
    unsafe fn from_common_config(common_config: PCIDeviceConfig) -> Self {
        let address = common_config.location.device_base_address().as_u64() as usize;
        Self {
            common_config,
            registers: PCIDeviceConfigType0Registers::from_address(address),
        }
    }

    pub fn iter_capabilities(self) -> PCIDeviceCapabilityIterator {
        // Check if the device even has capabilities.
        let has_caps = self
            .common_config
            .common_registers()
            .status()
            .read()
            .capabilities_list();
        if !has_caps {
            return PCIDeviceCapabilityIterator::new(None);
        }

        let cap_ptr = unsafe {
            PCIDeviceCapabilityHeader::new(
                self.registers.address,
                self.registers.capabilities_pointer().read(),
            )
        };
        PCIDeviceCapabilityIterator::new(cap_ptr)
    }

    fn bar_addresses(self) -> BARAddresses<6> {
        BARAddresses {
            bars: [
                self.registers.raw_bar0().read(),
                self.registers.raw_bar1().read(),
                self.registers.raw_bar2().read(),
                self.registers.raw_bar3().read(),
                self.registers.raw_bar4().read(),
                self.registers.raw_bar5().read(),
            ],
        }
    }

    pub fn bar(&self, bar_idx: usize) -> BARAddress {
        let bar_addresses = self.bar_addresses().interpreted();
        let bar_address = bar_addresses
            .get(bar_idx)
            .expect("invalid PCI device BAR index");
        bar_address.unwrap_or_else(|| panic!("failed to get BAR address, perhaps you tried to index the second half of a 64 bit BAR?"))
    }
}

impl fmt::Debug for PCIDeviceConfigType0 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let caps = PCIDeviceConfigType0Capabilities(self.iter_capabilities());

        f.debug_struct("PCIDeviceConfigType0")
            .field("BARs", &self.bar_addresses())
            .field("registers", &self.registers)
            // TODO: Don't print capabilities list as part of debugging this.
            .field("capabilities", &caps)
            .finish()
    }
}

/// Used just to help implement Debug for PCIDeviceConfigType0.
///
/// TODO: This is a hack. Don't do this.
struct PCIDeviceConfigType0Capabilities(PCIDeviceCapabilityIterator);

impl fmt::Debug for PCIDeviceConfigType0Capabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let iter = self.0.clone();
        f.debug_list().entries(iter).finish()
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

pub struct BARAddresses<const N: usize> {
    pub bars: [u32; N],
}

impl<const N: usize> BARAddresses<N> {
    /// Interprets the BAR addresses into `BARAddress`es. This is a bit non-trivial
    /// because adjacent BAR addresses can be part of the same 64 bit address, so we
    /// can't just look at them 1 by 1.
    fn interpreted(&self) -> [Option<BARAddress>; N] {
        let bars = self.bars;
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
                    let next_bar =
                        next_bar.expect("got 64 bit address BAR, but there is no next BAR");
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
}

impl<const N: usize> fmt::Debug for BARAddresses<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut bar_list = f.debug_list();
        for (i, bar) in self.interpreted().iter().enumerate() {
            let Some(bar) = bar else { continue; };

            match bar {
                BARAddress::Mem32Bit {
                    address,
                    prefetchable,
                } => {
                    let prefetch = if *prefetchable { " (prefetchable)" } else { "" };
                    bar_list.entry(&format_args!(
                        "BAR{i}: 32-bit memory at 0x{address:x}{prefetch}"
                    ));
                }
                BARAddress::Mem64Bit {
                    address,
                    prefetchable,
                } => {
                    let prefetch = if *prefetchable { " (prefetchable)" } else { "" };
                    bar_list.entry(&format_args!(
                        "BAR{i}: 64-bit memory at 0x{address:x}{prefetch}"
                    ));
                }
                BARAddress::IO(address) => {
                    bar_list.entry(&format_args!("BAR{i} I/O at 0x{address:x}"));
                }
            }
        }
        bar_list.finish()?;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PCIDeviceCapabilityHeader {
    config_base_address: usize,
    registers: PCIDeviceCapabilityHeaderRegisters,
}

impl PCIDeviceCapabilityHeader {
    /// Construct a new `PCIDeviceCapabilityHeaderPtr` from the given
    /// `config_region_base` and `offset`. The offset must be from the device
    /// configuration header. Returns `None` if the offset is 0.
    ///
    /// # Safety
    ///
    /// Both `config_region_base` and `offset` must be valid.
    unsafe fn new(config_base_address: usize, offset: u8) -> Option<Self> {
        if offset == 0 {
            return None;
        }

        let address = config_base_address + usize::from(offset);
        let registers = PCIDeviceCapabilityHeaderRegisters::from_address(address);

        Some(Self {
            config_base_address,
            registers,
        })
    }

    pub fn address(&self) -> usize {
        self.registers.address
    }

    /// Vendor-specific capability headers have an ID of 0x09.
    pub fn is_vendor_specific(&self) -> bool {
        self.registers.id().read() == 0x09
    }

    fn next_capability(&self) -> Option<Self> {
        unsafe { Self::new(self.config_base_address, self.registers.next().read()) }
    }
}

register_struct!(
    PCIDeviceCapabilityHeaderRegisters {
        0x0 => id: RegisterRO<u8>,
        0x1 => next: RegisterRO<u8>,
    }
);

#[derive(Debug, Clone)]
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
