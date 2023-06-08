use crate::vfs;

use super::block_group::{BlockGroupDescriptor, InodeBitmap};
use super::directory::{DirectoryBlock, DirectoryEntry};
use super::inode::Inode;
use super::superblock::{InodeNumber, Superblock, ROOT_DIRECTORY};
use super::BlockGroupIndex;

/// In-memory representation if ext2 file system, and main point of interaction
/// with file system.
#[derive(Debug)]
pub(crate) struct FileSystem<R> {
    // N.B. Storing raw blocks so writing them back to the disk device is
    // trivial, and to ensure we don't leak memory if we e.g. cast only part of
    // the block to a type and forget the rest of the bytes.
    superblock_block: vfs::BlockBuffer,
    block_group_descriptors_blocks: vfs::BlockBuffer,
    num_block_groups: usize,
    block_size: vfs::BlockSize,

    block_reader: R,
}

impl<R: vfs::BlockReader> FileSystem<R> {
    pub(crate) fn read(block_reader: R) -> Option<Self> {
        let superblock_block = block_reader.read_blocks(
            Superblock::SUPERBLOCK_BLOCK_SIZE,
            Superblock::SUPERBLOCK_BLOCK_INDEX,
            1,
        );
        let superblock: &Superblock = superblock_block.interpret_bytes(0);
        if !superblock.magic_valid() {
            return None;
        }
        let block_size = superblock.block_size();

        let num_block_groups = superblock.num_block_groups();
        let num_descriptor_blocks = num_block_groups.div_ceil(superblock.block_size().into());
        let descriptor_block_start = superblock.block_descriptor_table_start_block();
        let block_group_descriptors_blocks =
            block_reader.read_blocks(block_size, descriptor_block_start, num_descriptor_blocks);

        Some(Self {
            superblock_block,
            block_group_descriptors_blocks,
            num_block_groups,
            block_size,
            block_reader,
        })
    }

    pub(crate) fn superblock(&self) -> &Superblock {
        self.superblock_block.interpret_bytes(0)
    }

    fn block_group_descriptor(&self, index: BlockGroupIndex) -> Option<&BlockGroupDescriptor> {
        if index.0 as usize >= self.num_block_groups {
            return None;
        }
        let offset = index.0 as usize * core::mem::size_of::<BlockGroupDescriptor>();
        Some(self.block_group_descriptors_blocks.interpret_bytes(offset))
    }

    pub(crate) fn read_root(&mut self) -> Inode {
        self.read_inode(ROOT_DIRECTORY)
            .expect("couldn't read root directory inode!")
    }

    pub(crate) fn read_inode(&mut self, inode_number: InodeNumber) -> Option<Inode> {
        let (block_group_index, local_inode_index) = self.superblock().inode_location(inode_number);
        let block_group_descriptor = self.block_group_descriptor(block_group_index)?;

        let inode_bitmap_block_address =
            vfs::BlockIndex::from(u64::from(block_group_descriptor.inode_bitmap.0));
        let inode_bitmap_block =
            self.block_reader
                .read_blocks(self.block_size, inode_bitmap_block_address, 1);
        let inode_bitmap = InodeBitmap(inode_bitmap_block.data());
        let inode_used = inode_bitmap.is_used(local_inode_index)?;
        if !inode_used {
            return None;
        }

        let inode_table_block_address =
            vfs::BlockIndex::from(u64::from(block_group_descriptor.inode_table.0 .0));
        let inode_table_num_blocks = self.superblock().inode_table_num_blocks();
        let inode_table_blocks = self.block_reader.read_blocks(
            self.block_size,
            inode_table_block_address,
            inode_table_num_blocks,
        );
        let inode_offset = self.superblock().inode_offset(local_inode_index);
        let inode: &Inode = inode_table_blocks.interpret_bytes(inode_offset.0 as usize);
        Some(inode.clone())
    }

    pub(crate) fn inode_size(&self, inode: &Inode) -> u64 {
        // In revision 0, we only have 32-bit sizes.
        if self.superblock().rev_level == 0 {
            return u64::from(inode.size_low);
        }

        (u64::from(inode.size_high) << 32) | u64::from(inode.size_low)
    }

    pub(crate) fn iter_file_blocks<F>(&mut self, inode: &Inode, mut func: F)
    where
        F: FnMut(&[u8]),
    {
        let mut seen_size = 0;
        let total_size = self.inode_size(inode) as usize;

        let direct_blocks = inode.direct_blocks;
        for block_addr in direct_blocks.iter() {
            let addr = vfs::BlockIndex::from(u64::from(block_addr.0));
            let block_buf = self.block_reader.read_blocks(self.block_size, addr, 1);

            let data = block_buf.data();
            let data: &[u8] = if seen_size + data.len() > total_size {
                let slice_end = total_size - seen_size;
                &data[..slice_end]
            } else {
                data
            };
            seen_size += data.len();

            func(data);
        }
    }

    pub(crate) fn iter_directory<F>(&mut self, inode: &Inode, mut func: F)
    where
        F: FnMut(DirectoryEntry) -> bool,
    {
        assert!(inode.is_dir());

        self.iter_file_blocks(inode, |data| {
            let dir_block = DirectoryBlock(data);
            for dir_entry in dir_block.iter() {
                if !func(dir_entry) {
                    break;
                }
            }
        });
    }
}