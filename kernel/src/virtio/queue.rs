use alloc::vec::Vec;
use core::alloc::AllocError;
use core::mem;
use core::sync::atomic::{AtomicU16, Ordering};

use bitfield_struct::bitfield;
use x86_64::PhysAddr;

use crate::barrier::barrier;
use crate::memory::PhysicalBuffer;
use crate::registers::{RegisterRW, VolatileArrayRW};

use super::config::VirtIONotifyConfig;

/// Wrapper around `VirtQueue` that supports associating extra data with each
/// descriptor.
#[derive(Debug)]
pub(super) struct VirtQueueData<D> {
    data: Vec<Option<D>>,
    queue: VirtQueue,
}

impl<D> VirtQueueData<D> {
    pub(super) fn new(queue: VirtQueue) -> Self {
        let queue_size = queue.descriptors.descriptors.len();
        let mut data = Vec::with_capacity(queue_size);
        for _ in 0..queue_size {
            data.push(None);
        }

        Self { data, queue }
    }

    pub(super) fn index(&self) -> VirtQueueIndex {
        self.queue.index
    }

    /// See "2.7.13 Supplying Buffers to The Device"
    ///
    /// The caller must also call `notify_device` once they are done adding
    /// buffers.
    pub(super) fn add_buffer(&mut self, descriptors: &[ChainedVirtQueueDescriptorElem], data: D) {
        let desc_index = self.queue.descriptors.add_descriptor(descriptors);

        // Important to put the data in the vector before adding the descriptor
        // index to the available ring. Otherwise, the device might read the
        // descriptor index and trigger and interrupt before we put the data in
        // the vector.
        let previous_data = self.data[desc_index.0 as usize].replace(data);
        assert!(
            previous_data.is_none(),
            "descriptor index {} already has data",
            desc_index.0
        );

        self.queue.avail_ring.add_entry(desc_index);
    }

    pub(super) fn process_new_entries<F>(&mut self, mut f: F)
    where
        F: FnMut(VirtQueueUsedElem, VirtQueueDescriptorChainIterator, Option<D>),
    {
        self.queue
            .process_new_entries(|used_entry, descriptor_chain| {
                let desc_index = used_entry.desc_index();
                let data = self.data[desc_index.0 as usize].take();
                f(used_entry, descriptor_chain, data);
            });
    }

    /// Notify the device that we have written new data.
    pub(super) fn notify_device(&self) {
        self.queue.notify_device();
    }
}

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
    /// driver.
    last_processed_used_index: WrappingIndex,
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
            last_processed_used_index: WrappingIndex(0),
        }
    }

    /// Notify the device that we have written new data.
    fn notify_device(&self) {
        // Ensures that any previous writes to the virtqueue are visible to the
        // device before we notify it.
        barrier();
        unsafe {
            self.device_notify_config
                .notify_device(self.notify_offset, self.index);
        };
    }

    fn process_new_entries<F>(&mut self, mut f: F)
    where
        F: FnMut(VirtQueueUsedElem, VirtQueueDescriptorChainIterator),
    {
        let used_index = self.used_ring.idx.read();

        let last_processed = self.last_processed_used_index;
        for i in last_processed.0..used_index.0 {
            let i = WrappingIndex(i);
            let (used_elem, chain) = self.get_used_ring_entry(i);
            f(used_elem, chain);
        }

        self.last_processed_used_index = used_index;
    }

    fn get_used_ring_entry(
        &self,
        index: WrappingIndex,
    ) -> (VirtQueueUsedElem, VirtQueueDescriptorChainIterator) {
        // Load the used element
        let used_elem = self.used_ring.get_used_elem(index);

        // Load the associated descriptor
        let idx = used_elem.desc_index();
        let chain = VirtQueueDescriptorChainIterator::new(&self.descriptors, idx);

        (used_elem, chain)
    }
}

// See 2.7 Split Virtqueues for alignment
const _VIRTQ_DESC_ALIGN: usize = 16;
const _VIRTQ_AVAIL_ALIGN: usize = 2;
const _VIRTQ_USED_ALIGN: usize = 4;

/// See 2.7.5 The Virtqueue Descriptor Table
#[derive(Debug)]
pub(super) struct VirtQueueDescriptorTable {
    /// Physically contiguous buffer containing the descriptor table.
    buffer: PhysicalBuffer,

    /// Index into the next open descriptor slot.
    ///
    /// This is atomic so multiple writes can safely use the virtqueue.
    raw_next_index: AtomicU16,

    /// Array of descriptors.
    descriptors: VolatileArrayRW<RawVirtQueueDescriptor>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub(super) struct DescIndex(u16);

impl VirtQueueDescriptorTable {
    pub(super) unsafe fn allocate(queue_size: u16) -> Result<Self, AllocError> {
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
        let buffer = PhysicalBuffer::allocate_zeroed(mem_size)?;

        let descriptors = VolatileArrayRW::new(buffer.address(), queue_size);

        Ok(Self {
            buffer,
            raw_next_index: AtomicU16::new(0),
            descriptors,
        })
    }

