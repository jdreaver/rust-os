use core::fmt;

use alloc::string::String;
use alloc::vec::Vec;

use bitflags::bitflags;
use x86_64::{PhysAddr, VirtAddr};

use crate::memory::{PhysicalBuffer, PAGE_SIZE};

#[derive(Debug)]
pub(super) struct Level4PageTable(&'static mut PageTable);

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
        let level_4_table_ptr = level_4_table_frame.start_address().as_u64() as *mut _;
        let table = unsafe { &mut *level_4_table_ptr };
        Self(table)
    }

    /// Translates a virtual address to a physical address mapped by the page
    /// table.
    pub(super) fn translate_address(&self, addr: VirtAddr) -> Option<PhysicalPage> {
        let mut current_table = &*self.0;
        let mut current_level = PageTableLevel::Level4;

        loop {
            let entry = current_table.address_entry(current_level, addr);
            let target = entry.target(current_level)?;
            match target {
                PageTableTarget::Page(page) => return Some(page),
                PageTableTarget::NextTable { level, table } => {
                    current_table = table;
                    current_level = level;
                }
            }
        }
    }

    pub(super) fn map_to(
        &mut self,
        page: PhysicalPage,
        addr: VirtAddr,
        parent_flags: PageTableEntryFlags,
        flags: PageTableEntryFlags,
    ) -> Result<(), String> {
        assert!(
            page.size == PageSize::Size4KiB,
            "TODO: support more page sizes"
        );

        let mut current_table = &mut *self.0;
        let mut current_level = PageTableLevel::Level4;

        loop {
            let entry = current_table.address_entry_mut(current_level, addr);
            let target = entry.map_to(current_level, page, addr, parent_flags, flags)?;
            match target {
                PageTableTargetMut::Page(_) => return Ok(()),
                PageTableTargetMut::NextTable { level, table } => {
                    current_table = table;
                    current_level = level;
                }
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
        self.0 |= flags.bits();
    }

    fn address(self) -> PhysAddr {
        PhysAddr::new(self.0 & 0x000f_ffff_ffff_f000)
    }

    fn set_address(&mut self, addr: PhysAddr) {
        assert_eq!(addr.as_u64() & !0x000f_ffff_ffff_f000, 0);
        self.0 |= addr.as_u64();
    }

    fn is_present(self) -> bool {
        self.flags().contains(PageTableEntryFlags::PRESENT)
    }

    fn target(&self, level: PageTableLevel) -> Option<PageTableTarget> {
        if !self.is_present() {
            return None;
        }

        let target_addr = self.address();
        if self.flags().contains(PageTableEntryFlags::HUGE_PAGE) {
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
            return Some(PageTableTarget::Page(PhysicalPage {
                start_addr: target_addr,
                size: page_size,
            }));
        }

        level.next_lower_level().map_or(
            Some(PageTableTarget::Page(PhysicalPage {
                start_addr: target_addr,
                size: PageSize::Size4KiB,
            })),
            |level| {
                let table = unsafe { &mut *(target_addr.as_u64() as *mut PageTable) };
                Some(PageTableTarget::NextTable { level, table })
            },
        )
    }

    pub(super) fn map_to(
        &mut self,
        level: PageTableLevel,
        page: PhysicalPage,
        addr: VirtAddr,
        parent_flags: PageTableEntryFlags,
        flags: PageTableEntryFlags,
    ) -> Result<PageTableTargetMut, String> {
        assert!(
            page.size == PageSize::Size4KiB,
            "TODO: support more page sizes"
        );

        if !self.is_present() {
            // TODO: Check target page size here
            return match level.next_lower_level() {
                None => {
                    // We're at the lowest level, so we can map to a page.
                    self.set_address(page.start_addr);
                    self.set_flags(parent_flags | flags);
                    // Need to flush the TLB here
                    x86_64::instructions::tlb::flush(addr);
                    Ok(PageTableTargetMut::Page(page))
                }
                Some(level) => {
                    // We're not at the lowest level, so we need to create a new page table.
                    let new_table_addr = PhysicalBuffer::allocate_zeroed(PAGE_SIZE)
                        .map_err(|e| format!("Failed to allocate physical buffer: {}", e))?
                        .leak();
                    self.set_address(new_table_addr);
                    self.set_flags(parent_flags | flags);
                    let table = unsafe { &mut *(new_table_addr.as_u64() as *mut PageTable) };
                    Ok(PageTableTargetMut::NextTable { level, table })
                }
            };
        }

        assert!(
            !self.flags().contains(PageTableEntryFlags::HUGE_PAGE),
            "TODO: support huge pages"
        );

        match level.next_lower_level() {
            None => Err(format!(
                "Virtual address {addr:x?} is already mapped to a page at {:?}",
                self.address()
            )),
            Some(level) => {
                let table = unsafe { &mut *(self.address().as_u64() as *mut PageTable) };
                Ok(PageTableTargetMut::NextTable { level, table })
            }
        }
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

bitflags! {
    /// Flags for all levels of page table entries.
    #[derive(Debug, Clone, Copy)]
    pub(super) struct PageTableEntryFlags: u64 {
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
enum PageTableTarget<'a> {
    Page(PhysicalPage),
    NextTable {
        level: PageTableLevel,
        table: &'a PageTable,
    },
}

#[derive(Debug)]
enum PageTableTargetMut<'a> {
    Page(PhysicalPage),
    NextTable {
        level: PageTableLevel,
        table: &'a mut PageTable,
    },
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PhysicalPage {
    pub(super) start_addr: PhysAddr,
    pub(super) size: PageSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PageSize {
    Size4KiB,
    Size2MiB,
    Size1GiB,
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
