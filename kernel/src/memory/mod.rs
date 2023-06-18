mod heap;
mod page_table;
mod physical;
mod virt;

pub(crate) use page_table::*;
pub(crate) use physical::*;
pub(crate) use virt::*;

use x86_64::VirtAddr;

use bitmap_alloc::MemoryRegion;

pub(crate) unsafe fn init<I, R>(physical_memory_offset: VirtAddr, usable_memory_regions: R)
where
    I: Iterator<Item = MemoryRegion>,
    R: Fn() -> I,
{
    virt::init(physical_memory_offset);
    physical::init(usable_memory_regions);
    heap::init().expect("failed to initialize heap");
}
