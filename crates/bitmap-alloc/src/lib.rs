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

pub struct BitmapAllocator<'a> {
    /// The bitmap used to track allocations. Each bit represents a page of
    /// memory, and a 1 indicates that the page is in use.
    ///
    /// In the real kernel, this bitmap is itself stored in main memory, so
    /// there is a bit of a bootstrapping problem that needs to be solved.
    bitmap: &'a mut [u8], // TODO: Use u64, or even u128 if that is faster
}

impl<'a> BitmapAllocator<'a> {
    pub fn new(bitmap: &'a mut [u8]) -> BitmapAllocator<'a> {
        BitmapAllocator { bitmap }
    }

    /// Mark a given page as used. This is used internally by the allocator, but
    /// is also used by the kernel to mark reserved pages as used when
    /// initializing the allocator.
    pub fn mark_used(&mut self, page: usize) {
        let index = page / 8;
        let bit = page % 8;
        self.bitmap[index] |= 1 << bit;
    }

    fn mark_unused(&mut self, page: usize) {
        let index = page / 8;
        let bit = page % 8;
        self.bitmap[index] &= !(1 << bit);
    }

    /// Allocates a contiguous block of memory of the given size, and returns
    /// the starting page of the block. Returns `None` if no such block exists.
    pub fn allocate_contiguous(&mut self, num_pages: usize) -> Option<usize> {
        // TODO: Store the last allocation location, and start searching from
        // there. This will make allocations faster. Also consider resetting it
        // to the free start location whenever we do a free.

        let start = self.find_next_contiguous(num_pages)?;
        for page in start..start + num_pages {
            self.mark_used(page);
        }
        Some(start)
    }

    /// Finds the starting page of a contiguous block of memory of the given
    /// size, and returns the starting page of the block. Returns `None` if no
    /// such block exists.
    fn find_next_contiguous(&mut self, num_pages: usize) -> Option<usize> {
        let mut start = 0;
        let mut current_len = 0;
        for (i, byte) in self.bitmap.iter().enumerate() {
            // TODO: Maybe use the `count_zeros` intrinsic to check if we need
            // to skip the byte.

            // Shortcut: if the byte is 0xFF, then we know that all bits are
            // set, so we can skip this byte.
            if *byte == 0xFF {
                start += 8;
                continue;
            }

            // TODO: Use a combination of shifts and `leading_zeroes` (or
            // `trailing_zeros` if we want least significant bit iteration) to
            // find chunks of zeroes.
            for bit in 0..8 {
                let bit_free = *byte & (1 << bit) == 0;
                if bit_free {
                    current_len += 1;
                    if current_len == num_pages {
                        return Some(start);
                    }
                } else {
                    start = i * 8 + bit + 1;
                    current_len = 0;
                }
            }
        }

        None
    }

    /// Frees a contiguous block of memory of the given size, starting at the
    /// given page.
    pub fn free_contiguous(&mut self, start_page: usize, num_pages: usize) {
        for page in start_page..start_page + num_pages {
            self.mark_unused(page);
        }
    }
}

// TODO: Implement some bootstrapping code the kernel can use to initialize the
// allocator. We need to traverse memory regions to find the total amount of
// memory, find a suitable location for the free memory allocator, run a
// function that actually allocates the bitmap array at that location (this can
// be a passed in function so we can do something easy during tests), create the
// allocator, and then mark all the reserved pages as used.

// /// A region of memory that is either free or occupied. These are usually given
// /// to the kernel from the bootloader, and this type exists to give a layer of
// /// indirection between the bootloader memory region type and the allocator.
// #[derive(Debug, Clone)]
// pub struct MemoryRegion {
//     pub start_address: usize,
//     pub len_bytes: u64,
//     pub free: bool,
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_test() {
        let mut bitmap = [0; 2];
        let mut allocator = BitmapAllocator::new(&mut bitmap);

        let start = allocator.allocate_contiguous(1);
        assert_eq!(start, Some(0));

        let start = allocator.allocate_contiguous(100);
        assert_eq!(start, None);

        allocator.mark_used(3);
        let start = allocator.allocate_contiguous(5);
        assert_eq!(start, Some(4));

        assert_eq!(allocator.bitmap, [0b11111001, 0b00000001]);

        let start = allocator.allocate_contiguous(3);
        assert_eq!(start, Some(9));
        assert_eq!(allocator.bitmap, [0b11111001, 0b00001111]);

        allocator.free_contiguous(4, 5);
        assert_eq!(allocator.bitmap, [0b00001001, 0b00001110]);
    }
}
