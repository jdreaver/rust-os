use core::fmt;

use bitfield_struct::bitfield;

use crate::apic::ProcessorID;
use crate::interrupts::InterruptVector;
use crate::memory::KernPhysAddr;
use crate::register_struct;
use crate::registers::{RegisterRO, RegisterRW};

#[derive(Debug, Clone)]
pub(crate) enum PCIDeviceCapability {
    MSIX(MSIXCapability),
    VendorSpecific(PCIDeviceCapabilityHeader),
    Other(PCIDeviceCapabilityHeader),
}

#[derive(Debug, Clone)]
pub(crate) struct PCIDeviceCapabilityHeader {
    config_base_address: KernPhysAddr,
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
    pub(super) unsafe fn new(config_base_address: KernPhysAddr, offset: u8) -> Option<Self> {
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
                MSIXCapability::from_capability(self).expect("failed to create MSIX capability"),
            ),
            // TODO: Remove self.clone() from these calls
            Self::VENDOR_SPECIFIC_CAPABILITY_ID => {
                PCIDeviceCapability::VendorSpecific(self.clone())
            }
            _ => PCIDeviceCapability::Other(self.clone()),
        }
    }

    pub(crate) fn address(&self) -> KernPhysAddr {
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
        // TODO: Remove clone() calls here
        let next = self.ptr.clone();
        self.ptr = self.ptr.clone().and_then(|ptr| ptr.next_capability());
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
    pub(super) registers: MSIXCapabilityRegisters,
}

impl MSIXCapability {
    pub(crate) const MSIX_CAPABILITY_ID: u8 = 0x11;

