//! Code for bitmap allocator, used for physical memory allocation in the
//! kernel.

#![cfg_attr(not(test), no_std)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cargo_common_metadata,
    clippy::doc_markdown,
    clippy::implicit_hasher,
    clippy::implicit_return,
    clippy::len_without_is_empty,
    clippy::missing_const_for_fn,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::multiple_crate_versions,
    clippy::must_use_candidate,
    clippy::new_without_default,
    clippy::redundant_pub_crate,
    clippy::suboptimal_flops,
    clippy::upper_case_acronyms,
    clippy::wildcard_imports
)]

mod alloc;
mod bootstrap;

pub use alloc::*;
pub use bootstrap::*;
