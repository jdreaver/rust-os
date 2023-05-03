use x86_64::structures::paging::{FrameAllocator, OffsetPageTable, PageSize, PageTable, PhysFrame};
use x86_64::{PhysAddr, VirtAddr};

/// Initialize a new `OffsetPageTable`.
///
/// # Safety
///
/// This function is unsafe because the caller must guarantee that the complete
/// physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once to
/// avoid aliasing `&mut` references (which is undefined behavior).
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = active_level_4_page_table(physical_memory_offset);
    OffsetPageTable::new(level_4_table, physical_memory_offset)
}

/// Returns a mutable reference to the active level 4 table.
///
/// This function is unsafe because the caller must guarantee that the complete
/// physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once to
/// avoid aliasing `&mut` references (which is undefined behavior).
unsafe fn active_level_4_page_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    let (level_4_table_frame, _) = x86_64::registers::control::Cr3::read();

    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}

/// A region of memory we can use for allocation. These are usually given to the
/// kernel from the bootloader, and this type exists to give a layer of
/// indirection between the bootloader memory region type and the kernel.
#[derive(Debug, Clone, Copy)]
pub struct UsableMemoryRegion {
    pub start_address: PhysAddr,
    pub len: u64,
}

const MAX_USABLE_MEMORY_REGIONS: usize = 16;

/// A simple allocator that allocates memory by simply keeping track of the next
/// free region of memory and increasing a pointer past it.
///
/// This is a very simple allocator, and it's not very efficient. It's also very
/// easy to implement. Notably, it doesn't support freeing memory.
///
/// Use `from_iter` to instantiate this type from memory regions.
#[derive(Debug)]
pub struct NaiveFreeMemoryBlockAllocator {
    /// The list of memory regions that we can use for allocations. This would
    /// be a `Vec<UsageMemoryRegion>`, but we can't use `Vec` without an
    /// allocator, so we use a fixed-size array instead.
    usable_memory_regions: [UsableMemoryRegion; MAX_USABLE_MEMORY_REGIONS],

    /// The actual number of memory regions we received. Used to iterate over
    /// `usable_memory_regions`.
    num_memory_regions: usize,

    /// Index into `usable_memory_regions` of the next memory region to use for
    /// allocation.
    current_memory_region: usize,

    /// Where we are within the current memory region.
    region_offset_bytes: u64,
}

impl NaiveFreeMemoryBlockAllocator {
    /// # Safety
    ///
    /// This function is unsafe because the caller must guarantee that the all
    /// regions passed to this function must be unused.
    ///
    /// N.B. We can't implement this using `FromIterator` because we can't
    /// implement the `from_iter` method using `unsafe`.
    pub unsafe fn from_iter<T: IntoIterator<Item = UsableMemoryRegion>>(iter: T) -> Self {
        let mut usable_memory_regions = [UsableMemoryRegion {
            start_address: PhysAddr::new(0),
            len: 0,
        }; MAX_USABLE_MEMORY_REGIONS];

        let mut num_memory_regions = 0;
        for (i, region) in iter.into_iter().enumerate() {
            assert!(
                i < MAX_USABLE_MEMORY_REGIONS,
                "too many usable memory regions passed to the kernel, max is {MAX_USABLE_MEMORY_REGIONS}"
            );
            usable_memory_regions[i] = region;
            num_memory_regions += 1;
        }

        Self {
            usable_memory_regions,
            num_memory_regions,
            current_memory_region: 0,
            region_offset_bytes: 0,
        }
    }
}

unsafe impl<S: PageSize> FrameAllocator<S> for NaiveFreeMemoryBlockAllocator {
    // TODO: This logic is hairy. This should be unit tested.
    fn allocate_frame(&mut self) -> Option<PhysFrame<S>> {
        let page_size: u64 = S::SIZE;

        // Find the next memory region with enough space for our page.
        loop {
            if self.current_memory_region >= self.num_memory_regions {
                // We have run out of memory regions, so we can't allocate any more
                // frames.
                return None;
            }

            // Construct the next page
            let memory_region = self.usable_memory_regions[self.current_memory_region];
            let frame_address = memory_region.start_address + self.region_offset_bytes;
            // Pages must be aligned to their size.
            //
            // NOTE: This align_up call can waste a ton of space. For example,
            // if you just previously allocated a 4 KiB page that was on a 2 MiB
            // boundary, but now you are allocating a new 2 MiB page, align_up
            // will consume 2 MiB - 4 KiB of wasted space. This is a naive
            // allocator so that is okay for now.
            let frame_address = frame_address.align_up(page_size);

            // Make sure we have enough space in the current region. If not, go to the next one.
            if frame_address - memory_region.start_address >= memory_region.len {
                self.current_memory_region += 1;
                self.region_offset_bytes = 0;
            } else {
                let frame: PhysFrame<S> = PhysFrame::from_start_address(frame_address)
                    .expect("frame address is invalid, even though we just aligned it!");
                self.region_offset_bytes += page_size;
                return Some(frame);
            }
        }
    }
}