    pub(crate) fn from_capability(capability: &PCIDeviceCapabilityHeader) -> Option<Self> {
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
    pub(super) MSIXCapabilityRegisters {
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
pub(super) struct MSIXMessageControl {
    #[bits(11)]
    pub(super) table_size: u16,
    #[bits(3)]
    __reserved: u8,
    pub(super) function_mask: bool,
    pub(super) enable: bool,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
/// See "7.7.2.3 Table Offset/Table BIR Register for MSI-X (Offset 04h)" in PCI
/// Express Base Specification.
pub(super) struct MSIXTableOffset(u32);

impl MSIXTableOffset {
    /// BIR (BAR Indicator Register) is a 3-bit field that indicates which BAR
    /// contains the MSI-X table.
    pub(super) fn bar_indicator_register(self) -> u8 {
        (self.0 & 0b111) as u8
    }

    /// The offset of the MSI-X table from the base address of the BAR indicated
    /// by the BIR field. We mask off the first 3 bits to make this 32 bit (4
    /// byte) aligned.
    pub(super) fn table_offset(self) -> u32 {
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
pub(super) struct MSIXPendingBitArrayOffset(u32);

impl MSIXPendingBitArrayOffset {
    /// BIR (BAR Indicator Register) is a 3-bit field that indicates which BAR
    /// contains the MSI-X table.
    pub(super) fn bar_indicator_register(self) -> u8 {
        (self.0 & 0b111) as u8
    }

    /// The offset of the MSI-X table from the base address of the BAR indicated
    /// by the BIR field. We mask off the first 3 bits to make this 32 bit (4
    /// byte) aligned.
    pub(super) fn pba_offset(self) -> u32 {
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

#[derive(Debug)]
pub(crate) struct MSIXTable {
    address: KernPhysAddr,
    table_size: u16,
}

impl MSIXTable {
    pub(super) unsafe fn new(address: KernPhysAddr, table_size: u16) -> Self {
        Self {
            address,
            table_size,
        }
    }

    pub(super) fn entry(&self, index: usize) -> MSIXTableEntry {
        assert!(
            index < self.table_size as usize,
            "MSIXTable index out of bounds"
        );
        let entry_address = self.address + (index * 16);
        unsafe { MSIXTableEntry(RawMSIXTableEntry::from_address(entry_address)) }
    }
}

#[repr(transparent)]
#[derive(Debug, Clone)]
pub(crate) struct MSIXTableEntry(RawMSIXTableEntry);

impl MSIXTableEntry {
    pub(crate) fn set_interrupt_vector(
        &mut self,
        processor_id: ProcessorID,
        vector: InterruptVector,
    ) {
        self.0
            .message_address()
            .write(MSIXMessageAddress::new(processor_id));
        self.0
            .message_data()
            .write(MSIXMessageData::new().with_vector(vector.0));

        // Un-mask the entry (default is masked!)
        self.0.vector_control().modify_mut(|vec| {
            vec.set_mask_bit(false);
        });
    }
}

register_struct!(
    /// See "7.7.2 MSI-X Capability and Table Structure" in the PCI Express Base
    /// Specification.
    pub(crate) RawMSIXTableEntry {
        0x0 => message_address: RegisterRW<MSIXMessageAddress>,
        0x4 => message_upper_address: RegisterRW<u32>,
        0x8 => message_data: RegisterRW<MSIXMessageData>,
        0xc => vector_control: RegisterRW<MSIXVectorControl>,
    }
);

/// The PCI MSI-X message address is architecture specific, and this is the
/// Intel one. See "11.11.1 Message Address Register Format" in the Intel 64
/// Manual Volume 3.
#[repr(transparent)]
#[derive(Debug, Clone)]
pub(crate) struct MSIXMessageAddress(RawMSIXMessageAddress);

impl MSIXMessageAddress {
    const INTEL_PREFIX: u16 = 0x0fee;

    pub(crate) fn new(processor_id: ProcessorID) -> Self {
        Self(
            RawMSIXMessageAddress::new()
                .with_intel_prefix(Self::INTEL_PREFIX)
                // Setting RH and DM = false makes Destination ID a processor ID
                .with_destination_id(processor_id.0)
                .with_redirection_hint(false)
                .with_destination_mode(false),
        )
    }
}

#[bitfield(u32)]
/// The PCI MSI-X message address is architecture specific, and this is the
/// Intel one. See "11.11.1 Message Address Register Format" in the Intel 64
/// Manual Volume 3.
pub(crate) struct RawMSIXMessageAddress {
    #[bits(2)]
    __reserved: u8,
    pub(crate) destination_mode: bool,
    pub(crate) redirection_hint: bool,
    __reserved: u8,
    pub(crate) destination_id: u8,
    #[bits(12)]
    intel_prefix: u16,
}

#[bitfield(u32)]
/// The PCI MSI-X message data is architecture specific, and this is the
/// Intel one. See "11.11.2 Message Data Register Format" in the Intel 64
/// Manual Volume 3.
pub(crate) struct MSIXMessageData {
    pub(crate) vector: u8,
    #[bits(3)]
    pub(crate) delivery_mode: u8,
    #[bits(3)]
    __reserved: u8,
    level: bool,
    trigger_mode: bool,
    #[bits(16)]
    __reserved: u32,
}

#[bitfield(u32)]
/// See "7.7.2.8 Vector Control Register for MSI-X Table Entries"
pub(crate) struct MSIXVectorControl {
    pub(crate) mask_bit: bool,
    #[bits(15)]
    __reserved: u16,
    pub(crate) st_lower: u8,
    pub(crate) st_upper: u8,
}

// MSIXPBA isn't used for anything yet, but it's here for completeness.
//
// TODO: Remove the #[allow(dead_code)] here and on the impl when we use it.
#[allow(dead_code)]
#[derive(Debug)]
pub(super) struct MSIXPBA {
    address: KernPhysAddr,
    pba_size: u16,
}

#[allow(dead_code)]
impl MSIXPBA {
    pub(super) unsafe fn new(address: KernPhysAddr, pba_size: u16) -> Self {
        Self { address, pba_size }
    }

    pub(super) fn pba_size(&self) -> u16 {
        self.pba_size
    }

    pub(super) fn entry(&self, index: usize) -> Option<u64> {
        if index >= self.pba_size as usize {
            return None;
        }

        let entry_address = self.address + (index * 16);
        let ptr = entry_address.as_ptr::<u64>();
        Some(unsafe { ptr.read_volatile() })
    }
}
