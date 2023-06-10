use bitflags::bitflags;

use super::superblock::BlockAddress;

/// See <https://www.nongnu.org/ext2-doc/ext2.html#inode-table>
#[repr(C, packed)]
#[derive(Debug, Clone)]
pub(super) struct Inode {
    pub(super) mode: InodeMode,
    pub(super) uid: u16,
    pub(super) size_low: u32,
    pub(super) atime: u32,
    pub(super) ctime: u32,
    pub(super) mtime: u32,
    pub(super) dtime: u32,
    pub(super) gid: u16,
    pub(super) links_count: u16,
    pub(super) blocks: u32,
    pub(super) flags: u32,
    pub(super) osd1: u32,
    pub(super) direct_blocks: InodeDirectBlocks,
    pub(super) singly_indirect_block: BlockAddress,
    pub(super) doubly_indirect_block: BlockAddress,
    pub(super) triply_indirect_block: BlockAddress,
    pub(super) generation: u32,
    pub(super) file_acl: u32,
    /// High 32 bits of file size. This is dir_acl in revision 0.
    pub(super) size_high: u32,
    pub(super) faddr: u32,
    pub(super) osd2: [u8; 12],
}

impl Inode {
    pub(super) fn is_dir(&self) -> bool {
        let mode = self.mode;
        mode.contains(InodeMode::IFDIR)
    }

    pub(super) fn is_file(&self) -> bool {
        let mode = self.mode;
        mode.contains(InodeMode::IFREG)
    }
}

bitflags! {
    #[derive(Debug, Copy, Clone)]
    #[repr(transparent)]
    /// <https://www.nongnu.org/ext2-doc/ext2.html#i-mode>
    pub(super) struct InodeMode: u16 {
        // Access rights

        /// Others execute
        const IXOTH = 0x001;

        /// Others write
        const IWOTH = 0x002;

        /// Others read
        const IROTH = 0x004;

        /// Group execute
        const IXGRP = 0x008;

        /// Group write
        const IWGRP = 0x010;

        /// Group read
        const IRGRP = 0x020;

        /// User execute
        const IXUSR = 0x040;

        /// User write
        const IWUSR = 0x080;

        /// User read
        const IRUSR = 0x100;

        // Process execution user/group override

        /// Sticky bit
        const ISVTX = 0x200;

        /// Set process group id
        const ISGID = 0x400;

        /// Set process user id
        const ISUID = 0x800;

        // File format

        /// FIFO
        const IFIFO = 0x1000;

        /// Character device
        const IFCHR = 0x2000;

        /// Directory
        const IFDIR = 0x4000;

        /// Block device
        const IFBLK = 0x6000;

        /// Regular file
        const IFREG = 0x8000;

        /// Symbolic link
        const IFLNK = 0xA000;

        /// Socket
        const IFSOCK = 0xC000;
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct InodeDirectBlocks(pub(super) [BlockAddress; 12]);

impl InodeDirectBlocks {
    pub(super) fn empty() -> Self {
        Self([BlockAddress(0); 12])
    }

    pub(super) fn insert(&mut self, index: usize, block: BlockAddress) {
        assert!(index < self.0.len(), "index {index} out of bounds");
        self.0[index] = block;
    }
}
