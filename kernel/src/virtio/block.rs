use alloc::string::String;
use alloc::vec::Vec;
use bitflags::bitflags;
use core::mem;
use spin::RwLock;
use x86_64::PhysAddr;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

use crate::apic::ProcessorID;
use crate::interrupts::{InterruptHandlerID, InterruptVector};
use crate::memory::{KernPhysAddr, PhysicalBuffer};
use crate::registers::RegisterRO;
use crate::sync::{once_channel, OnceReceiver, OnceSender, SpinLock};
use crate::transmute::try_write_bytes_offset;
use crate::{register_struct, serial_println, strings};

use super::device::VirtIOInitializedDevice;
use super::queue::{
    ChainedVirtQueueDescriptorElem, VirtQueue, VirtQueueData, VirtQueueDescriptorFlags,
};
use super::VirtIODeviceConfig;

static VIRTIO_BLOCK: RwLock<Vec<SpinLock<VirtIOBlockDevice>>> = RwLock::new(Vec::new());

/// All virtio-block sectors are 512 bytes. Also, all virtio-block requests need
/// to be a multiple of 512 bytes.
pub(crate) const VIRTIO_BLOCK_SECTOR_SIZE_BYTES: u32 = 512;

pub(crate) fn try_init_virtio_block(device_config: VirtIODeviceConfig) {
    let device_id = device_config.pci_config().device_id();
    if device_id.vendor_id() != 0x1af4 {
        return;
    }
    if !VirtIOBlockDevice::VENDOR_IDS.contains(&device_id.device_id()) {
        return;
    }

    let mut devices = VIRTIO_BLOCK.write();

    let mut device = VirtIOBlockDevice::from_device(device_config);
    let device_index = devices.len();
    let handler_id = device_index as u32; // Use device index to disambiguate devices
    device.initialized_device.install_virtqueue_msix_handler(
        device.virtqueue.index(),
        0,
        ProcessorID(0),
        handler_id,
        virtio_block_interrupt,
    );

    devices.push(SpinLock::new(device));
}

pub(crate) fn virtio_block_print_devices() {
    let devices = VIRTIO_BLOCK.read();
    serial_println!("virtio block devices: {:#x?}", devices);
}

pub(crate) fn virtio_block_get_id(device_index: usize) -> OnceReceiver<VirtIOBlockResponse> {
    let devices_lock = VIRTIO_BLOCK.read();
    let mut device = devices_lock
        .get(device_index)
        .expect("invalid device index")
        .lock_disable_interrupts();
    device.add_request(&BlockRequest::GetID, None)
}

pub(crate) fn virtio_block_read(
    device_index: usize,
    sector: u64,
    num_sectors: u32,
) -> OnceReceiver<VirtIOBlockResponse> {
    let devices_lock = VIRTIO_BLOCK.read();
    let mut device = devices_lock
        .get(device_index)
        .expect("invalid device index")
        .lock_disable_interrupts();
    device.add_request(
        &BlockRequest::Read {
            sector,
            num_sectors,
        },
        None,
    )
}

pub(crate) fn virtio_block_write(
    device_index: usize,
    sector: u64,
    data: &[u8],
) -> OnceReceiver<VirtIOBlockResponse> {
    let devices_lock = VIRTIO_BLOCK.read();
    let mut device = devices_lock
        .get(device_index)
        .expect("invalid device index")
        .lock_disable_interrupts();

    let data_len = data.len() as u32;
    let data_len = data_len.next_multiple_of(VIRTIO_BLOCK_SECTOR_SIZE_BYTES);
    device.add_request(&BlockRequest::Write { sector, data_len }, Some(data))
}

