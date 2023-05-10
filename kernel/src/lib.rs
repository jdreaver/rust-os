#![no_std]
#![feature(abi_x86_interrupt)]
#![feature(allocator_api)]
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
    clippy::suboptimal_flops,
    clippy::upper_case_acronyms,
    clippy::wildcard_imports
)]

extern crate alloc;

pub mod acpi;
pub mod boot_info;
pub mod gdt;
pub mod heap;
pub mod interrupts;
pub mod memory;
pub mod pci;
pub mod registers;
pub mod serial;
pub mod strings;
pub mod virtio;

// So we can use the `paste!` macro in our macros.
pub extern crate paste;
