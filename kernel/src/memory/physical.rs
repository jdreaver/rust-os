use core::alloc::AllocError;

use x86_64::PhysAddr;

use bitmap_alloc::{bootstrap_allocator, BitmapAllocator, MemoryRegion};

use crate::sync::SpinLock;

use super::address::KernPhysAddr;
use super::page::{Page, PageRange};

/// Physical memory frame allocator used by all kernel contexts.
pub(super) static KERNEL_PHYSICAL_ALLOCATOR: LockedPhysicalMemoryAllocator =
    LockedPhysicalMemoryAllocator::new();

pub(super) unsafe fn init<I, R>(usable_memory_regions: R)
where
    I: Iterator<Item = MemoryRegion>,
    R: Fn() -> I,
{
    KERNEL_PHYSICAL_ALLOCATOR.init(usable_memory_regions);
}

/// Simply wraps `PhysicalMemoryAllocator` in a lock. This exists because some
/// `x86_64` functions want a `&mut Allocator` and we can't have multiple
/// mutable references to the same object.
pub(crate) struct LockedPhysicalMemoryAllocator<'a> {
    lock: SpinLock<Option<PhysicalMemoryAllocator<'a>>>,
}

impl LockedPhysicalMemoryAllocator<'_> {
    const fn new() -> Self {
        Self {
            lock: SpinLock::new(None),
        }
    }

    unsafe fn init<I, R>(&self, memory_regions: R)
    where
        I: Iterator<Item = MemoryRegion>,
        R: Fn() -> I,
    {
        let allocator = PhysicalMemoryAllocator::new(memory_regions);
        self.lock.lock().replace(allocator);
    }

    pub(super) fn with_lock<R>(&self, f: impl FnOnce(&mut PhysicalMemoryAllocator) -> R) -> R {
        let mut lock_guard = self.lock.lock();
        let allocator = lock_guard
            .as_mut()
            .expect("kernel memory allocator not initialized");
        f(allocator)
    }
}

// I'm not sure this is 100% correct, so I'm not doing it. In particular, I
// worry that deallocate is incorrect because I'm not sure what the
// characteristics of Layout are. It is better to be explicit that the physical
// memory allocator deals with pages. If we want contiguous heap-like
// allocation, we should implement a heap on top of physically contiguous
// memory, or do something like slab allocation on top of physically contiguous
// memory.
//
//
// /// We implement the `Allocator` trait for `PhysicalMemoryAllocator`
// /// so that we can use it for custom allocations for physically contiguous
// /// memory.
// unsafe impl Allocator for LockedPhysicalMemoryAllocator<'_> {
//     fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
//         let size = layout.size();
//         let num_pages = size.div_ceil(PhysicalMemoryAllocator::PAGE_SIZE);

//         let alignment = layout.align() as u64;
//         assert!(
//             alignment <= PhysicalMemoryAllocator::PAGE_SIZE as u64,
//             "alignment {alignment} must be <= page size {}. What the hell are we aligning???",
//             PhysicalMemoryAllocator::PAGE_SIZE,
//         );
//         let start_page = self.with_lock(|allocator| {
//             allocator
//                 .allocator
//                 .allocate_contiguous(num_pages)
//                 .ok_or(AllocError)
//         })?;
//         let start_address = start_page * PhysicalMemoryAllocator::PAGE_SIZE;
//         let actual_size = num_pages * PhysicalMemoryAllocator::PAGE_SIZE;
//         let ptr = unsafe { nonnull_ptr_slice_from_addr_len(start_address, actual_size) };

//         Ok(ptr)
//     }

//     unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
//         let size = layout.size();
//         let num_pages = size.div_ceil(PhysicalMemoryAllocator::PAGE_SIZE);
//         let start_addr = ptr.as_ptr() as usize;
//         assert!(
//             start_addr % PhysicalMemoryAllocator::PAGE_SIZE == 0,
//             "somehow start address of {start_addr} is not page aligned"
//         );
//         let start_page = start_addr / PhysicalMemoryAllocator::PAGE_SIZE;
//         self.with_lock(|allocator| {
//             allocator.allocator.free_contiguous(start_page, num_pages);
//         });
//     }
// }

// unsafe fn nonnull_ptr_slice_from_addr_len(addr: usize, len_bytes: usize) -> NonNull<[u8]> {
//     let ptr = addr as *mut u8;
//     NonNull::new_unchecked(core::slice::from_raw_parts_mut(ptr, len_bytes))
// }

