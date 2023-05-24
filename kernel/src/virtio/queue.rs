use core::mem;
use core::sync::atomic::AtomicU16;

use bitfield_struct::bitfield;

use crate::memory;
use crate::memory::AllocZeroedBufferError;
use crate::registers::{RegisterRW, VolatileArrayRW};

use super::config::VirtIONotifyConfig;

/// Wrapper around allocated virt queues for a an initialized VirtIO device.
#[derive(Debug)]
pub(super) struct VirtQueue {
    /// The queue's location in the device's virtqueue array.
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
    pub(super) fn new(
        index: u16,
        device_notify_config: VirtIONotifyConfig,
        notify_offset: u16,
        descriptors: VirtqDescriptorTable,
        avail_ring: VirtqAvailRing,
        used_ring: VirtqUsedRing,
    ) -> Self {
        Self {
            index,
            device_notify_config,
            notify_offset,
            descriptors,
            avail_ring,
            used_ring,
        }
    }

    /// See "2.7.13 Supplying Buffers to The Device"
    pub(super) fn add_buffer(
        &self,
        buffer_addr: u64,
        buffer_len: u32,
        flags: VirtqDescriptorFlags,
    ) {
        let desc_index = self
            .descriptors
            .add_descriptor(buffer_addr, buffer_len, flags);
        self.avail_ring.add_entry(desc_index);
        unsafe {
            self.device_notify_config
                .notify_device(self.notify_offset, self.index);
        };
    }

    pub(super) fn used_ring_index(&self) -> u16 {
        self.used_ring.idx.read()
    }

