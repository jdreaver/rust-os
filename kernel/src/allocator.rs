use linked_list_allocator::LockedHeap;
use x86_64::structures::paging::mapper::MapToError;
use x86_64::structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB};
use x86_64::VirtAddr;

/// NOTE: `LockedHeap` uses a spin lock under the hood, so we should ensure we
/// _never_ do allocations in interrupt handlers, because we can cause a
/// deadlock (imagine an interrupt handler fires while the kernel is in the
/// middle of an allocation).
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 100 * 1024; // 100 KiB

/// Maps pages for a kernel heap defined by `HEAP_START` and `HEAP_SIZE` and
/// initializes `ALLOCATOR` with this heap.
pub fn init_heap(
    // N.B. Can't make Mapper generic over page size because we will always need
    // a way to allocate page tables, which are 4 KiB. See
    // https://github.com/rust-osdev/x86_64/issues/390
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE as u64 - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush();
        }
    }

    unsafe {
        // `init() actually writes to the heap, which is why we can only
        // initialize the allocator after we map the pages.
        ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    Ok(())
}
