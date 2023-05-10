use alloc::vec::Vec;
use core::alloc::Allocator;
use core::fmt;
use core::mem;

use bitfield_struct::bitfield;
use x86_64::structures::paging::mapper::MapToError;
use x86_64::structures::paging::{FrameAllocator, Mapper, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

use crate::memory::AllocZeroedBufferError;
use crate::pci::{
    self, BARAddress, PCIDeviceCapabilityHeader, PCIDeviceConfig, PCIDeviceConfigType0,
    PCIDeviceConfigTypes,
};
use crate::registers::{RegisterRO, RegisterRW};
use crate::serial_println;
use crate::{memory, register_struct};

/// Holds the configuration for a VirtIO device.
#[derive(Debug, Clone, Copy)]
pub struct VirtIODeviceConfig {
    /// Common PCI configuration registers.
    pci_config: PCIDeviceConfig,

    /// Registers specifically for type 0 devices (which all VirtIO devices
    /// are).
    pci_type0_config: PCIDeviceConfigType0,

    common_virtio_config: VirtIOPCICommonConfigRegisters,
    isr: VirtIOPCIISRRegisters,
    notify_config: VirtIONotifyConfig,
}

impl VirtIODeviceConfig {
    pub fn from_pci_config(
        pci_config: PCIDeviceConfig,
        mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> Option<Self> {
        // Check that this is a VirtIO device.
        let vendor_id = pci_config.device_id().registers().vendor_id().read();
        if vendor_id != 0x1af4 {
            return None;
        };

        let config_type = pci_config
            .config_type()
            .expect("failed to read device config type");
        let PCIDeviceConfigTypes::GeneralDevice(pci_type0_config) = config_type else {
            panic!("invalid VirtIO device config type, expected Type 0");
        };

        // Scan capabilities to record the ones we need
        let mut common_virtio_config = None;
        let mut isr = None;
        let mut notify_config = None;
        for capability in pci_type0_config.iter_capabilities() {
            let capability = unsafe {
                VirtIOPCICapabilityHeader::from_pci_capability(pci_type0_config, &capability)
            };
            let Some(capability) = capability else { continue; };

            // The PCI config type is a way to access the configuration over PCI
            // (not PCI Express, which is the memory mapped method we are using).
            // Just skip it, because this requires accessing the capability config
            // over I/O, which we don't support. See "4.1.4.9 PCI configuration
            // access capability" in the spec.
            if capability.config_type() == VirtIOPCIConfigType::PCI {
                continue;
            }

            let config = capability.config(mapper, frame_allocator);

            // N.B. The VirtIO spec says that capabilities of the same type
            // should be ordered by preference. It also says "The driver
            // SHOULD use the first instance of each virtio structure type
            // they can support." That means we take the first instance of
            // each type we find, hence the use of `get_or_insert`.
            match config {
                VirtIOConfig::Common(cfg) => {
                    common_virtio_config.get_or_insert(cfg);
                }
                VirtIOConfig::Notify(cfg) => {
                    notify_config.get_or_insert(cfg);
                }
                VirtIOConfig::ISR(isr_regs) => {
                    isr.get_or_insert(isr_regs);
                }
                VirtIOConfig::Device => {
                    serial_println!("VirtIO Device config found: {:#x?}", capability);
                }
                VirtIOConfig::PCI => {
                    serial_println!("VirtIO PCI config found: {:#x?}", capability);
                }
                VirtIOConfig::SharedMemory => {
                    serial_println!("VirtIO SharedMemory config found: {:#x?}", capability);
                }
                VirtIOConfig::Vendor => {
                    serial_println!("VirtIO Vendor config found: {:#x?}", capability);
                }
            }
        }

        let common_virtio_config =
            common_virtio_config.expect("failed to find VirtIO common config");
        let isr = isr.expect("failed to find VirtIO ISR");
        let notify_config = notify_config.expect("failed to find VirtIO notify config");

        Some(Self {
            pci_config,
            pci_type0_config,
            common_virtio_config,
            isr,
            notify_config,
        })
    }

    /// See "3 General Initialization And Device Operation" and "4.1.5
    /// PCI-specific Initialization And Device Operation"
    pub fn initialize(self, physical_allocator: &impl Allocator) -> VirtIOInitializedDevice {
        let config = self.common_virtio_config;

        // Reset the VirtIO device by writing 0 to the status register (see
        // 4.1.4.3.1 Device Requirements: Common configuration structure layout)
        let mut status = VirtIOConfigStatus::new();
        config.device_status().write(status);

        // Set the ACKNOWLEDGE status bit to indicate that the driver knows
        // that the device is present.
        status.set_acknowledge(true);
        config.device_status().write(status);

        // Set the DRIVER status bit to indicate that the driver is ready to
        // drive the device.
        status.set_driver(true);
        config.device_status().write(status);

        // Feature negotiation. There are up to 128 feature bits, and
        // the feature registers are 32 bits wide, so we use the feature
        // selection registers 4 times to select features.
        //
        // (TODO: Make this configurable depending on device).
        for i in 0..4 {
            // Select the feature bits to negotiate
            config.device_feature_select().write(i);

            // Read the device feature bits
            let device_features = config.device_feature().read();
            serial_println!(
                "VirtIO device feature bits ({}): {:#034b}",
                i,
                device_features
            );

            // Write the features we want to enable (TODO: actually pick
            // features, don't just write them all back)
            let driver_features = device_features;
            config.driver_feature_select().write(i);
            config.driver_feature().write(driver_features);
        }

        // Set the FEATURES_OK status bit to indicate that the driver has
        // written the feature bits.
        status.set_features_ok(true);
        config.device_status().write(status);

        // Re-read the status to ensure that the FEATURES_OK bit is still set.
        status = config.device_status().read();
        assert!(status.features_ok(), "failed to set FEATURES_OK status bit");

        // Initialize virtqueues
        let num_queues = config.num_queues().read();
        let mut virtqueues = Vec::with_capacity(num_queues as usize);
        for i in 0..num_queues {
            config.queue_select().write(i);

            let queue_size = config.queue_size().read();

            let descriptors = unsafe {
                VirtqDescriptorTable::allocate(queue_size, physical_allocator)
                    .expect("failed to allocate driver ring buffer")
            };
            config.queue_desc().write(descriptors.physical_address);

            let avail_ring = unsafe {
                VirtqAvailRing::allocate(queue_size, physical_allocator)
                    .expect("failed to allocate driver ring buffer")
            };
            config.queue_driver().write(avail_ring.physical_address);

            let used_ring = unsafe {
                VirtqUsedRing::allocate(queue_size, physical_allocator)
                    .expect("failed to allocate driver ring buffer")
            };
            config.queue_device().write(used_ring.physical_address);

            // Enable the queue
            config.queue_enable().write(1);

            virtqueues.push(VirtQueue {
                index: i,
                device_notify_config: self.notify_config,
                notify_offset: config.queue_notify_off().read(),
                descriptors,
                avail_ring,
                used_ring,
            });
        }

        // TODO: Device-specific setup

        // Set the DRIVER_OK status bit to indicate that the driver
        // finished configuring the device.
        status.set_driver_ok(true);
        config.device_status().write(status);

        VirtIOInitializedDevice {
            config: self,
            virtqueues,
        }
    }

    pub fn common_virtio_config(&self) -> VirtIOPCICommonConfigRegisters {
        self.common_virtio_config
    }
}

#[derive(Clone, Copy)]
pub struct VirtIOPCICapabilityHeader {
    /// The body of the PCID device for this VirtIO device.
    device_config_body: PCIDeviceConfigType0,

    registers: VirtIOPCICapabilityHeaderRegisters,
}

impl fmt::Debug for VirtIOPCICapabilityHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VirtIOPCICapabilityHeader")
            .field("bar_address", &self.bar_address())
            .field("config_type", &self.config_type())
            .field("registers", &self.registers)
            .finish_non_exhaustive()
    }
}

