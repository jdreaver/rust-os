use super::{BlockAddress, LocalInodeIndex};

/// See <https://www.nongnu.org/ext2-doc/ext2.html#block-group-descriptor-structure>
#[repr(C, packed)]
#[derive(Debug)]
pub(crate) struct BlockGroupDescriptor {
    pub(crate) block_bitmap: BlockAddress,
    pub(crate) inode_bitmap: BlockAddress,
    pub(crate) inode_table: InodeTableBlockAddress,
    pub(crate) free_blocks_count: u16,
    pub(crate) free_inodes_count: u16,
    pub(crate) used_dirs_count: u16,
    _pad: u16,
    _reserved: [u8; 12],
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct InodeTableBlockAddress(pub(crate) BlockAddress);

/// See <https://www.nongnu.org/ext2-doc/ext2.html#block-bitmap>
#[derive(Debug)]
pub(crate) struct BlockBitmap<'a>(pub(crate) &'a [u8]);

impl<'a> BlockBitmap<'a> {
    /// Each bit represent the current state of a block within that block group,
    /// where 1 means "used" and 0 "free/available". The first block of this
    /// block group is represented by bit 0 of byte 0, the second by bit 1 of
    /// byte 0. The 8th block is represented by bit 7 (most significant bit) of
    /// byte 0 while the 9th block is represented by bit 0 (least significant
    /// bit) of byte 1.
    #[allow(dead_code)]
    pub(crate) fn is_used(&self, block: BlockAddress) -> Option<bool> {
        let index = block.0 / 8;
        let byte = self.0.get(index as usize)?;
        let bit = block.0 % 8;
        let mask = 1 << bit;
        Some(byte & mask != 0)
    }
}

/// See <https://www.nongnu.org/ext2-doc/ext2.html#inode-bitmap>
#[derive(Debug)]
pub(crate) struct InodeBitmap<'a>(pub(crate) &'a [u8]);

impl<'a> InodeBitmap<'a> {
    ///  The "Inode Bitmap" works in a similar way as the "Block Bitmap",
    ///  difference being in each bit representing an inode in the "Inode Table"
    ///  rather than a block. Since inode numbers start from 1 rather than 0,
    ///  the first bit in the first block group's inode bitmap represent inode
    ///  number 1.
    pub(crate) fn is_used(&self, local_index: LocalInodeIndex) -> Option<bool> {
        let index = local_index.0 / 8;
        let byte = self.0.get(index as usize)?;
        let bit = local_index.0 % 8;
        let mask = 1 << bit;
        Some(byte & mask != 0)
    }
}
