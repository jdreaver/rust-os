use crate::{BlockAddress, LocalInodeIndex};

/// See <https://www.nongnu.org/ext2-doc/ext2.html#block-group-descriptor-structure>
#[repr(C, packed)]
#[derive(Debug)]
pub struct BlockGroupDescriptor {
    pub block_bitmap: BlockAddress,
    pub inode_bitmap: BlockAddress,
    pub inode_table: InodeTableBlockAddress,
    pub free_blocks_count: u16,
    pub free_inodes_count: u16,
    pub used_dirs_count: u16,
    _pad: u16,
    _reserved: [u8; 12],
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct InodeTableBlockAddress(pub BlockAddress);

/// See <https://www.nongnu.org/ext2-doc/ext2.html#block-bitmap>
#[derive(Debug)]
pub struct BlockBitmap<'a>(pub &'a [u8]);

impl<'a> BlockBitmap<'a> {
    /// Each bit represent the current state of a block within that block group,
    /// where 1 means "used" and 0 "free/available". The first block of this
    /// block group is represented by bit 0 of byte 0, the second by bit 1 of
    /// byte 0. The 8th block is represented by bit 7 (most significant bit) of
    /// byte 0 while the 9th block is represented by bit 0 (least significant
    /// bit) of byte 1.
    pub fn is_used(&self, block: BlockAddress) -> Option<bool> {
        let index = block.0 / 8;
        let byte = self.0.get(index as usize)?;
        let bit = block.0 % 8;
        let mask = 1 << bit;
        Some(byte & mask != 0)
    }
}

/// See <https://www.nongnu.org/ext2-doc/ext2.html#inode-bitmap>
#[derive(Debug)]
pub struct InodeBitmap<'a>(pub &'a [u8]);

impl<'a> InodeBitmap<'a> {
    ///  The "Inode Bitmap" works in a similar way as the "Block Bitmap",
    ///  difference being in each bit representing an inode in the "Inode Table"
    ///  rather than a block. Since inode numbers start from 1 rather than 0,
    ///  the first bit in the first block group's inode bitmap represent inode
    ///  number 1.
    pub fn is_used(&self, local_index: LocalInodeIndex) -> Option<bool> {
        let index = local_index.0 / 8;
        let byte = self.0.get(index as usize)?;
        let bit = local_index.0 % 8;
        let mask = 1 << bit;
        Some(byte & mask != 0)
    }
}
