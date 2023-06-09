use alloc::vec::Vec;
use core::cmp::min;

use bitflags::Flags;

use crate::apic::ProcessorID;
use crate::barrier::barrier;
use crate::interrupts;
use crate::interrupts::{InterruptHandler, InterruptHandlerID};

use super::config::{VirtIOConfigStatus, VirtIODeviceConfig};
use super::features::{Features, ReservedFeatureBits};
use super::queue::{
    VirtQueue, VirtQueueAvailRing, VirtQueueDescriptorTable, VirtQueueIndex, VirtQueueUsedRing,
};

#[derive(Debug)]
pub(super) struct VirtIOInitializedDevice<F>
where
    F: Flags<Bits = u128>,
{
    pub(super) config: VirtIODeviceConfig,
    pub(super) _features: Features<F>,
}

impl<F> VirtIOInitializedDevice<F>
where
    F: Flags<Bits = u128>,
{
    /// See "3 General Initialization And Device Operation" and "4.1.5
    /// PCI-specific Initialization And Device Operation"
    pub(super) fn new(
        device_config: VirtIODeviceConfig,
        negotiate_device_bits: impl FnOnce(&mut F),
        max_virtqueues: u16,
    ) -> (Self, Vec<VirtQueue>) {
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
        let mut features = device_config.get_device_features::<F>();

        // Disable some features
        //
        // TODO: Record that we did this in the virtqueues so they know to set
        // `used_event` or not in the avail ring, instead of just assuming that
        // we did this.
        features.negotiate_reserved_bits(|bits| {
            // Disable VIRTIO_F_EVENT_IDX so we don't need to mess with `used_event`
            // in avail ring.
            bits.remove(ReservedFeatureBits::EVENT_IDX);

            // Disable VIRTIO_F_NOTIFICATION_DATA so we don't need to deal with
            // extra offset information when notifying device of new avail ring
            // entries.
            bits.remove(ReservedFeatureBits::NOTIFICATION_DATA);

            // We don't use NOTIF_CONFIG_DATA
            bits.remove(ReservedFeatureBits::NOTIF_CONFIG_DATA);
        });

        // Write the features we want to enable
        features.negotiate_device_bits(negotiate_device_bits);
        device_config.set_driver_features(&features);

        // Set the FEATURES_OK status bit to indicate that the driver has
        // written the feature bits.
        status.set_features_ok(true);
        config.device_status().write(status);

        // Re-read the status to ensure that the FEATURES_OK bit is still set.
        status = config.device_status().read();
        assert!(status.features_ok(), "failed to set FEATURES_OK status bit");

        // Initialize virtqueues
        let num_queues = config.num_queues().read();
        assert!(
            num_queues > 0,
            "number of queues in a VirtIO device must be greater than 0"
        );

        let num_queues = min(num_queues, max_virtqueues);
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

        let device = Self {
            config: device_config,
            _features: features,
        };
        (device, virtqueues)
    }

    pub(super) fn install_virtqueue_msix_handler(
        &mut self,
        virtqueue_index: VirtQueueIndex,
        msix_table_index: u16,
        processor_id: ProcessorID,
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
        let interrupt_vector = interrupts::install_interrupt_next_vector(handler_id, handler);
        let mut table_entry = msix.table_entry(msix_table_index as usize);
        table_entry.set_interrupt_vector(processor_id, interrupt_vector);
        msix.enable();
    }
}
