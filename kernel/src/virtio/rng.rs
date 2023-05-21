use core::ptr;

use spin::Mutex;
use x86_64::VirtAddr;

use crate::interrupts::InterruptHandlerID;
use crate::{apic, memory, serial_println};

use super::{VirtIODeviceConfig, VirtIOInitializedDevice, VirtqDescriptorFlags};

static VIRTIO_RNG: Mutex<Option<VirtIORNG>> = Mutex::new(None);

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
        !VIRTIO_RNG.lock().is_some(),
        "VirtIO RNG already initialized"
    );

    let mut virtio_rng = VirtIORNG::from_device(device_config);
    virtio_rng.enable_msix(0);

    serial_println!("VirtIO RNG initialized: {:#x?}", virtio_rng);

    VIRTIO_RNG.lock().replace(virtio_rng);
}

pub(crate) fn request_random_numbers() {
    // Disable interrupts so the interrupt handler doesn't try to take this same
    // lock while we are still in the critical section, resulting in a deadlock.
    //
    // TODO: Use separate locks and/or RWLocks so we don't need to take the
    // exact same mutex between writing to the virtq and reading from it.
    x86_64::instructions::interrupts::without_interrupts(|| {
        VIRTIO_RNG
            .lock()
            .as_mut()
            .expect("VirtIO RNG not initialized")
            .request_random_numbers();
    });
}

/// See "5.4 Entropy Device" in the VirtIO spec. The virtio entropy device
/// supplies high-quality randomness for guest use.
#[derive(Debug)]
struct VirtIORNG {
    initialized_device: VirtIOInitializedDevice,

    // How far into the used ring we've processed entries.
    processed_used_index: u16,
}

impl VirtIORNG {
    // There is just a single virtqueue
    const QUEUE_INDEX: u16 = 0;
    const VENDOR_IDS: [u16; 2] = [0x1005, 0x1044];

    fn from_device(device_config: VirtIODeviceConfig) -> Self {
        let device_id = device_config.pci_config().device_id().device_id();
        assert!(
            Self::VENDOR_IDS.contains(&device_id),
            "VirtIORNG: Device ID mismatch, got {device_id}"
        );

        let initialized_device = VirtIOInitializedDevice::new(device_config);

        Self {
            initialized_device,
            processed_used_index: 0,
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

    fn request_random_numbers(&mut self) {
        let virtq = self
            .initialized_device
            .get_virtqueue_mut(Self::QUEUE_INDEX)
            .unwrap();
        let buffer_virt_addr = VirtAddr::new(ptr::addr_of!(VIRTIO_RNG_BUFFER) as u64);
        let buffer_phys_addr = memory::translate_addr(buffer_virt_addr)
            .expect("failed to get VirtIO RNG buffer physical address");
        let buffer_size = core::mem::size_of_val(&VIRTIO_RNG_BUFFER);
        let flags = VirtqDescriptorFlags::new().with_device_write(true);
        virtq.add_buffer(buffer_phys_addr.as_u64(), buffer_size as u32, flags);
    }
}

fn virtio_rng_interrupt(vector: u8, handler_id: InterruptHandlerID) {
    serial_println!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    serial_println!(
        "!! VirtIO RNG interrupt (vec={}, id={}) !!!!!!!",
        vector,
        handler_id
    );
    serial_println!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");

    let mut lock = VIRTIO_RNG.lock();
    let rng = lock.as_mut().expect("VirtIO RNG not initialized");

    let virtq = rng
        .initialized_device
        .get_virtqueue_mut(VirtIORNG::QUEUE_INDEX)
        .unwrap();
    let used_index = virtq.used_ring_index() - 1;

    for i in rng.processed_used_index..=used_index {
        let (used_entry, descriptor) = virtq.get_used_ring_entry(i);
        // serial_println!("Got used entry: {:#x?}", (used_entry, descriptor));

        // The used entry should be using the exact same buffer we just
        // created, but let's pretend we didn't know that.
        let buffer = unsafe {
            core::slice::from_raw_parts(
                descriptor.addr as *const u8,
                // NOTE: Using the length from the used entry, not the buffer
                // length, b/c the RNG device might not have written the whole
                // thing!
                used_entry.len as usize,
            )
        };
        serial_println!("RNG buffer: {:x?}", buffer);
    }

    rng.processed_used_index = used_index;

    apic::end_of_interrupt();
}
