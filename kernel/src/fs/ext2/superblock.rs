use core::fmt;
use core::ops::Add;

use bitflags::bitflags;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

use crate::block::{BlockIndex, BlockSize};

use super::block_group::InodeTableBlockAddress;
use super::inode::{Inode, InodeDirectBlocks};
use super::strings::CStringBytes;

/// See <https://www.nongnu.org/ext2-doc/ext2.html#superblock>
#[repr(C, packed)]
#[derive(Debug, FromZeroes, FromBytes, AsBytes)]
pub(super) struct Superblock {
    pub(super) inodes_count: u32,
    pub(super) blocks_count: u32,
    pub(super) reserved_blocks_count: u32,
    pub(super) free_blocks_count: u32,
    pub(super) free_inodes_count: u32,
    pub(super) first_data_block: BlockAddress,
    pub(super) log_block_size: u32,
    pub(super) log_frag_size: u32,
    pub(super) blocks_per_group: u32,
    pub(super) frags_per_group: u32,
    pub(super) inodes_per_group: u32,
    pub(super) mount_time: u32,
    pub(super) write_time: u32,
    pub(super) mount_count: u16,
    pub(super) max_mount_count: u16,
    pub(super) magic: u16,
    pub(super) state: u16,
    pub(super) errors: u16,
    pub(super) minor_rev_level: u16,
    pub(super) lastcheck: u32,
    pub(super) checkinterval: u32,
    pub(super) creator_os: u32,
    pub(super) rev_level: u32,
    pub(super) def_resuid: u16,
    pub(super) def_resgid: u16,

    // EXT2_DYNAMIC_REV Specific
    pub(super) first_ino: u32,
    pub(super) inode_size: u16,
    pub(super) block_group_nr: u16,
    pub(super) feature_compat: FeatureCompatFlags,
    pub(super) feature_incompat: FeatureIncompatFlags,
    pub(super) feature_ro_compat: FeatureReadOnlyCompatFlags,
    pub(super) uuid: UUID,
    pub(super) volume_name: CStringBytes<[u8; 16]>,
    pub(super) last_mounted: CStringBytes<[u8; 64]>,
    pub(super) algo_bitmap: u32,

    // Performance Hints
    pub(super) prealloc_blocks: u8,
    pub(super) prealloc_dir_blocks: u8,
    pub(super) padding1: u16,

    // Journaling Support
    pub(super) journal_uuid: UUID,
    pub(super) journal_inum: u32,
    pub(super) journal_dev: u32,
    pub(super) last_orphan: u32,

    // Directory Indexing Support
    pub(super) hash_seed: [u32; 4],
    pub(super) def_hash_version: u8,
    pub(super) padding2: [u8; 3],

    // Other options
    pub(super) default_mount_options: u32,
    pub(super) first_meta_bg: u32,
}

impl Superblock {
    /// The superblock is always located at byte offset 1024 from the beginning of
    /// the file, block device or partition formatted with Ext2 and later variants
    /// (Ext3, Ext4).
    pub(super) const SUPERBLOCK_BLOCK_SIZE: BlockSize = BlockSize::new(1024);
    pub(super) const SUPERBLOCK_BLOCK_INDEX: BlockIndex = BlockIndex::new(1);

    /// 16bit value identifying the file system as Ext2. The value is currently
    /// fixed to EXT2_SUPER_MAGIC of value 0xEF53.
    /// <https://www.nongnu.org/ext2-doc/ext2.html#s-magic>
    pub(super) const MAGIC: u16 = 0xEF53;

    pub(super) fn magic_valid(&self) -> bool {
        self.magic == Self::MAGIC
    }

    /// The block size is computed using this 32bit value as the number of bits
    /// to shift left the value 1024. This value may only be non-negative.
    ///
    /// ```text
    /// block size = 1024 << s_log_block_size;
    /// ```
    ///
    /// Common block sizes include 1KiB, 2KiB, 4KiB and 8Kib.
    pub(super) fn block_size(&self) -> BlockSize {
        BlockSize::from(1024 << self.log_block_size)
    }

    /// The block descriptor table is usually right after the superblock, but
    /// the location is `first_data_block + 1`.
    pub(super) fn block_descriptor_table_start_block(&self) -> BlockIndex {
        BlockIndex::from(u64::from(self.first_data_block.0) + 1)
    }