impl VirtIOPCICapabilityHeader {
    /// # Safety
    ///
    /// Caller must ensure that the capability header is from a VirtIO device.
    pub unsafe fn from_pci_capability(
        device_config_body: PCIDeviceConfigType0,
        header: &PCIDeviceCapabilityHeader,
    ) -> Option<Self> {
        // VirtIO-specific capabilities must have an ID for vendor-specific.
        if !header.is_vendor_specific() {
            return None;
        }

        Some(Self {
            device_config_body,
            registers: VirtIOPCICapabilityHeaderRegisters::from_address(header.address()),
        })
    }

    fn bar_address(self) -> BARAddress {
        let bar = self.registers.bar().read();
        self.device_config_body.bar(bar as usize)
    }

    fn config_type(self) -> VirtIOPCIConfigType {
        let cfg_type = self.registers.cfg_type().read();
        VirtIOPCIConfigType::from_cfg_type(cfg_type).expect("invalid VirtIO config type")
    }

    /// Returns the VirtIO device configuration associated with this capability
    /// header.
    fn config(
        self,
        mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> VirtIOConfig {
        match self.config_type() {
            VirtIOPCIConfigType::Common => VirtIOConfig::Common(unsafe {
                let config_addr = self.compute_and_map_config_address(mapper, frame_allocator);
                VirtIOPCICommonConfigRegisters::from_address(config_addr.as_u64() as usize)
            }),
            VirtIOPCIConfigType::Notify => VirtIOConfig::Notify({
                let config_addr = self.compute_and_map_config_address(mapper, frame_allocator);

                // Per 4.1.4.4 Notification structure layout, the notify
                // configuration is in the capabilities struct and the notify
                // offset multiplier is right after the capabilities struct.
                let notify_off_ptr =
                    (self.registers.address + VIRTIO_CAPABILITY_HEADER_SIZE) as *const u32;
                let notify_off_multiplier = unsafe { *notify_off_ptr };

                VirtIONotifyConfig {
                    config_addr,
                    notify_off_multiplier,
                }
            }),
            VirtIOPCIConfigType::ISR => VirtIOConfig::ISR(unsafe {
                let config_addr = self.compute_and_map_config_address(mapper, frame_allocator);
                VirtIOPCIISRRegisters::from_address(config_addr.as_u64() as usize)
            }),
            VirtIOPCIConfigType::Device => VirtIOConfig::Device,
            VirtIOPCIConfigType::PCI => VirtIOConfig::PCI,
            VirtIOPCIConfigType::SharedMemory => VirtIOConfig::SharedMemory,
            VirtIOPCIConfigType::Vendor => VirtIOConfig::Vendor,
        }
    }

    /// Compute and map physical address for VirtIO capabilities that need to
    /// reach through a BAR to access their configuration.
    pub fn compute_and_map_config_address(
        self,
        mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> PhysAddr {
        let bar_phys_addr = match self.bar_address() {
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
                "VirtIO capability uses I/O BAR (address: {:#x}), not supported",
                address,
            ),
        };

        // Need to identity map the BAR target page(s) so we can access them
        // without faults. Note that these addresses can be outside of physical
        // memory, in which case they are intercepted by the PCI bus and handled
        // by the device, so we aren't mapping physical RAM pages here, we are
        // just ensuring these addresses are identity mapped in the page table
        // so they don't fault.
        let config_addr = bar_phys_addr + u64::from(self.registers.offset().read());
        let config_start_frame = PhysFrame::<Size4KiB>::containing_address(config_addr);
        let config_end_frame =
            PhysFrame::containing_address(config_addr + u64::from(self.registers.cap_len().read()));
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

        config_addr
    }
}

/// Ensure this matches the size of the VirtIO capability header! (See
/// `VirtIOPCICapabilityHeaderRegisters`.)
const VIRTIO_CAPABILITY_HEADER_SIZE: usize = 16;

register_struct!(
    /// See 4.1.4 Virtio Structure PCI Capabilities in spec
    VirtIOPCICapabilityHeaderRegisters {
        // TODO: Support field docstrings in register_struct! macro.
        // This should equal 0x9, which is the PCI capability ID meaning "vendor
        // specific".
        0x00 => cap_vndr: RegisterRO<u8>,
        0x01 => cap_next: RegisterRO<u8>,
        0x02 => cap_len: RegisterRO<u8>,
        0x03 => cfg_type: RegisterRO<u8>,
        0x04 => bar: RegisterRO<u8>,
        0x05 => id: RegisterRO<u8>,

        // 2 bytes of padding

        0x08 => offset: RegisterRO<u32>,
        // Length of the entire capability structure, in bytes.
        0x12 => length: RegisterRO<u32>,
    }
);

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
enum VirtIOConfig {
    Common(VirtIOPCICommonConfigRegisters),
    Notify(VirtIONotifyConfig),
    ISR(VirtIOPCIISRRegisters),
    Device,
    PCI,
    SharedMemory,
    Vendor,
}

register_struct!(
    /// 4.1.4.3 Common configuration structure layout
    VirtIOPCICommonConfigRegisters {
        0x00 => device_feature_select: RegisterRW<u32>,
        0x04 => device_feature: RegisterRO<u32>,
        0x08 => driver_feature_select: RegisterRW<u32>,
        0x12 => driver_feature: RegisterRW<u32>,
        0x10 => msix_config: RegisterRW<u16>,
        0x12 => num_queues: RegisterRO<u16>,
        0x14 => device_status: RegisterRW<VirtIOConfigStatus>,
        0x15 => config_generation: RegisterRO<u8>,
        0x16 => queue_select: RegisterRW<u16>,
        0x18 => queue_size: RegisterRW<u16>,
        0x1A => queue_msix_vector: RegisterRW<u16>,
        0x1C => queue_enable: RegisterRW<u16>,
        0x1E => queue_notify_off: RegisterRO<u16>,
        0x20 => queue_desc: RegisterRW<u64>,
        0x28 => queue_driver: RegisterRW<u64>,
        0x30 => queue_device: RegisterRW<u64>,
        0x38 => queue_notify_data: RegisterRO<u16>,
        0x3A => queue_reset: RegisterRW<u16>,
    }
);

#[bitfield(u8)]
/// 2.1 Device Status Field
pub struct VirtIOConfigStatus {
    /// ACKNOWLEDGE (1) Indicates that the guest OS has found the device and
    /// recognized it as a valid virtio device.
    acknowledge: bool,

