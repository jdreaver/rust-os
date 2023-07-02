use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;

use linked_list_allocator::Heap;
use x86_64::VirtAddr;

use crate::memory::with_kernel_page_table_lock;
use crate::sync::SpinLock;

use super::mapping::{
    allocate_and_map_pages, KERNEL_HEAP_REGION_MAX_SIZE, KERNEL_HEAP_REGION_START,
};
use super::page::{Page, PageRange, PageSize};
use super::page_table::{MapError, PageTableEntryFlags};

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap(SpinLock::new(Heap::empty()));

/// Wrapper around `linked_list_allocator::Heap` that implements `GlobalAlloc`.
/// The `LockedHeap` in that crate doesn't understand interrupts, so we wrap it
/// in our own `SpinLock`.
struct LockedHeap(SpinLock<Heap>);

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0
            .lock_disable_interrupts()
            .allocate_first_fit(layout)
            .ok()
            .map_or(core::ptr::null_mut::<u8>(), |allocation| {
                allocation.as_ptr()
            })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.0
            .lock_disable_interrupts()
            .deallocate(NonNull::new_unchecked(ptr), layout);
    }
}

const HEAP_START: usize = KERNEL_HEAP_REGION_START as usize;
const HEAP_SIZE: usize = 10 * 1024 * 1024; // 10 MiB

/// Maps pages for a kernel heap defined by `HEAP_START` and `HEAP_SIZE` and
/// initializes `ALLOCATOR` with this heap.
pub(super) fn init() -> Result<(), MapError> {
    assert!(HEAP_SIZE < KERNEL_HEAP_REGION_MAX_SIZE as usize);

    let heap_start_addr = VirtAddr::new(HEAP_START as u64);
    let heap_start = Page::containing_address(heap_start_addr, PageSize::Size4KiB);
    let page_range = PageRange::from_num_bytes(heap_start, HEAP_SIZE);
    let flags = PageTableEntryFlags::PRESENT | PageTableEntryFlags::WRITABLE;

    with_kernel_page_table_lock(|table| allocate_and_map_pages(table, page_range.iter(), flags))?;

    unsafe {
        // `init() actually writes to the heap, which is why we can only
        // initialize the allocator after we map the pages.
        ALLOCATOR
            .0
            .lock_disable_interrupts()
            .init(HEAP_START, HEAP_SIZE);
    }

    Ok(())
}
