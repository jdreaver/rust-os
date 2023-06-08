use alloc::vec::Vec;

use crate::virtio;

/// Something that knows how to read blocks from the disk backing the
/// filesystem.
///
/// Note: we use `&self` and not `&mut self` on these methods. It is assumed
/// that the block reader is using some form of locking.
///
/// TODO: This should be in a dedicated block layer (maybe with caching), not in
/// the VFS.
pub(crate) trait BlockReader {
    fn read_num_bytes(&self, addr: u64, num_bytes: usize) -> Vec<u8>;

    fn read_bytes<T>(&self, addr: u64) -> T {
        let buf = self.read_num_bytes(addr, core::mem::size_of::<T>());
        unsafe { buf.as_ptr().cast::<T>().read() }
    }
}

#[derive(Debug)]
pub(crate) struct VirtioBlockReader {
    device_id: usize,
}

impl VirtioBlockReader {
    pub(crate) fn new(device_id: usize) -> Self {
        Self { device_id }
    }
}

impl BlockReader for VirtioBlockReader {
    fn read_num_bytes(&self, addr: u64, num_bytes: usize) -> Vec<u8> {
        let sector = addr / u64::from(virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES);
        let sector_offset = addr as usize % virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES as usize;

        let total_bytes = sector_offset + num_bytes;
        let num_sectors = total_bytes.div_ceil(virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES as usize);

        let response =
            virtio::virtio_block_read(self.device_id, sector, num_sectors as u32).wait_sleep();
        let virtio::VirtIOBlockResponse::Read{ ref data } = response else {
            panic!("unexpected virtio block response: {:?}", response);
        };

        let mut data = data.clone();
        data.drain(0..sector_offset);
        data.drain(num_bytes..);
        data
    }
}
