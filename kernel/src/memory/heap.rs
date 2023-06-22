use linked_list_allocator::LockedHeap;
use x86_64::VirtAddr;

use super::mapping::{
    allocate_and_map_pages, KERNEL_HEAP_REGION_MAX_SIZE, KERNEL_HEAP_REGION_START,
};
use super::page::PageRange;
use super::page_table::{MapError, PageTableEntryFlags};

/// NOTE: `LockedHeap` uses a spin lock under the hood, so we should ensure we
/// _never_ do allocations in interrupt handlers, because we can cause a
/// deadlock (imagine an interrupt handler fires while the kernel is in the
/// middle of an allocation).
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

const HEAP_START: usize = KERNEL_HEAP_REGION_START as usize;
const HEAP_SIZE: usize = 10 * 1024 * 1024; // 10 MiB

/// Maps pages for a kernel heap defined by `HEAP_START` and `HEAP_SIZE` and
/// initializes `ALLOCATOR` with this heap.
pub(super) fn init() -> Result<(), MapError> {
    assert!(HEAP_SIZE < KERNEL_HEAP_REGION_MAX_SIZE as usize);

    let heap_start = VirtAddr::new(HEAP_START as u64);
    let heap_end = heap_start + HEAP_SIZE as u64;
    let page_range = PageRange::exclusive(heap_start, heap_end);
    let flags = PageTableEntryFlags::PRESENT | PageTableEntryFlags::WRITABLE;
    allocate_and_map_pages(page_range.iter(), flags)?;

    unsafe {
        // `init() actually writes to the heap, which is why we can only
        // initialize the allocator after we map the pages.
        ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    Ok(())
}