    pub(super) fn get_used_ring_entry(&self, index: u16) -> (VirtqUsedElem, VirtqDescriptor) {
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
#[derive(Debug)]
pub(super) struct VirtqDescriptorTable {
    /// The physical address for the queue's descriptor table.
    physical_address: u64,

    /// Index into the next open descriptor slot.
    ///
    /// This is atomic so multiple writes can safely use the virtqueue.
    raw_next_index: AtomicU16,

    /// Array of descriptors.
    descriptors: VolatileArrayRW<VirtqDescriptor>,
}

impl VirtqDescriptorTable {
    pub(super) unsafe fn allocate(queue_size: u16) -> Result<Self, AllocZeroedBufferError> {
        let queue_size = queue_size as usize;

        let mem_size = mem::size_of::<VirtqDescriptor>() * queue_size;

        // Check that this matches the spec. See 2.7 Split Virtqueues
        assert_eq!(
            mem_size,
            16 * queue_size,
            "Descriptor table size doesn't match the spec"
        );

        // VirtIO buffers must be physically contiguous, and they use physical
        // addresses.
        let physical_address =
            memory::allocate_physically_contiguous_zeroed_buffer(mem_size, VIRTQ_DESC_ALIGN)?
                .as_u64();

        let descriptors = VolatileArrayRW::new(physical_address as usize, queue_size);

        Ok(Self {
            physical_address,
            raw_next_index: AtomicU16::new(0),
            descriptors,
        })
    }

    pub(super) fn physical_address(&self) -> u64 {
        self.physical_address
    }

    /// Atomically increments the internal index and performs the necessary wrapping.
    fn next_index(&self) -> u16 {
        let index = self
            .raw_next_index
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // N.B. Assumes that the queue size is a power of 2, which is in the
        // virtio spec. Specifically, when we do fetch_add above, it performs a
        // wraparound when we max out u16, and this modulo logic only works in
        // conjunction with that when the queue size is a power of 2.
        index % self.descriptors.len() as u16
    }

    fn add_descriptor(
        &self,
        buffer_addr: u64,
        buffer_len: u32,
        flags: VirtqDescriptorFlags,
    ) -> u16 {
        // 2.7.13.1 Placing Buffers Into The Descriptor Table
        let desc_index = self.next_index();

        let descriptor = VirtqDescriptor {
            addr: buffer_addr,
            len: buffer_len,
            flags,
            next: 0,
        };

        self.descriptors.write(desc_index as usize, descriptor);

        desc_index
    }

    fn get_descriptor(&self, index: u16) -> VirtqDescriptor {
        self.descriptors.read(index as usize)
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub(super) struct VirtqDescriptor {
    /// Physical address for the buffer.
    pub(super) addr: u64,
    /// Length of the buffer, in bytes.
    pub(super) len: u32,
    pub(super) flags: VirtqDescriptorFlags,
    /// Next field if flags & NEXT
    pub(super) next: u16,
}

#[bitfield(u16)]
pub(super) struct VirtqDescriptorFlags {
    /// This marks a buffer as continuing via the next field.
    pub(super) next: bool,

    /// This marks a buffer as device write-only (otherwise device read-only).
    pub(super) device_write: bool,

    /// This means the buffer contains a list of buffer descriptors.
    pub(super) indirect: bool,

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
#[derive(Debug)]
pub(super) struct VirtqAvailRing {
    physical_address: u64,

    _flags: RegisterRW<VirtqAvailRingFlags>,

    /// idx field indicates where the driver would put the next descriptor entry
    /// in the ring (modulo the queue size). This starts at 0, and increases.
    idx: RegisterRW<u16>,

    ring: VolatileArrayRW<u16>,

    /// Only if VIRTIO_F_EVENT_IDX
    _used_event: RegisterRW<u16>,
}

impl VirtqAvailRing {
    pub(super) unsafe fn allocate(queue_size: u16) -> Result<Self, AllocZeroedBufferError> {
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

        // VirtIO buffers must be physically contiguous, and they use physical
        // addresses.
        let physical_address =
            memory::allocate_physically_contiguous_zeroed_buffer(struct_size, VIRTQ_AVAIL_ALIGN)?
                .as_u64();

        let flags = RegisterRW::from_address(physical_address as usize + flags_offset);
        let idx = RegisterRW::from_address(physical_address as usize + idx_offset);
        let ring_address = physical_address as usize + ring_offset;
        let ring = VolatileArrayRW::new(ring_address, queue_size);
        let used_event = RegisterRW::from_address(physical_address as usize + used_event_offset);

        Ok(Self {
            physical_address,
            _flags: flags,
            idx,
            ring,
            _used_event: used_event,
        })
    }

    pub(super) fn physical_address(&self) -> u64 {
        self.physical_address
    }

    fn add_entry(&self, desc_index: u16) {
        // 2.7.13.2 Updating The Available Ring
        //
        // TODO: Check that the driver doesn't add more entries than are
        // actually available. We don't want to overwrite the end of ring
        // buffer. This likely needs to be done at a higher level.
        let idx = self.idx.read() % self.ring.len() as u16;
        self.ring.write(idx as usize, desc_index);

        // 2.7.13.3 Updating idx
        self.idx.modify(|idx| idx.wrapping_add(1));
    }
}

#[bitfield(u16)]
pub(super) struct VirtqAvailRingFlags {
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
#[derive(Debug)]
pub(super) struct VirtqUsedRing {
    physical_address: u64,

    _flags: RegisterRW<VirtqUsedRingFlags>,

    /// idx field indicates where the device would put the next descriptor entry
    /// in the ring (modulo the queue size). This starts at 0, and increases.
    idx: RegisterRW<u16>,

    ring: VolatileArrayRW<VirtqUsedElem>,

    /// Only if VIRTIO_F_EVENT_IDX
    _avail_event: RegisterRW<u16>,
}

#[bitfield(u16)]
pub(super) struct VirtqUsedRingFlags {
    /// See 2.7.10 Available Buffer Notification Suppression
    no_notify: bool,

    #[bits(15)]
    __reserved: u16,
}

/// 2.7.8 The Virtqueue Used Ring
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub(super) struct VirtqUsedElem {
    /// Index of start of used descriptor chain.
    pub(super) id: u32,

    /// The number of bytes written into the device writable portion of the
    /// buffer described by the descriptor chain.
    pub(super) len: u32,
}

impl VirtqUsedRing {
    pub(super) unsafe fn allocate(queue_size: u16) -> Result<Self, AllocZeroedBufferError> {
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

        // VirtIO buffers must be physically contiguous, and they use physical
        // addresses.
        let physical_address =
            memory::allocate_physically_contiguous_zeroed_buffer(struct_size, VIRTQ_USED_ALIGN)?
                .as_u64();

        let flags = RegisterRW::from_address(physical_address as usize + flags_offset);
        let idx = RegisterRW::from_address(physical_address as usize + idx_offset);
        let ring_address = physical_address as usize + ring_offset;
        let ring = VolatileArrayRW::new(ring_address, queue_size);
        let avail_event = RegisterRW::from_address(physical_address as usize + avail_event_offset);

        Ok(Self {
            physical_address,
            _flags: flags,
            idx,
            ring,
            _avail_event: avail_event,
        })
    }

    pub(super) fn physical_address(&self) -> u64 {
        self.physical_address
    }

    fn get_used_elem(&self, idx: u16) -> VirtqUsedElem {
        self.ring.read(idx as usize)
    }
}
