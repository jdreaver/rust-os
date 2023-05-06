use x86_64::PhysAddr;

use crate::pci::PCIDeviceCapabilityHeaderPtr;

#[derive(Debug, Clone, Copy)]
pub struct VirtIOPCICapabilityHeaderPtr {
    /// Physical address of the capability structure.
    base_address: PhysAddr,
}

impl VirtIOPCICapabilityHeaderPtr {
    /// # Safety
    ///
    /// Caller must ensure that the capability header is from a VirtIO device.
    pub unsafe fn from_capability_header(header: &PCIDeviceCapabilityHeaderPtr) -> Self {
        Self {
            base_address: header.address(),
        }
    }
}

impl AsRef<VirtIOPCICapabilityHeader> for VirtIOPCICapabilityHeaderPtr {
    fn as_ref(&self) -> &VirtIOPCICapabilityHeader {
        unsafe { &*(self.base_address.as_u64() as *const VirtIOPCICapabilityHeader) }
    }
}

/// See 4.1.4 Virtio Structure PCI Capabilities in spec
#[repr(packed)]
#[derive(Debug, Clone, Copy)]
pub struct VirtIOPCICapabilityHeader {
    /// This should equal 0x9, which is the PCI capability ID meaning "vendor
    /// specific".
    cap_vndr: u8,
    cap_next: u8,
    cap_len: u8,
    cfg_type: u8,
    bar: u8,
    id: u8,
    padding: [u8; 2],
    offset: u32,

    /// Length of the entire capability structure, in bytes.
    length: u32,
}
