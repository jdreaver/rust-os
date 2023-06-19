use alloc::vec::Vec;
use core::fmt;
use core::ops::Index;

use bitflags::bitflags;
use x86_64::{PhysAddr, VirtAddr};

/// A `RawPageTable` with a level. This is the top-level type for this module.
pub(super) struct PageTable {
    level: PageTableLevel,
    table: &'static mut RawPageTable,
}

impl PageTable {
    /// Loads the page table from the current value of the CR3 register.
    ///
    /// # Safety
    ///
    /// This should only be called once to initialize the kernel's page table.
    /// If it is called multiple times there will be multiple mutable references
    /// to the same underlying page table structure.
    pub(super) unsafe fn level_4_from_cr3() -> Self {
        let (level_4_table_frame, _) = x86_64::registers::control::Cr3::read();
        let level_4_table_ptr = level_4_table_frame.start_address().as_u64() as *mut _;
        let table: &mut RawPageTable = unsafe { &mut *level_4_table_ptr };
        Self {
            level: PageTableLevel::Level4,
            table,
        }
    }

    /// Indexes into the page table given a virtual address. NOTE: this only
    /// goes one level deep. Use `translate_address` to recursively translate a
    /// virtual address.
    fn address_entry(&self, addr: VirtAddr) -> PageTableIndexedEntry {
        let index = PageTableIndex::from_address(self.level, addr);
        self.index_entry(index)
    }

    fn index_entry(&self, index: PageTableIndex) -> PageTableIndexedEntry {
        PageTableIndexedEntry {
            level: self.level,
            index,
            entry: self.table.0[index.0 as usize],
        }
    }

    /// Recursively translates a virtual address to a physical address mapped by
    /// the page table.
    pub(super) fn translate_address(&self, addr: VirtAddr) -> Option<PhysicalPage> {
        let entry = self.address_entry(addr);
        let target = entry.target()?;
        match target {
            PageTableTarget::Page(page) => Some(page),
            PageTableTarget::NextTable(next_table) => next_table.translate_address(addr),
        }
    }
}

impl fmt::Debug for PageTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageTable")
            .field("level", &self.level)
            .field(
                "present_entries",
                &self.table.present_entries(self.level).collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
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

/// All page table levels have 512 entries.
const NUM_PAGE_TABLE_ENTRIES: usize = 512;

/// Underlying type for all levels of page tables.
///
/// See 4.5 4-LEVEL PAGING AND 5-LEVEL PAGING
#[derive(Clone)]
#[repr(C, align(4096))]
struct RawPageTable([PageTableEntry; NUM_PAGE_TABLE_ENTRIES]);

impl RawPageTable {
    fn entries(&self, level: PageTableLevel) -> impl Iterator<Item = PageTableIndexedEntry> + '_ {
        self.0
            .iter()
            .enumerate()
            .map(move |(i, entry)| PageTableIndexedEntry {
                level,
                index: PageTableIndex::new(i as u16),
                entry: *entry,
            })
    }

    fn present_entries(
        &self,
        level: PageTableLevel,
    ) -> impl Iterator<Item = PageTableIndexedEntry> + '_ {
        self.entries(level)
            .filter(|entry| entry.entry.flags().contains(PageTableEntryFlags::PRESENT))
    }
}

impl Index<PageTableIndex> for RawPageTable {
    type Output = PageTableEntry;

    fn index(&self, index: PageTableIndex) -> &Self::Output {
        self.0
            .get(index.0 as usize)
            .expect("failed to get entry, somehow index was over 512")
    }
}

/// Simply a `PageTableEntry` with additional context about its level and
/// location in the page table.
struct PageTableIndexedEntry {
    level: PageTableLevel,
    index: PageTableIndex,
    entry: PageTableEntry,
}

impl PageTableIndexedEntry {
    /// Start address of the region the page table points to.
    fn virtual_address(&self) -> VirtAddr {
        let shift = (self.level as u64 - 1) * 9 + 12;
        let raw_addr = u64::from(self.index.0) << shift;
        let sign_extended = sign_extend_virtual_address(raw_addr);
        VirtAddr::new(sign_extended)
    }

    fn target(self) -> Option<PageTableTarget> {
        if !self.entry.flags().contains(PageTableEntryFlags::PRESENT) {
            return None;
        }

        let target_addr = self.entry.address();
        if self.entry.flags().contains(PageTableEntryFlags::HUGE_PAGE) {
            let page_size = match self.level {
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

        self.level.next_lower_level().map_or(
            Some(PageTableTarget::Page(PhysicalPage {
                start_addr: target_addr,
                size: PageSize::Size4KiB,
            })),
            |next_level| {
                let table = unsafe { &mut *(target_addr.as_u64() as *mut RawPageTable) };
                Some(PageTableTarget::NextTable(PageTable {
                    level: next_level,
                    table,
                }))
            },
        )
    }
}

fn sign_extend_virtual_address(address: u64) -> u64 {
    const SIGN_BIT: u64 = 0x0000_8000_0000_0000;
    const SIGN_MASK: u64 = 0xFFFF_8000_0000_0000;

    if (address & SIGN_BIT) == 0 {
        return address;
    }
    (address | SIGN_BIT) | SIGN_MASK
}

impl fmt::Debug for PageTableIndexedEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageTableIndexedEntry")
            .field("level", &self.level)
            .field("index", &self.index)
            .field("virtual_address", &self.virtual_address())
            .field("entry", &self.entry)
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

    fn address(self) -> PhysAddr {
        PhysAddr::new(self.0 & 0x000f_ffff_ffff_f000)
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
enum PageTableTarget {
    Page(PhysicalPage),
    NextTable(PageTable),
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PhysicalPage {
    start_addr: PhysAddr,
    size: PageSize,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum PageSize {
    Size4KiB,
    Size2MiB,
    Size1GiB,
}
