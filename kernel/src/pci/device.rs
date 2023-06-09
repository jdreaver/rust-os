use core::fmt;

use bitfield_struct::bitfield;
use x86_64::PhysAddr;

use crate::memory::{KernPhysAddr, Page, PageRange, PageSize, PageTableEntryFlags};
use crate::registers::{RegisterRO, RegisterRW};
use crate::{memory, register_struct};

use super::capabilities::{
    MSIXCapability, MSIXTable, PCIDeviceCapabilityHeader, PCIDeviceCapabilityIterator, MSIXPBA,
};
use super::device_id::PCIConfigDeviceID;
use super::location::PCIDeviceLocation;
use super::{MSIXTableEntry, PCIDeviceCapability};

const MAX_PCI_BUS: u8 = 255;
const MAX_PCI_BUS_DEVICE: u8 = 31;
const MAX_PCI_BUS_DEVICE_FUNCTION: u8 = 7;

/// <https://wiki.osdev.org/PCI#.22Brute_Force.22_Scan>
///
/// NOTE: I think this would miss devices that are behind a PCI bridge, except
/// we are enumerating all buses, so maybe it is fine?
pub(crate) fn for_pci_devices_brute_force<F>(base_addr: KernPhysAddr, mut f: F)
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

register_struct!(
    /// See <https://wiki.osdev.org/PCI#Common_Header_Fields> and "7.5.1.1 Type
    /// 0/1 Common Configuration Space" in spec
    pub(crate) PCIDeviceCommonConfigRegisters {
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

#[derive(Clone)]
pub(crate) struct PCIDeviceConfig {
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
        #[allow(unused_unsafe)]
        // If the vendor ID is 0xFFFF, then there is no device at this location.
        let device_id = unsafe { PCIConfigDeviceID::new(&location) };
        if device_id.registers().vendor_id().read() == 0xFFFF {
            return None;
        }

        let common_registers =
            unsafe { PCIDeviceCommonConfigRegisters::from_address(location.device_base_address()) };

        Some(Self {
            location,
            device_id,
            common_registers,
        })
    }

    pub(crate) fn device_id(&self) -> &PCIConfigDeviceID {
        &self.device_id
    }

    pub(crate) fn common_registers(&self) -> PCIDeviceCommonConfigRegisters {
        self.common_registers
    }

    pub(crate) fn config_type(&self) -> Result<PCIDeviceConfigTypes, &str> {
        let layout = self.common_registers.header_type().read().layout()?;
        let body = match layout {
            PCIDeviceConfigHeaderLayout::GeneralDevice => {
                PCIDeviceConfigTypes::GeneralDevice(unsafe {
                    // TODO: Remove self.clone()
                    PCIDeviceConfigType0::from_common_config(self.clone())
                })
            }
            PCIDeviceConfigHeaderLayout::PCIToPCIBridge => PCIDeviceConfigTypes::PCIToPCIBridge,
        };
        Ok(body)
    }

    pub(crate) fn iter_capabilities(&self) -> PCIDeviceCapabilityIterator {
        // Check if the device even has capabilities.
        let has_caps = self.common_registers().status().read().capabilities_list();
        if !has_caps {
            return PCIDeviceCapabilityIterator::new(None);
        }

        let cap_ptr = unsafe {
            PCIDeviceCapabilityHeader::new(
                self.common_registers.address,
                self.common_registers.capabilities_pointer().read(),
            )
        };
        PCIDeviceCapabilityIterator::new(cap_ptr)
    }
}

impl fmt::Debug for PCIDeviceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PCIDeviceConfig")
            .field("location", &self.location)
            .field("device_id", &self.device_id)
            .field("common_registers", &self.common_registers)
            // TODO: Don't print capabilities list as part of debugging this.
            .field("capabilities", &self.iter_capabilities())
            .finish()
    }
}

/// See "7.5.1.1.3 Command Register (Offset 04h)" in the PCI Express Spec
#[bitfield(u16)]
pub(crate) struct PCIDeviceConfigCommand {
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
pub(crate) struct PCIDeviceConfigStatus {
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
pub(crate) struct PCIDeviceConfigHeaderType {
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
pub(crate) enum PCIDeviceConfigHeaderLayout {
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

#[derive(Clone)]
pub(crate) enum PCIDeviceConfigTypes {
    GeneralDevice(PCIDeviceConfigType0),
    PCIToPCIBridge,
    // N.B. PCIToCardBusBridge doesn't exist any longer in PCI Express. Let's
    // just pretend it never existed.
    // PCIToCardBusBridge,
}

register_struct!(
    /// 7.5.1.2 Type 0 Configuration Space Header
    pub(crate) PCIDeviceConfigType0Registers {
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
        // 0x34 => capabilities_pointer: RegisterRW<u8>, // In common config
        // 7 bytes reserved
        // 0x3C => interrupt_line: RegisterRW<u8>, // In common config
        // 0x3D => interrupt_pin: RegisterRW<u8>, // In common config
        0x3E => min_grant: RegisterRW<u8>,
        0x3F => max_latency: RegisterRW<u8>,
    }
);

#[derive(Clone)]
pub(crate) struct PCIDeviceConfigType0 {
    common_config: PCIDeviceConfig,
    registers: PCIDeviceConfigType0Registers,
}

impl PCIDeviceConfigType0 {
    unsafe fn from_common_config(common_config: PCIDeviceConfig) -> Self {
        let address = common_config.location.device_base_address();
        Self {
            common_config,
            registers: PCIDeviceConfigType0Registers::from_address(address),
        }
    }

