use core::mem;
use core::sync::atomic::{AtomicU16, Ordering};

use bitfield_struct::bitfield;
use spin::Mutex;
use x86_64::PhysAddr;

use crate::barrier::barrier;
use crate::memory;
use crate::memory::AllocZeroedBufferError;
use crate::registers::{RegisterRW, VolatileArrayRW};

use super::config::VirtIONotifyConfig;

/// Wrapper around allocated virt queues for a an initialized VirtIO device.
#[derive(Debug)]
pub(super) struct VirtQueue {
    /// The queue's location in the device's virtqueue array.
    index: VirtQueueIndex,

    /// Device's notification config, inlined here to compute the notification
    /// address. See "4.1.4.4 Notification structure layout".
    device_notify_config: VirtIONotifyConfig,

    /// The queue's notification offset. See "4.1.4.4 Notification structure
    /// layout".
    notify_offset: u16,

    descriptors: VirtQueueDescriptorTable,
    avail_ring: VirtQueueAvailRing,
    used_ring: VirtQueueUsedRing,

    /// Used to record how many used ring entries have been processed by the
    /// driver. This is a mutex so we can ensure multiple threads using the
    /// driver don't process the same entries.
    last_processed_used_index: Mutex<WrappingIndex>,
}

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
/// The virtqueue's index within a device's virtqueue array. That is, the
/// virtqueue "number" for that device.
pub(super) struct VirtQueueIndex(pub(super) u16);

/// Index that wraps around when it reaches the virtqueue's size.
///
/// This type exists to ensure we don't accidentally try and index into an array
/// with the wrapping index. To actually index into a ring or descriptor table,
/// you need to take the mod of this index with the virtqueue's size.
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
struct WrappingIndex(u16);

impl WrappingIndex {
    fn increment(self) -> Self {
        Self(self.0.wrapping_add(1))
    }

    fn to_elem_index(self, queue_size: u16) -> u16 {
        // N.B. Assumes that the queue size is a power of 2, which is in the
        // virtio spec. Specifically, when we increment this index, it performs
        // a wraparound when we max out u16, and this modulo logic only works in
        // conjunction with that when the queue size is a power of 2.
        assert!(
            queue_size.is_power_of_two(),
            "queue size must be a power of two for index wrapping to work"
        );

        self.0 % queue_size
    }
}

impl VirtQueue {
    pub(super) fn new(
        index: VirtQueueIndex,
        device_notify_config: VirtIONotifyConfig,
        notify_offset: u16,
        descriptors: VirtQueueDescriptorTable,
        avail_ring: VirtQueueAvailRing,
        used_ring: VirtQueueUsedRing,
    ) -> Self {
        Self {
            index,
            device_notify_config,
            notify_offset,
            descriptors,
            avail_ring,
            used_ring,
            last_processed_used_index: Mutex::new(WrappingIndex(0)),
        }
    }

    /// See "2.7.13 Supplying Buffers to The Device"
    pub(super) fn add_buffer(&self, descriptors: &[ChainedVirtQueueDescriptorElem]) {
        let desc_index = self.descriptors.add_descriptor(descriptors);
        self.avail_ring.add_entry(desc_index);
        barrier();
        unsafe {
            self.device_notify_config
                .notify_device(self.notify_offset, self.index);
        };
    }

    pub(super) fn process_new_entries<F>(&self, mut f: F)
    where
        F: FnMut(VirtQueueUsedElem, VirtQueueDescriptorChainIterator),
    {
        let mut last_processed_lock = self.last_processed_used_index.lock();
        let last_processed = *last_processed_lock;
        let used_index = self.used_ring.idx.read();

        for i in last_processed.0..used_index.0 {
            let i = WrappingIndex(i);
            let (used_elem, chain) = self.get_used_ring_entry(i);
            f(used_elem, chain);
        }

        *last_processed_lock = used_index;
    }

    fn get_used_ring_entry(
        &self,
        index: WrappingIndex,
    ) -> (VirtQueueUsedElem, VirtQueueDescriptorChainIterator) {
        // Load the used element
        let used_elem = self.used_ring.get_used_elem(index);

        // Load the associated descriptor
        let chain = VirtQueueDescriptorChainIterator::new(&self.descriptors, used_elem.id as u16);

        (used_elem, chain)
    }
}

// See 2.7 Split Virtqueues for alignment
const VIRTQ_DESC_ALIGN: usize = 16;
const VIRTQ_AVAIL_ALIGN: usize = 2;
const VIRTQ_USED_ALIGN: usize = 4;

/// See 2.7.5 The Virtqueue Descriptor Table
#[derive(Debug)]
pub(super) struct VirtQueueDescriptorTable {
    /// The physical address for the queue's descriptor table.
    physical_address: PhysAddr,

    /// Index into the next open descriptor slot.
    ///
    /// This is atomic so multiple writes can safely use the virtqueue.
    raw_next_index: AtomicU16,

