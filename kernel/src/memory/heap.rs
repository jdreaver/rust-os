use linked_list_allocator::LockedHeap;
use x86_64::structures::paging::mapper::MapToError;
use x86_64::structures::paging::{Page, PageTableFlags, Size4KiB};
use x86_64::VirtAddr;

use super::mapping::{KERNEL_HEAP_REGION_MAX_SIZE, KERNEL_HEAP_REGION_START};
use super::virt::allocate_and_map_pages;

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
pub(super) fn init() -> Result<(), MapToError<Size4KiB>> {
    assert!(HEAP_SIZE < KERNEL_HEAP_REGION_MAX_SIZE as usize);

    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE as u64 - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    allocate_and_map_pages(page_range, flags)?;

    unsafe {
        // `init() actually writes to the heap, which is why we can only
        // initialize the allocator after we map the pages.
        ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    Ok(())
}
