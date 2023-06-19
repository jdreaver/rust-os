mod heap;
mod mapping;
mod page_table;
mod physical;
mod virt;

pub(crate) use mapping::*;
pub(crate) use physical::*;
pub(crate) use virt::*;

use bitmap_alloc::MemoryRegion;

use crate::boot_info::BootInfo;

pub(crate) unsafe fn init<I, R>(boot_info_data: &BootInfo, usable_memory_regions: R)
where
    I: Iterator<Item = MemoryRegion>,
    R: Fn() -> I,
{
    mapping::init(boot_info_data);
    virt::init(boot_info_data.higher_half_direct_map_offset);
    physical::init(usable_memory_regions);
    heap::init().expect("failed to initialize heap");
}
