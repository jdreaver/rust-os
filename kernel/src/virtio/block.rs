use bitflags::bitflags;
use core::{mem, ptr};
use spin::{Mutex, RwLock};
use x86_64::{PhysAddr, VirtAddr};

use crate::interrupts::InterruptHandlerID;
use crate::registers::RegisterRO;
use crate::{memory, register_struct, serial_println, strings};

use super::device::VirtIOInitializedDevice;
use super::queue::{ChainedVirtQueueDescriptorElem, VirtQueueDescriptorFlags, VirtQueueIndex};
use super::VirtIODeviceConfig;

// TODO: Support multiple block devices
static VIRTIO_BLOCK: RwLock<Option<VirtIOBlockDevice>> = RwLock::new(None);

pub(crate) fn try_init_virtio_block(device_config: VirtIODeviceConfig) {
    let device_id = device_config.pci_config().device_id();
    if device_id.vendor_id() != 0x1af4 {
        return;
    }
    if !VirtIOBlockDevice::VENDOR_IDS.contains(&device_id.device_id()) {
        return;
    }

    let mut device = VirtIOBlockDevice::from_device(device_config);
    device.initialized_device.install_virtqueue_msix_handler(
        VirtQueueIndex(0),
        0,
        0,
        123,
        virtio_block_interrupt,
    );
    serial_println!("VirtIOBlockDevice: {:#x?}", device);

    VIRTIO_BLOCK.write().replace(device);
}

// N.B. The drive MUST provide a 20 byte buffer when requesting device ID.
static DEVICE_ID_BUFFER: [u8; 20] = [0; 20];

static READ_BUFFER: [u8; 512] = [0; 512];

pub(crate) fn virtio_block_get_id() {
    let device_lock = VIRTIO_BLOCK.read();
    let device = device_lock
        .as_ref()
        .expect("VirtIOBlockDevice not initialized");

    let virtq = device
        .initialized_device
        .get_virtqueue(VirtQueueIndex(0))
        .unwrap();
    let buffer_virt_addr = VirtAddr::new(ptr::addr_of!(DEVICE_ID_BUFFER) as u64);
    let buffer_phys_addr = memory::translate_addr(buffer_virt_addr)
        .expect("failed to get VirtIO device ID buffer physical address");
    let buffer_size = core::mem::size_of_val(&DEVICE_ID_BUFFER) as u32;

    let request = BlockRequest::GetID {
        data_addr: buffer_phys_addr,
        data_len: buffer_size,
    };
    let raw_request = request.to_raw();
    virtq.add_buffer(&raw_request.to_descriptor_chain());
}

pub(crate) fn virtio_block_read() {
    let device_lock = VIRTIO_BLOCK.read();
    let device = device_lock
        .as_ref()
        .expect("VirtIOBlockDevice not initialized");

    let virtq = device
        .initialized_device
        .get_virtqueue(VirtQueueIndex(0))
        .unwrap();
    let buffer_virt_addr = VirtAddr::new(ptr::addr_of!(READ_BUFFER) as u64);
    let buffer_phys_addr = memory::translate_addr(buffer_virt_addr)
        .expect("failed to get VirtIO device ID buffer physical address");
    let buffer_size = core::mem::size_of_val(&READ_BUFFER) as u32;

    let request = BlockRequest::Read {
        sector: 0,
        data_addr: buffer_phys_addr,
        data_len: buffer_size,
    };
    let raw_request = request.to_raw();
    virtq.add_buffer(&raw_request.to_descriptor_chain());
}

