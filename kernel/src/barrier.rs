//! Functions for dealing with memory barriers/fences. These are needed
//! particularly during memory-mapped device IO to ensure stores and loads are
//! performed in the correct order, and the CPU is not allowed to reorder them.
//! See:
//!
//! - <https://www.kernel.org/doc/Documentation/memory-barriers.txt>
//! - <https://en.wikipedia.org/wiki/Memory_barrier>

use core::arch::x86_64;

/// A memory barrier that prevents the CPU from reordering loads and stores
/// across it.
///
/// This is a full memory barrier, and on x86_64 is implemented using the
/// `mfence` instruction.
#[inline]
pub(crate) fn barrier() {
    unsafe { x86_64::_mm_mfence() }
}

/// A memory barrier that prevents the CPU from reordering loads across it.
///
/// This is a read memory barrier, and on x86_64 is implemented using the
/// `lfence` instruction.
#[inline]
// Starts with underscore to avoid dead code warning.
pub(crate) fn _read_barrier() {
    unsafe { x86_64::_mm_lfence() }
}

/// A memory barrier that prevents the CPU from reordering stores across it.
///
/// This is a write memory barrier, and on x86_64 is implemented using the
/// `sfence` instruction.
#[inline]
// Starts with underscore to avoid dead code warning.
pub(crate) fn _write_barrier() {
    unsafe { x86_64::_mm_sfence() }
}
