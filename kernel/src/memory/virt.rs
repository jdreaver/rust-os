use x86_64::structures::paging::mapper::{MapToError, Translate, UnmapError};
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

use crate::sync::{InitCell, SpinLock};

use super::physical::KERNEL_PHYSICAL_ALLOCATOR;

/// Page table mapper used by all kernel contexts.
static KERNEL_MAPPER: KernelMapper = KernelMapper::new();

/// Initialize the `KERNEL_MAPPER` with the passed `physical_memory_offset`.
///
/// # Safety
///
/// This function is unsafe because the caller must guarantee that the complete
/// physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once to
/// avoid aliasing `&mut` references (which is undefined behavior).
pub(super) unsafe fn init(physical_memory_offset: VirtAddr) {
    KERNEL_MAPPER.init(physical_memory_offset);
}

/// SpinLock wrapper around `OffsetPageTable`.
struct KernelMapper {
    lock: SpinLock<Option<OffsetPageTable<'static>>>,
}

/// Holds the physical location of the kernel's page table.
static KERNEL_PAGE_TABLE_ADDR: InitCell<PhysAddr> = InitCell::new();

impl KernelMapper {
    const fn new() -> Self {
        Self {
            lock: SpinLock::new(None),
        }
    }

    unsafe fn init(&self, physical_memory_offset: VirtAddr) {
        let (level_4_table_frame, _) = x86_64::registers::control::Cr3::read();

        let phys = level_4_table_frame.start_address();
        KERNEL_PAGE_TABLE_ADDR.init(phys);
        let virt = physical_memory_offset + phys.as_u64();
        let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

        let level_4_table = unsafe { &mut *page_table_ptr };
        let mapper = unsafe { OffsetPageTable::new(level_4_table, physical_memory_offset) };
        self.lock.lock().replace(mapper);
    }

    fn with_lock<R>(&self, f: impl FnOnce(&mut OffsetPageTable) -> R) -> R {
        let mut lock_guard = self.lock.lock();
        let mapper = lock_guard
            .as_mut()
            .expect("kernel memory mapper not initialized");
        f(mapper)
    }
}

pub(crate) fn kernel_default_page_table_address() -> PhysAddr {
    *KERNEL_PAGE_TABLE_ADDR
        .get()
        .expect("kernel page table frame not initialized")
}

/// Translate a given physical address to a virtual address, if possible.
pub(crate) fn translate_addr(addr: VirtAddr) -> Option<PhysAddr> {
    KERNEL_MAPPER.with_lock(|mapper| mapper.translate_addr(addr))
}

/// Allocates a physical frame for the given virtual page of memory and maps the
/// virtual page to the physical frame in the page table. Useful for
/// initializing a virtual region that is known not to be backed by memory, like
/// initializing the kernel heap.
pub(crate) fn allocate_and_map_pages(
    pages: impl Iterator<Item = Page>,
    flags: PageTableFlags,
) -> Result<(), MapToError<Size4KiB>> {
    KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
        KERNEL_MAPPER.with_lock(|mapper| unsafe {
            for page in pages {
                let frame = allocator
                    .allocate_frame()
                    .ok_or(MapToError::FrameAllocationFailed)?;
                mapper.map_to(page, frame, flags, allocator)?.flush();
            }
            Ok(())
        })
    })
}

pub(crate) fn map_page_to_frame(
    page: Page,
    frame: PhysFrame,
    flags: PageTableFlags,
) -> Result<(), MapToError<Size4KiB>> {
    KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
        KERNEL_MAPPER.with_lock(|mapper| unsafe {
            mapper.map_to(page, frame, flags, allocator)?.flush();
            Ok(())
        })
    })
}

/// Maps a page to a non-existent frame.
pub(crate) unsafe fn map_guard_page(page: Page) -> Result<(), MapToError<Size4KiB>> {
    KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
        KERNEL_MAPPER.with_lock(|mapper| unsafe {
            let frame = PhysFrame::containing_address(PhysAddr::new(0));
            let page_flags = PageTableFlags::empty();
            let parent_table_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
            mapper
                .map_to_with_table_flags(page, frame, page_flags, parent_table_flags, allocator)?
                .flush();
            Ok(())
        })
    })
}

pub(crate) unsafe fn unmap_page(page: Page) -> Result<(), UnmapError> {
    KERNEL_MAPPER.with_lock(|mapper| {
        let (_, flush) = mapper.unmap(page)?;
        flush.flush();
        Ok(())
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