    /// DRIVER (2) Indicates that the guest OS knows how to drive the device.
    driver: bool,

    /// DRIVER_OK (4) Indicates that the guest OS knows how to drive the device.
    driver_ok: bool,

    /// FEATURES_OK (8) Indicates that the features negotiated by the driver are
    /// acceptable to the device. This bit is optional since not all devices
    /// support feature negotiation, and some devices may accept any subset of
    /// the features offered by the driver.
    features_ok: bool,

    __reserved: bool,
    __reserved: bool,

    /// DEVICE_NEEDS_RESET (64) Indicates that the device has experienced an
    /// error from which it can’t recover. The device has stopped working. The
    /// driver should not send any further requests to the device, and should
    /// reset the device at the earliest convenience.
    device_needs_reset: bool,

    /// FAILED (128) Indicates that something went wrong in the guest, and it
    /// has given up on the device. This could be an internal error, or the
    /// driver didn’t like the device for some reason, or even a fatal error
    /// during device operation. The device should not be used any further
    /// without a reset.
    failed: bool,
}

register_struct!(
    /// 4.1.4.5 ISR status capability
    VirtIOPCIISRRegisters {
        0x00 => isr: RegisterRW<VirtIOISRStatus>,
    }
);

#[bitfield(u32)]
/// 4.1.4.5 ISR status capability
pub struct VirtIOISRStatus {
    queue_interrupt: bool,
    device_config_interrupt: bool,

