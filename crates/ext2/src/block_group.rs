use crate::BlockAddress;

/// See <https://www.nongnu.org/ext2-doc/ext2.html#block-group-descriptor-structure>
#[repr(C, packed)]
#[derive(Debug)]
pub struct BlockGroupDescriptor {
    pub block_bitmap: BlockAddress,
    pub inode_bitmap: BlockAddress,
    pub inode_table: BlockAddress,
    pub free_blocks_count: u16,
    pub free_inodes_count: u16,
    pub used_dirs_count: u16,
    _pad: u16,
    _reserved: [u8; 12],
}
