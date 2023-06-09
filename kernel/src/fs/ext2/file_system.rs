use alloc::string::String;

use crate::block::{
    BlockBuffer, BlockBufferView, BlockDevice, BlockDeviceDriver, BlockIndex, BlockSize,
};

use super::block_group::{BlockGroupDescriptor, InodeBitmap};
use super::directory::{
    DirectoryBlock, DirectoryEntry, DirectoryEntryFileType, DirectoryEntryHeader,
};
use super::inode::{Inode, InodeDirectBlocks, InodeMode};
use super::superblock::{
    BlockAddress, InodeNumber, LocalInodeIndex, OffsetBytes, Superblock, ROOT_DIRECTORY,
};
use super::BlockGroupIndex;

/// In-memory representation if ext2 file system, and main point of interaction
/// with file system.
#[derive(Debug)]
pub(crate) struct FileSystem<D> {
    // N.B. Storing raw blocks so writing them back to the disk device is
    // trivial, and to ensure we don't leak memory if we e.g. cast only part of
    // the block to a type and forget the rest of the bytes.
    superblock: BlockBufferView<Superblock>,
    block_group_descriptors_blocks: BlockBuffer,
    num_block_groups: usize,
    block_size: BlockSize,

    device: BlockDevice<D>,
}

impl<D: BlockDeviceDriver + 'static> FileSystem<D> {
    pub(crate) fn read(device: BlockDevice<D>) -> Option<Self> {
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

        // Increase the mount count and write the superblock back
        superblock.mount_count += 1;
        superblock.flush();

        Some(Self {
            superblock,
            block_group_descriptors_blocks,
            num_block_groups,
            block_size,
            device,
        })
    }

    pub(crate) fn superblock(&self) -> &Superblock {
        &self.superblock
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
        let (block_group_index, local_inode_index) = self.superblock.inode_location(inode_number);
        let block_group_descriptor = self.block_group_descriptor(block_group_index)?;

        let mut inode_bitmap_block = self.inode_bitmap_block(block_group_descriptor);
        let inode_bitmap = InodeBitmap(inode_bitmap_block.data_mut());
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

    pub(crate) fn inode_size(&self, inode: &Inode) -> u64 {
        // In revision 0, we only have 32-bit sizes.
        if self.superblock.rev_level == 0 {
            return u64::from(inode.size_low);
        }

        (u64::from(inode.size_high) << 32) | u64::from(inode.size_low)
    }

    pub(crate) fn append_to_inode_data(&mut self, inode: &Inode, data: &[u8]) {
        let direct_blocks = inode.direct_blocks;
        let Some(last_block) = direct_blocks.iter().last() else {
            log::error!("append_to_file: no blocks in inode. TODO: Support adding new blocks");
            return;
        };
        let last_block = BlockIndex::from(u64::from(last_block.0));

        let mut index_in_block = self.inode_size(inode) % u64::from(u16::from(self.block_size)) + 1;
        let mut block_buf = self.device.read_blocks(self.block_size, last_block, 1);
        let block_data = block_buf.data_mut();
        for byte in data.iter() {
            if index_in_block >= block_data.len() as u64 {
                log::error!("append_to_file: ran out of space in the block. TODO: Support adding new blocks");
                return;
            }

            block_data[index_in_block as usize] = *byte;
            index_in_block += 1;
        }

        block_buf.flush();
    }

    pub(crate) fn iter_file_blocks<F>(&mut self, inode: &Inode, mut func: F)
    where
        F: FnMut(usize, &mut BlockBuffer),
    {
        let direct_blocks = inode.direct_blocks;
        for (i, block_addr) in direct_blocks.iter().enumerate() {
            let addr = BlockIndex::from(u64::from(block_addr.0));
            let mut block_buf = self.device.read_blocks(self.block_size, addr, 1);

            func(i, &mut block_buf);
        }
    }

    pub(crate) fn iter_directory<F>(&mut self, inode: &Inode, mut func: F)
    where
        F: FnMut(DirectoryEntry) -> bool,
    {
        assert!(inode.is_dir());

        // Invariant: the directory data length is always a multiple of the
        // block size. Extra space manifests as directory entries with rec_len
        // longer than necessary.
        let inode_size = self.inode_size(inode);
        let block_size = u64::from(u16::from(self.superblock.block_size()));
        assert!(inode_size % block_size == 0, "invariant violated: directory size {inode_size} not a multiple of block size {block_size}");

        self.iter_file_blocks(inode, |_, data| {
            let dir_block = DirectoryBlock(data.data());
            for dir_entry in dir_block.iter() {
                log::warn!("dir_entry: {:?}", dir_entry);
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
        let (block_group_index, _) = self.superblock.inode_location(parent_number);
        let block_group_descriptor = self.block_group_descriptor(block_group_index)?;

        let mut inode_bitmap_block = self.inode_bitmap_block(block_group_descriptor);
        let mut inode_bitmap = InodeBitmap(inode_bitmap_block.data_mut());
        let Some(local_inode_index) = inode_bitmap.reserve_next_free() else {
            log::error!("no free inode found in block group {block_group_descriptor:?}");
            return None;
        };
        inode_bitmap_block.flush();

        // TODO: Reserve some blocks for file content?

        // Add inode entry to block group's inode table
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
            blocks: 0,
            flags: 0,
            osd1: 0,
            direct_blocks: InodeDirectBlocks::empty(),
            singly_indirect_block: BlockAddress(0),
            doubly_indirect_block: BlockAddress(0),
            triply_indirect_block: BlockAddress(0),
            generation: 0,
            file_acl: 0,
            size_high: 0,
            faddr: 0,
            osd2: [0; 12],
        };
        let (mut inode_block, inode_offset) =
            self.inode_block(block_group_descriptor, local_inode_index);
        *inode_block.interpret_bytes_mut(inode_offset.0 as usize) = inode;
        inode_block.flush();

        // Add inode to parent directory's list of directories
        let inode_number = self
            .superblock()
            .inode_number(block_group_index, local_inode_index);
        let entry = DirectoryEntry {
            header: DirectoryEntryHeader {
                inode: inode_number,
                rec_len: 0,
                name_len: name.len() as u8,
                file_type: DirectoryEntryFileType::RegularFile,
            },
            name: String::from(name),
        };

        // TODO: Iterate through the existing directory entries, and find a spot
        // where we can fit the new entry. We need to adjust the previous entry
        // to point to the new entry, and the new entry to point to the next
        // entry (or the end of the block).

        // TODO: We need to adjust the previous record, maybe:
        //
        // https://www.nongnu.org/ext2-doc/ext2.html#ifdir-rec-len
        //
        // rec_len
        //
        // 16bit unsigned displacement to the next directory entry from the
        // start of the current directory entry. This field must have a value at
        // least to the length of the current record.
        //
        // The directory entries must be aligned on 4 bytes boundaries and there
        // cannot be any directory entry spanning multiple data blocks. If an
        // entry cannot completely fit in one block, it must be pushed to the
        // next data block and the rec_len of the previous entry properly
        // adjusted.

        // Adjust block group statistics
        let block_group_descriptor = self.block_group_descriptor_mut(block_group_index)?;
        block_group_descriptor.free_inodes_count -= 1;
        self.flush_block_group_descriptors();

        // Adjust superblock statistics
        self.superblock.free_inodes_count -= 1;
        self.superblock.flush();

        None
    }
}