    #[bits(30)]
    __reserved: u32,
}

/// 4.1.4.4 Notification structure layout
#[derive(Debug, Clone, Copy)]
pub struct VirtIONotifyConfig {
    /// Physical address for the configuration area for the Notify capability
    /// (BAR + offset has already been applied).
    config_addr: PhysAddr,

    notify_off_multiplier: u32,
}

impl VirtIONotifyConfig {
    /// 4.1.4.4 Notification structure layout
    fn queue_notify_address(&self, queue_notify_offset: u16) -> PhysAddr {
        let offset = u64::from(queue_notify_offset) * u64::from(self.notify_off_multiplier);
        self.config_addr + offset
    }

    /// 4.1.5.2 Available Buffer Notifications: When VIRTIO_F_NOTIFICATION_DATA
    /// has not been negotiated, the driver sends an available buffer
    /// notification to the device by writing the 16-bit virtqueue index of this
    /// virtqueue to the Queue Notify address.
    ///
    /// # Safety
    ///
    /// Caller must ensure that `queue_notify_offset` and `queue_index` are
    /// valid.
    unsafe fn notify_device(&self, queue_notify_offset: u16, queue_index: u16) {
        // 4.1.5.2 Available Buffer Notifications: When
        // VIRTIO_F_NOTIFICATION_DATA has not been negotiated, the driver sends
        // an available buffer notification to the device by writing the 16-bit
        // virtqueue index of this virtqueue to the Queue Notify address.
        let notify_addr = self.queue_notify_address(queue_notify_offset);
        let notify_ptr = notify_addr.as_u64() as *mut u16;
        unsafe {
            notify_ptr.write_volatile(queue_index);
        }
    }
}

#[derive(Debug)]
pub struct VirtIOInitializedDevice {
    config: VirtIODeviceConfig,
    virtqueues: Vec<VirtQueue>,
}

impl VirtIOInitializedDevice {
    pub fn config(&self) -> &VirtIODeviceConfig {
        &self.config
    }

