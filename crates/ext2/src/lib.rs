//! Pure code for interacting with ext2 filesystems. Used in our kernel.
//!
//! # Resources
//!
//! - <https://wiki.osdev.org/Ext2>
//! - <https://www.nongnu.org/ext2-doc/ext2.html>
//! - <https://en.wikipedia.org/wiki/Ext2>
//! - <https://git.kernel.org/pub/scm/utils/util-linux/util-linux.git/tree/libblkid/src/superblocks/ext.c>

#![cfg_attr(not(test), no_std)]
#![feature(int_roundings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cargo_common_metadata,
    clippy::doc_markdown,
    clippy::implicit_hasher,
    clippy::implicit_return,
    clippy::missing_const_for_fn,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::multiple_crate_versions,
    clippy::must_use_candidate,
    clippy::new_without_default,
    clippy::redundant_pub_crate,
    clippy::suboptimal_flops,
    clippy::wildcard_imports
)]

pub mod block_group;
pub mod directory;
pub mod inode;
mod strings;
pub mod superblock;

pub use block_group::*;
pub use directory::*;
pub use inode::*;
pub use superblock::*;
