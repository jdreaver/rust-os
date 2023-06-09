use crate::block::{BlockBuffer, BlockDevice, BlockDeviceDriver, BlockIndex, BlockSize};

use super::block_group::{BlockGroupDescriptor, InodeBitmap};
use super::directory::{DirectoryBlock, DirectoryEntry};
use super::inode::Inode;
use super::superblock::{InodeNumber, Superblock, ROOT_DIRECTORY};
use super::BlockGroupIndex;

/// In-memory representation if ext2 file system, and main point of interaction
/// with file system.
#[derive(Debug)]
pub(crate) struct FileSystem<D> {
    // N.B. Storing raw blocks so writing them back to the disk device is
    // trivial, and to ensure we don't leak memory if we e.g. cast only part of
    // the block to a type and forget the rest of the bytes.
    superblock_block: BlockBuffer,
    block_group_descriptors_blocks: BlockBuffer,
    num_block_groups: usize,
    block_size: BlockSize,

    device: BlockDevice<D>,
}

impl<D: BlockDeviceDriver + 'static> FileSystem<D> {
    pub(crate) fn read(device: BlockDevice<D>) -> Option<Self> {
        let mut superblock_block = device.read_blocks(
            Superblock::SUPERBLOCK_BLOCK_SIZE,
            Superblock::SUPERBLOCK_BLOCK_INDEX,
            1,
        );
        let superblock: &mut Superblock = superblock_block.interpret_bytes_mut(0);
        if !superblock.magic_valid() {
            return None;
        }
        let block_size = superblock.block_size();

        let num_block_groups = superblock.num_block_groups();
        let num_descriptor_blocks =
            num_block_groups.div_ceil(usize::from(u16::from(superblock.block_size())));
        let descriptor_block_start = superblock.block_descriptor_table_start_block();
        let block_group_descriptors_blocks =
            device.read_blocks(block_size, descriptor_block_start, num_descriptor_blocks);

        // Increase the mount count and write the superblock back
        superblock.mount_count += 1;
        superblock_block.flush();

        Some(Self {
            superblock_block,
            block_group_descriptors_blocks,
            num_block_groups,
            block_size,
            device,
        })
    }

    pub(crate) fn superblock(&self) -> &Superblock {
        self.superblock_block.interpret_bytes(0)
    }

    pub(crate) fn superblock_mut(&mut self) -> &mut Superblock {
        self.superblock_block.interpret_bytes_mut(0)
    }

    fn block_group_descriptor(&self, index: BlockGroupIndex) -> Option<&BlockGroupDescriptor> {
        let offset = self.block_group_descriptor_ofsset(index)?;
        Some(self.block_group_descriptors_blocks.interpret_bytes(offset))
    }

    fn block_group_descriptor_mut(
        &mut self,
        index: BlockGroupIndex,
    ) -> Option<&mut BlockGroupDescriptor> {
        let offset = self.block_group_descriptor_ofsset(index)?;
        Some(
            self.block_group_descriptors_blocks
                .interpret_bytes_mut(offset),
        )
    }

    fn block_group_descriptor_ofsset(&self, index: BlockGroupIndex) -> Option<usize> {
        if index.0 as usize >= self.num_block_groups {
            return None;
        }
        Some(index.0 as usize * core::mem::size_of::<BlockGroupDescriptor>())
    }

    fn flush_block_group_descriptors(&mut self) {
        self.block_group_descriptors_blocks.flush();
    }

    pub(crate) fn read_root(&mut self) -> (Inode, InodeNumber) {
        let inode = self
            .read_inode(ROOT_DIRECTORY)
            .expect("couldn't read root directory inode!");
        (inode, ROOT_DIRECTORY)
    }

    pub(crate) fn read_inode(&mut self, inode_number: InodeNumber) -> Option<Inode> {
        let (block_group_index, local_inode_index) = self.superblock().inode_location(inode_number);
        let block_group_descriptor = self.block_group_descriptor(block_group_index)?;

        let mut inode_bitmap_block = self.inode_bitmap_block(block_group_descriptor);
        let inode_bitmap = InodeBitmap(inode_bitmap_block.data_mut());
        let inode_used = inode_bitmap.is_used(local_inode_index)?;
        if !inode_used {
            return None;
        }

        let (inode_block_index, inode_offset) = self
            .superblock()
            .inode_block_and_offset(block_group_descriptor.inode_table, local_inode_index);
        let inode_block = self
            .device
            .read_blocks(self.block_size, inode_block_index, 1);
        let inode: &Inode = inode_block.interpret_bytes(inode_offset.0 as usize);
        Some(inode.clone())
    }

    fn inode_bitmap_block(&self, block_group_descriptor: &BlockGroupDescriptor) -> BlockBuffer {
        let inode_bitmap_block_address =
            BlockIndex::from(u64::from(block_group_descriptor.inode_bitmap.0));
        self.device
            .read_blocks(self.block_size, inode_bitmap_block_address, 1)
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
        F: FnMut(&mut BlockBuffer),
    {
        let direct_blocks = inode.direct_blocks;
        for block_addr in direct_blocks.iter() {
            let addr = BlockIndex::from(u64::from(block_addr.0));
            let mut block_buf = self.device.read_blocks(self.block_size, addr, 1);

            func(&mut block_buf);
        }
    }

    pub(crate) fn iter_file_data<F>(&mut self, inode: &Inode, mut func: F)
    where
        F: FnMut(&[u8]),
    {
        let mut seen_size = 0;
        let total_size = self.inode_size(inode) as usize;

        self.iter_file_blocks(inode, |block_buf| {
            let data = block_buf.data();
            let data: &[u8] = if seen_size + data.len() > total_size {
                let slice_end = total_size - seen_size;
                &data[..slice_end]
            } else {
                data
            };
            seen_size += data.len();

            func(data);
        });
    }

    pub(crate) fn iter_directory<F>(&mut self, inode: &Inode, mut func: F)
    where
        F: FnMut(DirectoryEntry) -> bool,
    {
        assert!(inode.is_dir());

        self.iter_file_data(inode, |data| {
            let dir_block = DirectoryBlock(data);
            for dir_entry in dir_block.iter() {
                if !func(dir_entry) {
                    break;
                }
            }
        });
    }

    pub(crate) fn create_file(
        &mut self,
        parent: &Inode,
        parent_number: InodeNumber,
        name: &str,
    ) -> Option<(Inode, InodeNumber)> {
        assert!(
            parent.is_dir(),
            "tried to create file in non-directory inode {parent:?}"
        );

        // Reserve inode number in parent's block group by finding free entry in bitmap
        let (block_group_index, _) = self.superblock().inode_location(parent_number);
        let block_group_descriptor = self.block_group_descriptor(block_group_index)?;

        let mut inode_bitmap_block = self.inode_bitmap_block(block_group_descriptor);
        let mut inode_bitmap = InodeBitmap(inode_bitmap_block.data_mut());
        let Some(inode_index) = inode_bitmap.reserve_next_free() else {
            log::error!("no free inode found in block group {block_group_descriptor:?}");
            return None;
        };
        inode_bitmap_block.flush();
        log::warn!(
            "reserved inode {inode_index:?} for {name} in block group {block_group_descriptor:?}"
        );

        // Reserve some blocks for file content

        // Add inode entry to block group's inode table

        // Add inode to parent directory's list of directories

        // Adjust block group statistics
        let block_group_descriptor = self.block_group_descriptor_mut(block_group_index)?;
        block_group_descriptor.free_inodes_count -= 1;
        self.flush_block_group_descriptors();

        // Adjust superblock statistics
        self.superblock_mut().free_inodes_count -= 1;
        self.superblock_block.flush();

        None
    }
}
