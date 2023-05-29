use core::alloc::AllocError;
use core::ptr;

use spin::Mutex;
use x86_64::structures::paging::mapper::{MapToError, Translate};
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageSize, PageTable, PageTableFlags, PhysFrame,
    Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

use bitmap_alloc::{bootstrap_allocator, BitmapAllocator, MemoryRegion};

/// Page table mapper used by all kernel contexts.
static KERNEL_MAPPER: KernelMapper = KernelMapper::new();

/// Physical memory frame allocator used by all kernel contexts.
static KERNEL_PHYSICAL_ALLOCATOR: LockedPhysicalMemoryAllocator =
    LockedPhysicalMemoryAllocator::new();

/// Initialize the `KERNEL_MAPPER` with the passed `physical_memory_offset`.
///
/// # Safety
///
/// This function is unsafe because the caller must guarantee that the complete
/// physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once to
/// avoid aliasing `&mut` references (which is undefined behavior).
pub(crate) unsafe fn init<I, R>(physical_memory_offset: VirtAddr, usable_memory_regions: R)
where
    I: Iterator<Item = MemoryRegion>,
    R: Fn() -> I,
{
    KERNEL_MAPPER.init(physical_memory_offset);
    KERNEL_PHYSICAL_ALLOCATOR.init(usable_memory_regions);
}

/// Mutex wrapper around `OffsetPageTable`.
struct KernelMapper {
    mutex: Mutex<Option<OffsetPageTable<'static>>>,
}

impl KernelMapper {
    const fn new() -> Self {
        Self {
            mutex: Mutex::new(None),
        }
    }

    unsafe fn init(&self, physical_memory_offset: VirtAddr) {
        let (level_4_table_frame, _) = x86_64::registers::control::Cr3::read();

        let phys = level_4_table_frame.start_address();
        let virt = physical_memory_offset + phys.as_u64();
        let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

        let level_4_table = unsafe { &mut *page_table_ptr };
        let mapper = unsafe { OffsetPageTable::new(level_4_table, physical_memory_offset) };
        self.mutex.lock().replace(mapper);
    }

    fn with_lock<R>(&self, f: impl FnOnce(&mut OffsetPageTable) -> R) -> R {
        let mut mutex_guard = self.mutex.lock();
        let mapper = mutex_guard
            .as_mut()
            .expect("kernel memory mapper not initialized");
        f(mapper)
    }
}

/// Translate a given physical address to a virtual address, if possible.
pub(crate) fn translate_addr(addr: VirtAddr) -> Option<PhysAddr> {
    KERNEL_MAPPER.with_lock(|mapper| mapper.translate_addr(addr))
}

/// Allocates a physical frame of memory for the given size.
pub(crate) fn allocate_physical_frame<S: PageSize>() -> Option<PhysFrame<S>> {
    KERNEL_PHYSICAL_ALLOCATOR
        .mutex
        .lock()
        .as_mut()
        .expect("kernel memory allocator not initialized")
        .allocate_frame()
}

/// Allocates a physical frame for the given virtual page of memory and maps the
/// virtual page to the physical frame in the page table. Useful for
/// initializing a virtual region that is known not to be backed by memory, like
/// initializing the kernel heap.
pub(crate) fn allocate_and_map_page(
    page: Page,
    flags: PageTableFlags,
) -> Result<(), MapToError<Size4KiB>> {
    let frame = allocate_physical_frame::<Size4KiB>().ok_or(MapToError::FrameAllocationFailed)?;
    KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
        KERNEL_MAPPER.with_lock(|mapper| unsafe {
            mapper.map_to(page, frame, flags, allocator)?.flush();
            Ok(())
        })
    })
}

pub(crate) fn identity_map_physical_frame(
    frame: PhysFrame,
    flags: PageTableFlags,
) -> Result<(), MapToError<Size4KiB>> {
    KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
        KERNEL_MAPPER.with_lock(|mapper| {
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
        })
    })
}

/// Wrapper around `BitmapAllocator` that knows how to deal with the kernel.
struct PhysicalMemoryAllocator<'a> {
    allocator: BitmapAllocator<'a>,
}

pub(crate) const PAGE_SIZE: usize = Size4KiB::SIZE as usize;