fn virtio_block_interrupt(_vector: u8, _handler_id: InterruptHandlerID) {
    serial_println!("!!! virtio_block_interrupt !!!");

    let device_lock = VIRTIO_BLOCK.read();
    let device = device_lock
        .as_ref()
        .expect("VirtIOBlockDevice not initialized");

    let virtq = device
        .initialized_device
        .get_virtqueue(VirtQueueIndex(0))
        .unwrap();

    let used_index = virtq.used_ring_index();

    let mut used_index_lock = device.processed_used_index.lock();
    let last_processed: u16 = *used_index_lock;

    for i in last_processed..used_index {
        let (_, mut descriptor_chain) = virtq.get_used_ring_entry(i);
        let raw_request = RawBlockRequest::from_descriptor_chain(&mut descriptor_chain);
        let request = BlockRequest::from_raw(&raw_request);
        serial_println!("Got response: {:#x?}", request);

        match request {
            BlockRequest::Read {
                sector: _,
                data_addr,
                data_len,
            } => {
                let buffer = unsafe {
                    core::slice::from_raw_parts(data_addr.as_u64() as *const u8, data_len as usize)
                };
                serial_println!("Read response: {:x?}", buffer);
            }
            BlockRequest::GetID {
                data_addr,
                data_len,
            } => {
                // The used entry should be using the exact same buffer we just
                // created, but let's pretend we didn't know that.
                let s = unsafe {
                    // The device ID response is a null-terminated string with a max
                    // size of the buffer size (if the string size == buffer size, there
                    // is no null terminator)
                    strings::c_str_from_pointer(data_addr.as_u64() as *const u8, data_len as usize)
                };
                serial_println!("Device ID response: {s}");
            }
        }
    }

    *used_index_lock = used_index;
}

/// See "5.2 Block Device" in the VirtIO spec.
#[derive(Debug)]
struct VirtIOBlockDevice {
    initialized_device: VirtIOInitializedDevice,
    _block_config: BlockConfigRegisters,

    /// How far into the used ring we've processed entries.
    processed_used_index: Mutex<u16>, // TODO: Abstract/dedup with RNG device
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
            VirtIOInitializedDevice::new(device_config, |features: &mut BlockDeviceFeatureBits| {
                // Don't use multi queue for now
                features.remove(BlockDeviceFeatureBits::MQ);
            });

        let block_config = unsafe {
            BlockConfigRegisters::from_address(
                initialized_device.config.device_config_phys_addr().as_u64() as usize,
            )
        };

