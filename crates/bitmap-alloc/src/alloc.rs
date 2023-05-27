pub struct BitmapAllocator<'a> {
    /// The bitmap used to track allocations. Each bit represents a page of
    /// memory, and a 1 indicates that the page is in use.
    ///
    /// In the real kernel, this bitmap is itself stored in main memory, so
    /// there is a bit of a bootstrapping problem that needs to be solved.
    bitmap: &'a mut [u8], // TODO: Use u64, or even u128 if that is faster
}

impl<'a> BitmapAllocator<'a> {
    pub(crate) const BITS_PER_CHUNK: usize = u8::BITS as usize;

    pub(crate) fn new(bitmap: &'a mut [u8]) -> BitmapAllocator<'a> {
        BitmapAllocator { bitmap }
    }

    /// Mark a given page as used. This is used internally by the allocator, but
    /// is also used by the kernel to mark reserved pages as used when
    /// initializing the allocator.
    pub(crate) fn mark_used(&mut self, page: usize) {
        let index = page / Self::BITS_PER_CHUNK;
        let bit = page % Self::BITS_PER_CHUNK;
        // N.B. If this assertion is removed for performance reasons, make sure
        // to add it to our tests, or conditionally compile it for tests.
        assert!(
            self.bitmap[index] & (1 << bit) == 0,
            "page {page} is already used"
        );
        self.bitmap[index] |= 1 << bit;
    }

    fn mark_unused(&mut self, page: usize) {
        let index = page / Self::BITS_PER_CHUNK;
        let bit = page % Self::BITS_PER_CHUNK;
        // N.B. If this assertion is removed for performance reasons, make sure
        // to add it to our tests, or conditionally compile it for tests.
        assert!(
            self.bitmap[index] & (1 << bit) != 0,
            "page {page} is already unused"
        );
        self.bitmap[index] &= !(1 << bit);
    }

    /// Allocates a contiguous block of memory of the given size, and returns
    /// the starting page of the block. Returns `None` if no such block exists.
    pub fn allocate_contiguous(&mut self, num_pages: usize) -> Option<usize> {
        // TODO: Store the last allocation location, and start searching from
        // there. This will make allocations faster. Also consider resetting it
        // to the free start location whenever we do a free.

        assert!(num_pages > 0, "cannot allocate 0 pages");

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

            // (TODO: buggy) Shortcut: if the byte is all 1's, then we know that
            // all bits are set, so we can skip this byte.
            // if *byte == Self::BITS_MAX {
            //     start += Self::BITS_PER_CHUNK;
            //     current_len = 0;
            //     continue;
            // }

            // TODO: Use a combination of shifts and `leading_zeroes` (or
            // `trailing_zeros` if we want least significant bit iteration) to
            // find chunks of zeroes.
            for bit in 0..Self::BITS_PER_CHUNK {
                let bit_free = *byte & (1 << bit) == 0;
                if bit_free {
                    current_len += 1;
                    if current_len == num_pages {
                        return Some(start);
                    }
                } else {
                    // Set start to the next bit, since we know that the current
                    // bit is not free.
                    start = i * Self::BITS_PER_CHUNK + bit + 1;
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;

    use proptest::prelude::*;

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

    #[derive(Debug, Clone)]
    enum AllocOrFree {
        Alloc(usize),
        Free(usize),
    }

    fn alloc_or_free_strategy(max_alloc: usize) -> impl Strategy<Value = AllocOrFree> {
        prop_oneof![
            (1..max_alloc).prop_map(AllocOrFree::Alloc),
            prop::num::usize::ANY.prop_map(AllocOrFree::Free),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            // Bump up the number of test cases to make sure we get a good
            // distribution of tests. Default is 256. See:
            // https://docs.rs/proptest/latest/proptest/test_runner/struct.Config.html#structfield.cases
            cases: 10_000, .. ProptestConfig::default()
        })]

        #[test]
        fn alloc_free(
            bitmap_elems in 2..20_usize,
            initial_allocs in prop::collection::vec(1..20_usize, 5..15),
            allocs in prop::collection::vec(alloc_or_free_strategy(80), 1..20)
        ) {
            let mut bitmap = vec![0; bitmap_elems];
            let mut allocator = BitmapAllocator::new(&mut bitmap);

            // N.B. BTreeSet gives us consistent iteration order, which is
            // important for determinism. We get randomness from the index we
            // use for `Free`.
            let mut allocated_pages = BTreeSet::new();

            // Initial round of allocations to fill things up.
            for num_pages in initial_allocs {
                let start = allocator.allocate_contiguous(num_pages);
                if let Some(start) = start {
                    allocated_pages.insert((start, num_pages));
                }
            }

            for alloc_or_free in allocs {
                match alloc_or_free {
                    AllocOrFree::Alloc(num_pages) => {
                        // N.B. This test relies on an assertion being present
                        // in the allocator that will fail if we try to allocate
                        // a page that is already allocated.
                        let start = allocator.allocate_contiguous(num_pages);
                        if let Some(start) = start {
                            allocated_pages.insert((start, num_pages));
                        }
                    },
                    AllocOrFree::Free(raw_idx) => {
                        if allocated_pages.is_empty() {
                            continue;
                        }

                        // N.B. This test relies on an assertion being present
                        // in the allocator that will fail if a page is freed
                        // but not allocated.
                        let alloc = allocated_pages.iter().nth(raw_idx % allocated_pages.len()).cloned();
                        if let Some(alloc@(start, num_pages)) = alloc {
                            allocator.free_contiguous(start, num_pages);
                            allocated_pages.remove(&alloc);
                        }
                    },
                }
            }

            // Deallocate everything that has been allocated and ensure all
            // pages are freed.
            for (start, num_pages) in allocated_pages {
                allocator.free_contiguous(start, num_pages);
            }

            assert!(allocator.bitmap.iter().all(|&byte| byte == 0));
        }
    }
}
