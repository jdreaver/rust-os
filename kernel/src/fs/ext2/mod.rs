//! Code for interacting with ext2 filesystems.
//!
//! # Resources
//!
//! - <https://wiki.osdev.org/Ext2>
//! - <https://www.nongnu.org/ext2-doc/ext2.html>
//! - <https://en.wikipedia.org/wiki/Ext2>
//! - <https://git.kernel.org/pub/scm/utils/util-linux/util-linux.git/tree/libblkid/src/superblocks/ext.c>
//! - "CHAPTER 18: The Ext2 and Ext3 Filesystems" in "Understanding the Linux Kernel - Bovet (3rd ed, 2005)"

mod block_group;
mod directory;
mod file_system;
mod inode;
mod strings;
mod superblock;
mod vfs;

pub(crate) use vfs::*;