        Self {
            initialized_device,
            _block_config: block_config,
            processed_used_index: Mutex::new(0),
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

#[derive(Debug)]
enum BlockRequest {
    Read {
        sector: u64,
        data_addr: PhysAddr,
        data_len: u32,
    },
    GetID {
        data_addr: PhysAddr,
        data_len: u32,
    },
}

impl BlockRequest {
    fn to_raw(&self) -> RawBlockRequest {
        match self {
            Self::Read {
                sector,
                data_addr,
                data_len,
            } => {
                assert!(
                    *data_len % 512 == 0,
                    "Data length for read requests must be a multiple of 512"
                );
                RawBlockRequest::new(BlockRequestType::In, *sector, *data_addr, *data_len)
            }
            Self::GetID {
                data_addr,
                data_len,
            } => {
                assert!(
                    *data_len == 20,
                    "GetID requests MUST have a data buffer of exactly 20 bytes"
                );
                RawBlockRequest::new(BlockRequestType::GetID, 0, *data_addr, *data_len)
            }
        }
    }

    fn from_raw(raw: &RawBlockRequest) -> Self {
        match raw.request_type {
            BlockRequestType::In => Self::Read {
                sector: raw.sector,
                data_addr: raw.data_addr,
                data_len: raw.data_len,
            },
            BlockRequestType::GetID => Self::GetID {
                data_addr: raw.data_addr,
                data_len: raw.data_len,
            },
            _ => panic!("Unsupported block request type: {:?}", raw.request_type),
        }
    }
}

/// Wraps the different components of a block device request.
///
/// Under the hood this is:
///
/// ```c
/// struct virtio_blk_req {
///     le32 type;
///     le32 reserved;
///     le64 sector;
///     u8 data[];
///     u8 status;
/// };
/// ```
///
/// However, that `data` member is dynamically sized, and also the header, data,
/// and footer need different flags set when written to the descriptor table.
/// That means this needs to be split up into 3 chained descriptors when we
/// write this to the descriptor table.
#[derive(Debug)]
struct RawBlockRequest {
    request_type: BlockRequestType,
    sector: u64,
    data_addr: PhysAddr,
    data_len: u32,
    status: BlockRequestStatus,
}

#[repr(C)]
#[derive(Debug)]
struct RawBlockRequestHeader {
    request_type: u32,
    reserved: u32,
    sector: u64,
}

impl RawBlockRequest {
    fn new(
        request_type: BlockRequestType,
        sector: u64,
        data_addr: PhysAddr,
        data_len: u32,
    ) -> Self {
        Self {
            request_type,
            sector,
            data_addr,
            data_len,

            // Trick: write 111 to status, which is invalid, so we can be certain
            // that the device wrote the status. If we leave it as 0 we can't be
            // certain that the device wrote the status.
            status: BlockRequestStatus::Unset,
        }
    }

    /// Creates a descriptor (that is chained) for the block request.
    ///
    /// TODO: Ensure we de-allocate the components when done with the descriptor.
    fn to_descriptor_chain(&self) -> [ChainedVirtQueueDescriptorElem; 3] {
        // Allocate header
        let header_addr = memory::allocate_physically_contiguous_zeroed_buffer(
            mem::size_of::<RawBlockRequestHeader>(),
            mem::align_of::<RawBlockRequestHeader>(),
        )
        .expect("failed to allocate RawBlockRequest header");

        // Set header fields
        let header_ptr = header_addr.as_u64() as *mut RawBlockRequestHeader;
        unsafe {
            header_ptr.write_volatile(RawBlockRequestHeader {
                request_type: self.request_type as u32,
                reserved: 0,
                sector: self.sector,
            });
        };

        let header_desc = ChainedVirtQueueDescriptorElem {
            addr: header_addr,
            len: mem::size_of::<RawBlockRequestHeader>() as u32,
            flags: VirtQueueDescriptorFlags::new().with_device_write(false),
        };

        // Buffer descriptor
        let buffer_desc = ChainedVirtQueueDescriptorElem {
            addr: self.data_addr,
            len: self.data_len,
            flags: VirtQueueDescriptorFlags::new().with_device_write(true),
        };

        // Allocate status
        let status_addr = memory::allocate_physically_contiguous_zeroed_buffer(1, 1)
            .expect("failed to allocate RawBlockRequest status");
        let status_ptr = status_addr.as_u64() as *mut u8;
        unsafe {
            status_ptr.write_volatile(self.status as u8);
        };
        serial_println!("status_addr: {:#x}", status_addr);

        let status_desc = ChainedVirtQueueDescriptorElem {
            addr: status_addr,
            len: 1,
            flags: VirtQueueDescriptorFlags::new().with_device_write(true),
        };

        [header_desc, buffer_desc, status_desc]
    }

    fn from_descriptor_chain(
        mut chain: impl Iterator<Item = ChainedVirtQueueDescriptorElem>,
    ) -> Self {
        let header_desc = chain.next().expect("missing header descriptor");
        let buffer_desc = chain.next().expect("missing buffer descriptor");
        let status_desc = chain.next().expect("missing status descriptor");
        assert!(chain.next().is_none(), "too many descriptors");

        assert!(header_desc.len == mem::size_of::<RawBlockRequestHeader>() as u32);
        let header_ptr = header_desc.addr.as_u64() as *const RawBlockRequestHeader;
        let header = unsafe { header_ptr.read_volatile() };

        let request_type = BlockRequestType::from_u32(header.request_type)
            .expect("invalid request type in header");

        assert!(status_desc.len == 1);
        let status_ptr = status_desc.addr.as_u64() as *const u8;
        let raw_status = unsafe { status_ptr.read_volatile() };
        let status =
            BlockRequestStatus::from_u8(raw_status).expect("invalid status in status descriptor");

        Self {
            request_type,
            sector: header.sector,
            data_addr: buffer_desc.addr,
            data_len: buffer_desc.len,
            status,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockRequestType {
    In = 0,
    Out = 1,
    Flush = 4,
    /// N.B. The "ID" for a device is the `serial=` option when creating the
    /// device in QEMU.
    GetID = 8,
    GetLifetime = 10,
    Discard = 11,
    WriteZeroes = 13,
    SecureErase = 14,
}

impl BlockRequestType {
    fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::In),
            1 => Some(Self::Out),
            4 => Some(Self::Flush),
            8 => Some(Self::GetID),
            10 => Some(Self::GetLifetime),
            11 => Some(Self::Discard),
            13 => Some(Self::WriteZeroes),
            14 => Some(Self::SecureErase),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum BlockRequestStatus {
    Ok = 0,
    IoErr = 1,
    Unsupported = 2,

    /// Trick: this is an invalid status, but we use it when we create a block
    /// request so we can tell if the device actually wrote the status. If we
    /// just left it as 0 we wouldn't know if the device wrote the status.
    Unset = 0b111,
}

impl BlockRequestStatus {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Ok),
            1 => Some(Self::IoErr),
            2 => Some(Self::Unsupported),
            0b111 => Some(Self::Unset),
            _ => None,
        }
    }
}
