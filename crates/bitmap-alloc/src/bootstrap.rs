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
    let bitmap_bytes = total_memory.div_ceil(page_size);
    let bitmap_start = iter_regions()
        .filter(|r| r.free)
        .find_map(|region| {
            let start = region.start_address;

            // Start address must be page-aligned
            assert!(
                start % page_size == 0,
                "regions start address must be page-aligned"
            );

            let fits = region.len_bytes as usize >= bitmap_bytes - (start - region.start_address);
            if fits {
                Some(start)
            } else {
                None
            }
        })
        .expect("couldn't find a free region large enough to store the allocator bitmap");

    // Allocate the bitmap
    let bitmap_len = bitmap_bytes.div_ceil(u64::BITS as usize);
    let bitmap = allocate_bitmap(bitmap_start, bitmap_len);

    // Mark all used regions as used in the bitmap
    let mut alloc = BitmapAllocator::new(bitmap);
    for region in iter_regions() {
        // Ensure all regions are page-aligned, or else the logic here is much more complicated.
        assert!(
            region.start_address % page_size == 0,
            "regions start address must be page-aligned"
        );

        if region.free {
            continue;
        }

        let start = region.start_address / page_size;
        let end = (region.start_address + region.len_bytes as usize).div_ceil(page_size);
        for page in start..end {
            alloc.mark_used(page);
        }
    }

    alloc
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
            MemoryRegion {
                start_address: 0x400,
                len_bytes: 0x100,
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
        let bitmap_start_page = 0x1010 / page_size;
        let bitmap_end_page = (0x1010 + TEST_BITMAP_LEN - 1) / page_size;
        for region in regions.iter() {
            let start_page = region.start_address / page_size;
            let end_page = (region.start_address + region.len_bytes as usize) / page_size;

            for page in start_page..end_page {
                // Bitmap itself takes up space!
                let expect_occupied =
                    !region.free || (page >= bitmap_start_page && page <= bitmap_end_page);
                let index = page / BitmapAllocator::BITS_PER_CHUNK;
                let bit = page % BitmapAllocator::BITS_PER_CHUNK;
                let val = unsafe { TEST_BITMAP[index] & (1 << bit) };

                if expect_occupied {
                    assert!(val != 0, "page {page} should be occupied");
                } else {
                    assert!(val == 0, "page {page} should be free");
                }
            }
        }
    }
}