impl PhysicalMemoryAllocator<'_> {
    unsafe fn new<I, R>(memory_regions: R) -> Self
    where
        I: Iterator<Item = MemoryRegion>,
        R: Fn() -> I,
    {
        let allocator =
            bootstrap_allocator(PAGE_SIZE, memory_regions, |bitmap_addr, bitmap_len| {
                let ptr = bitmap_addr as *mut u64;
                core::slice::from_raw_parts_mut(ptr, bitmap_len)
            });
        Self { allocator }
    }
}

unsafe impl<S: PageSize> FrameAllocator<S> for PhysicalMemoryAllocator<'_> {
    fn allocate_frame(&mut self) -> Option<PhysFrame<S>> {
        assert!(
            S::SIZE as usize % PAGE_SIZE == 0,
            "frame size {:?} must be a multiple of page size {}",
            S::SIZE,
            PAGE_SIZE
        );
        let num_pages = S::SIZE as usize / PAGE_SIZE;
        let frame_page = self.allocator.allocate_contiguous(num_pages)?;
        let frame_address = PhysAddr::new(frame_page as u64 * PAGE_SIZE as u64);
        let frame: PhysFrame<S> = PhysFrame::containing_address(frame_address);
        Some(frame)
    }
}

/// Simply wraps `PhysicalMemoryAllocator` in a mutex. This exists because some
/// `x86_64` functions want a `&mut Allocator` and we can't have multiple
/// mutable references to the same object.
pub(crate) struct LockedPhysicalMemoryAllocator<'a> {
    mutex: Mutex<Option<PhysicalMemoryAllocator<'a>>>,
}

impl LockedPhysicalMemoryAllocator<'_> {
    const fn new() -> Self {
        Self {
            mutex: Mutex::new(None),
        }
    }

    unsafe fn init<I, R>(&self, memory_regions: R)
    where
        I: Iterator<Item = MemoryRegion>,
        R: Fn() -> I,
    {
        let allocator = PhysicalMemoryAllocator::new(memory_regions);
        self.mutex.lock().replace(allocator);
    }

    fn with_lock<R>(&self, f: impl FnOnce(&mut PhysicalMemoryAllocator) -> R) -> R {
        let mut mutex_guard = self.mutex.lock();
        let allocator = mutex_guard
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

unsafe impl<S: PageSize> FrameAllocator<S> for LockedPhysicalMemoryAllocator<'_> {
    fn allocate_frame(&mut self) -> Option<PhysFrame<S>> {
        self.mutex
            .lock()
            .as_mut()
            .expect("kernel memory allocator not initialized")
            .allocate_frame()
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
    start_page: usize,
    num_pages: usize,
}

impl PhysicalBuffer {
    // Don't need to expose this b/c allocate_zeroed is safer.
    fn allocate(min_bytes: usize) -> Result<Self, AllocError> {
        let num_pages = min_bytes.div_ceil(PAGE_SIZE);
        let start_page = KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
            allocator
                .allocator
                .allocate_contiguous(num_pages)
                .ok_or(AllocError)
        })?;
        Ok(Self {
            start_page,
            num_pages,
        })
    }

    pub(crate) fn allocate_zeroed(min_bytes: usize) -> Result<Self, AllocError> {
        let buffer = Self::allocate(min_bytes)?;
        let ptr = buffer.address().as_u64() as *mut u8;
        unsafe {
            ptr::write_bytes(ptr, 0, buffer.len_bytes());
        }
        Ok(buffer)
    }

    pub(crate) fn address(&self) -> PhysAddr {
        PhysAddr::new(self.start_page as u64 * PAGE_SIZE as u64)
    }

    pub(crate) fn len_bytes(&self) -> usize {
        self.num_pages * PAGE_SIZE
    }

    pub(crate) unsafe fn write_offset<T>(&mut self, offset: usize, val: T) {
        let buffer_len = self.len_bytes();
        assert!(
            offset < self.len_bytes(),
            "tried to write value at offset {offset} but buffer only has {buffer_len} bytes"
        );
        let ptr = (self.address().as_u64() + offset as u64) as *mut T;
        ptr::write_volatile(ptr, val);
    }

    // TODO: Don't allow leaking. We are only doing this temporarily.
    pub(crate) fn leak(self) -> PhysAddr {
        let buf = core::mem::ManuallyDrop::new(self);
        buf.address()
    }
}

impl Drop for PhysicalBuffer {
    fn drop(&mut self) {
        KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
            allocator
                .allocator
                .free_contiguous(self.start_page, self.num_pages);
        });
    }
}
