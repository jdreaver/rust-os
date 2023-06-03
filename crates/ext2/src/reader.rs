use alloc::vec::Vec;

use crate::{
    BlockGroupDescriptor, DirectoryBlock, DirectoryEntry, Inode, InodeBitmap, InodeNumber,
    OffsetBytes, Superblock, ROOT_DIRECTORY,
};

pub struct FilesystemReader<R> {
    superblock: Superblock,
    block_reader: R,
}

impl<R: BlockReader> FilesystemReader<R> {
    pub fn read(mut block_reader: R) -> Option<Self> {
        let superblock: Superblock = block_reader.read_bytes(Superblock::OFFSET_BYTES);
        if !superblock.magic_valid() {
            return None;
        }

        Some(Self {
            superblock,
            block_reader,
        })
    }

    pub fn superblock(&self) -> &Superblock {
        &self.superblock
    }

    pub fn read_root(&mut self) -> Inode {
        self.read_inode(ROOT_DIRECTORY)
            .expect("couldn't read root directory inode!")
    }

    pub fn read_inode(&mut self, inode_number: InodeNumber) -> Option<Inode> {
        let (block_group_index, local_inode_index) = self.superblock.inode_location(inode_number);
        let block_group_offset = self.superblock.block_descriptor_offset(block_group_index);
        let block_group_descriptor: BlockGroupDescriptor =
            self.block_reader.read_bytes(block_group_offset);

        let inode_bitmap_block_address = block_group_descriptor.inode_bitmap;
        let inode_bitmap_offset = self
            .superblock
            .block_address_bytes(inode_bitmap_block_address);
        let inode_bitmap_buf = self
            .block_reader
            .read_num_bytes(inode_bitmap_offset, self.superblock.block_size().0 as usize);
        let inode_bitmap = InodeBitmap(&inode_bitmap_buf);
        let inode_used = inode_bitmap.is_used(local_inode_index)?;
        if !inode_used {
            return None;
        }

        let inode_table_block_address = block_group_descriptor.inode_table;
        let inode_offset = self
            .superblock
            .inode_offset(inode_table_block_address, local_inode_index);
        Some(self.block_reader.read_bytes(inode_offset))
    }

    pub fn iter_directory<F>(&mut self, inode: &Inode, func: F)
    where
        F: Fn(DirectoryEntry),
    {
        assert!(inode.is_dir());

        let direct_blocks = inode.direct_blocks;
        for block_addr in direct_blocks.iter() {
            let block_offset = self.superblock.block_address_bytes(block_addr);
            let block_buf = self
                .block_reader
                .read_num_bytes(block_offset, self.superblock.block_size().0 as usize);
            let dir_block = DirectoryBlock(block_buf.as_slice());
            for dir_entry in dir_block.iter() {
                func(dir_entry);
            }
        }
    }
}

/// Something that knows how to read blocks from the disk backing the
/// filesystem.
pub trait BlockReader {
    fn read_num_bytes(&mut self, addr: OffsetBytes, num_bytes: usize) -> Vec<u8>;

    fn read_bytes<T>(&mut self, addr: OffsetBytes) -> T {
        let buf = self.read_num_bytes(addr, core::mem::size_of::<T>());
        unsafe { buf.as_ptr().cast::<T>().read() }
    }
}