    pub fn get_virtqueue_mut(&mut self, index: u16) -> Option<&mut VirtQueue> {
        self.virtqueues.get_mut(index as usize)
    }
}

/// Wrapper around allocated virt queues for a an initialized VirtIO device.
#[derive(Debug)]
pub struct VirtQueue {
    /// The queue's index in the device's virtqueue array.
    index: u16,

    /// Device's notification config, inlined here to compute the notification
    /// address. See "4.1.4.4 Notification structure layout".
    device_notify_config: VirtIONotifyConfig,

    /// The queue's notification offset. See "4.1.4.4 Notification structure
    /// layout".
    notify_offset: u16,

    descriptors: VirtqDescriptorTable,
    avail_ring: VirtqAvailRing,
    used_ring: VirtqUsedRing,
}

impl VirtQueue {
    /// See "2.7.13 Supplying Buffers to The Device"
    pub fn add_buffer(&mut self, buffer_addr: u64, buffer_len: u32, flags: VirtqDescriptorFlags) {
        let desc_index = self
            .descriptors
            .add_descriptor(buffer_addr, buffer_len, flags);
        self.avail_ring.add_entry(desc_index);
        unsafe {
            self.device_notify_config
                .notify_device(self.notify_offset, self.index);
        };
    }

    pub fn index(&self) -> u16 {
        self.index
    }

    pub fn used_ring_index(&self) -> u16 {
        self.used_ring.idx.read()
    }

    pub fn get_used_ring_entry(&self, index: u16) -> (VirtqUsedElem, VirtqDescriptor) {
        // Load the used element
        let used_elem = self.used_ring.get_used_elem(index);

        // Load the associated descriptor
        let descriptor = self.descriptors.get_descriptor(used_elem.id as u16);

        (used_elem, descriptor)
    }
}

// See 2.7 Split Virtqueues for alignment
const VIRTQ_DESC_ALIGN: usize = 16;
const VIRTQ_AVAIL_ALIGN: usize = 2;
const VIRTQ_USED_ALIGN: usize = 4;

/// See 2.7.5 The Virtqueue Descriptor Table
pub struct VirtqDescriptorTable {
    /// The physical address for the queue's descriptor table.
    physical_address: u64,

    /// Index into the next open descriptor slot.
    next_index: u16,

    /// Array of descriptors.
    descriptors: &'static mut [VirtqDescriptor],
}

impl VirtqDescriptorTable {
    unsafe fn allocate(
        queue_size: u16,
        physical_allocator: &impl Allocator,
    ) -> Result<Self, AllocZeroedBufferError> {
        let queue_size = queue_size as usize;

        let mem_size = mem::size_of::<VirtqDescriptor>() * queue_size;

        // Check that this matches the spec. See 2.7 Split Virtqueues
        assert_eq!(
            mem_size,
            16 * queue_size,
            "Descriptor table size doesn't match the spec"
        );

        let physical_address =
            memory::allocate_zeroed_buffer(physical_allocator, mem_size, VIRTQ_DESC_ALIGN)?;

        let descriptors =
            core::slice::from_raw_parts_mut(physical_address as *mut VirtqDescriptor, mem_size);

        Ok(Self {
            physical_address,
            next_index: 0,
            descriptors,
        })
    }

