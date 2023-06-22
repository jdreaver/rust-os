use x86_64::VirtAddr;

use bitmap_alloc::BitmapAllocator;

use crate::memory;
use crate::memory::{Page, PageRange, PageSize, PageTableEntryFlags, UnmapError, PAGE_SIZE};
use crate::sync::SpinLock;

/// Size of a kernel stack, including the guard page (so subtract one page to get
/// usable stack space).
///
/// N.B. This is quite large because apparently Rust programs compiled with
/// debug mode use a ton of the stack. We don't need this much stack in release
/// mode.
const KERNEL_STACK_SIZE_PAGES: usize = 16;
const KERNEL_STACK_SIZE_BYTES: usize = KERNEL_STACK_SIZE_PAGES * PAGE_SIZE;
const KERNEL_STACK_START_VIRT_ADDR: usize = memory::KERNEL_STACK_REGION_START as usize;

const MAX_KERNEL_STACKS: usize = 256;
const MAX_KERNEL_ALLOC_BIT_CHUNKS: usize = MAX_KERNEL_STACKS.div_ceil(u64::BITS as usize);

static mut KERNEL_ALLOC_BIT_CHUNKS: [u64; MAX_KERNEL_ALLOC_BIT_CHUNKS] =
    [0; MAX_KERNEL_ALLOC_BIT_CHUNKS];

static KERNEL_STACK_ALLOCATOR: SpinLock<Option<KernelStackAllocator>> = SpinLock::new(None);

pub(super) fn stack_init() {
    assert!(
        MAX_KERNEL_STACKS * KERNEL_STACK_SIZE_PAGES < memory::KERNEL_STACK_REGION_MAX_SIZE as usize
    );

    let allocator = KernelStackAllocator::new();
    KERNEL_STACK_ALLOCATOR.lock().replace(allocator);
}

pub(super) fn allocate_stack() -> KernelStack {
    let mut lock = KERNEL_STACK_ALLOCATOR.lock_disable_interrupts();
    let allocator = lock
        .as_mut()
        .expect("kernel stack allocator not initialized");
    allocator.allocate().expect("out of kernel stacks")
}

/// Allocator that hands out kernel stacks. All kernel stacks are the same size,
/// and they have a guard page at the end of the stack.
struct KernelStackAllocator<'a> {
    allocator: BitmapAllocator<'a>,
}

impl KernelStackAllocator<'_> {
    /// Create a new kernel stack allocator.
    fn new() -> Self {
        let bits = unsafe { &mut KERNEL_ALLOC_BIT_CHUNKS };
        let allocator = BitmapAllocator::new(bits);
        Self { allocator }
    }

    fn allocate(&mut self) -> Option<KernelStack> {
        // Allocate virtual memory
        let stack_index = self.allocator.allocate_contiguous(1)?;
        let start_addr = VirtAddr::new(
            (KERNEL_STACK_START_VIRT_ADDR + stack_index * KERNEL_STACK_SIZE_BYTES) as u64,
        );
        let stack = KernelStack { start_addr };

        // Map the guard page as invalid
        unsafe {
            match memory::unmap_page(stack.guard_page()) {
                Err(UnmapError::PageNotMapped) => {}
                Ok(page) => panic!("somehow the guard page was physically mapped! {:?}", page),
                Err(e) => panic!("failed to unmap kernel stack guard page: {:?}", e),
            }
        }

        // Map the physical memory into the virtual address space
        let pages = stack.physically_mapped_pages();
        let flags = PageTableEntryFlags::PRESENT | PageTableEntryFlags::WRITABLE;
        memory::allocate_and_map_pages(pages.iter(), flags)
            .expect("failed to map kernel stack pages");

        // Zero out the memory
        unsafe {
            let ptr = start_addr.as_mut_ptr::<u8>().add(PAGE_SIZE);
            ptr.write_bytes(0, KERNEL_STACK_SIZE_BYTES - PAGE_SIZE);
        };

        Some(stack)
    }

    fn free(&mut self, stack: &KernelStack) {
        let stack_index = (stack.start_addr.as_u64() - KERNEL_STACK_START_VIRT_ADDR as u64)
            / KERNEL_STACK_SIZE_BYTES as u64;
        self.allocator.free_contiguous(stack_index as usize, 1);

        for page in stack.physically_mapped_pages().iter() {
            unsafe {
                memory::unmap_page(page).expect("failed to unmap kernel stack page");
            };
        }
    }
}

#[derive(Debug)]
pub(super) struct KernelStack {
    /// Virtual address of the start of top of stack (highest address in the
    /// stack).
    start_addr: VirtAddr,
}

impl KernelStack {
    /// Get the virtual address of the top (highest memory address) of the
    /// stack.
    pub(crate) fn top_addr(&self) -> VirtAddr {
        self.start_addr + KERNEL_STACK_SIZE_BYTES
    }

    fn guard_page(&self) -> Page<VirtAddr> {
        assert!(self.start_addr.as_u64() % PAGE_SIZE as u64 == 0);
        Page {
            start_addr: self.start_addr,
            size: PageSize::Size4KiB,
        }
    }

    fn physically_mapped_pages(&self) -> PageRange<VirtAddr> {
        let start_addr = self.start_addr + PAGE_SIZE;
        let end_addr = self.start_addr + KERNEL_STACK_SIZE_BYTES;
        PageRange::exclusive(start_addr, end_addr)
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        let mut lock = KERNEL_STACK_ALLOCATOR.lock_disable_interrupts();
        let allocator = lock
            .as_mut()
            .expect("kernel stack allocator not initialized");
        allocator.free(self);
    }
}

/// Useful function for page faults to determine if we hit a kernel guard page.
pub(crate) fn is_kernel_guard_page(addr: VirtAddr) -> bool {
    let above_kernel_stack = addr.as_u64() >= KERNEL_STACK_START_VIRT_ADDR as u64;
    let kernel_stack_size = KERNEL_STACK_SIZE_BYTES as u64 * MAX_KERNEL_STACKS as u64;
    let within_kernel_stack =
        addr.as_u64() < KERNEL_STACK_START_VIRT_ADDR as u64 + kernel_stack_size;

    if !(above_kernel_stack && within_kernel_stack) {
        return false;
    }

    // The guard page is the first page in each stack
    let relative_start = addr.as_u64() - KERNEL_STACK_START_VIRT_ADDR as u64;
    let stack_page_index = relative_start / PAGE_SIZE as u64;
    stack_page_index % KERNEL_STACK_SIZE_PAGES as u64 == 0
}
