use core::fmt;

use bitflags::bitflags;

use crate::BlockAddress;

/// See <https://www.nongnu.org/ext2-doc/ext2.html#inode-table>
#[repr(C, packed)]
#[derive(Debug)]
pub struct Inode {
    pub mode: InodeMode,
    pub uid: u16,
    pub size_low: u32,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub gid: u16,
    pub links_count: u16,
    pub blocks: u32,
    pub flags: u32,
    pub osd1: u32,
    pub direct_blocks: InodeDirectBlocks,
    pub singly_indirect_block: BlockAddress,
    pub doubly_indirect_block: BlockAddress,
    pub triply_indirect_block: BlockAddress,
    pub generation: u32,
    pub file_acl: u32,
    /// High 32 bits of file size. This is dir_acl in revision 0.
    pub size_high: u32,
    pub faddr: u32,
    pub osd2: [u8; 12],
}

impl Inode {
    pub fn is_dir(&self) -> bool {
        let mode = self.mode;
        mode.contains(InodeMode::IFDIR)
    }

    pub fn is_file(&self) -> bool {
        let mode = self.mode;
        mode.contains(InodeMode::IFREG)
    }
}

bitflags! {
    #[derive(Debug, Copy, Clone)]
    #[repr(transparent)]
    /// <https://www.nongnu.org/ext2-doc/ext2.html#i-mode>
    pub struct InodeMode: u16 {
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

#[derive(Clone, Copy)]
pub struct InodeDirectBlocks(pub [BlockAddress; 12]);

impl InodeDirectBlocks {
    pub fn iter(&self) -> InodeDirectBlockIterator {
        InodeDirectBlockIterator {
            direct_blocks: *self,
            index: 0,
        }
    }
}

impl fmt::Debug for InodeDirectBlocks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

pub struct InodeDirectBlockIterator {
    direct_blocks: InodeDirectBlocks,
    index: usize,
}

impl Iterator for InodeDirectBlockIterator {
    type Item = BlockAddress;

    fn next(&mut self) -> Option<Self::Item> {
        let block = self.direct_blocks.0.get(self.index)?;
        self.index += 1;

        if block.0 == 0 {
            None
        } else {
            Some(*block)
        }
    }
}
