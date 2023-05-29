use alloc::sync::Arc;
use alloc::vec::Vec;
use bitflags::bitflags;
use hashbrown::HashMap;
use spin::Mutex;

use crate::interrupts::InterruptHandlerID;
use crate::sync::InitCell;
use crate::{memory, serial_println};

use super::device::VirtIOInitializedDevice;
use super::queue::{
    ChainedVirtQueueDescriptorElem, DescIndex, VirtQueueDescriptorFlags, VirtQueueIndex,
};
use super::VirtIODeviceConfig;

static VIRTIO_RNG: InitCell<VirtIORNG> = InitCell::new();

pub(crate) fn try_init_virtio_rng(device_config: VirtIODeviceConfig) {
    let device_id = device_config.pci_config().device_id();
    if device_id.vendor_id() != 0x1af4 {
        return;
    }
    if !VirtIORNG::VENDOR_IDS.contains(&device_id.device_id()) {
        return;
    }

    let mut virtio_rng = VirtIORNG::from_device(device_config);
    virtio_rng.enable_msix(0);

    serial_println!("VirtIO RNG initialized: {virtio_rng:#x?}");

    VIRTIO_RNG.init(virtio_rng);
}

pub(crate) fn request_random_numbers(num_bytes: u32) -> Arc<InitCell<Vec<u8>>> {
    VIRTIO_RNG
        .get()
        .expect("VirtIO RNG not initialized")
        .request_random_numbers(num_bytes)
}

/// See "5.4 Entropy Device" in the VirtIO spec. The virtio entropy device
/// supplies high-quality randomness for guest use.
#[derive(Debug)]
struct VirtIORNG {
    initialized_device: VirtIOInitializedDevice,
    // TODO: The only reason we use Vec is so we have a sized type. We shouldn't
    // need Vec here.
    requests: Mutex<HashMap<DescIndex, Arc<InitCell<Vec<u8>>>>>,
}

impl VirtIORNG {
    // There is just a single virtqueue
    const QUEUE_INDEX: VirtQueueIndex = VirtQueueIndex(0);
    const VENDOR_IDS: [u16; 2] = [0x1005, 0x1044];

    fn from_device(device_config: VirtIODeviceConfig) -> Self {
        let device_id = device_config.pci_config().device_id().device_id();
        assert!(
            Self::VENDOR_IDS.contains(&device_id),
            "VirtIORNG: Device ID mismatch, got {device_id}"
        );

        let initialized_device =
            VirtIOInitializedDevice::new(device_config, |_: &mut RNGFeatureBits| {});

        Self {
            initialized_device,
            requests: Mutex::new(HashMap::new()),
        }
    }

    fn enable_msix(&mut self, processor_id: u8) {
        let msix_table_id = 0;
        let handler_id = 1; // If we had multiple RNG devices, we could disambiguate them
        self.initialized_device.install_virtqueue_msix_handler(
            Self::QUEUE_INDEX,
            msix_table_id,
            processor_id,
            handler_id,
            virtio_rng_interrupt,
        );
    }

    fn request_random_numbers(&self, num_bytes: u32) -> Arc<InitCell<Vec<u8>>> {
        let virtq = self
            .initialized_device
            .get_virtqueue(Self::QUEUE_INDEX)
            .unwrap();

        // Create a descriptor chain for the buffer
        let addr = memory::allocate_physically_contiguous_zeroed_buffer(num_bytes as usize, 4)
            .expect("failed to allocate rng buffer");
        let desc = ChainedVirtQueueDescriptorElem {
            addr,
            len: num_bytes,
            flags: VirtQueueDescriptorFlags::new().with_device_write(true),
        };
        let desc_index = virtq.add_buffer(&[desc]);

        // Disable interrupts so IRQ doesn't deadlock the mutex
        let cell = x86_64::instructions::interrupts::without_interrupts(|| {
            let mut requests = self.requests.lock();
            let cell = Arc::new(InitCell::new());
            let copied_cell = cell.clone();
            requests.insert(desc_index, cell);
            copied_cell
        });

        // Now that the cell is created, we can notify the device of the new
        // buffer
        //
        // TODO: I think there is a race condition here and we could still miss
        // the notification if the device is fast enough. I think according to
        // the spec, even telling the device to not send notifications could be
        // ignored. Before returning, we should check if the device has already
        // notified us. Long term, it is probably better to be more robust and
        // not rely 100% on interrupts when checking on the virtqueue; maybe we
        // should periodically check on a timer.
        virtq.notify_device();

        cell
    }
}

bitflags! {
    #[derive(Debug)]
    #[repr(transparent)]
    /// VirtIO RNG device has no device-specific feature bits. See "5.4.3
    /// Feature bits".
    struct RNGFeatureBits: u128 {
    }
}

fn virtio_rng_interrupt(_vector: u8, _handler_id: InterruptHandlerID) {
    let rng = VIRTIO_RNG.get().expect("VirtIO RNG not initialized");

    let virtq = rng
        .initialized_device
        .get_virtqueue(VirtIORNG::QUEUE_INDEX)
        .unwrap();

    let mut requests = rng.requests.lock();

    virtq.process_new_entries(|used_entry, mut descriptor_chain| {
        let Some(cell) = requests.remove(&used_entry.desc_index()) else {
            serial_println!("VirtIO RNG: no request for used entry: {used_entry:#x?}");
            return;
        };

        let descriptor = descriptor_chain.next().expect("no descriptor in chain");
        assert!(
            descriptor_chain.next().is_none(),
            "more than one descriptor in RNG chain"
        );

        // The used entry should be using the exact same buffer we just
        // created, but let's pretend we didn't know that.
        let buffer = unsafe {
            core::slice::from_raw_parts(
                descriptor.addr.as_u64() as *const u8,
                // NOTE: Using the length from the used entry, not the buffer
                // length, b/c the RNG device might not have written the whole
                // thing!
                used_entry.len as usize,
            )
        };

        cell.init(buffer.to_vec());

        // Free the buffer
        //
        // TODO: It would be pretty sweet to be able to pass the buffer back to
        // the caller and let the cell free it when dropped, instead of copying
        // it and then dropping this allocation. However, we have to make sure
        // we aren't mixing up the heap with the physical memory allocator, and
        // also ensure we aren't mixing up physical and virtual addresses.
        serial_println!("Freeing {:x?} (size {})", descriptor.addr, descriptor.len);
        memory::free_physically_contiguous_buffer(descriptor.addr, descriptor.len as usize);
    });
}
