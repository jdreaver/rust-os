use alloc::vec::Vec;

use crate::vfs;

use super::block_group::{BlockGroupDescriptor, InodeBitmap};
use super::directory::{DirectoryBlock, DirectoryEntry};
use super::inode::Inode;
use super::superblock::{InodeNumber, Superblock, ROOT_DIRECTORY};

#[derive(Debug)]
pub(crate) struct FilesystemReader<R> {
    superblock: Superblock,
    block_reader: R,
}

impl<R: vfs::BlockReader> FilesystemReader<R> {
    pub(crate) fn read(block_reader: R) -> Option<Self> {
        let superblock: Superblock = block_reader.read_bytes(Superblock::OFFSET_BYTES.0);
        if !superblock.magic_valid() {
            return None;
        }

        Some(Self {
            superblock,
            block_reader,
        })
    }

    pub(crate) fn superblock(&self) -> &Superblock {
        &self.superblock
    }

    pub(crate) fn read_root(&mut self) -> Inode {
        self.read_inode(ROOT_DIRECTORY)
            .expect("couldn't read root directory inode!")
    }

    pub(crate) fn read_inode(&mut self, inode_number: InodeNumber) -> Option<Inode> {
        let (block_group_index, local_inode_index) = self.superblock.inode_location(inode_number);
        let block_group_offset = self.superblock.block_descriptor_offset(block_group_index);
        let block_group_descriptor: BlockGroupDescriptor =
            self.block_reader.read_bytes(block_group_offset.0);

        let inode_bitmap_block_address = block_group_descriptor.inode_bitmap;
        let inode_bitmap_offset = self
            .superblock
            .block_address_bytes(inode_bitmap_block_address);
        let inode_bitmap_buf = self.block_reader.read_num_bytes(
            inode_bitmap_offset.0,
            self.superblock.block_size().0 as usize,
        );
        let inode_bitmap = InodeBitmap(&inode_bitmap_buf);
        let inode_used = inode_bitmap.is_used(local_inode_index)?;
        if !inode_used {
            return None;
        }

        let inode_table_block_address = block_group_descriptor.inode_table;
        let inode_offset = self
            .superblock
            .inode_offset(inode_table_block_address, local_inode_index);
        Some(self.block_reader.read_bytes(inode_offset.0))
    }

    pub(crate) fn inode_size(&self, inode: &Inode) -> u64 {
        // In revision 0, we only have 32-bit sizes.
        if self.superblock.rev_level == 0 {
            return u64::from(inode.size_low);
        }

        (u64::from(inode.size_high) << 32) | u64::from(inode.size_low)
    }

    pub(crate) fn iter_file_blocks<F>(&mut self, inode: &Inode, mut func: F)
    where
        F: FnMut(Vec<u8>),
    {
        let mut seen_size = 0;
        let total_size = self.inode_size(inode);

        let direct_blocks = inode.direct_blocks;
        for block_addr in direct_blocks.iter() {
            let block_offset = self.superblock.block_address_bytes(block_addr);
            let mut block_buf = self
                .block_reader
                .read_num_bytes(block_offset.0, self.superblock.block_size().0 as usize);

            if seen_size + block_buf.len() as u64 > total_size {
                block_buf.truncate((total_size - seen_size) as usize);
            }
            seen_size += block_buf.len() as u64;

            func(block_buf);
        }
    }

    pub(crate) fn iter_directory<F>(&mut self, inode: &Inode, mut func: F)
    where
        F: FnMut(DirectoryEntry) -> bool,
    {
        assert!(inode.is_dir());

        self.iter_file_blocks(inode, |block_buf| {
            let dir_block = DirectoryBlock(block_buf.as_slice());
            for dir_entry in dir_block.iter() {
                if !func(dir_entry) {
                    break;
                }
            }
        });
    }
}
