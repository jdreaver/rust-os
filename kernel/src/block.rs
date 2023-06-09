use core::fmt::Debug;
use core::ops::{Add, Deref, DerefMut};

use alloc::boxed::Box;
use alloc::sync::Arc;
use zerocopy::FromBytes;

use crate::transmute::{TransmuteCollection, TransmuteView};
use crate::virtio;

/// Wrapper around a `BlockDeviceDriver` implementation.
#[derive(Debug)]
pub(crate) struct BlockDevice<D> {
    driver: Arc<D>,
}

impl<D: BlockDeviceDriver + 'static> BlockDevice<D> {
    pub(crate) fn new(driver: D) -> Self {
        Self {
            driver: Arc::new(driver),
        }
    }

    /// Number of blocks to read using the given block size.
    pub(crate) fn read_blocks(
        &self,
        block_size: BlockSize,
        start_block: BlockIndex,
        num_blocks: usize,
    ) -> BlockBuffer {
        let block_size: u16 = block_size.0;
        let device_block_size: u16 = self.driver.device_block_size().0;

        let device_start_block =
            BlockIndex(start_block.0 * u64::from(block_size / device_block_size));
        let device_num_blocks = num_blocks * block_size.div_ceil(device_block_size) as usize;
        let data = self
            .driver
            .read_device_blocks(device_start_block, device_num_blocks);

        BlockBuffer {
            device_start_block,
            _device_num_blocks: device_num_blocks,
            data,
            driver: self.driver.clone(),
        }
    }
}

pub(crate) trait BlockDeviceDriver: Debug {
    fn device_block_size(&self) -> BlockSize;

    /// Number of _device_ blocks to read, using the device's block size.
    fn read_device_blocks(&self, start_block: BlockIndex, num_blocks: usize) -> Box<[u8]>;

    fn write_device_blocks(&self, start_block: BlockIndex, data: &[u8]);
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
    device_start_block: BlockIndex,
    _device_num_blocks: usize,
    data: Box<[u8]>,
    driver: Arc<dyn BlockDeviceDriver>,
}

impl BlockBuffer {
    pub(crate) fn data(&self) -> &[u8] {
        &self.data
    }

    pub(crate) fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub(crate) fn into_view<T: FromBytes>(self) -> Option<TransmuteView<Self, T>> {
        TransmuteView::new(self)
    }

    pub(crate) fn into_collection<T: FromBytes>(self) -> TransmuteCollection<Self, T> {
        TransmuteCollection::new(self)
    }

    /// Flushes the block back to disk
    pub(crate) fn flush(&self) {
        assert!(
            self.data.len() % self.driver.device_block_size().0 as usize == 0,
            "BlockBuffer flush: data buffer is not a multiple of the device block size"
        );
        self.driver
            .write_device_blocks(self.device_start_block, &self.data);
    }
}

impl Deref for BlockBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for BlockBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl<T> AsRef<T> for BlockBuffer
where
    T: ?Sized,
    <Self as Deref>::Target: AsRef<T>,
{
    fn as_ref(&self) -> &T {
        self.deref().as_ref()
    }
}

impl<T> AsMut<T> for BlockBuffer
where
    T: ?Sized,
    <Self as Deref>::Target: AsMut<T>,
{
    fn as_mut(&mut self) -> &mut T {
        self.deref_mut().as_mut()
    }
}

pub(crate) fn virtio_block_device(device_id: usize) -> BlockDevice<VirtioBlockDevice> {
    BlockDevice::new(VirtioBlockDevice::new(device_id))
}

#[derive(Debug)]
pub(crate) struct VirtioBlockDevice {
    device_id: usize,
}

impl VirtioBlockDevice {
    fn new(device_id: usize) -> Self {
        Self { device_id }
    }
}

impl BlockDeviceDriver for VirtioBlockDevice {
    fn device_block_size(&self) -> BlockSize {
        BlockSize::try_from(virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES as u16)
            .expect("invalid virtio block size")
    }

    fn read_device_blocks(&self, start_block: BlockIndex, num_blocks: usize) -> Box<[u8]> {
        let response = virtio::virtio_block_read(self.device_id, start_block.0, num_blocks as u32)
            .wait_sleep();
        let virtio::VirtIOBlockResponse::Read(mut response) = response else {
            panic!("unexpected virtio block response: {:?}", response);
        };
        Box::from(&*response.data())
    }

    fn write_device_blocks(&self, start_block: BlockIndex, data: &[u8]) {
        let response = virtio::virtio_block_write(self.device_id, start_block.0, data).wait_sleep();
        let virtio::VirtIOBlockResponse::Write = response else {
            panic!("unexpected virtio block response: {:?}", response);
        };
    }
}
