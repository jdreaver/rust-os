use core::fmt;
use core::ops::Add;

use bitflags::bitflags;

use crate::block::{BlockIndex, BlockSize};

use super::block_group::InodeTableBlockAddress;
use super::strings::CStringBytes;

/// See <https://www.nongnu.org/ext2-doc/ext2.html#superblock>
#[repr(C, packed)]
#[derive(Debug)]
pub(crate) struct Superblock {
    pub(crate) inodes_count: u32,
    pub(crate) blocks_count: u32,
    pub(crate) reserved_blocks_count: u32,
    pub(crate) free_blocks_count: u32,
    pub(crate) free_inodes_count: u32,
    pub(crate) first_data_block: BlockAddress,
    pub(crate) log_block_size: u32,
    pub(crate) log_frag_size: u32,
    pub(crate) blocks_per_group: u32,
    pub(crate) frags_per_group: u32,
    pub(crate) inodes_per_group: u32,
    pub(crate) mount_time: u32,
    pub(crate) write_time: u32,
    pub(crate) mount_count: u16,
    pub(crate) max_mount_count: u16,
    pub(crate) magic: u16,
    pub(crate) state: u16,
    pub(crate) errors: u16,
    pub(crate) minor_rev_level: u16,
    pub(crate) lastcheck: u32,
    pub(crate) checkinterval: u32,
    pub(crate) creator_os: u32,
    pub(crate) rev_level: u32,
    pub(crate) def_resuid: u16,
    pub(crate) def_resgid: u16,

    // EXT2_DYNAMIC_REV Specific
    pub(crate) first_ino: u32,
    pub(crate) inode_size: u16,
    pub(crate) block_group_nr: u16,
    pub(crate) feature_compat: FeatureCompatFlags,
    pub(crate) feature_incompat: FeatureIncompatFlags,
    pub(crate) feature_ro_compat: FeatureReadOnlyCompatFlags,
    pub(crate) uuid: UUID,
    pub(crate) volume_name: CStringBytes<16>,
    pub(crate) last_mounted: CStringBytes<64>,
    pub(crate) algo_bitmap: u32,

    // Performance Hints
    pub(crate) prealloc_blocks: u8,
    pub(crate) prealloc_dir_blocks: u8,
    pub(crate) padding1: u16,

    // Journaling Support
    pub(crate) journal_uuid: UUID,
    pub(crate) journal_inum: u32,
    pub(crate) journal_dev: u32,
    pub(crate) last_orphan: u32,

    // Directory Indexing Support
    pub(crate) hash_seed: [u32; 4],
    pub(crate) def_hash_version: u8,
    pub(crate) padding2: [u8; 3],

    // Other options
    pub(crate) default_mount_options: u32,
    pub(crate) first_meta_bg: u32,
}

impl Superblock {
    /// The superblock is always located at byte offset 1024 from the beginning of
    /// the file, block device or partition formatted with Ext2 and later variants
    /// (Ext3, Ext4).
    pub(crate) const SUPERBLOCK_BLOCK_SIZE: BlockSize = BlockSize::new(1024);
    pub(crate) const SUPERBLOCK_BLOCK_INDEX: BlockIndex = BlockIndex::new(1);

    /// 16bit value identifying the file system as Ext2. The value is currently
    /// fixed to EXT2_SUPER_MAGIC of value 0xEF53.
    /// <https://www.nongnu.org/ext2-doc/ext2.html#s-magic>
    pub(crate) const MAGIC: u16 = 0xEF53;

