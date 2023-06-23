use core::alloc::AllocError;
use core::fmt;

use alloc::vec::Vec;

use bitflags::bitflags;
use x86_64::{PhysAddr, VirtAddr};

use super::address::KernPhysAddr;
use super::page::{Page, PageSize};
use super::physical::{PhysicalMemoryAllocator, PAGE_SIZE};

#[derive(Debug)]
pub(crate) struct Level4PageTable(&'static mut PageTable);

impl Level4PageTable {
    /// Loads the page table from the current value of the CR3 register.
    ///
    /// # Safety
    ///
    /// This should only be called once to initialize the kernel's page table.
    /// If it is called multiple times there will be multiple mutable references
    /// to the same underlying page table structure.
    pub(super) unsafe fn from_cr3() -> Self {
        let (level_4_table_frame, _) = x86_64::registers::control::Cr3::read();
        let addr = KernPhysAddr::from(level_4_table_frame.start_address());
        let table = unsafe { &mut *addr.as_mut_ptr() };
        Self(table)
    }

    pub(crate) fn physical_address(&self) -> PhysAddr {
        let table_ptr = self.0 as *const _ as u64;
        let addr = KernPhysAddr::new(table_ptr);
        PhysAddr::from(addr)
    }

    /// Allocates a clone of the page table into a new physical page.
    pub(super) fn allocate_clone(&self, allocator: &mut PhysicalMemoryAllocator) -> Self {
        let page = allocator
            .allocate_page()
            .expect("failed to allocate page for Level4PageTable clone");
        let new_table = unsafe { &mut *page.start_addr().as_mut_ptr::<PageTable>() };
        new_table.entries.copy_from_slice(&self.0.entries);
        Self(new_table)
    }

    /// Translates a virtual address to a physical page mapped by the page
    /// table.
    pub(super) fn translate_address(&self, addr: VirtAddr) -> TranslateResult {
        let mut current_table = &*self.0;
        let mut current_level = PageTableLevel::Level4;

        loop {
            let entry = current_table.address_entry(current_level, addr);
            let target = entry.target(current_level);
            match target {
                PageTableTarget::Unmapped => return TranslateResult::Unmapped,
                PageTableTarget::Page { page, flags } => {
                    let offset = addr.as_u64() % page.size().size_bytes() as u64;
                    return TranslateResult::Mapped(AddressPageMapping {
                        page,
                        flags,
                        offset,
                    });
                }
                PageTableTarget::NextTable { level, table } => {
                    current_table = table;
                    current_level = level;
                }
            }
        }
    }

    pub(super) fn map_to(
        &mut self,
        allocator: &mut PhysicalMemoryAllocator,
        page: Page<VirtAddr>,
        map_target: MapTarget,
        flags: PageTableEntryFlags,
    ) -> Result<Page<KernPhysAddr>, MapError> {
        let mut current_table = &mut *self.0;
        let mut current_level = PageTableLevel::Level4;

        let parent_flags = flags
            & (PageTableEntryFlags::PRESENT
                | PageTableEntryFlags::WRITABLE
                | PageTableEntryFlags::USER_ACCESSIBLE);

        loop {
            let entry = current_table.address_entry_mut(current_level, page.start_addr());
            let (entry, target) = entry.target_mut(current_level);
            match target {
                PageTableTarget::Unmapped => {
                    match current_level.next_lower_level() {
                        None => {
                            // We're at the lowest level, so we can map to a page.
                            //
                            // TODO: Check target page size instead of saying lowest level
                            // == make a page.
                            let target_page = map_target.get_target_page(allocator, page.size())?;
                            entry.set_target_page(&target_page, flags);
                            // Need to flush the TLB here
                            page.flush();
                            return Ok(target_page);
                        }
                        Some(next_level) => {
                            // We're not at the lowest level, so we need to create a new page table.
                            let table =
                                entry.allocate_and_map_child_table(allocator, parent_flags)?;
                            current_table = table;
                            current_level = next_level;
                        }
                    }
                }
                PageTableTarget::Page { page, flags } => {
                    return Err(MapError::PageAlreadyMapped {
                        existing_target: page,
                        flags,
                    })
                }
                PageTableTarget::NextTable { level, table } => {
                    current_table = table;
                    current_level = level;
                }
            }
        }
    }

    pub(super) fn set_flags(
        &mut self,
        page: Page<VirtAddr>,
        flags: PageTableEntryFlags,
    ) -> Result<(), SetFlagsError> {
        let mut current_table = &mut *self.0;
        let mut current_level = PageTableLevel::Level4;

        loop {
            let entry = current_table.address_entry_mut(current_level, page.start_addr());
            let (entry, target) = entry.target_mut(current_level);
            match target {
                PageTableTarget::Unmapped => return Err(SetFlagsError::PageNotMapped),
                PageTableTarget::Page { .. } => {
                    entry.set_flags(flags);
                    page.flush();
                    return Ok(());
                }
                PageTableTarget::NextTable { level, table } => {
                    current_table = table;
                    current_level = level;
                }
            }
        }
    }

    /// Unmaps a given virtual page and returns the underlying physical page.
    ///
    /// NOTE: this function does not handle deallocation of the physical page.
    pub(super) fn unmap(
        &mut self,
        allocator: &mut PhysicalMemoryAllocator,
        page: Page<VirtAddr>,
        free_physical_page: bool,
    ) -> Result<Page<KernPhysAddr>, UnmapError> {
        let mut current_table = &mut *self.0;
        let mut current_level = PageTableLevel::Level4;

        loop {
            let entry = current_table.address_entry_mut(current_level, page.start_addr());
            let (entry, target) = entry.target_mut(current_level);
            match target {
                PageTableTarget::Unmapped => return Err(UnmapError::PageNotMapped),
                PageTableTarget::Page {
                    page: target_page,
                    flags: _,
                } => {
                    if target_page.size() != page.size() {
                        return Err(UnmapError::PageWrongSize {
                            expected_size: page.size(),
                            actual_size: target_page.size(),
                        });
                    }
                    entry.clear();
                    page.flush();

                    if free_physical_page {
                        allocator.free_page(target_page);
                    }

                    return Ok(target_page);
                }
                PageTableTarget::NextTable { level, table } => {
                    current_table = table;
                    current_level = level;
                }
            }
        }
    }

    /// Unmaps the lower half of the page table. This ensures that the kernel
    /// page table doesn't touch anything in the lower half of the address
    /// space, so it can be free for userspace when cloned.
    pub(super) fn unmap_lower_half(&mut self) {
        for i in 0..256 {
            // TODO: Recursively free any intermediate child tables (but don't
            // try and free the leaf pages since they probably weren't actually
            // allocated).
            self.0.entries[i].clear();
        }
    }

    /// Fills in entries for level 3 tables in the top half of the level 4
    /// table. This ensures that when this page table is cloned for new
    /// processes, the kernel's mappings are preserved.
    pub(super) fn fill_top_half_entries(&mut self, allocator: &mut PhysicalMemoryAllocator) {
        let flags = PageTableEntryFlags::PRESENT | PageTableEntryFlags::WRITABLE;

        for i in 256..512 {
            let entry = &mut self.0.entries[i];
            let (entry, target) = entry.target_mut(PageTableLevel::Level4);
            match target {
                PageTableTarget::Unmapped => {
                    entry
                        .allocate_and_map_child_table(allocator, flags)
                        .expect("failed to create level 3 table for top half of kernel page table");
                }
                PageTableTarget::Page { .. } => {
                    panic!("somehow the level 4 table had a page mapped!")
                }
                PageTableTarget::NextTable { .. } => {
                    // Already mapped, just set flags
                    entry.set_flags(flags);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum TranslateResult {
    Unmapped,
    Mapped(AddressPageMapping),
}

/// Result of mapping a virtual address to a page.
#[derive(Debug, Clone)]
pub(crate) struct AddressPageMapping {
    pub(crate) page: Page<KernPhysAddr>,
    #[allow(dead_code)]
    pub(crate) flags: PageTableEntryFlags,
    pub(crate) offset: u64,
}

impl AddressPageMapping {
    pub(crate) fn address(&self) -> KernPhysAddr {
        self.page.start_addr() + self.offset
    }
}

/// Target of the `map_to` operation.
#[derive(Debug, Clone, Copy)]
pub(super) enum MapTarget {
    /// Map the virtual page to the given physical page that has already been allocated.
    ExistingPhysPage(Page<KernPhysAddr>),
    /// Allocate a new physical page and map the virtual page to it.
    NewPhysPage,
}

impl MapTarget {
    fn get_target_page(
        self,
        allocator: &mut PhysicalMemoryAllocator,
        target_page_size: PageSize,
    ) -> Result<Page<KernPhysAddr>, AllocError> {
        match self {
            Self::ExistingPhysPage(page) => {
                assert!(
                    page.size() == target_page_size,
                    "ERROR: {page:?} was expected to have size {target_page_size:?}",
                );

                Ok(page)
            }
            Self::NewPhysPage => {
                assert!(
                    target_page_size.size_bytes() == PAGE_SIZE,
                    "ERROR: page size must be {PAGE_SIZE} bytes. TODO: support larger pages (and handle alignment requirements!)",
                );
                let target_page = allocator.allocate_page()?;
                Ok(target_page)
            }
        }
    }
}

/// All page table levels have 512 entries.
const NUM_PAGE_TABLE_ENTRIES: usize = 512;

/// Underlying type for all levels of page tables.
///
/// See 4.5 4-LEVEL PAGING AND 5-LEVEL PAGING
#[derive(Clone)]
#[repr(C, align(4096))]
struct PageTable {
    entries: [PageTableEntry; NUM_PAGE_TABLE_ENTRIES],
}

impl PageTable {
    /// Indexes into the page table given a virtual address.
    fn address_entry(&self, level: PageTableLevel, addr: VirtAddr) -> &PageTableEntry {
        let index = PageTableIndex::from_address(level, addr);
        &self.entries[index.0 as usize]
    }

    fn address_entry_mut(&mut self, level: PageTableLevel, addr: VirtAddr) -> &mut PageTableEntry {
        let index = PageTableIndex::from_address(level, addr);
        &mut self.entries[index.0 as usize]
    }
}

impl fmt::Debug for PageTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let present_entries = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_present())
            .collect::<Vec<_>>();
        f.debug_struct("PageTable")
            .field("present_entries", &present_entries)
            .finish()
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct PageTableEntry(u64);

impl PageTableEntry {
    fn flags(self) -> PageTableEntryFlags {
        PageTableEntryFlags::from_bits_truncate(self.0)
    }

    fn set_flags(&mut self, flags: PageTableEntryFlags) {
        self.0 = self.raw_address() | flags.bits();
    }

    fn raw_address(self) -> u64 {
        self.0 & 0x000f_ffff_ffff_f000
    }

    fn address(self) -> KernPhysAddr {
        let phys = PhysAddr::new(self.raw_address());
        KernPhysAddr::from(phys)
    }

    fn set_address(&mut self, addr: PhysAddr) {
        assert_eq!(addr.as_u64() & !0x000f_ffff_ffff_f000, 0);
        self.0 |= addr.as_u64();
    }

    fn is_present(self) -> bool {
        self.flags().contains(PageTableEntryFlags::PRESENT)
    }

    fn target(&self, level: PageTableLevel) -> PageTableTarget<&PageTable> {
        self.target_inner(level, |addr| unsafe { &*(addr.as_ptr::<PageTable>()) })
    }

    fn target_mut(
        &mut self,
        level: PageTableLevel,
    ) -> (&mut Self, PageTableTarget<&mut PageTable>) {
        let target = self.target_inner(level, |addr| unsafe {
            &mut *(addr.as_mut_ptr::<PageTable>())
        });
        (self, target)
    }

    fn target_inner<T, F>(self, level: PageTableLevel, load_table: F) -> PageTableTarget<T>
    where
        F: Fn(KernPhysAddr) -> T,
    {
        if !self.is_present() {
            return PageTableTarget::Unmapped;
        }

        let flags = self.flags();
        if flags.contains(PageTableEntryFlags::HUGE_PAGE) {
            let page_size = match level {
                PageTableLevel::Level4 => {
                    panic!("found huge page flag on level 4 page table entry! {self:?}")
                }
                PageTableLevel::Level3 => PageSize::Size1GiB,
                PageTableLevel::Level2 => PageSize::Size2MiB,
                PageTableLevel::Level1 => {
                    panic!("found huge page flag on level 1 page table entry! {self:?}")
                }
            };
            return PageTableTarget::Page {
                page: Page::from_start_addr(self.address(), page_size),
                flags,
            };
        }

        level.next_lower_level().map_or_else(
            || PageTableTarget::Page {
                page: Page::from_start_addr(self.address(), PageSize::Size4KiB),
                flags,
            },
            |level| {
                let table = load_table(self.address());
                PageTableTarget::NextTable { level, table }
            },
        )
    }

    fn clear(&mut self) {
        self.0 = 0;
    }

    fn set_target_page(&mut self, page: &Page<KernPhysAddr>, flags: PageTableEntryFlags) {
        self.set_address(PhysAddr::from(page.start_addr()));
        self.set_flags(flags | PageTableEntryFlags::PRESENT);
    }

    fn allocate_and_map_child_table(
        &mut self,
        allocator: &mut PhysicalMemoryAllocator,
        flags: PageTableEntryFlags,
    ) -> Result<&mut PageTable, AllocError> {
        // Allocate a new zeroed paged to hold the child page table
        let mut new_table_page = allocator.allocate_page()?;
        new_table_page.zero();

        // Tell the current entry to point to the new child
        let new_table_addr = new_table_page.start_addr();
        self.set_address(PhysAddr::from(new_table_addr));
        self.set_flags(flags | PageTableEntryFlags::PRESENT);

        // Return the child table we just created
        Ok(unsafe { &mut *(new_table_addr.as_mut_ptr::<PageTable>()) })
    }
}

impl fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageTableEntry")
            .field("flags", &self.flags())
            .field("address", &self.address())
            .finish()
    }
}

#[derive(Debug)]
pub(crate) enum MapError {
    PhysicalPageAllocationFailed(AllocError),
    #[allow(dead_code)]
    PageAlreadyMapped {
        existing_target: Page<KernPhysAddr>,
        flags: PageTableEntryFlags,
    },
}

impl From<AllocError> for MapError {
    fn from(e: AllocError) -> Self {
        Self::PhysicalPageAllocationFailed(e)
    }
}

#[derive(Debug)]
pub(crate) enum UnmapError {
    PageNotMapped,
    #[allow(dead_code)]
    PageWrongSize {
        expected_size: PageSize,
        actual_size: PageSize,
    },
}

#[derive(Debug)]
pub(crate) enum SetFlagsError {
    PageNotMapped,
}

bitflags! {
    /// Flags for all levels of page table entries.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct PageTableEntryFlags: u64 {
        /// Indicates if entry is valid. If this bit is unset, the entry is ignored.
        const PRESENT = 1;

        /// If unset, then the region represented by this entry cannot be written to.
        const WRITABLE = 1 << 1;

        /// If set, access from ring 3 is permitted.
        const USER_ACCESSIBLE = 1 << 2;

        /// If this bit is set, a "write-through" policy is used for the cache,
        /// else a "write-back" policy is used.
        const PAGE_WRITE_THROUGH = 1 << 3;

        /// Disables caching.
        const PAGE_CACHE_DISABLE = 1 << 4;

        /// Set by the CPU when the mapped frame or page table is accessed.
        const ACCESSED = 1 << 5;

        /// Set by the CPU on a write to the mapped frame.
        const DIRTY = 1 << 6;

        /// Only allowed in level 2 or 3 tables. If set in level 3, then the
        /// entry points to a 1 GiB page. If set in level 2, then the entry
        /// points to a 2 MiB page.
        const HUGE_PAGE = 1 << 7;

        /// Indicates that the mapping is present in all address spaces, so it
        /// isn't flushed from the TLB on an address space switch.
        const GLOBAL = 1 << 8;

        // Bits available to the OS to do whatever it wants. We can use these in
        // the future.
        const OS_BIT_9 = 1 << 9;
        const OS_BIT_10 = 1 << 10;
        const OS_BIT_11 = 1 << 11;
        const OS_BIT_52 = 1 << 52;
        const OS_BIT_53 = 1 << 53;
        const OS_BIT_54 = 1 << 54;
        const OS_BIT_55 = 1 << 55;
        const OS_BIT_56 = 1 << 56;
        const OS_BIT_57 = 1 << 57;
        const OS_BIT_58 = 1 << 58;
        const OS_BIT_59 = 1 << 59;
        const OS_BIT_60 = 1 << 60;
        const OS_BIT_61 = 1 << 61;
        const OS_BIT_62 = 1 << 62;

        /// If set, then the memory in the region cannot be executed (e.g. it
        /// cannot hold code, and we will get a page fault if the instruction
        /// pointer points here).
        ///
        /// Requires no-execute page protection feature set in the EFER
        /// register.
        const NO_EXECUTE = 1 << 63;
    }
}

/// Index into a page table. Guaranteed to be in the range 0..512 (9 bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PageTableIndex(u16);

impl PageTableIndex {
    fn new(index: u16) -> Self {
        assert!(usize::from(index) < NUM_PAGE_TABLE_ENTRIES);
        Self(index)
    }

    /// Computes an index into a page table for a virtual address.
    fn from_address(level: PageTableLevel, address: VirtAddr) -> Self {
        let shift = 12 + (u64::from(level as u16) - 1) * 9;
        let mask = 0b1_1111_1111;
        let index = ((address.as_u64() >> shift) & mask) as u16;
        Self::new(index)
    }
}

/// What a page table entry points to.
#[derive(Debug)]
enum PageTableTarget<T> {
    Unmapped,
    Page {
        page: Page<KernPhysAddr>,
        flags: PageTableEntryFlags,
    },
    NextTable {
        level: PageTableLevel,
        table: T,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageTableLevel {
    Level1 = 1,
    Level2,
    Level3,
    Level4,
}

impl PageTableLevel {
    fn next_lower_level(self) -> Option<Self> {
        match self {
            Self::Level4 => Some(Self::Level3),
            Self::Level3 => Some(Self::Level2),
            Self::Level2 => Some(Self::Level1),
            Self::Level1 => None,
        }
    }
}

/// Start address of the region the page table entry points to.
#[allow(dead_code)]
fn page_table_entry_virtual_address(level: PageTableLevel, index: PageTableIndex) -> VirtAddr {
    let shift = (level as u64 - 1) * 9 + 12;
    let raw_addr = u64::from(index.0) << shift;
    let sign_extended = sign_extend_virtual_address(raw_addr);
    VirtAddr::new(sign_extended)
}

#[allow(dead_code)]
fn sign_extend_virtual_address(address: u64) -> u64 {
    const SIGN_BIT: u64 = 0x0000_8000_0000_0000;
    const SIGN_MASK: u64 = 0xFFFF_8000_0000_0000;

    if (address & SIGN_BIT) == 0 {
        return address;
    }
    (address | SIGN_BIT) | SIGN_MASK
}
