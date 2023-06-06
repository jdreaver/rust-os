use alloc::vec::Vec;
use bitflags::bitflags;

use crate::interrupts::InterruptHandlerID;
use crate::memory::PhysicalBuffer;
use crate::sync::{once_channel, OnceReceiver, OnceSender, SpinLock};

use super::device::VirtIOInitializedDevice;
use super::queue::{
    ChainedVirtQueueDescriptorElem, VirtQueue, VirtQueueData, VirtQueueDescriptorFlags,
};
use super::VirtIODeviceConfig;

static VIRTIO_RNG: SpinLock<Option<VirtIORNG>> = SpinLock::new(None);

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

    VIRTIO_RNG.lock_disable_interrupts().replace(virtio_rng);
}

pub(crate) fn request_random_numbers(num_bytes: u32) -> OnceReceiver<Vec<u8>> {
    let mut lock = VIRTIO_RNG.lock_disable_interrupts();
    let rng = lock.as_mut().expect("VirtIO RNG not initialized");
    rng.request_random_numbers(num_bytes)
}

/// See "5.4 Entropy Device" in the VirtIO spec. The virtio entropy device
/// supplies high-quality randomness for guest use.
#[derive(Debug)]
struct VirtIORNG {
    initialized_device: VirtIOInitializedDevice<RNGFeatureBits>,
    virtqueue: VirtQueueData<VirtIORNGRequest>,
}

impl VirtIORNG {
    const VENDOR_IDS: [u16; 2] = [0x1005, 0x1044];

    fn from_device(device_config: VirtIODeviceConfig) -> Self {
        let device_id = device_config.pci_config().device_id().device_id();
        assert!(
            Self::VENDOR_IDS.contains(&device_id),
            "VirtIORNG: Device ID mismatch, got {device_id}"
        );

        let (initialized_device, virtqueues) =
            VirtIOInitializedDevice::new(device_config, |_: &mut RNGFeatureBits| {}, 1);

        let num_virtqueues = virtqueues.len();
        let virtqueue = if let Ok::<[VirtQueue; 1], _>([virtqueue]) = virtqueues.try_into() {
            VirtQueueData::new(virtqueue)
        } else {
            panic!("VirtIORNG: expected exactly one virtqueue, got {num_virtqueues}");
        };

        Self {
            initialized_device,
            virtqueue,
        }
    }

    fn enable_msix(&mut self, processor_id: u8) {
        let msix_table_id = 0;
        let handler_id = 1; // If we had multiple RNG devices, we could disambiguate them
        self.initialized_device.install_virtqueue_msix_handler(
            self.virtqueue.index(),
            msix_table_id,
            processor_id,
            handler_id,
            virtio_rng_interrupt,
        );
    }

    fn request_random_numbers(&mut self, num_bytes: u32) -> OnceReceiver<Vec<u8>> {
        assert!(num_bytes > 0, "cannot request zero bytes from RNG!");

        // Create a descriptor chain for the buffer
        let buffer = PhysicalBuffer::allocate_zeroed(num_bytes as usize)
            .expect("failed to allocate rng buffer");
        let desc = ChainedVirtQueueDescriptorElem {
            addr: buffer.address(),
            len: num_bytes,
            flags: VirtQueueDescriptorFlags::new().with_device_write(true),
        };
        let (sender, receiver) = once_channel();
        let request = VirtIORNGRequest {
            _descriptor_buffer: buffer,
            sender,
        };

        self.virtqueue.add_buffer(&[desc], request);
        self.virtqueue.notify_device();

        receiver
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

#[derive(Debug)]
struct VirtIORNGRequest {
    // Buffer is kept here so we can drop it when we are done with the request.
    _descriptor_buffer: PhysicalBuffer,
    sender: OnceSender<Vec<u8>>,
}

fn virtio_rng_interrupt(_vector: u8, _handler_id: InterruptHandlerID) {
    let mut lock = VIRTIO_RNG.lock_disable_interrupts();
    let rng = lock.as_mut().expect("VirtIO RNG not initialized");

    rng.virtqueue
        .process_new_entries(|used_entry, mut descriptor_chain, request| {
            let Some(request) = request else {
                log::warn!("VirtIO RNG: no request for used entry: {used_entry:#x?}");
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

            request.sender.send(buffer.to_vec());
        });
}
