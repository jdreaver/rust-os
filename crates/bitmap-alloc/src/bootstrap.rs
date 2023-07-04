use crate::BitmapAllocator;

/// A region of memory that is either free or occupied. These are usually given
/// to the kernel from the bootloader, and this type exists to give a layer of
/// indirection between the bootloader memory region type and the allocator.
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    pub start_address: usize,
    pub len_bytes: u64,
    pub free: bool,
}

impl MemoryRegion {
    /// Aligns the region to the given page size. This is useful for ensuring
    /// that the region covers entire pages, which is a requirement for the
    /// allocator.
    ///
    /// Note that free regions that aren't page-aligned are rounded _up_ to the
    /// page size, and non-free regions are rounded _down_ to the page size.
    /// This ensures that non-free regions take precedence over free regions in
    /// a given page.
    ///
    /// Returns `None` if the region is too small to be aligned to the given
    /// page size.
    fn page_aligned_start_end_address(&self, page_size: usize) -> Option<(usize, usize)> {
        if self.free {
            let aligned_start = shift_up_page_size(self.start_address, page_size);

            let end = self.start_address + self.len_bytes as usize;
            let aligned_end = shift_down_page_size(end, page_size);

            if aligned_end <= aligned_start {
                return None;
            }

            Some((aligned_start, aligned_end))
        } else {
            let aligned_start = shift_down_page_size(self.start_address, page_size);

            let end = self.start_address + self.len_bytes as usize;
            let aligned_end = shift_up_page_size(end, page_size);

            Some((aligned_start, aligned_end))
        }
    }
}

fn shift_up_page_size(value: usize, page_size: usize) -> usize {
    let shift = if value % page_size == 0 {
        0
    } else {
        page_size - value % page_size
    };
    value + shift
}

fn shift_down_page_size(value: usize, page_size: usize) -> usize {
    value - value % page_size
}

/// This function bootstraps the allocator. The allocator bitmap needs to be in
/// physical memory. We need to figure out where to put it, we need to make sure
/// it is big enough, and we need to ensure that the pages the bitmap is located
/// in are marked as used in the bitmap.
pub fn bootstrap_allocator<'a, I, R, A>(
    page_size: usize,
    iter_regions: R,
    allocate_bitmap: A,
) -> BitmapAllocator<'a>
where
    I: Iterator<Item = MemoryRegion>,
    R: Fn() -> I,
    A: Fn(usize, usize) -> &'a mut [u64],
{
    // Compute total memory size
    let total_memory = iter_regions()
        .map(|r| r.start_address + r.len_bytes as usize)
        .max()
        .expect("no memory regions found");

    // Find a region where we can place the bitmap
    let bitmap_bits = total_memory.div_ceil(page_size);
    let bitmap_bytes = bitmap_bits.div_ceil(u64::BITS as usize);
    let bitmap_start = iter_regions()
        .filter(|r| r.free)
        .find_map(|region| {
            let Some((start, end)) = region.page_aligned_start_end_address(page_size) else { return None; };

            let fits = bitmap_bytes <= end - start;
            if fits {
                Some(start)
            } else {
                None
            }
        })
        .expect("couldn't find a free region large enough to store the allocator bitmap");

    // Allocate the bitmap and set all regions as used. We default to used and
    // then free unused because memory maps often have holes. For example,
    // 0xa0000 through 0xfffff is used for VGA and BIOS memory, and at the time
    // of writing limine just totally ignores it. Also, limine just doesn't
    // report on the first page of memory.
    let bitmap = allocate_bitmap(bitmap_start, bitmap_bytes);
    bitmap.fill(u64::MAX);

    let mut alloc = BitmapAllocator::new(bitmap);

    // Mark all unused regions as free in the bitmap
    for region in iter_regions() {
        if !region.free {
            continue;
        }
        mark_region(false, &region, page_size, &mut alloc);
    }

    // Mark the bitmap storage area as used
    let bitmap_region = MemoryRegion {
        start_address: bitmap_start,
        len_bytes: bitmap_bytes as u64,
        free: false,
    };
    mark_region(true, &bitmap_region, page_size, &mut alloc);

    alloc
}

