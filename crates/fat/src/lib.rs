//! Pure code for interacting with FAT filesystems. Used in our kernel.
//!
//! # Resources
//!
//! - <https://en.wikipedia.org/wiki/Design_of_the_FAT_file_system>
//! - <https://academy.cba.mit.edu/classes/networking_communications/SD/FAT.pdf>
//! - <https://wiki.osdev.org/FAT>

#![cfg_attr(not(test), no_std)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cargo_common_metadata,
    clippy::implicit_hasher,
    clippy::implicit_return,
    clippy::missing_const_for_fn,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::multiple_crate_versions,
    clippy::must_use_candidate,
    clippy::new_without_default,
    clippy::suboptimal_flops,
    clippy::wildcard_imports
)]

mod boot;
mod io;

pub use boot::*;
pub use io::*;