    /// Array of descriptors.
    descriptors: VolatileArrayRW<RawVirtQueueDescriptor>,
}

impl VirtQueueDescriptorTable {
    pub(super) unsafe fn allocate(queue_size: u16) -> Result<Self, AllocZeroedBufferError> {
        let queue_size = queue_size as usize;

        // Queue size being a power of 2 is in the spec, and is important for
        // the wrapping logic to work.
        assert!(
            queue_size.is_power_of_two(),
            "queue size must be a power of two for index wrapping to work"
        );

        let mem_size = mem::size_of::<RawVirtQueueDescriptor>() * queue_size;

        // Check that this matches the spec. See 2.7 Split Virtqueues
        assert_eq!(
            mem_size,
            16 * queue_size,
            "Descriptor table size doesn't match the spec"
        );

        // VirtIO buffers must be physically contiguous, and they use physical
        // addresses.
        let physical_address =
            memory::allocate_physically_contiguous_zeroed_buffer(mem_size, VIRTQ_DESC_ALIGN)?;

        let descriptors = VolatileArrayRW::new(physical_address.as_u64() as usize, queue_size);

        Ok(Self {
            physical_address,
            raw_next_index: AtomicU16::new(0),
            descriptors,
        })
    }

    pub(super) fn physical_address(&self) -> PhysAddr {
        self.physical_address
    }

    /// Atomically increments the internal index and performs the necessary wrapping.
    fn next_index(&self) -> WrappingIndex {
        WrappingIndex(self.raw_next_index.fetch_add(1, Ordering::Relaxed))
    }

    /// Adds a group of chained descriptors to the descriptor table. Returns the
    /// index of the first descriptor.
    fn add_descriptor(&self, descriptors: &[ChainedVirtQueueDescriptorElem]) -> u16 {
        let mut first_idx: Option<u16> = None;
        let mut prev_idx: Option<u16> = None;

        for desc in descriptors.iter() {
            let idx = self.next_index().to_elem_index(self.descriptors.len() as u16);
            first_idx.get_or_insert(idx);

            // Modify the previous descriptor to point to this one.
            if let Some(prev_index) = prev_idx {
                self.descriptors.modify_mut(prev_index as usize, |desc| {
                    desc.flags.set_next(true);
                    desc.next = idx;
                });
            }

            assert!(
                !desc.flags.indirect(),
                "ChainedVirtQueueDescriptorElem should not set the INDIRECT flag"
            );

            let descriptor = RawVirtQueueDescriptor {
                addr: desc.addr,
                len: desc.len,
                flags: desc.flags,
                next: 0,
            };

            self.descriptors.write(idx as usize, descriptor);

            prev_idx = Some(idx);
        }

        first_idx.expect("can't add empty descriptor")
    }

    fn get_descriptor(&self, index: u16) -> RawVirtQueueDescriptor {
        self.descriptors.read(index as usize)
    }
}

/// See "2.7.5 The Virtqueue Descriptor Table". This is a virtqueue descriptor
/// without the `next` field, and is meant to be used in an array to represent a
/// single descriptor.
pub(super) struct ChainedVirtQueueDescriptorElem {
    /// Physical address for the buffer.
    pub(super) addr: PhysAddr,
    /// Length of the buffer, in bytes.
    pub(super) len: u32,
    pub(super) flags: VirtQueueDescriptorFlags,
}

/// Iterator over a chain of descriptors.
pub(super) struct VirtQueueDescriptorChainIterator<'a> {
    descriptors: &'a VirtQueueDescriptorTable,
    current_index: Option<u16>,
}

impl VirtQueueDescriptorChainIterator<'_> {
    pub(super) fn new(
        descriptors: &VirtQueueDescriptorTable,
        start_index: u16,
    ) -> VirtQueueDescriptorChainIterator {
        VirtQueueDescriptorChainIterator {
            descriptors,
            current_index: Some(start_index),
        }
    }
}

