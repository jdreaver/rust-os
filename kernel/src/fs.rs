use alloc::vec::Vec;

use crate::virtio;

pub(crate) struct VirtioBlockReader {
    device_id: usize,
}

impl VirtioBlockReader {
    pub fn new(device_id: usize) -> Self {
        Self { device_id }
    }
}

impl ext2::BlockReader for VirtioBlockReader {
    fn read_num_bytes(&mut self, addr: ext2::OffsetBytes, num_bytes: usize) -> Vec<u8> {
        let sector = addr.0 / u64::from(virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES);
        let sector_offset = addr.0 as usize % virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES as usize;

        let total_bytes = sector_offset + num_bytes;
        let num_sectors = total_bytes.div_ceil(virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES as usize);

        let response =
            virtio::virtio_block_read(self.device_id, sector, num_sectors as u32).wait_sleep();
        let virtio::VirtIOBlockResponse::Read{ ref data } = *response else {
            panic!("unexpected virtio block response: {:?}", response);
        };

        let mut data = data.clone();
        data.drain(0..sector_offset);
        data.drain(num_bytes..);
        data
    }
}