    fn bar_addresses(&self) -> BARAddresses<6> {
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

    pub(crate) fn bar(&self, bar_idx: u8) -> BARAddress {
        let bar_addresses = self.bar_addresses().interpreted();
        let bar_address = bar_addresses
            .get(bar_idx as usize)
            .expect("invalid PCI device BAR index");
        bar_address.unwrap_or_else(|| panic!("failed to get BAR address, perhaps you tried to index the second half of a 64 bit BAR?"))
    }

    /// Capabilities often grab their configuration at a BAR address. The region
    /// pointed to by the BAR offset (often with some offset) has a certain
    /// size, and the region needs to be mapped in our page tables. This
    /// function identity maps the region pointed to by the desired BAR, and
    /// also returns the physical address of the desired start of the desired
    /// region.
    pub(crate) fn bar_region_physical_address(
        &self,
        bar_idx: u8,
        physical_offset: u32,
        region_size: u64,
    ) -> KernPhysAddr {
        let bar_phys_addr = match self.bar(bar_idx) {
            // TODO: Use the prefetchable field when doing mapping.
            BARAddress::Mem32Bit {
                address,
                prefetchable: _,
            } => PhysAddr::new(u64::from(address)),
            BARAddress::Mem64Bit {
                address,
                prefetchable: _,
            } => PhysAddr::new(address),
            BARAddress::IO(address) => panic!(
                "VirtIO capability uses I/O BAR (address: {:#x}), not supported",
                address,
            ),
        };
        let bar_phys_addr = KernPhysAddr::from(bar_phys_addr);

        // Need to identity map the BAR target page(s) so we can access them
        // without faults. Note that these addresses can be outside of physical
        // memory, in which case they are intercepted by the PCI bus and handled
        // by the device, so we aren't mapping physical RAM pages here, we are
        // just ensuring these addresses are identity mapped in the page table
        // so they don't fault.
        let config_start_addr = bar_phys_addr + u64::from(physical_offset);
        let start_page = Page::containing_address(config_start_addr, PageSize::Size4KiB);
        let pages = PageRange::from_num_bytes(start_page, region_size as usize);
        let flags = PageTableEntryFlags::PRESENT | PageTableEntryFlags::WRITABLE;
        memory::with_kernel_page_table_lock(|table| {
            memory::identity_map_physical_pages(table, pages.iter(), flags)
                .expect("failed to identity map PCI BAR frame");
        });

        config_start_addr
    }

    pub(crate) fn msix_config(&self) -> Option<MSIXConfig> {
        for cap in self.common_config.iter_capabilities() {
            if let PCIDeviceCapability::MSIX(msix_cap) = cap.capability() {
                return Some(MSIXConfig::new(self, msix_cap));
            }
        }
        None
    }
}

#[allow(clippy::missing_fields_in_debug)]
impl fmt::Debug for PCIDeviceConfigType0 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PCIDeviceConfigType0")
            .field("BARs", &self.bar_addresses())
            .field("registers", &self.registers)
            .finish()
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum BARAddress {
    /// 32-bit BAR address. Uses a single BAR register.
    Mem32Bit { address: u32, prefetchable: bool },

    /// 64-bit BAR address. Uses a single BAR register.
    Mem64Bit { address: u64, prefetchable: bool },

    /// I/O BAR address. Uses a single BAR register.
    IO(u32),
}

pub(crate) struct BARAddresses<const N: usize> {
    pub(crate) bars: [u32; N],
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

/// Contains all of the different bits of MSI-X configuration for a device,
/// including the header, the MSI-X table, and the MSI-X PBA.
#[derive(Debug)]
pub(crate) struct MSIXConfig {
    capability: MSIXCapability,
    table: MSIXTable,
    _pba: MSIXPBA,
}

impl MSIXConfig {
    pub(super) fn new(device_config: &PCIDeviceConfigType0, capability: MSIXCapability) -> Self {
        // N.B. The table size is encoded as N - 1 for some reason, so we add 1.
        let table_size = capability.registers.message_control().read().table_size() + 1;

        let table_offset_bits = capability.registers.table_offset().read();
        let table_bar_idx = table_offset_bits.bar_indicator_register();
        let table_offset = table_offset_bits.table_offset();
        let table_region_size = table_size as usize * core::mem::size_of::<MSIXTableEntry>();
        let table_address = device_config.bar_region_physical_address(
            table_bar_idx,
            table_offset,
            table_region_size as u64,
        );
        let table = unsafe { MSIXTable::new(table_address, table_size) };

        let pba_offset_bits = capability.registers.pending_bit_array_offset().read();
        let pba_bar_idx = pba_offset_bits.bar_indicator_register();
        let pba_offset = pba_offset_bits.pba_offset();
        let pba_region_size = table_size as usize * core::mem::size_of::<u64>();
        let pba_address = device_config.bar_region_physical_address(
            pba_bar_idx,
            pba_offset,
            pba_region_size as u64,
        );
        let pba = unsafe { MSIXPBA::new(pba_address, table_size) };
        Self {
            capability,
            table,
            _pba: pba,
        }
    }

    pub(crate) fn enable(&self) {
        self.capability
            .registers
            .message_control()
            .modify_mut(|control| {
                control.set_enable(true);
            });
    }

    pub(crate) fn table_entry(&self, index: usize) -> MSIXTableEntry {
        self.table.entry(index)
    }
}