    pub(crate) fn magic_valid(&self) -> bool {
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
    pub(crate) fn block_size(&self) -> BlockSize {
        BlockSize::from(1024 << self.log_block_size)
    }

    /// The block descriptor table is usually right after the superblock, but
    /// the location is `first_data_block + 1`.
    pub(crate) fn block_descriptor_table_start_block(&self) -> BlockIndex {
        BlockIndex::from(u64::from(self.first_data_block.0) + 1)
    }

    pub(crate) fn num_block_groups(&self) -> usize {
        let num_blocks = self.blocks_count as usize;
        let blocks_per_group = self.blocks_per_group as usize;
        num_blocks.div_ceil(blocks_per_group)
    }

    /// Index for the block group containing the inode.
    pub(crate) fn inode_location(
        &self,
        inode_number: InodeNumber,
    ) -> (BlockGroupIndex, LocalInodeIndex) {
        let inode_index = inode_number.0 - 1;
        let block_group_index = BlockGroupIndex(inode_index / self.inodes_per_group);
        let local_inode_index = LocalInodeIndex(inode_index % self.inodes_per_group);
        (block_group_index, local_inode_index)
    }

    /// Convert from local inode index to global inode number.
    pub(crate) fn inode_number(
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
    pub(crate) fn inode_block_and_offset(
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
}

/// Address of a block in the filesystem.
#[repr(transparent)]
#[derive(Debug, Copy, Clone)]
pub(crate) struct BlockAddress(pub(crate) u32);

impl Add<u32> for BlockAddress {
    type Output = Self;

    fn add(self, rhs: u32) -> Self::Output {
        Self(self.0 + rhs)
    }
}

/// Address in bytes from the start of the disk.
#[derive(Debug, Copy, Clone)]
pub(crate) struct OffsetBytes(pub(crate) u64);

impl Add<Self> for OffsetBytes {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

/// How many inodes are in each block group.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub(crate) struct InodesPerGroup(pub(crate) u32);

/// "Global" inode number within the filesystem.
#[derive(Debug, Copy, Clone)]
pub(crate) struct InodeNumber(pub(crate) u32);

/// The root directory of the filesystem is always inode 2.
pub(crate) const ROOT_DIRECTORY: InodeNumber = InodeNumber(2);

/// A `LocalInodeIndex` is an inode's index within a block group.
#[derive(Debug, Copy, Clone)]
pub(crate) struct LocalInodeIndex(pub(crate) u32);

/// Index for a given block group.
#[derive(Debug, Copy, Clone)]
pub(crate) struct BlockGroupIndex(pub(crate) u32);

bitflags! {
    #[derive(Debug, Copy, Clone)]
    #[repr(transparent)]
    /// <https://www.nongnu.org/ext2-doc/ext2.html#s-feature-compat>
    pub(crate) struct FeatureCompatFlags: u32 {
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

bitflags! {
    #[derive(Debug, Copy, Clone)]
    #[repr(transparent)]
    /// <https://www.nongnu.org/ext2-doc/ext2.html#s-feature-incompat>
    pub(crate) struct FeatureIncompatFlags: u32 {
        /// Disk/File compression is used
        const COMPRESSION = 0x0001;
        const FILETYPE = 0x0002;
        const RECOVER = 0x0004;
        const JOURNAL_DEV = 0x0008;
        const META_BG = 0x0010;
    }
}

bitflags! {
    #[derive(Debug, Copy, Clone)]
    #[repr(transparent)]
    /// <https://www.nongnu.org/ext2-doc/ext2.html#s-feature-ro-compat>
    pub(crate) struct FeatureReadOnlyCompatFlags: u32 {
        /// Sparse Superblock
        const SPARSE_SUPER = 0x0001;

        /// Filesystem uses a 64bit file size
        const LARGE_FILE = 0x0002;

        /// Binary tree sorted directory files
        const BTREE_DIR = 0x0004;
    }
}

#[derive(Copy, Clone)]
#[repr(transparent)]
pub(crate) struct UUID(pub(crate) [u8; 16]);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_check() {
        let mut bytes = [0u8; 1024];
        bytes[56] = 0x53;
        bytes[57] = 0xEF;
        let superblock: Superblock = unsafe { bytes.as_ptr().cast::<Superblock>().read() };
        assert!(superblock.magic_valid());
    }
}