    fn add_descriptor(
        &mut self,
        buffer_addr: u64,
        buffer_len: u32,
        flags: VirtqDescriptorFlags,
    ) -> u16 {
        // 2.7.13.1 Placing Buffers Into The Descriptor Table
        let desc_index = self.next_index;
        self.next_index = (self.next_index + 1) % self.descriptors.len() as u16;

        let descriptor = VirtqDescriptor {
            addr: buffer_addr,
            len: buffer_len,
            flags,
            next: 0,
        };

        self.descriptors[desc_index as usize] = descriptor;

        desc_index
    }

    fn get_descriptor(&self, index: u16) -> VirtqDescriptor {
        *self
            .descriptors
            .get(index as usize)
            .expect("Invalid descriptor index")
    }
}

impl fmt::Debug for VirtqDescriptorTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VirtqDescriptorTable")
            .field("physical_address", &self.physical_address)
            .field("next_index", &self.next_index)
            .field(
                "descriptors",
                &format_args!("&[{}]", self.descriptors.len()),
            )
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VirtqDescriptor {
    /// Physical address for the buffer.
    pub addr: u64,
    /// Length of the buffer, in bytes.
    pub len: u32,
    pub flags: VirtqDescriptorFlags,
    /// Next field if flags & NEXT
    pub next: u16,
}

#[bitfield(u16)]
pub struct VirtqDescriptorFlags {
    /// This marks a buffer as continuing via the next field.
    pub next: bool,

    /// This marks a buffer as device write-only (otherwise device read-only).
    pub device_write: bool,

    /// This means the buffer contains a list of buffer descriptors.
    pub indirect: bool,

    #[bits(13)]
    __padding: u16,
}

/// Wrapper around the virtq avail (driver -> device) ring. See 2.7.6 The
/// Virtqueue Available Ring
///
/// The driver uses the available ring to offer buffers to the device: each ring
/// entry refers to the head of a descriptor chain. It is only written by the
/// driver and read by the device.
///
/// The struct in the spec is:
///
/// ```ignore
///     struct virtq_avail {
///             le16 flags;
///             le16 idx;
///             le16 ring[];
///             le16 used_event; /* Only if VIRTIO_F_EVENT_IDX: */
///     };
/// ```
pub struct VirtqAvailRing {
    physical_address: u64,

    flags: RegisterRW<VirtqAvailRingFlags>,

    /// idx field indicates where the driver would put the next descriptor entry
    /// in the ring (modulo the queue size). This starts at 0, and increases.
    idx: RegisterRW<u16>,

    ring: &'static mut [u16],

    /// Only if VIRTIO_F_EVENT_IDX
    used_event: RegisterRW<u16>,
}

impl VirtqAvailRing {
    unsafe fn allocate(
        queue_size: u16,
        physical_allocator: &impl Allocator,
    ) -> Result<Self, AllocZeroedBufferError> {
        let queue_size = queue_size as usize;

        // Compute sizes before we do allocations.
        let flags_offset = 0;
        let idx_offset = mem::size_of::<VirtqAvailRingFlags>();
        let ring_offset = idx_offset + mem::size_of::<u16>();
        let ring_len = queue_size * mem::size_of::<u16>();
        let used_event_offset = ring_offset + ring_len;
        let struct_size = used_event_offset + mem::size_of::<u16>();

        // Check that this matches the spec. See 2.7 Split Virtqueues
        assert_eq!(
            struct_size,
            6 + 2 * queue_size,
            "VirtqAvailRing size doesn't match the spec"
        );

        let physical_address =
            memory::allocate_zeroed_buffer(physical_allocator, struct_size, VIRTQ_AVAIL_ALIGN)?;

        let flags = RegisterRW::from_address(physical_address as usize + flags_offset);
        let idx = RegisterRW::from_address(physical_address as usize + idx_offset);
        let ring_address = physical_address as usize + ring_offset;
        let ring = core::slice::from_raw_parts_mut(ring_address as *mut u16, ring_len);
        let used_event = RegisterRW::from_address(physical_address as usize + used_event_offset);

        Ok(Self {
            physical_address,
            flags,
            idx,
            ring,
            used_event,
        })
    }