    pub(super) fn num_block_groups(&self) -> usize {
        let num_blocks = self.blocks_count as usize;
        let blocks_per_group = self.blocks_per_group as usize;
        num_blocks.div_ceil(blocks_per_group)
    }

    /// Index for the block group containing the inode.
    pub(super) fn inode_location(
        &self,
        inode_number: InodeNumber,
    ) -> (BlockGroupIndex, LocalInodeIndex) {
        let inode_index = inode_number.0 - 1;
        let block_group_index = BlockGroupIndex(inode_index / self.inodes_per_group);
        let local_inode_index = LocalInodeIndex(inode_index % self.inodes_per_group);
        (block_group_index, local_inode_index)
    }

    /// Convert from local inode index to global inode number.
    pub(super) fn inode_number(
        &self,
        block_group_index: BlockGroupIndex,
        local_inode_index: LocalInodeIndex,
    ) -> InodeNumber {
        let inode_index = block_group_index.0 * self.inodes_per_group + local_inode_index.0;
        InodeNumber(inode_index + 1)
    }

    /// See <https://www.nongnu.org/ext2-doc/ext2.html#inode-table>
    ///
    /// Returns the block containing the inode and the offset of the inode
    /// within that blocks.
    pub(super) fn inode_block_and_offset(
        &self,
        table_address: InodeTableBlockAddress,
        local_inode_index: LocalInodeIndex,
    ) -> (BlockIndex, OffsetBytes) {
        let byte_offset = u64::from(self.inode_size) * u64::from(local_inode_index.0);
        let block_size: u64 = u64::from(u16::from(self.block_size()));
        let block_offset = BlockIndex::from(byte_offset / block_size);
        let block_index = BlockIndex::from(u64::from(table_address.0 .0)) + block_offset;
        let relative_byte_offset = OffsetBytes(byte_offset % block_size);
        (block_index, relative_byte_offset)
    }

    pub(super) fn inode_size(&self, inode: &Inode) -> u64 {
        // In revision 0, we only have 32-bit sizes.
        if self.rev_level == 0 {
            return u64::from(inode.size_low);
        }

        (u64::from(inode.size_high) << 32) | u64::from(inode.size_low)
    }

    pub(super) fn iter_inode_blocks(
        &self,
        inode: &Inode,
    ) -> impl Iterator<Item = (OffsetBytes, BlockAddress)> {
        let block_size = u64::from(u16::from(self.block_size()));

        // Remember, inode.blocks is number of 512 byte blocks, not
        // filesystem-sized blocks. Need to divide by block size to get actual
        // number of blocks.
        let num_blocks = (inode.blocks as usize * 512).div_ceil(block_size as usize);

        InodeBlockIterator {
            direct_blocks: inode.direct_blocks,
            block_size,
            num_blocks,
            seen_blocks: 0,
            index: 0,
        }
    }
}

/// Address of a block in the filesystem.
#[repr(transparent)]
#[derive(Debug, Copy, Clone, FromZeroes, FromBytes, AsBytes, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct BlockAddress(pub(super) u32);

impl Add<u32> for BlockAddress {
    type Output = Self;

    fn add(self, rhs: u32) -> Self::Output {
        Self(self.0 + rhs)
    }
}

/// Address in bytes from the start of the disk.
#[derive(Debug, Copy, Clone)]
pub(super) struct OffsetBytes(pub(super) u64);

