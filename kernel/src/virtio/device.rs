use alloc::vec::Vec;
use core::fmt;

use bitflags::Flags;

use crate::barrier::barrier;
use crate::interrupts::{InterruptHandler, InterruptHandlerID};
use crate::{interrupts, serial_println};

use super::config::{VirtIOConfigStatus, VirtIODeviceConfig};
use super::features::ReservedFeatureBits;
use super::queue::{
    VirtQueue, VirtQueueAvailRing, VirtQueueDescriptorTable, VirtQueueIndex, VirtQueueUsedRing,
};

#[derive(Debug)]
pub(super) struct VirtIOInitializedDevice {
    pub(super) config: VirtIODeviceConfig,
    virtqueues: Vec<VirtQueue>,
}

impl VirtIOInitializedDevice {
    /// See "3 General Initialization And Device Operation" and "4.1.5
    /// PCI-specific Initialization And Device Operation"
    pub(super) fn new<F>(
        device_config: VirtIODeviceConfig,
        negotiate_device_bits: impl FnOnce(&mut F),
    ) -> Self
    where
        F: fmt::Debug + Flags<Bits = u128>,
    {
        let config = device_config.common_virtio_config();

        // Reset the VirtIO device by writing 0 to the status register (see
        // 4.1.4.3.1 Device Requirements: Common configuration structure layout)
        let mut status = VirtIOConfigStatus::new();
        config.device_status().write(status);
        barrier();

        // Set the ACKNOWLEDGE status bit to indicate that the driver knows
        // that the device is present.
        status.set_acknowledge(true);
        config.device_status().write(status);
        barrier();

        // Set the DRIVER status bit to indicate that the driver is ready to
        // drive the device.
        status.set_driver(true);
        config.device_status().write(status);
        barrier();

        // Feature negotiation. There are up to 128 feature bits, and
        // the feature registers are 32 bits wide, so we use the feature
        // selection registers 4 times to select features.
        let mut device_features = device_config.get_device_features::<F>();
        serial_println!("VirtIO device feature bits: {device_features:#?}");

        // Disable VIRTIO_F_EVENT_IDX so we don't need to mess with `used_event`
        // in avail ring.
        //
        // TODO: Record that we did this in the virtqueues so they know to set
        // `used_event` or not in the avail ring, instead of just assuming that
        // we did this.
        device_features.negotiate_reserved_bits(|bits| {
            bits.remove(ReservedFeatureBits::EVENT_IDX);
        });

        // Write the features we want to enable
        let mut driver_features = device_features;
        driver_features.negotiate_device_bits(negotiate_device_bits);
        serial_println!("VirtIO driver feature bits: {driver_features:#?}");
        device_config.set_driver_features(&driver_features);

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
            let idx = VirtQueueIndex(i);
            config.queue_select().write(idx);

            let queue_size = config.queue_size().read();
            assert!(
                queue_size > 0,
                "queue size for queue {i} must be greater than 0"
            );

            let descriptors = unsafe {
                VirtQueueDescriptorTable::allocate(queue_size)
                    .expect("failed to allocate driver ring buffer")
            };
            config.queue_desc().write(descriptors.physical_address());

            let avail_ring = unsafe {
                VirtQueueAvailRing::allocate(queue_size)
                    .expect("failed to allocate driver ring buffer")
            };
            config.queue_driver().write(avail_ring.physical_address());

            let used_ring = unsafe {
                VirtQueueUsedRing::allocate(queue_size)
                    .expect("failed to allocate driver ring buffer")
            };
            config.queue_device().write(used_ring.physical_address());

            // Enable the queue
            config.queue_enable().write(1);

            virtqueues.push(VirtQueue::new(
                idx,
                device_config.notify_config(),
                config.queue_notify_off().read(),
                descriptors,
                avail_ring,
                used_ring,
            ));
        }

        // TODO: Device-specific setup

        // Set the DRIVER_OK status bit to indicate that the driver
        // finished configuring the device.
        status.set_driver_ok(true);
        config.device_status().write(status);

        Self {
            config: device_config,
            virtqueues,
        }
    }

    pub(super) fn get_virtqueue(&self, index: VirtQueueIndex) -> Option<&VirtQueue> {
        self.virtqueues.get(index.0 as usize)
    }

    pub(super) fn install_virtqueue_msix_handler(
        &mut self,
        virtqueue_index: VirtQueueIndex,
        msix_table_index: u16,
        processor_number: u8,
        handler_id: InterruptHandlerID,
        handler: InterruptHandler,
    ) {
        // Select the virtqueue and tell it to use the given MSI-X table index
        let common_config = self.config.common_virtio_config();
        common_config.queue_select().write(virtqueue_index);
        common_config.queue_msix_vector().write(msix_table_index);

        // Read back the virtqueue's MSI-X table index to ensure that it was
        // set correctly
        assert_eq!(
            common_config.queue_msix_vector().read(),
            msix_table_index,
            "failed to set virtqueue's MSI-X table index"
        );

        // Install the interrupt handler via MSI-X
        let msix = self
            .config
            .pci_type0_config()
            .msix_config()
            .expect("failed to get MSIX config for VirtIO device");
        let interrupt_vector = interrupts::install_interrupt(handler_id, handler);
        let table_entry = msix.table_entry(msix_table_index as usize);
        table_entry.set_interrupt_vector(processor_number, interrupt_vector);
        msix.enable();
    }
}