impl Iterator for VirtQueueDescriptorChainIterator<'_> {
    type Item = ChainedVirtQueueDescriptorElem;

    fn next(&mut self) -> Option<ChainedVirtQueueDescriptorElem> {
        let current_index = self.current_index?;
        let descriptor = self.descriptors.get_descriptor(current_index);

        if descriptor.flags.next() {
            self.current_index = Some(descriptor.next);
        } else {
            self.current_index = None;
        }

        Some(ChainedVirtQueueDescriptorElem {
            addr: descriptor.addr,
            len: descriptor.len,
            flags: descriptor.flags,
        })
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
/// See "2.7.5 The Virtqueue Descriptor Table". This is "raw" because we have
/// `ChainedVirtQueueDescriptor`
struct RawVirtQueueDescriptor {
    /// Physical address for the buffer.
    pub(super) addr: PhysAddr,
    /// Length of the buffer, in bytes.
    pub(super) len: u32,
    pub(super) flags: VirtQueueDescriptorFlags,
    /// Next field if flags & NEXT
    pub(super) next: u16,
}

#[bitfield(u16)]
pub(super) struct VirtQueueDescriptorFlags {
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
pub(super) struct VirtQueueAvailRing {
    physical_address: PhysAddr,

    _flags: RegisterRW<VirtQueueAvailRingFlags>,

    /// idx field indicates where the driver would put the next descriptor entry
    /// in the ring (modulo the queue size). This starts at 0, and increases.
    idx: RegisterRW<WrappingIndex>,

    ring: VolatileArrayRW<u16>,

    /// Only if VIRTIO_F_EVENT_IDX
    _used_event: RegisterRW<u16>,
}

impl VirtQueueAvailRing {
    pub(super) unsafe fn allocate(queue_size: u16) -> Result<Self, AllocZeroedBufferError> {
        let queue_size = queue_size as usize;

        // Compute sizes before we do allocations.
        let flags_offset = 0;
        let idx_offset = mem::size_of::<VirtQueueAvailRingFlags>();
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
            memory::allocate_physically_contiguous_zeroed_buffer(struct_size, VIRTQ_AVAIL_ALIGN)?;

        let phys_addr_usize = physical_address.as_u64() as usize;
        let flags = RegisterRW::from_address(phys_addr_usize + flags_offset);
        let idx = RegisterRW::from_address(phys_addr_usize + idx_offset);
        let ring_address = phys_addr_usize + ring_offset;
        let ring = VolatileArrayRW::new(ring_address, queue_size);
        let used_event = RegisterRW::from_address(phys_addr_usize + used_event_offset);

        Ok(Self {
            physical_address,
            _flags: flags,
            idx,
            ring,
            _used_event: used_event,
        })
    }

    pub(super) fn physical_address(&self) -> PhysAddr {
        self.physical_address
    }

    fn add_entry(&self, desc_index: u16) {
        // 2.7.13.2 Updating The Available Ring
        //
        // TODO: Check that the driver doesn't add more entries than are
        // actually available. We don't want to overwrite the end of ring
        // buffer. This likely needs to be done at a higher level.
        let idx = self.idx.read().0 % self.ring.len() as u16;
        self.ring.write(idx as usize, desc_index);

        // 2.7.13.3 Updating idx
        barrier();
        self.idx.modify(WrappingIndex::increment);
    }
}

#[bitfield(u16)]
pub(super) struct VirtQueueAvailRingFlags {
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
pub(super) struct VirtQueueUsedRing {
    physical_address: PhysAddr,

    _flags: RegisterRW<VirtQueueUsedRingFlags>,

    /// idx field indicates where the device would put the next descriptor entry
    /// in the ring (modulo the queue size). This starts at 0, and increases.
    idx: RegisterRW<WrappingIndex>,

    ring: VolatileArrayRW<VirtQueueUsedElem>,

    /// Only if VIRTIO_F_EVENT_IDX
    _avail_event: RegisterRW<u16>,
}

#[bitfield(u16)]
pub(super) struct VirtQueueUsedRingFlags {
    /// See 2.7.10 Available Buffer Notification Suppression
    no_notify: bool,

    #[bits(15)]
    __reserved: u16,
}

/// 2.7.8 The Virtqueue Used Ring
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub(super) struct VirtQueueUsedElem {
    /// Index of start of used descriptor chain.
    pub(super) id: u32,

    /// The number of bytes written into the device writable portion of the
    /// buffer described by the descriptor chain.
    pub(super) len: u32,
}

impl VirtQueueUsedRing {
    pub(super) unsafe fn allocate(queue_size: u16) -> Result<Self, AllocZeroedBufferError> {
        let queue_size = queue_size as usize;

        // Compute sizes before we do allocations.
        let flags_offset = 0;
        let idx_offset = mem::size_of::<VirtQueueUsedRingFlags>();
        let ring_offset = idx_offset + mem::size_of::<u16>();
        let ring_len = queue_size * mem::size_of::<VirtQueueUsedElem>();
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
            memory::allocate_physically_contiguous_zeroed_buffer(struct_size, VIRTQ_USED_ALIGN)?;

        let phys_addr_usize = physical_address.as_u64() as usize;
        let flags = RegisterRW::from_address(phys_addr_usize + flags_offset);
        let idx = RegisterRW::from_address(phys_addr_usize + idx_offset);
        let ring_address = phys_addr_usize + ring_offset;
        let ring = VolatileArrayRW::new(ring_address, queue_size);
        let avail_event = RegisterRW::from_address(phys_addr_usize + avail_event_offset);

        Ok(Self {
            physical_address,
            _flags: flags,
            idx,
            ring,
            _avail_event: avail_event,
        })
    }

    pub(super) fn physical_address(&self) -> PhysAddr {
        self.physical_address
    }

    fn get_used_elem(&self, idx: WrappingIndex) -> VirtQueueUsedElem {
        let elem_idx = idx.to_elem_index(self.ring.len() as u16);
        self.ring.read(elem_idx as usize)
    }
}