impl Add<Self> for OffsetBytes {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

/// How many inodes are in each block group.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub(super) struct InodesPerGroup(pub(super) u32);

/// "Global" inode number within the filesystem.
#[derive(Debug, Copy, Clone, FromZeroes, FromBytes, AsBytes)]
#[repr(transparent)]
pub(super) struct InodeNumber(pub(super) u32);

/// The root directory of the filesystem is always inode 2.
pub(super) const ROOT_DIRECTORY: InodeNumber = InodeNumber(2);

/// A `LocalInodeIndex` is an inode's index within a block group.
#[derive(Debug, Copy, Clone)]
pub(super) struct LocalInodeIndex(pub(super) u32);

/// Index for a given block group.
#[derive(Debug, Copy, Clone)]
pub(super) struct BlockGroupIndex(pub(super) u32);

/// <https://www.nongnu.org/ext2-doc/ext2.html#s-feature-compat>
#[derive(Debug, Copy, Clone, FromZeroes, FromBytes, AsBytes)]
#[repr(transparent)]
pub(super) struct FeatureCompatFlags(u32);

bitflags! {
    impl FeatureCompatFlags: u32 {
        /// Block pre-allocation for new directories
        const DIR_PREALLOC = 0x0001;

        const IMAGIC_INODES = 0x0002;

        /// An Ext3 journal exists
        const HAS_JOURNAL = 0x0004;

        /// Extended inode attributes are present
        const EXT_ATTR = 0x0008;

        /// Non-standard inode size used
        const RESIZE_INODE = 0x0010;

        /// Directory indexing (HTree)
        const DIR_INDEX = 0x0020;
    }
}

#[derive(Debug, Copy, Clone, FromZeroes, FromBytes, AsBytes)]
#[repr(transparent)]
/// <https://www.nongnu.org/ext2-doc/ext2.html#s-feature-incompat>
pub(super) struct FeatureIncompatFlags(u32);

bitflags! {
    impl FeatureIncompatFlags: u32 {
        /// Disk/File compression is used
        const COMPRESSION = 0x0001;
        const FILETYPE = 0x0002;
        const RECOVER = 0x0004;
        const JOURNAL_DEV = 0x0008;
        const META_BG = 0x0010;
    }
}

#[derive(Debug, Copy, Clone, FromZeroes, FromBytes, AsBytes)]
#[repr(transparent)]
/// <https://www.nongnu.org/ext2-doc/ext2.html#s-feature-ro-compat>
pub(super) struct FeatureReadOnlyCompatFlags(u32);

bitflags! {
    impl FeatureReadOnlyCompatFlags: u32 {
        /// Sparse Superblock
        const SPARSE_SUPER = 0x0001;

        /// Filesystem uses a 64bit file size
        const LARGE_FILE = 0x0002;

        /// Binary tree sorted directory files
        const BTREE_DIR = 0x0004;
    }
}

#[derive(Copy, Clone, FromZeroes, FromBytes, AsBytes)]
#[repr(transparent)]
pub(super) struct UUID(pub(super) [u8; 16]);

impl fmt::Debug for UUID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let write_bytes = |f: &mut fmt::Formatter<'_>, start: usize, end: usize| -> fmt::Result {
            for i in start..=end {
                write!(f, "{:02x}", self.0[i])?;
            }
            Ok(())
        };

        write!(f, "UUID(")?;
        write_bytes(f, 0, 3)?;
        write!(f, "-")?;
        write_bytes(f, 4, 5)?;
        write!(f, "-")?;
        write_bytes(f, 6, 7)?;
        write!(f, "-")?;
        write_bytes(f, 8, 9)?;
        write!(f, "-")?;
        write_bytes(f, 10, 15)?;
        write!(f, ")")
    }
}

struct InodeBlockIterator {
    direct_blocks: InodeDirectBlocks,
    block_size: u64,
    num_blocks: usize,
    seen_blocks: usize,
    index: usize,
}

impl Iterator for InodeBlockIterator {
    type Item = (OffsetBytes, BlockAddress);

    fn next(&mut self) -> Option<Self::Item> {
        // Iterate through blocks, skipping blocks that aren't allocated (for
        // e.g. file holes).
        while self.seen_blocks < self.num_blocks {
            if self.index == 12 {
                todo!("support indirect blocks");
            }

            let index = self.index;
            self.index += 1;

            let block = self.direct_blocks.0.get(index)?;

            if block.0 == 0 {
                continue;
            }

            self.seen_blocks += 1;
            let offset = OffsetBytes(index as u64 * self.block_size);
            let address = BlockAddress(block.0);

            return Some((offset, address));
        }
        None
    }
}

#[cfg(feature = "tests")]
mod tests {
    use super::*;

    use crate::tests::kernel_test;
    use crate::transmute::try_cast_bytes_ref;

    #[kernel_test]
    fn test_magic_check() {
        let mut bytes = [0u8; 1024];
        bytes[56] = 0x53;
        bytes[57] = 0xEF;
        let superblock: &Superblock = try_cast_bytes_ref::<Superblock>(&bytes).unwrap();
        assert!(superblock.magic_valid());
    }
}