    pub(super) fn physical_address(&self) -> PhysAddr {
        PhysAddr::from(self.buffer.address())
    }

    /// Atomically increments the internal index and performs the necessary wrapping.
    fn next_index(&self) -> WrappingIndex {
        WrappingIndex(self.raw_next_index.fetch_add(1, Ordering::Relaxed))
    }

    /// Adds a group of chained descriptors to the descriptor table. Returns the
    /// index of the first descriptor.
    fn add_descriptor(&mut self, descriptors: &[ChainedVirtQueueDescriptorElem]) -> DescIndex {
        let mut first_idx: Option<DescIndex> = None;
        let mut prev_idx: Option<DescIndex> = None;

        for desc in descriptors.iter() {
            let idx = self
                .next_index()
                .to_elem_index(self.descriptors.len() as u16);
            let idx = DescIndex(idx);
            first_idx.get_or_insert(idx);

            // Modify the previous descriptor to point to this one.
            if let Some(prev_index) = prev_idx {
                self.descriptors.modify_mut(prev_index.0 as usize, |desc| {
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
                next: DescIndex(0),
            };

            self.descriptors.write(idx.0 as usize, descriptor);

            prev_idx = Some(idx);
        }

        first_idx.expect("can't add empty descriptor")
    }

    fn get_descriptor(&self, index: DescIndex) -> RawVirtQueueDescriptor {
        self.descriptors.read(index.0 as usize)
    }
}

/// See "2.7.5 The Virtqueue Descriptor Table". This is a virtqueue descriptor
/// without the `next` field, and is meant to be used in an array to represent a
/// single descriptor.
#[derive(Debug)]
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
    current_index: Option<DescIndex>,
}

impl VirtQueueDescriptorChainIterator<'_> {
    pub(super) fn new(
        descriptors: &VirtQueueDescriptorTable,
        start_index: DescIndex,
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

#[derive(Debug, Clone)]
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
    pub(super) next: DescIndex,
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
    buffer: PhysicalBuffer,

    _flags: RegisterRW<VirtQueueAvailRingFlags>,

    /// idx field indicates where the driver would put the next descriptor entry
    /// in the ring (modulo the queue size). This starts at 0, and increases.
    idx: RegisterRW<WrappingIndex>,

    ring: VolatileArrayRW<DescIndex>,

    /// Only if VIRTIO_F_EVENT_IDX
    _used_event: RegisterRW<u16>,
}

impl VirtQueueAvailRing {
    pub(super) unsafe fn allocate(queue_size: u16) -> Result<Self, AllocError> {
        let queue_size = queue_size as usize;

        // Compute sizes before we do allocations.
        let flags_offset = 0_u64;
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
        let buffer = PhysicalBuffer::allocate_zeroed(struct_size)?;

        let addr = buffer.address();
        let flags = RegisterRW::from_address(addr + flags_offset);
        let idx = RegisterRW::from_address(addr + idx_offset);
        let ring = VolatileArrayRW::new(addr + ring_offset, queue_size);
        let used_event = RegisterRW::from_address(addr + used_event_offset);

        Ok(Self {
            buffer,
            _flags: flags,
            idx,
            ring,
            _used_event: used_event,
        })
    }

    pub(super) fn physical_address(&self) -> PhysAddr {
        PhysAddr::from(self.buffer.address())
    }

    fn add_entry(&mut self, desc_index: DescIndex) {
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
    buffer: PhysicalBuffer,

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
    id: u32,

    /// The number of bytes written into the device writable portion of the
    /// buffer described by the descriptor chain.
    pub(super) len: u32,
}

impl VirtQueueUsedElem {
    // For some reason, the spec says that the id field is a u32, but descriptor
    // indices are u16.
    fn desc_index(self) -> DescIndex {
        match u16::try_from(self.id) {
            Ok(idx) => DescIndex(idx),
            Err(e) => panic!("used ring id entry {} is invalid: {e}", self.id),
        }
    }
}

impl VirtQueueUsedRing {
    pub(super) unsafe fn allocate(queue_size: u16) -> Result<Self, AllocError> {
        let queue_size = queue_size as usize;

        // Compute sizes before we do allocations.
        let flags_offset = 0_u64;
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
        let buffer = PhysicalBuffer::allocate_zeroed(struct_size)?;

        let addr = buffer.address();
        let flags = RegisterRW::from_address(addr + flags_offset);
        let idx = RegisterRW::from_address(addr + idx_offset);
        let ring = VolatileArrayRW::new(addr + ring_offset, queue_size);
        let avail_event = RegisterRW::from_address(addr + avail_event_offset);

        Ok(Self {
            buffer,
            _flags: flags,
            idx,
            ring,
            _avail_event: avail_event,
        })
    }

    pub(super) fn physical_address(&self) -> PhysAddr {
        PhysAddr::from(self.buffer.address())
    }

    fn get_used_elem(&self, idx: WrappingIndex) -> VirtQueueUsedElem {
        let elem_idx = idx.to_elem_index(self.ring.len() as u16);
        self.ring.read(elem_idx as usize)
    }
}