    fn add_entry(&mut self, desc_index: u16) {
        // 2.7.13.2 Updating The Available Ring
        let idx = self.idx.read();
        self.ring[idx as usize] = desc_index;

        // 2.7.13.3 Updating idx
        self.idx.modify(|idx| idx.wrapping_add(1));
    }
}

impl fmt::Debug for VirtqAvailRing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VirtqAvailRing")
            .field("physical_address", &self.physical_address)
            .field("flags", &self.flags)
            .field("idx", &self.idx)
            .field("ring", &format_args!("&[{}]", self.ring.len()))
            .field("used_event", &self.used_event)
            .finish()
    }
}

#[bitfield(u16)]
pub struct VirtqAvailRingFlags {
    /// See 2.7.7 Used Buffer Notification Suppression
    no_interrupt: bool,

    #[bits(15)]
    __reserved: u16,
}

/// Wrapper around the virtq used (device -> drive) ring. See 2.7.8 The
/// Virtqueue Used Ring.
///
/// The used ring is where the device returns buffers once it is done with them:
/// it is only written to by the device, and read by the driver.
///
/// The struct in the spec is:
///
/// ```ignore
/// struct virtq_used {
///         le16 flags;
///         le16 idx;
///         struct virtq_used_elem ring[];
///         le16 avail_event; /* Only if VIRTIO_F_EVENT_IDX */
/// };
/// ```
pub struct VirtqUsedRing {
    physical_address: u64,

    flags: RegisterRW<VirtqUsedRingFlags>,

    /// idx field indicates where the device would put the next descriptor entry
    /// in the ring (modulo the queue size). This starts at 0, and increases.
    idx: RegisterRW<u16>,

    ring: &'static mut [VirtqUsedElem],

    /// Only if VIRTIO_F_EVENT_IDX
    avail_event: RegisterRW<u16>,
}

#[bitfield(u16)]
pub struct VirtqUsedRingFlags {
    /// See 2.7.10 Available Buffer Notification Suppression
    no_notify: bool,

    #[bits(15)]
    __reserved: u16,
}

/// 2.7.8 The Virtqueue Used Ring
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VirtqUsedElem {
    /// Index of start of used descriptor chain.
    pub id: u32,

    /// The number of bytes written into the device writable portion of the
    /// buffer described by the descriptor chain.
    pub len: u32,
}

impl VirtqUsedRing {
    unsafe fn allocate(
        queue_size: u16,
        physical_allocator: &impl Allocator,
    ) -> Result<Self, AllocZeroedBufferError> {
        let queue_size = queue_size as usize;

        // Compute sizes before we do allocations.
        let flags_offset = 0;
        let idx_offset = mem::size_of::<VirtqUsedRingFlags>();
        let ring_offset = idx_offset + mem::size_of::<u16>();
        let ring_len = queue_size * mem::size_of::<VirtqUsedElem>();
        let avail_event_offset = ring_offset + ring_len;
        let struct_size = avail_event_offset + mem::size_of::<u16>();

        // Check that this matches the spec. See 2.7 Split Virtqueues
        assert_eq!(
            struct_size,
            6 + 8 * queue_size,
            "VirtqUsedRing size doesn't match the spec"
        );

        let physical_address =
            memory::allocate_zeroed_buffer(physical_allocator, struct_size, VIRTQ_USED_ALIGN)?;

        let flags = RegisterRW::from_address(physical_address as usize + flags_offset);
        let idx = RegisterRW::from_address(physical_address as usize + idx_offset);
        let ring_address = physical_address as usize + ring_offset;
        let ring = core::slice::from_raw_parts_mut(ring_address as *mut VirtqUsedElem, ring_len);
        let avail_event = RegisterRW::from_address(physical_address as usize + avail_event_offset);

        Ok(Self {
            physical_address,
            flags,
            idx,
            ring,
            avail_event,
        })
    }

    fn get_used_elem(&self, idx: u16) -> VirtqUsedElem {
        *self
            .ring
            .get(idx as usize)
            .expect("virt queue used elem idx out of bounds")
    }
}

impl fmt::Debug for VirtqUsedRing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VirtqUsedRing")
            .field("physical_address", &self.physical_address)
            .field("flags", &self.flags)
            .field("idx", &self.idx)
            .field("ring", &format_args!("&[{}]", self.ring.len()))
            .field("avail_event", &self.avail_event)
            .finish()
    }
}
