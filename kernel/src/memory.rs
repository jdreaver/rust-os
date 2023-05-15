use core::alloc::{AllocError, Allocator, Layout, LayoutError};
use core::ptr::NonNull;

use spin::Mutex;
use x86_64::structures::paging::mapper::{MapToError, Translate};
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageSize, PageTable, PageTableFlags, PhysFrame,
    Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

/// Page table mapper used by all kernel contexts.
static KERNEL_MAPPER: Mutex<Option<OffsetPageTable<'static>>> = Mutex::new(None);

/// Physical memory frame allocator used by all kernel contexts.
///
/// TODO: Change name to `KERNEL_PHYSICAL_ALLOCATOR`?
pub(crate) static KERNEL_ALLOCATOR: LockedNaiveFreeMemoryBlockAllocator =
    LockedNaiveFreeMemoryBlockAllocator {
        mutex: Mutex::new(None),
    };

/// Initialize the `KERNEL_MAPPER` with the passed `physical_memory_offset`.
///
/// # Safety
///
/// This function is unsafe because the caller must guarantee that the complete
/// physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once to
/// avoid aliasing `&mut` references (which is undefined behavior).
pub(crate) unsafe fn init(
    physical_memory_offset: VirtAddr,
    usable_memory_regions: impl Iterator<Item = UsableMemoryRegion>,
) {
    let (level_4_table_frame, _) = x86_64::registers::control::Cr3::read();

    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    let level_4_table = &mut *page_table_ptr;

    KERNEL_MAPPER
        .lock()
        .replace(OffsetPageTable::new(level_4_table, physical_memory_offset));

    let allocator = NaiveFreeMemoryBlockAllocator::from_iter(usable_memory_regions);
    KERNEL_ALLOCATOR.mutex.lock().replace(allocator);
}

/// Translate a given physical address to a virtual address, if possible.
pub(crate) fn translate_addr(addr: VirtAddr) -> Option<PhysAddr> {
    KERNEL_MAPPER
        .lock()
        .as_ref()
        .expect("kernel mapper not initialized")
        .translate_addr(addr)
}

