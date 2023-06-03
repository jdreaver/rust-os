use core::ops::{Add, Mul};

use bitflags::bitflags;

use crate::strings::CStringBytes;

/// See <https://www.nongnu.org/ext2-doc/ext2.html#superblock>
#[repr(C, packed)]
#[derive(Debug)]
pub struct Superblock {
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub reserved_blocks_count: u32,
    pub free_blocks_count: u32,
    pub free_inodes_count: u32,
    pub first_data_block: BlockAddress,
    pub log_block_size: u32,
    pub log_frag_size: u32,
    pub blocks_per_group: u32,
    pub frags_per_group: u32,
    pub inodes_per_group: u32,
    pub mount_time: u32,
    pub write_time: u32,
    pub mount_count: u16,
    pub max_mount_count: u16,
    pub magic: u16,
    pub state: u16,
    pub errors: u16,
    pub minor_rev_level: u16,
    pub lastcheck: u32,
    pub checkinterval: u32,
    pub creator_os: u32,
    pub rev_level: u32,
    pub def_resuid: u16,
    pub def_resgid: u16,

    // EXT2_DYNAMIC_REV Specific
    pub first_ino: u32,
    pub inode_size: u16,
    pub block_group_nr: u16,
    pub feature_compat: FeatureCompatFlags,
    pub feature_incompat: FeatureIncompatFlags,
    pub feature_ro_compat: FeatureReadOnlyCompatFlags,
    pub uuid: [u8; 16],
    pub volume_name: CStringBytes<16>,
    pub last_mounted: CStringBytes<64>,
    pub algo_bitmap: u32,

    // Performance Hints
    pub prealloc_blocks: u8,
    pub prealloc_dir_blocks: u8,
    pub padding1: u16,

    // Journaling Support
    pub journal_uuid: [u8; 16],
    pub journal_inum: u32,
    pub journal_dev: u32,
    pub last_orphan: u32,

    // Directory Indexing Support
    pub hash_seed: [u32; 4],
    pub def_hash_version: u8,
    pub padding2: [u8; 3],

    // Other options
    pub default_mount_options: u32,
    pub first_meta_bg: u32,
}

impl Superblock {
    /// The superblock is always located at byte offset 1024 from the beginning of
    /// the file, block device or partition formatted with Ext2 and later variants
    /// (Ext3, Ext4).
    pub const OFFSET_BYTES: usize = 1024;

    /// 16bit value identifying the file system as Ext2. The value is currently
    /// fixed to EXT2_SUPER_MAGIC of value 0xEF53.
    /// <https://www.nongnu.org/ext2-doc/ext2.html#s-magic>
    pub const MAGIC: u16 = 0xEF53;

    pub fn magic_valid(&self) -> bool {
        self.magic == Self::MAGIC
    }

    /// The block size is computed using this 32bit value as the number of bits to shift left the value 1024. This value may only be non-negative.
    ///
    /// ```text
    /// block size = 1024 << s_log_block_size;
    /// ```
    ///
    /// Common block sizes include 1KiB, 2KiB, 4KiB and 8Kib.
    pub fn block_size(&self) -> BlockSize {
        BlockSize(1024 << self.log_block_size)
    }

    pub fn block_descriptor_table_offset(&self) -> OffsetBytes {
        (self.first_data_block + 1) * self.block_size()
    }

    pub fn block_descriptor_offset(&self, block_group: u64) -> OffsetBytes {
        self.block_descriptor_table_offset() + OffsetBytes(block_group * 32)
    }

    pub fn num_block_groups(&self) -> usize {
        let num_blocks = self.blocks_count as usize;
        let blocks_per_group = self.blocks_per_group as usize;
        num_blocks.div_ceil(blocks_per_group)
    }
}

/// Address of a block in the filesystem.
#[repr(transparent)]
#[derive(Debug, Copy, Clone)]
pub struct BlockAddress(pub u32);

impl Add<u32> for BlockAddress {
    type Output = Self;

    fn add(self, rhs: u32) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl Mul<BlockSize> for BlockAddress {
    type Output = OffsetBytes;

    fn mul(self, rhs: BlockSize) -> Self::Output {
        OffsetBytes(u64::from(self.0) * u64::from(rhs.0))
    }
}

/// Size of a block in bytes.
#[derive(Debug, Copy, Clone)]
pub struct BlockSize(pub u32);

/// Address in bytes from the start of the disk.
#[derive(Debug, Copy, Clone)]
pub struct OffsetBytes(pub u64);

impl Add<Self> for OffsetBytes {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

bitflags! {
    #[derive(Debug, Copy, Clone)]
    #[repr(transparent)]
    /// <https://www.nongnu.org/ext2-doc/ext2.html#s-feature-compat>
    pub struct FeatureCompatFlags: u32 {
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
    pub struct FeatureIncompatFlags: u32 {
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
    pub struct FeatureReadOnlyCompatFlags: u32 {
        /// Sparse Superblock
        const SPARSE_SUPER = 0x0001;

        /// Filesystem uses a 64bit file size
        const LARGE_FILE = 0x0002;

        /// Binary tree sorted directory files
        const BTREE_DIR = 0x0004;
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
