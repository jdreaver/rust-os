//! Main memory mapping code.
//!
//! Here is a table describing the memory layout used by the kernel, similar to
//! <https://www.kernel.org/doc/Documentation/x86/x86_64/mm.txt>
//!
//! | Start addr            | Offset  | End addr              | Size    | Description |
//! |-----------------------|---------|-----------------------|---------|-------------|
//! | 0x0000_0000_0000_0000 | 0       | 0x0000_7fff_ffff_ffff | 128 TiB | Canonical virtual address space |
//! | 0x0000_8000_0000_0000 | +128 TB | 0xffff_7fff_ffff_ffff | ~16M TB | Empty space that is not allowed to be accessed in x86_64 |
//! | 0xffff_8000_0000_0000 | -128 TB | 0xffff_bfff_ffff_ffff |   64 TB | Direct mapping of physical memory. Also includes device mappings like PCI. |
//! | 0xffff_c000_0000_0000 | -64 TB  | 0xffff_cfff_ffff_ffff |   16 TB | Kernel heap (very large, could be split up) |
//! | 0xffff_d000_0000_0000 | -48 TB  | 0xffff_dfff_ffff_ffff |   16 TB | Kernel stack allocations (separate from heap, very large and could be split up) |
//! | 0xffff_e000_0000_0000 | -32 TB  | 0xffff_ffff_efff_ffff |  ~32 TB | (empty space) |
//! | 0xffff_ffff_8000_0000 | -2 GB   | 0xffff_ffff_ffff_ffff |    2 GB | Kernel text and data segments |

use x86_64::{PhysAddr, VirtAddr};

use crate::boot_info::BootInfo;
use crate::serial_println;
use crate::sync::SpinLock;

use super::address::KernPhysAddr;
use super::page::{Page, PageSize};
use super::page_table::{
    Level4PageTable, MapError, MapTarget, PageTableEntryFlags, SetFlagsError, UnmapError,
};
use super::physical::KERNEL_PHYSICAL_ALLOCATOR;

pub(crate) const HIGHER_HALF_START: u64 = 0xffff_8000_0000_0000;

pub(crate) const KERNEL_PHYSICAL_MAPPING_START: u64 = HIGHER_HALF_START;
pub(crate) const KERNEL_PHYSICAL_MAPPING_END: u64 = 0xffff_b777_7777_7777;

pub(crate) const KERNEL_HEAP_REGION_START: u64 = 0xffff_c000_0000_0000;
pub(crate) const KERNEL_HEAP_REGION_MAX_SIZE: u64 = 0x0000_1000_0000_0000;

pub(crate) const KERNEL_STACK_REGION_START: u64 = 0xffff_d000_0000_0000;
pub(crate) const KERNEL_STACK_REGION_MAX_SIZE: u64 = 0xffff_1000_0000_0000;

pub(crate) const KERNEL_TEXT_DATA_REGION_START: u64 = 0xffff_ffff_8000_0000;

static KERNEL_PAGE_TABLE: SpinLock<Option<Level4PageTable>> = SpinLock::new(None);

pub(super) fn init(boot_info_data: &BootInfo) {
    assert!(
        boot_info_data.higher_half_direct_map_offset.as_u64() == HIGHER_HALF_START,
        "higher half start address mismatch"
    );
    assert!(
        boot_info_data.kernel_address_virtual_base.as_u64() == KERNEL_TEXT_DATA_REGION_START,
        "kernel text/data region start address mismatch"
    );

    let mut lock = KERNEL_PAGE_TABLE.lock();
    assert!(lock.is_none(), "kernel page table already initialized");
    let page_table = unsafe { Level4PageTable::from_cr3() };
    lock.replace(page_table);
}

pub(super) fn clean_up_kernel_page_table() {
    let mut page_table_lock = KERNEL_PAGE_TABLE.lock();
    let table = page_table_lock
        .as_mut()
        .expect("kernel page table not initialized");

    table.unmap_lower_half();

    KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
        table.fill_top_half_entries(allocator);
    });
}

pub(crate) fn with_kernel_page_table_lock<F, R>(f: F) -> R
where
    F: FnOnce(&mut Level4PageTable) -> R,
{
    let mut page_table_lock = KERNEL_PAGE_TABLE.lock();
    let table = page_table_lock
        .as_mut()
        .expect("kernel page table not initialized");

    f(table)
}

pub(crate) fn clone_kernel_page_table() -> Level4PageTable {
    let mut page_table_lock = KERNEL_PAGE_TABLE.lock();
    let table = page_table_lock
        .as_mut()
        .expect("kernel page table not initialized");

    KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| table.allocate_clone(allocator))
}

