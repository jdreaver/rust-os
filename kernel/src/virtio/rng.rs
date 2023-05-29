use core::fmt;
use core::ptr;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use bitflags::bitflags;
use spin::{Mutex, RwLock};
use x86_64::VirtAddr;

use crate::interrupts::InterruptHandlerID;
use crate::{memory, serial_println};

use super::device::VirtIOInitializedDevice;
use super::queue::{ChainedVirtQueueDescriptorElem, VirtQueueDescriptorFlags, VirtQueueIndex};
use super::VirtIODeviceConfig;

static VIRTIO_RNG: RwLock<Option<VirtIORNG>> = RwLock::new(None);

// TODO: Use separate buffers so we can have multiple requests in flight at
// once. Currently all requests will use the same buffer.
static VIRTIO_RNG_BUFFER: [u8; 16] = [0; 16];

pub(crate) fn try_init_virtio_rng(device_config: VirtIODeviceConfig) {
    let device_id = device_config.pci_config().device_id();
    if device_id.vendor_id() != 0x1af4 {
        return;
    }
    if !VirtIORNG::VENDOR_IDS.contains(&device_id.device_id()) {
        return;
    }

    assert!(
        !VIRTIO_RNG.read().is_some(),
        "VirtIO RNG already initialized"
    );

    let mut virtio_rng = VirtIORNG::from_device(device_config);
    virtio_rng.enable_msix(0);

    serial_println!("VirtIO RNG initialized: {virtio_rng:#x?}");

    VIRTIO_RNG.write().replace(virtio_rng);
}

pub(crate) fn request_random_numbers<F: FnOnce(Box<[u8]>) + 'static>(callback: F) {
    VIRTIO_RNG
        .read()
        .as_ref()
        .expect("VirtIO RNG not initialized")
        .request_random_numbers(Box::new(callback));
}

/// See "5.4 Entropy Device" in the VirtIO spec. The virtio entropy device
/// supplies high-quality randomness for guest use.
#[derive(Debug)]
struct VirtIORNG {
    initialized_device: VirtIOInitializedDevice,
    requests: Mutex<VecDeque<VirtIORNGRequest>>,
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
            requests: Mutex::new(VecDeque::new()),
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

    fn request_random_numbers(&self, callback: Box<dyn FnOnce(Box<[u8]>)>) {
        // Disable interrupts so IRQ doesn't deadlock the mutex
        x86_64::instructions::interrupts::without_interrupts(|| {
            let mut requests = self.requests.lock();
            requests.push_back(VirtIORNGRequest { callback });
        });

        let virtq = self
            .initialized_device
            .get_virtqueue(Self::QUEUE_INDEX)
            .unwrap();
        let buffer_virt_addr = VirtAddr::new(ptr::addr_of!(VIRTIO_RNG_BUFFER) as u64);
        let buffer_phys_addr = memory::translate_addr(buffer_virt_addr)
            .expect("failed to get VirtIO RNG buffer physical address");
        let buffer_size = core::mem::size_of_val(&VIRTIO_RNG_BUFFER);
        let flags = VirtQueueDescriptorFlags::new().with_device_write(true);
        let desc = ChainedVirtQueueDescriptorElem {
            addr: buffer_phys_addr,
            len: buffer_size as u32,
            flags,
        };
        virtq.add_buffer(&[desc]);
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

/// A request to the VirtIO RNG device.
struct VirtIORNGRequest {
    callback: Box<dyn FnOnce(Box<[u8]>)>,
}

// FnOnce doesn't implement Send.
unsafe impl Send for VirtIORNGRequest {}

impl fmt::Debug for VirtIORNGRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VirtIORNGRequest").finish()
    }
}

fn virtio_rng_interrupt(_vector: u8, _handler_id: InterruptHandlerID) {
    let rng_lock = VIRTIO_RNG.read();
    let rng = rng_lock.as_ref().expect("VirtIO RNG not initialized");

    let virtq = rng
        .initialized_device
        .get_virtqueue(VirtIORNG::QUEUE_INDEX)
        .unwrap();

    let mut requests = rng.requests.lock();

    virtq.process_new_entries(|used_entry, mut descriptor_chain| {
        let Some(request) = requests.pop_front() else {
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

        (request.callback)(buffer.into());
    });
}
