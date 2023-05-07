use core::fmt::{self, Write};

use x86_64::structures::paging::mapper::MapToError;
use x86_64::structures::paging::{FrameAllocator, Mapper, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

use crate::pci::{
    self, PCIDeviceCapabilityHeaderPtr, PCIDeviceConfigBodyType0Ptr, PCIeDeviceConfig,
};
use crate::strings::IndentWriter;

// /// TODO: This is a hack. We are hard-coding the PCI virtio addresses from QEMU
// /// (see `info mtree`) so we can access VirtIO device configs. We should instead
// /// inspect the VirtIO PCI devices to find this memory, and then map it.

/// Temporary function for debugging how we get VirtIO information.
pub fn print_virtio_device<W: Write>(
    w: &mut W,
    device: &PCIeDeviceConfig,
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    // TODO: Move everything from here down into a "VirtIODevice" type

    let header = device.header();
    assert_eq!(
        header.vendor_id(),
        0x1af4,
        "invalid vendor ID, not a VirtIO device"
    );

    let pci::PCIDeviceConfigBody::GeneralDevice(body) = device
            .body()
            .expect("failed to read device body")
            else { return; };

    let w = &mut IndentWriter::new(w, 2);

    writeln!(w, "Found VirtIO device: {header:?}").expect("failed to write");
    w.indent();

    for (i, capability) in body.iter_capabilities().enumerate() {
        writeln!(w, "VirtIO Capability {i}:").expect("failed to write");
        w.indent();

        let virtio_cap =
            unsafe { VirtIOPCICapabilityHeaderPtr::from_pci_capability(body, &capability) };
        virtio_cap
            .print(w)
            .expect("failed to print VirtIO capability header");

        // The PCI config type is a way to access the configuration over PCI
        // (not PCI Express, which is the memory mapped method we are using).
        // Just skip it, because this requires accessing the capability config
        // over I/O, which we don't support. See "4.1.4.9 PCI configuration
        // access capability" in the spec.
        if virtio_cap.config_type() == VirtIOPCIConfigType::PCI {
            w.unindent();
            continue;
        }

        let config = virtio_cap.config(mapper, frame_allocator);
        match config {
            VirtIOConfigPtr::Common(cfg) => {
                let cfg = cfg.as_ref();
                writeln!(w, "VirtIO Common Config: {cfg:#x?}").expect("failed to write");
            }
            VirtIOConfigPtr::Notify => {
                writeln!(w, "VirtIO Notify Config: TODO").expect("failed to write");
            }
            VirtIOConfigPtr::ISR => {
                writeln!(w, "VirtIO ISR Config: TODO").expect("failed to write");
            }
            VirtIOConfigPtr::Device => {
                writeln!(w, "VirtIO Device Config: TODO").expect("failed to write");
            }
            VirtIOConfigPtr::PCI => {
                writeln!(w, "VirtIO PCI Config: TODO").expect("failed to write");
            }
            VirtIOConfigPtr::SharedMemory => {
                writeln!(w, "VirtIO Shared Memory Config: TODO").expect("failed to write");
            }
            VirtIOConfigPtr::Vendor => {
                writeln!(w, "VirtIO Vendor Config: TODO").expect("failed to write");
            }
        }

        w.unindent();
    }

    w.unindent();
}

#[derive(Debug, Clone, Copy)]
pub struct VirtIOPCICapabilityHeaderPtr {
    /// The body of the PCID device for this VirtIO device.
    device_config_body: PCIDeviceConfigBodyType0Ptr,

    /// Physical address of the capability structure.
    base_address: PhysAddr,
}

impl VirtIOPCICapabilityHeaderPtr {
    /// # Safety
    ///
    /// Caller must ensure that the capability header is from a VirtIO device.
    pub unsafe fn from_pci_capability(
        device_config_body: PCIDeviceConfigBodyType0Ptr,
        header: &PCIDeviceCapabilityHeaderPtr,
    ) -> Self {
        Self {
            device_config_body,
            base_address: header.address(),
        }
    }

    fn config_type(self) -> VirtIOPCIConfigType {
        let cfg_type = self.as_ref().cfg_type;
        VirtIOPCIConfigType::from_cfg_type(cfg_type).expect("invalid VirtIO config type")
    }

    /// Returns the VirtIO device configuration associated with this capability
    /// header.
    fn config(
        self,
        mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> VirtIOConfigPtr {
        let capability = self.as_ref();
        let config_type = self.config_type();

        let bar_address = self.device_config_body.bar(capability.bar as usize);

        let bar_phys_addr = match bar_address {
            // TODO: Use the prefetchable field when doing mapping.
            pci::BARAddress::Mem32Bit {
                address,
                prefetchable: _,
            } => PhysAddr::new(u64::from(address)),
            pci::BARAddress::Mem64Bit {
                address,
                prefetchable: _,
            } => PhysAddr::new(address),
            pci::BARAddress::IO(address) => panic!(
                "VirtIO capability {:?} uses I/O BAR (address: {:#x}), not supported",
                config_type, address,
            ),
        };

        // Need to identity map the BAR target page(s) so we can access them
        // without faults. Note that these addresses can be outside of physical
        // memory, in which case they are intercepted by the PCI bus and handled
        // by the device, so we aren't mapping physical RAM pages here, we are
        // just ensuring these addresses are identity mapped in the page table
        // so they don't fault.
        let config_addr = bar_phys_addr + u64::from(capability.offset);
        let config_start_frame = PhysFrame::<Size4KiB>::containing_address(config_addr);
        let config_end_frame =
            PhysFrame::containing_address(config_addr + u64::from(capability.cap_len));
        let frame_range = PhysFrame::range_inclusive(config_start_frame, config_end_frame);
        for frame in frame_range {
            let map_result = unsafe {
                mapper.identity_map(
                    frame,
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                    frame_allocator,
                )
            };
            match map_result {
                // These errors are okay. They just mean the frame is already
                // identity mapped (well, hopefully).
                Ok(_) | Err(MapToError::ParentEntryHugePage | MapToError::PageAlreadyMapped(_)) => {
                }
                Err(e) => panic!("failed to map VirtIO device config page: {:?}", e),
            }
        }

        unsafe { VirtIOConfigPtr::from_address(config_type, config_addr) }
    }

    pub fn print<W: Write>(&self, w: &mut IndentWriter<W>) -> fmt::Result {
        writeln!(w, "VirtIO PCI capability header:")?;

        w.indent();
        let header = self.as_ref();
        let offset = header.offset;
        let length = header.length;
        let config_type = self.config_type();

        writeln!(w, "cap_vndr: {:#x}", header.cap_vndr)?;
        writeln!(w, "cap_next: {:#x}", header.cap_next)?;
        writeln!(w, "cap_len: {:#x}", header.cap_len)?;
        writeln!(w, "cfg_type: {:#x} ({config_type:?})", header.cfg_type)?;
        writeln!(w, "bar: {:#x}", header.bar)?;
        writeln!(w, "id: {:#x}", header.id)?;
        writeln!(w, "offset: {offset:#x}")?;
        writeln!(w, "length: {length:?}")?;
        w.unindent();

        Ok(())
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
    _padding: [u8; 2],
    offset: u32,

    /// Length of the entire capability structure, in bytes.
    length: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VirtIOPCIConfigType {
    Common = 1,
    Notify = 2,
    ISR = 3,
    Device = 4,
    PCI = 5,
    SharedMemory = 8,
    Vendor = 9,
}

impl VirtIOPCIConfigType {
    fn from_cfg_type(cfg_type: u8) -> Option<Self> {
        match cfg_type {
            1 => Some(Self::Common),
            2 => Some(Self::Notify),
            3 => Some(Self::ISR),
            4 => Some(Self::Device),
            5 => Some(Self::PCI),
            8 => Some(Self::SharedMemory),
            9 => Some(Self::Vendor),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum VirtIOConfigPtr {
    Common(VirtIOPCICommonConfigPtr),
    Notify,
    ISR,
    Device,
    PCI,
    SharedMemory,
    Vendor,
}

impl VirtIOConfigPtr {
    /// # Safety
    ///
    /// Caller must ensure that the given BAR (base address register) is valid
    /// and is for the VirtIO device.
    pub unsafe fn from_address(config_type: VirtIOPCIConfigType, config_addr: PhysAddr) -> Self {
        match config_type {
            VirtIOPCIConfigType::Common => Self::Common(VirtIOPCICommonConfigPtr {
                address: config_addr,
            }),
            VirtIOPCIConfigType::Notify => Self::Notify,
            VirtIOPCIConfigType::ISR => Self::ISR,
            VirtIOPCIConfigType::Device => Self::Device,
            VirtIOPCIConfigType::PCI => Self::PCI,
            VirtIOPCIConfigType::SharedMemory => Self::SharedMemory,
            VirtIOPCIConfigType::Vendor => Self::Vendor,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct VirtIOPCICommonConfigPtr {
    address: PhysAddr,
}

impl AsRef<VirtIOPCICommonConfig> for VirtIOPCICommonConfigPtr {
    fn as_ref(&self) -> &VirtIOPCICommonConfig {
        unsafe { &*(self.address.as_u64() as *const VirtIOPCICommonConfig) }
    }
}

#[derive(Debug, Clone, Copy)]
struct VirtIOPCICommonConfig {
    device_feature_select: u32,
    device_feature: u32,
    driver_feature_select: u32,
    driver_feature: u32,
    msix_config: u16,
    num_queues: u16,
    device_status: u8,
    config_generation: u8,
    queue_select: u16,
    queue_size: u16,
    queue_msix_vector: u16,
    queue_enable: u16,
    queue_notify_off: u16,
    queue_desc: u64,
    queue_driver: u64,
    queue_device: u64,
    queue_notify_data: u16,
    queue_reset: u16,
}
