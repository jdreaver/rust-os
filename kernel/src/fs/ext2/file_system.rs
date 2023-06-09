use crate::block::{
    BlockBuffer, BlockBufferView, BlockDevice, BlockDeviceDriver, BlockIndex, BlockSize,
};

use super::block_group::{BlockBitmap, BlockGroupDescriptor, InodeBitmap};
use super::directory::{DirectoryBlock, DirectoryEntryFileType};
use super::inode::{Inode, InodeDirectBlocks, InodeMode};
use super::superblock::{
    BlockAddress, BlockGroupIndex, InodeNumber, LocalInodeIndex, OffsetBytes, Superblock,
    ROOT_DIRECTORY,
};

/// In-memory representation if ext2 file system, and main point of interaction
/// with file system.
#[derive(Debug)]
pub(super) struct FileSystem<D> {
    superblock: BlockBufferView<Superblock>,
    block_group_descriptors: BlockGroupDescriptorBlocks,
    block_size: BlockSize,

    device: BlockDevice<D>,
}

#[derive(Debug)]
struct BlockGroupDescriptorBlocks {
    blocks: BlockBuffer,
    num_block_groups: usize,
}

impl BlockGroupDescriptorBlocks {
    fn new(blocks: BlockBuffer, num_block_groups: usize) -> Self {
        assert!(
            blocks.data().len() >= num_block_groups * core::mem::size_of::<BlockGroupDescriptor>(),
            "block buffer not large enough to hold all block group descriptors"
        );
        Self {
            blocks,
            num_block_groups,
        }
    }

    fn get(&self, index: BlockGroupIndex) -> Option<&BlockGroupDescriptor> {
        let offset = self.offset(index)?;
        Some(self.blocks.interpret_bytes(offset))
    }

    fn get_mut(&mut self, index: BlockGroupIndex) -> Option<&mut BlockGroupDescriptor> {
        let offset = self.offset(index)?;
        Some(self.blocks.interpret_bytes_mut(offset))
    }

    fn offset(&self, index: BlockGroupIndex) -> Option<usize> {
        if index.0 as usize >= self.num_block_groups {
            return None;
        }
        Some(index.0 as usize * core::mem::size_of::<BlockGroupDescriptor>())
    }

    fn flush(&self) {
        self.blocks.flush();
    }
}

