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
    pub first_data_block: u32,
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
    pub feature_compat: u32,
    pub feature_incompat: u32,
    pub feature_ro_compat: u32,
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
    pub fn block_size(&self) -> usize {
        1024 << self.log_block_size as usize
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