/// Wrapper around `BitmapAllocator` that knows how to deal with the kernel.
pub(super) struct PhysicalMemoryAllocator<'a> {
    pub(super) allocator: BitmapAllocator<'a>,
}

pub(crate) const PAGE_SIZE: usize = 4096; // 4 KiB

impl PhysicalMemoryAllocator<'_> {
    unsafe fn new<I, R>(memory_regions: R) -> Self
    where
        I: Iterator<Item = MemoryRegion>,
        R: Fn() -> I,
    {
        let allocator =
            bootstrap_allocator(PAGE_SIZE, memory_regions, |bitmap_addr, bitmap_len| {
                // Make sure to use a kernel physical address pointer
                let phys_addr = PhysAddr::new(bitmap_addr as u64);
                let kern_phys_addr = KernPhysAddr::from_phys_addr(phys_addr);
                let ptr = kern_phys_addr.as_mut_ptr::<u64>();
                core::slice::from_raw_parts_mut(ptr, bitmap_len)
            });
        Self { allocator }
    }
}

impl PhysicalMemoryAllocator<'_> {
    pub(super) fn allocate_zeroed_page(&mut self) -> Result<Page<KernPhysAddr>, AllocError> {
        let pages = self.allocate_zeroed_pages(1)?;
        let mut pages = pages.iter();
        let page = pages.next().expect("somehow we got less than one page!");
        assert!(pages.next().is_none(), "somehow we got more than one page!");
        Ok(page)
    }

    pub(super) fn allocate_zeroed_pages(
        &mut self,
        num_pages: usize,
    ) -> Result<PageRange<KernPhysAddr>, AllocError> {
        let page = self
            .allocator
            .allocate_contiguous(num_pages)
            .ok_or(AllocError)?;

        assert!(page > 0, "we allocated the zero page, which shouldn't happen since the first page should be reserved");

        let phys_addr = PhysAddr::new((page * PAGE_SIZE) as u64);
        let start_addr = KernPhysAddr::from(phys_addr);
        let page_slice = unsafe {
            core::slice::from_raw_parts_mut(start_addr.as_mut_ptr::<u8>(), num_pages * PAGE_SIZE)
        };
        page_slice.fill(0);

        let end_addr = start_addr + (num_pages * PAGE_SIZE);
        Ok(PageRange::exclusive(start_addr, end_addr))
    }

    pub(super) fn free_pages(&mut self, pages: &PageRange<KernPhysAddr>) {
        let start_addr = PhysAddr::from(pages.start_addr());
        let start_page = start_addr.as_u64() as usize / pages.page_size().size_bytes();
        self.allocator
            .free_contiguous(start_page, pages.num_pages());
    }
}

/// Physically contiguous buffer of memory. Allocates by page, so it can
/// allocate more memory than requested. Useful for e.g. Direct Memory Access
/// (DMA) like with VirtIO buffers.
///
/// NOTE: This type implements `Drop` and will free the allocated memory when
/// it goes out of scope.
#[derive(Debug)]
pub(crate) struct PhysicalBuffer {
    pages: PageRange<KernPhysAddr>,
}

impl PhysicalBuffer {
    pub(crate) fn allocate_zeroed_pages(num_pages: usize) -> Result<Self, AllocError> {
        let pages = KERNEL_PHYSICAL_ALLOCATOR
            .with_lock(|allocator| allocator.allocate_zeroed_pages(num_pages))?;
        Ok(Self { pages })
    }

    pub(crate) fn allocate_zeroed(min_bytes: usize) -> Result<Self, AllocError> {
        let num_pages = min_bytes.div_ceil(PAGE_SIZE);
        Self::allocate_zeroed_pages(num_pages)
    }

    pub(crate) fn as_slice_mut(&mut self) -> &mut [u8] {
        let ptr = self.address().as_mut_ptr::<u8>();
        let len_bytes = self.pages.num_pages() * self.pages.page_size().size_bytes();
        unsafe { core::slice::from_raw_parts_mut(ptr, len_bytes) }
    }

    pub(crate) fn address(&self) -> KernPhysAddr {
        self.pages.start_addr()
    }
}

impl Drop for PhysicalBuffer {
    fn drop(&mut self) {
        KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
            allocator.free_pages(&self.pages);
        });
    }
}
