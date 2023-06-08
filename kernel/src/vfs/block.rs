use core::ops::Add;

use alloc::vec::Vec;

use crate::virtio;

/// Wrapper around a block device driver.
///
/// Note: we use `&self` and not `&mut self` on these methods. It is assumed
/// some form of lock is wrapping this.
///
/// TODO: This should be in a dedicated block layer (maybe with caching), not in
/// the VFS.
pub(crate) trait BlockDevice {
    fn device_block_size(&self) -> BlockSize;

    /// Number of _device_ blocks to read, using the device's block size.
    fn read_device_blocks(&self, start_block: BlockIndex, num_blocks: usize) -> Vec<u8>;

    /// Number of blocks to read using the given block size.
    fn read_blocks(
        &self,
        block_size: BlockSize,
        start_block: BlockIndex,
        num_blocks: usize,
    ) -> BlockBuffer {
        let block_size: u16 = block_size.0;
        let device_block_size: u16 = self.device_block_size().0;

        let device_start_block =
            BlockIndex(start_block.0 * u64::from(block_size / device_block_size));
        let device_num_blocks = num_blocks * block_size.div_ceil(device_block_size) as usize;
        let data = self.read_device_blocks(device_start_block, device_num_blocks);

        BlockBuffer {
            _index: start_block,
            data,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct BlockSize(u16);

impl BlockSize {
    pub(crate) const fn new(value: u16) -> Self {
        Self(value)
    }
}

impl From<u16> for BlockSize {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl From<BlockSize> for u16 {
    fn from(value: BlockSize) -> Self {
        value.0
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct BlockIndex(u64);

impl BlockIndex {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }
}

impl From<u64> for BlockIndex {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<BlockIndex> for u64 {
    fn from(value: BlockIndex) -> Self {
        value.0
    }
}

impl Add for BlockIndex {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

/// In-memory buffer for a disk block.
#[derive(Debug)]
pub(crate) struct BlockBuffer {
    /// Index into the block device this buffer is for.
    _index: BlockIndex,
    data: Vec<u8>,
}

impl BlockBuffer {
    pub(crate) fn data(&self) -> &[u8] {
        &self.data
    }

    pub(crate) fn _data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Index into the data block for this buffer, representing the bytes as a
    /// mutable reference to a type.
    pub(crate) fn _interpret_bytes_mut<T>(&mut self, offset: usize) -> &mut T {
        assert!(
            self.data.len() >= core::mem::size_of::<T>(),
            "data buffer is not large enough"
        );

        let ptr = unsafe { self.data.as_mut_ptr().add(offset).cast::<T>() };
        assert!(ptr.is_aligned(), "pointer {ptr:p} not aligned!");
        unsafe { ptr.as_mut().expect("pointer is null") }
    }

    /// Index into the data block for this buffer, representing the bytes as a
    /// reference to a type.
    pub(crate) fn interpret_bytes<T>(&self, offset: usize) -> &T {
        assert!(
            self.data.len() >= core::mem::size_of::<T>(),
            "data buffer is not large enough"
        );

        let ptr = unsafe { self.data.as_ptr().add(offset).cast::<T>() };
        assert!(ptr.is_aligned(), "pointer {ptr:p} not aligned!");
        unsafe { ptr.as_ref().expect("pointer is null") }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct VirtioBlockReader {
    device_id: usize,
}

impl VirtioBlockReader {
    pub(crate) fn new(device_id: usize) -> Self {
        Self { device_id }
    }
}

impl BlockDevice for VirtioBlockReader {
    fn device_block_size(&self) -> BlockSize {
        BlockSize::try_from(virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES as u16)
            .expect("invalid virtio block size")
    }

    fn read_device_blocks(&self, start_block: BlockIndex, num_blocks: usize) -> Vec<u8> {
        let response = virtio::virtio_block_read(self.device_id, start_block.0, num_blocks as u32)
            .wait_sleep();
        let virtio::VirtIOBlockResponse::Read{ ref data } = response else {
            panic!("unexpected virtio block response: {:?}", response);
        };
        data.clone()
    }
}
