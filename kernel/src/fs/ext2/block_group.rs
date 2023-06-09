use super::superblock::{BlockAddress, LocalInodeIndex};

/// See <https://www.nongnu.org/ext2-doc/ext2.html#block-group-descriptor-structure>
#[repr(C, packed)]
#[derive(Debug)]
pub(super) struct BlockGroupDescriptor {
    pub(super) block_bitmap: BlockAddress,
    pub(super) inode_bitmap: BlockAddress,
    pub(super) inode_table: InodeTableBlockAddress,
    pub(super) free_blocks_count: u16,
    pub(super) free_inodes_count: u16,
    pub(super) used_dirs_count: u16,
    _pad: u16,
    _reserved: [u8; 12],
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub(super) struct InodeTableBlockAddress(pub(super) BlockAddress);

/// See <https://www.nongnu.org/ext2-doc/ext2.html#block-bitmap>
///
/// Each bit represent the current state of a block within that block group,
/// where 1 means "used" and 0 "free/available". The first block of this block
/// group is represented by bit 0 of byte 0, the second by bit 1 of byte 0. The
/// 8th block is represented by bit 7 (most significant bit) of byte 0 while the
/// 9th block is represented by bit 0 (least significant bit) of byte 1.
#[derive(Debug)]
pub(super) struct BlockBitmap<'a>(Bitmap<'a>);

impl<'a> BlockBitmap<'a> {
    pub(super) fn new(bytes: &'a mut [u8]) -> Self {
        Self(Bitmap(bytes))
    }

    pub(super) fn reserve_next_free(&mut self) -> Option<BlockAddress> {
        let index = self.0.reserve_next_free()?;
        Some(BlockAddress(index as u32))
    }
}

/// See <https://www.nongnu.org/ext2-doc/ext2.html#inode-bitmap>
///
///  The "Inode Bitmap" works in a similar way as the "Block Bitmap", difference
///  being in each bit representing an inode in the "Inode Table" rather than a
///  block. Since inode numbers start from 1 rather than 0, the first bit in the
///  first block group's inode bitmap represent inode number 1.
#[derive(Debug)]
pub(super) struct InodeBitmap<'a>(Bitmap<'a>);

impl<'a> InodeBitmap<'a> {
    pub(super) fn new(bytes: &'a mut [u8]) -> Self {
        Self(Bitmap(bytes))
    }

    pub(super) fn is_used(&self, local_index: LocalInodeIndex) -> Option<bool> {
        self.0.is_used(local_index.0 as usize)
    }

    pub(super) fn reserve_next_free(&mut self) -> Option<LocalInodeIndex> {
        let index = self.0.reserve_next_free()?;
        Some(LocalInodeIndex(index as u32))
    }
}

#[derive(Debug)]
struct Bitmap<'a>(pub(super) &'a mut [u8]);

impl<'a> Bitmap<'a> {
    pub(super) fn is_used(&self, index: usize) -> Option<bool> {
        let byte = self.0.get(index / 8)?;
        let bit = index % 8;
        let mask = 1 << bit;
        Some(byte & mask != 0)
    }

    /// Finds the next open entry in the bitmap, and returns its index. Returns
    /// `None` if there are no more remaining entries.
    pub(super) fn reserve_next_free(&mut self) -> Option<usize> {
        for (index, byte) in self.0.iter_mut().enumerate() {
            if *byte == 0xFF {
                continue;
            }
            for bit in 0..8 {
                let mask = 1 << bit;
                if *byte & mask == 0 {
                    *byte |= mask;
                    return Some(index * 8 + bit);
                }
            }
        }
        None
    }
}
