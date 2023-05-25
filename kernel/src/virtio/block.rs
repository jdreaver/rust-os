use bitflags::bitflags;

use crate::registers::RegisterRO;
use crate::{register_struct, serial_println};

use super::device::VirtIOInitializedDevice;
use super::VirtIODeviceConfig;

pub(crate) fn try_init_virtio_block(device_config: VirtIODeviceConfig) {
    let device_id = device_config.pci_config().device_id();
    if device_id.vendor_id() != 0x1af4 {
        return;
    }
    if !VirtIOBlockDevice::VENDOR_IDS.contains(&device_id.device_id()) {
        return;
    }

    let device = VirtIOBlockDevice::from_device(device_config);
    serial_println!("VirtIOBlockDevice: {:#x?}", device);
}

/// See "5.2 Block Device" in the VirtIO spec.
#[derive(Debug)]
struct VirtIOBlockDevice {
    initialized_device: VirtIOInitializedDevice,
    block_config: BlockConfigRegisters,
}

impl VirtIOBlockDevice {
    // There is just a single virtqueue
    const VENDOR_IDS: [u16; 2] = [0x1001, 0x1042];

    fn from_device(device_config: VirtIODeviceConfig) -> Self {
        let device_id = device_config.pci_config().device_id().device_id();
        assert!(
            Self::VENDOR_IDS.contains(&device_id),
            "VirtIOBlockDevice: Device ID mismatch, got {device_id}"
        );

        let initialized_device =
            VirtIOInitializedDevice::new(device_config, |_: &mut BlockDeviceFeatureBits| {});

        let block_config = unsafe {
            BlockConfigRegisters::from_address(
                initialized_device.config.device_config_phys_addr().as_u64() as usize,
            )
        };

        Self {
            initialized_device,
            block_config,
        }
    }
}

bitflags! {
    #[derive(Debug)]
    #[repr(transparent)]
    /// See "5.2.3 Feature bits"
    struct BlockDeviceFeatureBits: u64 {
        /// Maximum size of any single segment is in size_max.
        const SIZE_MAX = 1 << 1;
        /// Maximum number of segments in a request is in seg_max.
        const SEG_MAX = 1 << 2;
        /// Disk-style geometry specified in geometry.
        const GEOMETRY = 1 << 4;
        /// Block size of disk is in blk_size.
        const BLK_SIZE = 1 << 6;
        /// Cache flush command support.
        const FLUSH = 1 << 9;
        /// Device exports information on optimal I/O alignment.
        const TOPOLOGY = 1 << 10;
        /// Device can toggle its cache between writeback and writethrough modes.
        const CONFIG_WCE = 1 << 11;
        /// Device supports multiqueue.
        const MQ = 1 << 12;
        /// Device can support discard command, maximum discard sectors size in
        const DISCARD = 1 << 13;
        /// Device can support write zeroes command, maximum write zeroes
        const WRITE_ZEROES = 1 << 14;
        /// Device supports providing storage lifetime information.
        const LIFETIME = 1 << 15;
        /// Device supports secure erase command, maximum erase sectors
        const SECURE_ERASE = 1 << 16;
    }
}

register_struct!(
    /// See "5.2.4 Device configuration layout"
    BlockConfigRegisters {
        0x00 => capacity: RegisterRO<u64>,
        0x08 => size_max: RegisterRO<u32>,
        0x0c => seg_max: RegisterRO<u32>,
        0x10 => geometry: RegisterRO<BlockConfigGeometry>,
        0x14 => blk_size: RegisterRO<u32>,
        0x18 => topology: RegisterRO<BlockConfigTopology>,
        0x20 => writeback: RegisterRO<u8>,
        // 0x21 => unused0: RegisterRO<u8>,
        0x22 => num_queues: RegisterRO<u16>,
        0x24 => max_discard_sectors: RegisterRO<u32>,
        0x28 => max_discard_seg: RegisterRO<u32>,
        0x2c => discard_sector_alignment: RegisterRO<u32>,
        0x30 => max_write_zeroes_sectors: RegisterRO<u32>,
        0x34 => max_write_zeroes_seg: RegisterRO<u32>,
        0x38 => write_zeroes_may_unmap: RegisterRO<u8>,
        // 0x39 => unused1: RegisterRO<[u8; 3]>,
        0x3c => max_secure_erase_sectors: RegisterRO<u32>,
        0x40 => max_secure_erase_seg: RegisterRO<u32>,
        0x44 => secure_erase_sector_alignment: RegisterRO<u32>,
    }
);

#[repr(C)]
#[derive(Debug)]
struct BlockConfigGeometry {
    cylinders: u16,
    heads: u8,
    sectors: u8,
}

#[repr(C)]
#[derive(Debug)]
struct BlockConfigTopology {
    physical_block_exp: u8,
    alignment_offset: u8,
    min_io_size: u16,
    opt_io_size: u32,
}
