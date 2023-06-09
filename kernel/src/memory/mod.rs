mod address;
mod heap;
mod mapping;
mod page;
mod page_table;
mod physical;

pub(crate) use address::*;
pub(crate) use mapping::*;
pub(crate) use page::*;
pub(crate) use page_table::*;
pub(crate) use physical::*;

use bitmap_alloc::MemoryRegion;

use crate::boot_info::BootInfo;

pub(crate) unsafe fn init<I, R>(boot_info_data: &BootInfo, usable_memory_regions: R)
where
    I: Iterator<Item = MemoryRegion>,
    R: Fn() -> I,
{
    mapping::init(boot_info_data);
    physical::init(usable_memory_regions);
    mapping::clean_up_kernel_page_table();
    heap::init().expect("failed to initialize heap");
}