fn virtio_block_interrupt(_vector: InterruptVector, handler_id: InterruptHandlerID) {
    let devices_lock = VIRTIO_BLOCK.read();
    let device: &mut VirtIOBlockDevice = &mut devices_lock
        .get(handler_id as usize)
        .expect("invalid device index")
        .lock_disable_interrupts();

    device.virtqueue.process_new_entries(|used_entry, mut descriptor_chain, data| {
        let Some(data) = data else {
            serial_println!("VirtIO Block: no virtqueue data entry for used entry: {used_entry:#x?}");
            return;
        };
        let buffer = data.buffer;

        let raw_request = RawBlockRequest::from_descriptor_chain(&mut descriptor_chain);
        assert!(raw_request.status == BlockRequestStatus::Ok, "expected an OK status, got {:#x?}", raw_request.status);

        let request = BlockRequest::from_raw(&raw_request);

        match request {
            BlockRequest::Read { .. } => {
                let response = VirtIOBlockReadResponse {
                    buffer,
                    data_len: raw_request.data_len as usize,
                };
                data.sender.send(VirtIOBlockResponse::Read(response));
            }
            BlockRequest::Write { .. } => {
                data.sender.send(VirtIOBlockResponse::Write);
                // N.B. Buffer gets dropped here. Do it explicitly.
                drop(buffer);
            }
            BlockRequest::GetID => {
                let s = unsafe {
                    // The device ID response is a null-terminated string with a max
                    // size of the buffer size (if the string size == buffer size, there
                    // is no null terminator)
                    strings::c_str_from_pointer(
                        buffer.address().as_ptr::<u8>(),
                        raw_request.data_len as usize,
                    )
                };
                data.sender.send(VirtIOBlockResponse::GetID { id: String::from(s) });
                // N.B. Buffer gets dropped here. Do it explicitly.
                drop(buffer);
            }
        }
    });
}

/// See "5.2 Block Device" in the VirtIO spec.
#[derive(Debug)]
struct VirtIOBlockDevice {
    initialized_device: VirtIOInitializedDevice<BlockDeviceFeatureBits>,
    _block_config: BlockConfigRegisters,
    virtqueue: VirtQueueData<BlockDeviceDescData>,
}

impl VirtIOBlockDevice {
    const VENDOR_IDS: [u16; 2] = [0x1001, 0x1042];

    fn from_device(device_config: VirtIODeviceConfig) -> Self {
        let device_id = device_config.pci_config().device_id().device_id();
        assert!(
            Self::VENDOR_IDS.contains(&device_id),
            "VirtIOBlockDevice: Device ID mismatch, got {device_id}"
        );

        let (initialized_device, virtqueues) = VirtIOInitializedDevice::new(
            device_config,
            |features: &mut BlockDeviceFeatureBits| {
                // Don't use multi queue for now
                features.remove(BlockDeviceFeatureBits::MQ);

                // Don't do flushes or caching either for now
                features.remove(BlockDeviceFeatureBits::CONFIG_WCE);
                features.remove(BlockDeviceFeatureBits::FLUSH);
            },
            1,
        );
        let num_virtqueues = virtqueues.len();
        let virtqueue = if let Ok::<[VirtQueue; 1], _>([virtqueue]) = virtqueues.try_into() {
            VirtQueueData::new(virtqueue)
        } else {
            panic!("VirtIORNG: expected exactly one virtqueue, got {num_virtqueues}");
        };

        let block_config = unsafe {
            BlockConfigRegisters::from_address(initialized_device.config.device_config_phys_addr())
        };

        Self {
            initialized_device,
            _block_config: block_config,
            virtqueue,
        }
    }

    fn add_request(
        &mut self,
        request: &BlockRequest,
        request_data: Option<&[u8]>,
    ) -> OnceReceiver<VirtIOBlockResponse> {
        let raw_request = request.to_raw();
        let (desc_chain, buffer) = raw_request.to_descriptor_chain(request_data);

        let (sender, receiver) = once_channel();
        let data = BlockDeviceDescData { buffer, sender };

        self.virtqueue.add_buffer(&desc_chain, data);
        self.virtqueue.notify_device();
        receiver
    }
}

