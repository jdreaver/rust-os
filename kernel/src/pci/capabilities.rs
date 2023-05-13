use core::fmt;

use bitfield_struct::bitfield;

use crate::register_struct;
use crate::registers::{RegisterRO, RegisterRW};

#[derive(Debug, Clone)]
pub(crate) enum PCIDeviceCapability {
    MSIX(MSIXCapability),
    VendorSpecific(PCIDeviceCapabilityHeader),
    Other(PCIDeviceCapabilityHeader),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PCIDeviceCapabilityHeader {
    config_base_address: usize,
    registers: PCIDeviceCapabilityHeaderRegisters,
}

impl PCIDeviceCapabilityHeader {
    const VENDOR_SPECIFIC_CAPABILITY_ID: u8 = 0x09;

    /// Construct a new `PCIDeviceCapabilityHeaderPtr` from the given
    /// `config_region_base` and `offset`. The offset must be from the device
    /// configuration header. Returns `None` if the offset is 0.
    ///
    /// # Safety
    ///
    /// Both `config_region_base` and `offset` must be valid.
    pub(super) unsafe fn new(config_base_address: usize, offset: u8) -> Option<Self> {
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

    /// Determine the specific capability type of this capability header.
    pub(crate) fn capability(&self) -> PCIDeviceCapability {
        match self.registers.id().read() {
            MSIXCapability::MSIX_CAPABILITY_ID => PCIDeviceCapability::MSIX(
                MSIXCapability::from_capability(*self).expect("failed to create MSIX capability"),
            ),
            Self::VENDOR_SPECIFIC_CAPABILITY_ID => PCIDeviceCapability::VendorSpecific(*self),
            _ => PCIDeviceCapability::Other(*self),
        }
    }

    pub(crate) fn address(&self) -> usize {
        self.registers.address
    }

    fn next_capability(&self) -> Option<Self> {
        unsafe { Self::new(self.config_base_address, self.registers.next().read()) }
    }
}

register_struct!(
    pub(crate) PCIDeviceCapabilityHeaderRegisters {
        0x0 => id: RegisterRO<u8>,
        0x1 => next: RegisterRO<u8>,
    }
);

#[derive(Clone)]
pub(crate) struct PCIDeviceCapabilityIterator {
    ptr: Option<PCIDeviceCapabilityHeader>,
}

impl PCIDeviceCapabilityIterator {
    pub(super) fn new(ptr: Option<PCIDeviceCapabilityHeader>) -> Self {
        Self { ptr }
    }
}

impl Iterator for PCIDeviceCapabilityIterator {
    type Item = PCIDeviceCapabilityHeader;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.ptr;
        self.ptr = self.ptr.and_then(|ptr| ptr.next_capability());
        next
    }
}

impl fmt::Debug for PCIDeviceCapabilityIterator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let iter = self.clone().map(|cap| cap.capability());
        f.debug_list().entries(iter).finish()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MSIXCapability {
    registers: MSIXCapabilityRegisters,
}

impl MSIXCapability {
    pub(crate) const MSIX_CAPABILITY_ID: u8 = 0x11;

    pub(crate) fn from_capability(capability: PCIDeviceCapabilityHeader) -> Option<Self> {
        if capability.registers.id().read() != Self::MSIX_CAPABILITY_ID {
            return None;
        }

        let registers = unsafe { MSIXCapabilityRegisters::from_address(capability.address()) };

        Some(Self { registers })
    }
}

register_struct!(
    /// See "7.7.2 MSI-X Capability and Table Structure" in the PCI Express Base
    /// Specification.
    pub(crate) MSIXCapabilityRegisters {
        0x0 => capability_id: RegisterRO<u8>,
        0x1 => next_capability: RegisterRO<u8>,
        0x2 => message_control: RegisterRW<MSIXMessageControl>,
        0x4 => table_offset: RegisterRW<MSIXTableOffset>,
        0x8 => pending_bit_array_offset: RegisterRW<MSIXPendingBitArrayOffset>,
    }
);

#[bitfield(u16)]
/// See "7.7.2.2 Message Control Register for MSI-X (Offset 02h)" in PCI Express
/// Base Specification.
pub(crate) struct MSIXMessageControl {
    #[bits(11)]
    table_size: u16,
    #[bits(3)]
    __reserved: u8,
    function_mask: bool,
    enable: bool,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
/// See "7.7.2.3 Table Offset/Table BIR Register for MSI-X (Offset 04h)" in PCI
/// Express Base Specification.
pub(crate) struct MSIXTableOffset(u32);

impl MSIXTableOffset {
    /// BIR (BAR Indicator Register) is a 3-bit field that indicates which BAR
    /// contains the MSI-X table.
    pub(crate) fn bar_indicator_register(self) -> u8 {
        (self.0 & 0b111) as u8
    }

    /// The offset of the MSI-X table from the base address of the BAR indicated
    /// by the BIR field. We mask off the first 3 bits to make this 32 bit (4
    /// byte) aligned.
    pub(crate) fn table_offset(self) -> u32 {
        self.0 & 0b1111_1111_1111_1111_1111_1111_1111_1000
    }
}

impl fmt::Debug for MSIXTableOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MSIXTableOffset")
            .field("bar_indicator_register", &self.bar_indicator_register())
            .field("table_offset", &self.table_offset())
            .finish()
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
/// See "7.7.2.4 PBA Offset/PBA BIR Register for MSI-X (Offset 08h)" in PCI
/// Express Base Specification.
pub(crate) struct MSIXPendingBitArrayOffset(u32);

impl MSIXPendingBitArrayOffset {
    /// BIR (BAR Indicator Register) is a 3-bit field that indicates which BAR
    /// contains the MSI-X table.
    pub(crate) fn bar_indicator_register(self) -> u8 {
        (self.0 & 0b111) as u8
    }

    /// The offset of the MSI-X table from the base address of the BAR indicated
    /// by the BIR field. We mask off the first 3 bits to make this 32 bit (4
    /// byte) aligned.
    pub(crate) fn pba_offset(self) -> u32 {
        self.0 & 0b1111_1111_1111_1111_1111_1111_1111_1000
    }
}

impl fmt::Debug for MSIXPendingBitArrayOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MSIXPendingBitArrayOffset")
            .field("bar_indicator_register", &self.bar_indicator_register())
            .field("pba_offset", &self.pba_offset())
            .finish()
    }
}
