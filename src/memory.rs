use x86_64::structures::paging::{
    FrameAllocator, OffsetPageTable, PageSize, PageTable, PhysFrame, Size4KiB,
};
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

impl UsableMemoryRegion {
    fn available_pages_in_region(&self, page_size: u64) -> usize {
        (self.len / page_size) as usize
    }

    fn page_start_addr(&self, page: usize, page_size: u64) -> PhysAddr {
        self.start_address + (page as u64 * page_size)
    }
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

    /// How many pages we are within the current region.
    current_page_within_region: usize,
}

impl NaiveFreeMemoryBlockAllocator {
    /// # Safety
    ///
    /// This function is unsafe because the caller must guarantee that the all
    /// frames that are passed to this function must be unused.
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
            current_page_within_region: 0,
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for NaiveFreeMemoryBlockAllocator {
    // TODO: This logic is hairy. This should be unit tested.
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        // TODO: Any way to ensure we use the Size4KiB type parameter to this
        // trait?
        let page_size: u64 = Size4KiB::SIZE;

        // Find the next free page
        loop {
            if self.current_memory_region >= self.num_memory_regions {
                // We have run out of memory regions, so we can't allocate any more
                // frames.
                return None;
            }

            let current_region = self.usable_memory_regions[self.current_memory_region];
            let available_pages = current_region.available_pages_in_region(page_size);
            if available_pages <= self.current_page_within_region {
                self.current_memory_region += 1;
                self.current_page_within_region = 0;
            } else {
                break;
            }
        }

        // Construct the next page
        let memory_region = self.usable_memory_regions[self.current_memory_region];
        let current_page =
            memory_region.page_start_addr(self.current_page_within_region, page_size);
        let frame: PhysFrame<Size4KiB> = PhysFrame::containing_address(current_page);
        self.current_page_within_region += 1;

        Some(frame)
    }
}