impl<D: BlockDeviceDriver + 'static> FileSystem<D> {
    pub(super) fn read(device: BlockDevice<D>) -> Option<Self> {
        let mut superblock: BlockBufferView<Superblock> = device
            .read_blocks(
                Superblock::SUPERBLOCK_BLOCK_SIZE,
                Superblock::SUPERBLOCK_BLOCK_INDEX,
                1,
            )
            .into_view();
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
        let block_group_descriptors =
            BlockGroupDescriptorBlocks::new(block_group_descriptors_blocks, num_block_groups);

        // Increase the mount count and write the superblock back
        superblock.mount_count += 1;
        superblock.flush();

        Some(Self {
            superblock,
            block_group_descriptors,
            block_size,
            device,
        })
    }

    pub(super) fn superblock(&self) -> &Superblock {
        &self.superblock
    }

    pub(super) fn read_root(&mut self) -> (Inode, InodeNumber) {
        let inode = self
            .read_inode(ROOT_DIRECTORY)
            .expect("couldn't read root directory inode!");
        (inode, ROOT_DIRECTORY)
    }

    pub(super) fn read_inode(&mut self, inode_number: InodeNumber) -> Option<Inode> {
        let (block_group_index, local_inode_index) = self.superblock.inode_location(inode_number);
        let block_group_descriptor = self.block_group_descriptors.get(block_group_index)?;

        let mut inode_bitmap_block = self.inode_bitmap_block(block_group_descriptor);
        let inode_bitmap = InodeBitmap::new(inode_bitmap_block.data_mut());
        let inode_used = inode_bitmap.is_used(local_inode_index)?;
        if !inode_used {
            return None;
        }

        let (inode_block, inode_offset) =
            self.inode_block(block_group_descriptor, local_inode_index);
        let inode: &Inode = inode_block.interpret_bytes(inode_offset.0 as usize);
        Some(inode.clone())
    }

    fn inode_bitmap_block(&self, block_group_descriptor: &BlockGroupDescriptor) -> BlockBuffer {
        let inode_bitmap_block_address =
            BlockIndex::from(u64::from(block_group_descriptor.inode_bitmap.0));
        self.device
            .read_blocks(self.block_size, inode_bitmap_block_address, 1)
    }

    fn inode_block(
        &self,
        block_group_descriptor: &BlockGroupDescriptor,
        local_inode_index: LocalInodeIndex,
    ) -> (BlockBuffer, OffsetBytes) {
        let (inode_block_index, inode_offset) = self
            .superblock()
            .inode_block_and_offset(block_group_descriptor.inode_table, local_inode_index);
        let buf = self
            .device
            .read_blocks(self.block_size, inode_block_index, 1);
        (buf, inode_offset)
    }

    fn block_bitmap_block(&self, block_group_descriptor: &BlockGroupDescriptor) -> BlockBuffer {
        let block_bitmap_block_address =
            BlockIndex::from(u64::from(block_group_descriptor.block_bitmap.0));
        self.device
            .read_blocks(self.block_size, block_bitmap_block_address, 1)
    }

    pub(super) fn inode_size(&self, inode: &Inode) -> u64 {
        // In revision 0, we only have 32-bit sizes.
        if self.superblock.rev_level == 0 {
            return u64::from(inode.size_low);
        }

        (u64::from(inode.size_high) << 32) | u64::from(inode.size_low)
    }

    pub(super) fn write_inode(&mut self, inode: Inode, inode_number: InodeNumber) {
        let (block_group_index, local_inode_index) = self.superblock.inode_location(inode_number);
        let block_group_descriptor = self
            .block_group_descriptors
            .get(block_group_index)
            .expect("failed to write inode, block group descriptor not found!");

        // Assert inode is marked as used
        let mut inode_bitmap_block = self.inode_bitmap_block(block_group_descriptor);
        let inode_bitmap = InodeBitmap::new(inode_bitmap_block.data_mut());
        let inode_used = inode_bitmap
            .is_used(local_inode_index)
            .expect("failed to read inode bitmap is_used!");
        assert!(inode_used, "inode {inode_number:?} is not marked as used!");

        // Write and flush inode block
        let (mut inode_block, inode_offset) =
            self.inode_block(block_group_descriptor, local_inode_index);
        *inode_block.interpret_bytes_mut(inode_offset.0 as usize) = inode;
        inode_block.flush();
    }

    pub(super) fn iter_file_blocks<F>(&mut self, inode: &Inode, mut func: F)
    where
        F: FnMut(usize, BlockBuffer) -> bool,
    {
        let direct_blocks = inode.direct_blocks;
        for (i, block_addr) in direct_blocks.iter().enumerate() {
            let addr = BlockIndex::from(u64::from(block_addr.0));
            let block_buf = self.device.read_blocks(self.block_size, addr, 1);

            if !func(i, block_buf) {
                return;
            }
        }
    }

    pub(super) fn iter_directory_blocks<F>(&mut self, inode: &Inode, mut func: F)
    where
        F: FnMut(DirectoryBlock) -> bool,
    {
        assert!(inode.is_dir());

        // Invariant: the directory data length is always a multiple of the
        // block size. Extra space manifests as directory entries with rec_len
        // longer than necessary.
        let inode_size = self.inode_size(inode);
        let block_size = u64::from(u16::from(self.superblock.block_size()));
        assert!(inode_size % block_size == 0, "invariant violated: directory size {inode_size} not a multiple of block size {block_size}");

        self.iter_file_blocks(inode, |_, buf| {
            let dir_block = DirectoryBlock::from_existing_block(buf);
            func(dir_block)
        });
    }

    pub(super) fn create_file(
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
        let (block_group_index, _) = self.superblock.inode_location(parent_number);
        let block_group_descriptor = self.block_group_descriptors.get(block_group_index)?;

        let mut inode_bitmap_block = self.inode_bitmap_block(block_group_descriptor);
        let mut inode_bitmap = InodeBitmap::new(inode_bitmap_block.data_mut());
        let Some(local_inode_index) = inode_bitmap.reserve_next_free() else {
            log::error!("no free inode found in block group {block_group_descriptor:?}");
            return None;
        };
        inode_bitmap_block.flush();

        // Reserve one blocks for file content
        let mut block_bitmap_block = self.block_bitmap_block(block_group_descriptor);
        let mut block_bitmap = BlockBitmap::new(block_bitmap_block.data_mut());
        let Some(block_address) = block_bitmap.reserve_next_free() else {
            log::error!("no free block found in block group {block_group_descriptor:?}");
            return None;
        };
        block_bitmap_block.flush();

        // Add inode entry to block group's inode table
        let mut direct_blocks = InodeDirectBlocks::empty();
        direct_blocks.insert(0, block_address);
        let block_size = u32::from(u16::from(self.superblock.block_size()));
        let blocks = block_size / 512; // Remember, blocks are in units if 512 bytes!
        let inode = Inode {
            mode: InodeMode::IROTH
                | InodeMode::IRGRP
                | InodeMode::IWUSR
                | InodeMode::IRUSR
                | InodeMode::IFREG,
            uid: parent.uid,
            size_low: 0,
            atime: 0,
            ctime: 0,
            mtime: 0,
            dtime: 0,
            gid: parent.gid,
            links_count: 1,
            blocks, // Reserved above
            flags: 0,
            osd1: 0,
            direct_blocks,
            singly_indirect_block: BlockAddress(0),
            doubly_indirect_block: BlockAddress(0),
            triply_indirect_block: BlockAddress(0),
            generation: 0,
            file_acl: 0,
            size_high: 0,
            faddr: 0,
            osd2: [0; 12],
        };
        let inode_number = self
            .superblock()
            .inode_number(block_group_index, local_inode_index);
        let cloned_inode = inode.clone();
        self.write_inode(inode, inode_number);

        // Add inode to parent directory's list of directories
        let mut found_free_entry = false;
        self.iter_directory_blocks(parent, |mut dir_block| {
            if dir_block
                .insert_entry(name, inode_number, DirectoryEntryFileType::RegularFile)
                .is_some()
            {
                dir_block.flush();
                found_free_entry = true;
                return false;
            }
            true
        });

        if !found_free_entry {
            log::error!("no free entry found in parent directory {parent:?}");
            return None;
        }

        // Adjust block group statistics
        let block_group_descriptor = self.block_group_descriptors.get_mut(block_group_index)?;
        block_group_descriptor.free_inodes_count -= 1;
        self.block_group_descriptors.flush();

        // Adjust superblock statistics
        self.superblock.free_inodes_count -= 1;
        self.superblock.flush();

        Some((cloned_inode, inode_number))
    }
}