/// A region of memory we can use for allocation. These are usually given to the
/// kernel from the bootloader, and this type exists to give a layer of
/// indirection between the bootloader memory region type and the kernel.
#[derive(Debug, Clone, Copy)]
pub(crate) struct UsableMemoryRegion {
    pub(crate) start_address: PhysAddr,
    pub(crate) len: u64,
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
pub(crate) struct NaiveFreeMemoryBlockAllocator {
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
    pub(crate) unsafe fn from_iter<T: IntoIterator<Item = UsableMemoryRegion>>(iter: T) -> Self {
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

    /// Allocates a series of frames contiguous in physical memory. Returns the
    /// start address of the allocated memory.
    fn allocate_contiguous_memory(
        &mut self,
        num_bytes: u64,
        alignment: Option<u64>,
    ) -> Option<PhysAddr> {
        // TODO: This logic is hairy. This should be unit tested.

        // Find the next memory region with enough space for our page.
        loop {
            if self.current_memory_region >= self.num_memory_regions {
                // We have run out of memory regions, so we can't allocate any more
                // frames.
                return None;
            }

            // Construct figure out if we have enough space in the current region.
            let memory_region = self.usable_memory_regions[self.current_memory_region];

            let start_address = memory_region.start_address + self.region_offset_bytes;
            // If we have an alignment requirement (e.g. pages must be aligned
            // to their size) we need to apply it.
            //
            // NOTE: This align_up call can waste a ton of space with this naive
            // memory allocation scheme. For example, if you just previously
            // allocated a 4 KiB page that was on a 2 MiB boundary, but now you
            // are allocating a new 2 MiB page, align_up will consume 2 MiB - 4
            // KiB of wasted space. This is a naive allocator so that is okay
            // for now.
            let start_address =
                alignment.map_or(start_address, |alignment| start_address.align_up(alignment));

            if start_address - memory_region.start_address >= memory_region.len {
                self.current_memory_region += 1;
                self.region_offset_bytes = 0;
            } else {
                self.region_offset_bytes += num_bytes;
                return Some(start_address);
            }
        }
    }
}

unsafe impl<S: PageSize> FrameAllocator<S> for NaiveFreeMemoryBlockAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<S>> {
        let page_size: u64 = S::SIZE;
        let frame_address = self.allocate_contiguous_memory(page_size, Some(page_size))?;
        let frame: PhysFrame<S> = PhysFrame::containing_address(frame_address);
        Some(frame)
    }
}

/// Allocates a physical frame of memory for the given size.
pub(crate) fn allocate_frame<S: PageSize>() -> Option<PhysFrame<S>> {
    KERNEL_ALLOCATOR
        .mutex
        .lock()
        .as_mut()
        .expect("kernel memory allocator not initialized")
        .allocate_frame()
}

/// Allocates a physical page for the given virtual page of memory and maps the
/// virtual page to the physical page.
pub(crate) fn allocate_and_map_page(
    page: Page,
    flags: PageTableFlags,
) -> Result<(), MapToError<Size4KiB>> {
    let frame = allocate_frame::<Size4KiB>().ok_or(MapToError::FrameAllocationFailed)?;
    let mut alloc_guard = KERNEL_ALLOCATOR.mutex.lock();
    let allocator = alloc_guard
        .as_mut()
        .expect("kernel memory allocator not initialized");
    let mut mapper_guard = KERNEL_MAPPER.lock();
    let mapper = mapper_guard
        .as_mut()
        .expect("kernel mapper not initialized");
    unsafe {
        mapper.map_to(page, frame, flags, allocator)?.flush();
        Ok(())
    }
}

pub(crate) fn identity_map_frame(
    frame: PhysFrame,
    flags: PageTableFlags,
) -> Result<(), MapToError<Size4KiB>> {
    let mut alloc_guard = KERNEL_ALLOCATOR.mutex.lock();
    let allocator = alloc_guard
        .as_mut()
        .expect("kernel memory allocator not initialized");
    let mut mapper_guard = KERNEL_MAPPER.lock();
    let mapper = mapper_guard
        .as_mut()
        .expect("kernel mapper not initialized");
    let map_result = unsafe { mapper.identity_map(frame, flags, allocator) };
    match map_result {
        Ok(flusher) => {
            flusher.flush();
            Ok(())
        }
        // These errors are okay. They just mean the frame is already identity
        // mapped (well, hopefully).
        Err(MapToError::ParentEntryHugePage | MapToError::PageAlreadyMapped(_)) => Ok(()),
        Err(e) => Err(e),
    }
}

/// `NaiveFreeMemoryBlockAllocator` behind a `Mutex`
pub(crate) struct LockedNaiveFreeMemoryBlockAllocator {
    mutex: Mutex<Option<NaiveFreeMemoryBlockAllocator>>,
}

/// We implement the `Allocator` trait for `LockedNaiveFreeMemoryBlockAllocator`
/// so that we can use it for custom allocations for physically contiguous
/// memory.
unsafe impl Allocator for LockedNaiveFreeMemoryBlockAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let size = layout.size() as u64;
        let alignment = layout.align() as u64;
        let start_address = {
            let mut mutex_guard = self.mutex.lock();
            let allocator = mutex_guard
                .as_mut()
                .expect("kernel memory allocator not initialized");
            allocator
                .allocate_contiguous_memory(size, Some(alignment))
                .ok_or(AllocError)?
        };

        let slice = unsafe {
            core::slice::from_raw_parts_mut(start_address.as_u64() as *mut u8, layout.size())
        };

        let ptr: NonNull<[u8]> = unsafe { NonNull::new_unchecked(slice) };

        Ok(ptr)
    }

    unsafe fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {
        // NaiveFreeMemoryBlockAllocator doesn't support deallocation.
    }
}

unsafe impl<S: PageSize> FrameAllocator<S> for LockedNaiveFreeMemoryBlockAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<S>> {
        self.mutex
            .lock()
            .as_mut()
            .expect("kernel memory allocator not initialized")
            .allocate_frame()
    }
}

/// Error type used in `allocate_zeroed_buffer`.
#[derive(Debug, Clone)]
pub(crate) enum AllocZeroedBufferError {
    LayoutError(LayoutError),
    AllocError(AllocError),
}

/// Useful utility wrapper around `Allocator.allocate_zeroed`.
pub(crate) fn allocate_zeroed_buffer(
    size: usize,
    alignment: usize,
) -> Result<u64, AllocZeroedBufferError> {
    let layout =
        Layout::from_size_align(size, alignment).map_err(AllocZeroedBufferError::LayoutError)?;
    let address = KERNEL_ALLOCATOR
        .allocate_zeroed(layout)
        .map_err(AllocZeroedBufferError::AllocError)?;
    Ok(address.addr().get() as u64)
}