bitflags! {
    #[derive(Debug)]
    #[repr(transparent)]
    /// See "5.2.3 Feature bits"
    struct BlockDeviceFeatureBits: u128 {
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
    Read { sector: u64, num_sectors: u32 },
    Write { sector: u64, data_len: u32 },
    GetID,
}

impl BlockRequest {
    /// All GET_ID requests MUST have a length of 20 bytes.
    const GET_ID_DATA_LEN: u32 = 20;

    fn to_raw(&self) -> RawBlockRequest {
        match self {
            Self::Read {
                sector,
                num_sectors,
            } => {
                let data_len = num_sectors * VIRTIO_BLOCK_SECTOR_SIZE_BYTES;
                RawBlockRequest::new(BlockRequestType::In, *sector, data_len)
            }
            Self::Write { sector, data_len } => {
                RawBlockRequest::new(BlockRequestType::Out, *sector, *data_len)
            }
            Self::GetID => RawBlockRequest::new(BlockRequestType::GetID, 0, Self::GET_ID_DATA_LEN),
        }
    }

    fn from_raw(raw: &RawBlockRequest) -> Self {
        match raw.request_type {
            BlockRequestType::In => {
                assert!(raw.data_len % VIRTIO_BLOCK_SECTOR_SIZE_BYTES == 0);
                let num_sectors = raw.data_len / VIRTIO_BLOCK_SECTOR_SIZE_BYTES;
                Self::Read {
                    sector: raw.sector,
                    num_sectors,
                }
            }
            BlockRequestType::Out => {
                assert!(raw.data_len % VIRTIO_BLOCK_SECTOR_SIZE_BYTES == 0);
                Self::Write {
                    sector: raw.sector,
                    data_len: raw.data_len,
                }
            }
            BlockRequestType::GetID => Self::GetID,
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
    data_len: u32,
    status: BlockRequestStatus,
}

#[repr(C)]
#[derive(Debug, FromZeroes, FromBytes, AsBytes)]
struct RawBlockRequestHeader {
    request_type: u32,
    reserved: u32,
    sector: u64,
}

impl RawBlockRequest {
    fn new(request_type: BlockRequestType, sector: u64, data_len: u32) -> Self {
        Self {
            request_type,
            sector,
            data_len,

            // Trick: write 111 to status, which is invalid, so we can be certain
            // that the device wrote the status. If we leave it as 0 we can't be
            // certain that the device wrote the status.
            status: BlockRequestStatus::Unset,
        }
    }

    /// Creates a descriptor (that is chained) for the block request. The
    /// returned buffer holds the underlying data for the descriptor chain. It
    /// is important that the buffer is not dropped before the descriptor chain.
    fn to_descriptor_chain(
        &self,
        request_data: Option<&[u8]>,
    ) -> ([ChainedVirtQueueDescriptorElem; 3], PhysicalBuffer) {
        // Compute how much data we need
        let header_align = core::mem::align_of::<RawBlockRequestHeader>();
        let header_offset = self.data_len + (self.data_len % header_align as u32);
        let header_size = core::mem::size_of::<RawBlockRequestHeader>() as u32;

        let status_align = core::mem::align_of::<BlockRequestStatus>();
        let status_raw_offset = header_offset + header_size;
        let status_offset = status_raw_offset + (status_raw_offset % status_align as u32);
        let status_size = core::mem::size_of::<BlockRequestStatus>() as u32;

        let total_size = status_offset + core::mem::size_of::<BlockRequestStatus>() as u32;
        let mut buffer = PhysicalBuffer::allocate_zeroed(total_size as usize)
            .expect("failed to allocate block request buffer");

        // Copy data, if present
        let buffer_address = buffer.address();
        let buffer_bytes = buffer.as_slice_mut();
        if let Some(request_data) = request_data {
            assert!(request_data.len() <= self.data_len as usize);
            for (i, byte) in request_data.iter().enumerate() {
                buffer_bytes[i] = *byte;
            }
        }

        // Put header right after data
        try_write_bytes_offset(
            buffer_bytes,
            header_offset as usize,
            RawBlockRequestHeader {
                request_type: self.request_type as u32,
                reserved: 0,
                sector: self.sector,
            },
        )
        .expect("failed to cast header bytes");
        let header_addr = buffer_address + u64::from(header_offset);
        let header_desc = ChainedVirtQueueDescriptorElem {
            addr: PhysAddr::from(header_addr),
            len: header_size,
            flags: VirtQueueDescriptorFlags::new().with_device_write(false),
        };

        // Buffer descriptor. Data is located right at the beginning of the
        // buffer.
        let writeable = self.request_type != BlockRequestType::Out;
        let buffer_desc = ChainedVirtQueueDescriptorElem {
            addr: PhysAddr::from(buffer_address),
            len: self.data_len,
            flags: VirtQueueDescriptorFlags::new().with_device_write(writeable),
        };

        // Put status right after header
        try_write_bytes_offset(buffer_bytes, status_offset as usize, self.status as u8)
            .expect("failed to cast header bytes");
        let status_addr = buffer_address + u64::from(status_offset);
        let status_desc = ChainedVirtQueueDescriptorElem {
            addr: PhysAddr::from(status_addr),
            len: status_size,
            flags: VirtQueueDescriptorFlags::new().with_device_write(true),
        };

        ([header_desc, buffer_desc, status_desc], buffer)
    }

    fn from_descriptor_chain(
        mut chain: impl Iterator<Item = ChainedVirtQueueDescriptorElem>,
    ) -> Self {
        let header_desc = chain.next().expect("missing header descriptor");
        let buffer_desc = chain.next().expect("missing buffer descriptor");
        let status_desc = chain.next().expect("missing status descriptor");
        assert!(chain.next().is_none(), "too many descriptors");

        assert!(header_desc.len == mem::size_of::<RawBlockRequestHeader>() as u32);
        let header_addr = KernPhysAddr::from(header_desc.addr);
        let header_ptr = header_addr.as_ptr::<RawBlockRequestHeader>();
        let header = unsafe { header_ptr.read_volatile() };

        let request_type = BlockRequestType::from_u32(header.request_type)
            .expect("invalid request type in header");

        assert!(status_desc.len == 1);
        let status_addr = KernPhysAddr::from(status_desc.addr);
        let status_ptr = status_addr.as_ptr::<u8>();
        let raw_status = unsafe { status_ptr.read_volatile() };
        let status =
            BlockRequestStatus::from_u8(raw_status).expect("invalid status in status descriptor");

        Self {
            request_type,
            sector: header.sector,
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
            x if x == Self::In as u32 => Some(Self::In),
            x if x == Self::Out as u32 => Some(Self::Out),
            x if x == Self::Flush as u32 => Some(Self::Flush),
            x if x == Self::GetID as u32 => Some(Self::GetID),
            x if x == Self::GetLifetime as u32 => Some(Self::GetLifetime),
            x if x == Self::Discard as u32 => Some(Self::Discard),
            x if x == Self::WriteZeroes as u32 => Some(Self::WriteZeroes),
            x if x == Self::SecureErase as u32 => Some(Self::SecureErase),
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

#[derive(Debug)]
struct BlockDeviceDescData {
    // Buffer is kept here so we can drop it when we are done with the request.
    buffer: PhysicalBuffer,
    sender: OnceSender<VirtIOBlockResponse>,
}

#[derive(Debug)]
pub(crate) enum VirtIOBlockResponse {
    Read(VirtIOBlockReadResponse),
    Write, // No data, just ACK that it completed
    GetID { id: String },
}

#[derive(Debug)]
pub(crate) struct VirtIOBlockReadResponse {
    buffer: PhysicalBuffer,
    data_len: usize,
}

impl VirtIOBlockReadResponse {
    pub(crate) fn data(&mut self) -> &mut [u8] {
        &mut self.buffer.as_slice_mut()[..self.data_len]
    }
}
