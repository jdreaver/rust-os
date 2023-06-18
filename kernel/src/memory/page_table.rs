use alloc::vec::Vec;
use bitfield_struct::bitfield;

use core::fmt;

/// See 4.5 4-LEVEL PAGING AND 5-LEVEL PAGING
#[derive(Clone)]
#[repr(C, align(4096))]
pub(crate) struct Level4PageTable {
    entries: [Level4PageTableEntry; 512],
}

impl fmt::Debug for Level4PageTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Level4PageTable")
            .field(
                "entries",
                &self
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| entry.present())
                    .collect::<Vec<(usize, &Level4PageTableEntry)>>(),
            )
            .finish()
    }
}

#[bitfield(u64)]
/// See Table 4-15. Format of a PML4 Entry (PML4E) that References a Page-Directory-Pointer Table
pub(crate) struct Level4PageTableEntry {
    /// Present; must be 1 to reference a page-directory-pointer table
    present: bool,

    /// Read/write; if 0, writes may not be allowed to the 512-GByte region
    /// controlled by this entry
    read_write: bool,

    /// User/supervisor; if 0, user-mode accesses are not allowed
    user_supervisor: bool,

    /// Page-level write-through; indirectly determines the memory type used to
    /// access the page-directory-pointer table referenced by this entry
    page_write_through: bool,

    /// Page-level cache disable; indirectly determines the memory type used to
    /// access the page-directory-pointer table referenced by this entry
    page_cache_disable: bool,

    /// Accessed; indicates whether this entry has been used for linear-address
    accessed: bool,
    __ignored_1: bool,

    /// Reserved (must be 0)
    _page_size: bool,

    #[bits(3)]
    __ignored_2: u8,

    /// For ordinary paging, ignored; for HLAT paging, restart (if 1,
    /// linear-address translation is restarted with ordinary paging)
    hlat_restart: bool,

    /// Physical address of 4-KByte aligned page-directory-pointer table
    /// referenced by this entry. Only up to MAXPHYSADDR bits can be used from
    /// this field.
    #[bits(40)]
    page_directory_pointer_table_address: u64,

    /// Can be used by the OS for whatever it wants.
    #[bits(11)]
    ignored: u16,

    /// If IA32_EFER.NXE = 1, execute-disable (if 1, instruction fetches are not
    /// allowed from the 512-GByte region controlled by this entry; see Section
    /// 4.6); otherwise, reserved (must be 0)
    no_execute: bool,
}
