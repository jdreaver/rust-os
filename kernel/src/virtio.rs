use x86_64::structures::paging::{FrameAllocator, Mapper, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

use crate::pci::{self, PCIDeviceCapabilityHeaderPtr, PCIeDeviceConfig};
use crate::serial_println;

// /// TODO: This is a hack. We are hard-coding the PCI virtio addresses from QEMU
// /// (see `info mtree`) so we can access VirtIO device configs. We should instead
// /// inspect the VirtIO PCI devices to find this memory, and then map it.

/// Temporary function for debugging how we get VirtIO information.
pub fn print_virtio_device(
    device: &PCIeDeviceConfig,
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    let header = device.header();
    assert_eq!(
        header.vendor_id(),
        0x1af4,
        "invalid vendor ID, not a VirtIO device"
    );

    serial_println!("Found VirtIO device: {:?}", header);

    let pci::PCIDeviceConfigBody::GeneralDevice(body) = device
            .body()
            .expect("failed to read device body")
            else { return; };

    for (i, capability) in body.iter_capabilities().enumerate() {
        let virtio_cap =
            unsafe { VirtIOPCICapabilityHeaderPtr::from_capability_header(&capability) };
        serial_println!("VirtIO capability {}: {:#x?}", i, virtio_cap.as_ref());

        let config_type = virtio_cap.config_type();
        serial_println!("VirtIO config type: {:?}", config_type);

        if config_type == VirtIOPCIConfigType::Common {
            let bar_idx = virtio_cap.as_ref().bar;
            serial_println!("common: bar_idx: {}", bar_idx);
            let bar = body.bar(bar_idx as usize);
            // TODO: We get a bar BAR here for the virtio GPU. It wants address
            // 0x800000000, or the address at 32 GiB.
            let offset = virtio_cap.as_ref().offset;
            let cap_length = virtio_cap.as_ref().length;
            serial_println!(
                "bar: {:#x?}, offset: {:#x?}, cap_length: {:#x?}",
                bar_idx,
                bar,
                cap_length
            );

            // Need to identity map the BAR target page(s) so we can access them
            // without faults. Note that these addresses can be outside of
            // physical memory, in which case they are intercepted by the PCI
            // bus and handled by the device, so we aren't mapping physical RAM
            // pages here, we are just ensuring these addresses are identity
            // mapped in the page table so they don't fault.
            let bar_start = bar + u64::from(offset);
            let bar_start_frame = PhysFrame::<Size4KiB>::containing_address(bar_start);
            let bar_end_frame = PhysFrame::containing_address(bar_start + u64::from(cap_length));
            let frame_range = PhysFrame::range_inclusive(bar_start_frame, bar_end_frame);
            for frame in frame_range {
                unsafe {
                    mapper
                        .identity_map(
                            frame,
                            PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                            frame_allocator,
                        )
                        .expect("failed to identity map VirtIO BAR page")
                        .flush();
                };
            }

            let common_cfg = unsafe { VirtIOPCICommonConfigPtr::from_bar_offset(bar, offset) };
            serial_println!("VirtIO common config: {:#x?}", common_cfg.as_ref());
        }
    }
}

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

    fn config_type(self) -> VirtIOPCIConfigType {
        let cfg_type = self.as_ref().cfg_type;
        VirtIOPCIConfigType::from_cfg_type(cfg_type).expect("invalid VirtIO config type")
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

struct VirtIOPCICommonConfigPtr {
    address: PhysAddr,
}

impl VirtIOPCICommonConfigPtr {
    /// # Safety
    ///
    /// Caller must ensure that the given BAR (base address register) is valid
    /// and is for the VirtIO device.
    pub unsafe fn from_bar_offset(bar: PhysAddr, offset: u32) -> Self {
        Self {
            address: bar + u64::from(offset),
        }
    }
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
