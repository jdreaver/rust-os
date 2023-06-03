use alloc::vec::Vec;

use crate::virtio;

/// Information about and ext2 filesystem on top of a virtio-block device.
pub(crate) struct EXT2Filesystem {
    device_id: usize,
    superblock: ext2::Superblock,
}

impl EXT2Filesystem {
    pub(crate) fn read_from_disk(device_id: usize) -> Option<Self> {
        let superblock: ext2::Superblock =
            read_virtio_bytes(device_id, ext2::Superblock::OFFSET_BYTES);
        if !superblock.magic_valid() {
            return None;
        }

        Some(Self {
            device_id,
            superblock,
        })
    }

    pub(crate) fn superblock(&self) -> &ext2::Superblock {
        &self.superblock
    }

    pub(crate) fn read_root(&self) -> ext2::Inode {
        self.read_inode(ext2::ROOT_DIRECTORY)
            .expect("couldn't read root directory inode!")
    }

    pub(crate) fn read_inode(&self, inode_number: ext2::InodeNumber) -> Option<ext2::Inode> {
        let (block_group_index, local_inode_index) = self.superblock.inode_location(inode_number);
        let block_group_offset = self.superblock.block_descriptor_offset(block_group_index);
        let block_group_descriptor: ext2::BlockGroupDescriptor =
            read_virtio_bytes(self.device_id, block_group_offset);

        let inode_bitmap_block_address = block_group_descriptor.inode_bitmap;
        let inode_bitmap_offset = self
            .superblock
            .block_address_bytes(inode_bitmap_block_address);
        let inode_bitmap_buf = read_virtio_num_bytes(
            self.device_id,
            inode_bitmap_offset,
            self.superblock.block_size().0 as usize,
        );
        let inode_bitmap = ext2::InodeBitmap(&inode_bitmap_buf);
        let inode_used = inode_bitmap.is_used(local_inode_index)?;
        if !inode_used {
            return None;
        }

        let inode_table_block_address = block_group_descriptor.inode_table;
        let inode_offset = self
            .superblock
            .inode_offset(inode_table_block_address, local_inode_index);
        crate::serial_println!(
            "inode_table_block_address: {:#x?}",
            inode_table_block_address
        );
        crate::serial_println!("inode_offset: {:#x?}", inode_offset);
        Some(read_virtio_bytes(self.device_id, inode_offset))
    }
}

fn read_virtio_bytes<T>(device_id: usize, addr: ext2::OffsetBytes) -> T {
    let buf = read_virtio_num_bytes(device_id, addr, core::mem::size_of::<T>());
    unsafe { buf.as_ptr().cast::<T>().read() }
}

fn read_virtio_num_bytes(device_id: usize, addr: ext2::OffsetBytes, num_bytes: usize) -> Vec<u8> {
    let sector = addr.0 / u64::from(virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES);
    let sector_offset = addr.0 as usize % virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES as usize;

    let total_bytes = sector_offset + num_bytes;
    let num_sectors = total_bytes.div_ceil(virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES as usize);

    let response = virtio::virtio_block_read(device_id, sector, num_sectors as u32).wait_sleep();
    let virtio::VirtIOBlockResponse::Read{ ref data } = *response else {
        panic!("unexpected virtio block response: {:?}", response);
    };

    let mut data = data.clone();
    data.drain(0..sector_offset);
    data.drain(num_bytes..);
    data
}