fn mark_region(used: bool, region: &MemoryRegion, page_size: usize, alloc: &mut BitmapAllocator) {
    let Some((start_addr, end_addr)) = region.page_aligned_start_end_address(page_size) else { return; };
    let start = start_addr / page_size;
    let end = end_addr / page_size;

    for page in start..end {
        if used {
            alloc.mark_used(page);
        } else {
            alloc.mark_unused(page);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_BITMAP_LEN: usize = 2;
    static mut TEST_BITMAP: [u64; TEST_BITMAP_LEN] = [0_u64; TEST_BITMAP_LEN];

    #[test]
    fn bootstrap() {
        let regions = vec![
            MemoryRegion {
                start_address: 0,
                len_bytes: 0x100,
                free: false,
            },
            MemoryRegion {
                start_address: 0x100,
                len_bytes: 0x100,
                free: true,
            },
            MemoryRegion {
                start_address: 0x200,
                len_bytes: 0x200,
                free: false,
            },
            // N.B. There is a hole here
            MemoryRegion {
                start_address: 0x419,
                len_bytes: 0xd1,
                free: true,
            },
            MemoryRegion {
                start_address: 0x500,
                len_bytes: 0x20,
                free: false,
            },
        ];

        let page_size = 0x10;
        let iter_regions = || regions.iter().cloned();
        let allocate_bitmap = |start, len| -> &mut [u64] {
            assert_eq!(start, 0x100);
            assert_eq!(len, TEST_BITMAP_LEN);
            unsafe { &mut TEST_BITMAP }
        };

        let _ = bootstrap_allocator(page_size, iter_regions, allocate_bitmap);

        // Check that memory regions are properly occupied
        let bitmap_page = 0x10;
        for region in regions.iter() {
            let start_page = region.start_address.div_ceil(page_size);
            let end_page = (region.start_address + region.len_bytes as usize) / page_size;

            for page in start_page..end_page {
                // Bitmap itself takes up space!
                let expect_occupied = !region.free || (page == bitmap_page);
                test_page(page, expect_occupied);
            }
        }

        // Check that the memory hole is used
        test_page(0x390 / page_size, true);
        test_page(0x400 / page_size, true);
        test_page(0x410 / page_size, true);
        test_page(0x420 / page_size, false);
        test_page(0x430 / page_size, false);
    }

    fn test_page(page: usize, expect_occupied: bool) {
        let index = page / BitmapAllocator::BITS_PER_CHUNK;
        let bit = page % BitmapAllocator::BITS_PER_CHUNK;
        let val = unsafe { TEST_BITMAP[index] & (1 << bit) };

        if expect_occupied {
            assert!(val != 0, "page {page} should be occupied");
        } else {
            assert!(val == 0, "page {page} should be free");
        }
    }

    #[test]
    fn page_aligned_start_end_address_free() {
        let region = MemoryRegion {
            start_address: 0x101,
            len_bytes: 0x100,
            free: true,
        };
        let got = region.page_aligned_start_end_address(0x10);
        let expect = Some((0x110, 0x200));
        assert_eq!(got, expect);

        let region = MemoryRegion {
            start_address: 0x100,
            len_bytes: 0x100,
            free: true,
        };
        let got = region.page_aligned_start_end_address(0x10);
        let expect = Some((0x100, 0x200));
        assert_eq!(got, expect);
    }

    #[test]
    fn page_aligned_start_end_address_free_too_small() {
        let region = MemoryRegion {
            start_address: 0x101,
            len_bytes: 0x10,
            free: true,
        };
        let got = region.page_aligned_start_end_address(0x10);
        assert_eq!(got, None);

        let region = MemoryRegion {
            start_address: 0x101,
            len_bytes: 0xf,
            free: true,
        };
        let got = region.page_aligned_start_end_address(0x10);
        assert_eq!(got, None);

        let region = MemoryRegion {
            start_address: 0x101,
            len_bytes: 0xe,
            free: true,
        };
        let got = region.page_aligned_start_end_address(0x10);
        assert_eq!(got, None);

        let region = MemoryRegion {
            start_address: 0x101,
            len_bytes: 0x1f,
            free: true,
        };
        let got = region.page_aligned_start_end_address(0x10);
        let expect = Some((0x110, 0x120));
        assert_eq!(got, expect);
    }
}