/// Allocates a physical frame for the given virtual page of memory and maps the
/// virtual page to the physical frame in the page table. Useful for
/// initializing a virtual region that is known not to be backed by memory, like
/// initializing the kernel heap.
pub(crate) fn allocate_and_map_pages(
    page_table: &mut Level4PageTable,
    pages: impl Iterator<Item = Page<VirtAddr>>,
    flags: PageTableEntryFlags,
) -> Result<(), MapError> {
    KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
        for page in pages {
            page_table.map_to(allocator, page, MapTarget::NewPhysPage, flags)?;
        }

        Ok(())
    })
}

pub(crate) fn set_page_flags(
    page_table: &mut Level4PageTable,
    pages: impl Iterator<Item = Page<VirtAddr>>,
    flags: PageTableEntryFlags,
) -> Result<(), SetFlagsError> {
    for page in pages {
        page_table.set_flags(page, flags)?;
    }
    Ok(())
}

pub(crate) unsafe fn unmap_page(
    page_table: &mut Level4PageTable,
    page: Page<VirtAddr>,
) -> Result<Page<KernPhysAddr>, UnmapError> {
    KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| page_table.unmap(allocator, page, false))
}

/// Unmaps a given virtual page from the page table and frees the physical page
/// it was mapped to.
///
/// # Safety
///
/// Caller must ensure the underlying physical page is not in use.
pub(crate) unsafe fn unmap_and_free_page(
    page_table: &mut Level4PageTable,
    page: Page<VirtAddr>,
) -> Result<Page<KernPhysAddr>, UnmapError> {
    KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| page_table.unmap(allocator, page, true))
}

pub(crate) fn identity_map_physical_pages(
    page_table: &mut Level4PageTable,
    phys_pages: impl Iterator<Item = Page<KernPhysAddr>>,
    flags: PageTableEntryFlags,
) -> Result<(), MapError> {
    KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
        for phys_page in phys_pages {
            let virt_addr = VirtAddr::from(phys_page.start_addr());
            let virt_page = Page::from_start_addr(virt_addr, phys_page.size());
            let result = page_table.map_to(
                allocator,
                virt_page,
                MapTarget::ExistingPhysPage(phys_page),
                flags,
            );
            match result {
                // These errors are okay. They just mean the frame is already identity
                // mapped (well, hopefully).
                Ok(_) | Err(MapError::PageAlreadyMapped { .. }) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    })
}

pub(crate) fn test_new_page_table() {
    let mut lock = KERNEL_PAGE_TABLE.lock();
    let table = lock.as_mut().expect("kernel page table not initialized");

    serial_println!("{table:#?}");

    let target_addr = VirtAddr::new(0x1234);
    let target = table.translate_address(target_addr);
    serial_println!("Target of {target_addr:x?}: {target:x?}");

    let target_addr = VirtAddr::new(0x4000_1234);
    let target = table.translate_address(target_addr);
    serial_println!("Target of {target_addr:x?}: {target:x?}");

    let target_addr = VirtAddr::new(0xffff_ffff_8000_1234);
    let target = table.translate_address(target_addr);
    serial_println!("Target of {target_addr:x?}: {target:x?}");

    let map_virt = Page::from_start_addr(VirtAddr::new(0x4_0000_0000), PageSize::Size4KiB);
    let map_phys = Page::from_start_addr(
        KernPhysAddr::from(PhysAddr::new(0x1_0000_0000)),
        PageSize::Size4KiB,
    );

    let result = KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
        table.map_to(
            allocator,
            map_virt,
            MapTarget::ExistingPhysPage(map_phys),
            PageTableEntryFlags::PRESENT | PageTableEntryFlags::WRITABLE,
        )
    });

    serial_println!("Mapping {map_virt:?} to {map_phys:?}, result: {:?}", result);

    let target = table.translate_address(map_virt.start_addr());
    serial_println!("Target of {:x?}: {target:x?}", map_virt.start_addr());

    let unmap_result =
        KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| table.unmap(allocator, map_virt, false));
    serial_println!("Unmap result: {:?}", unmap_result);

    let map_virt = Page::from_start_addr(VirtAddr::new(0x4_0001_0000), PageSize::Size4KiB);
    let result = KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| {
        table.map_to(
            allocator,
            map_virt,
            MapTarget::NewPhysPage,
            PageTableEntryFlags::PRESENT | PageTableEntryFlags::WRITABLE,
        )
    });

    serial_println!("Mapped {map_virt:?}, result: {:?}", result);

    let target = table.translate_address(map_virt.start_addr());
    serial_println!("Target of {target_addr:x?}: {target:x?}");

    let unmap_result =
        KERNEL_PHYSICAL_ALLOCATOR.with_lock(|allocator| table.unmap(allocator, map_virt, true));
    serial_println!("Unmap result: {:?}", unmap_result);
}
