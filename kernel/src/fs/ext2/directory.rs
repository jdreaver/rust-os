use alloc::vec::Vec;

use crate::block::BlockBuffer;

use super::superblock::InodeNumber;

/// See <https://www.nongnu.org/ext2-doc/ext2.html#linked-directories>
#[derive(Debug)]
pub(super) struct DirectoryBlock {
    block: BlockBuffer,

    /// Start point for each directory entry.
    entry_locations: Vec<usize>,
}

impl DirectoryBlock {
    pub(super) fn from_block(mut block: BlockBuffer) -> Self {
        let bytes = block.data_mut();

        let mut entry_locations = Vec::new();
        let mut offset = 0;
        while offset < bytes.len() {
            entry_locations.push(offset);
            let entry = unsafe { DirectoryEntry::from_bytes(&bytes[offset..]) };
            offset += entry.header().rec_len as usize;
        }

        Self {
            block,
            entry_locations,
        }
    }

    // pub(super) fn get_entry(&self, index: usize) -> Option<DirectoryEntry> {
    //     let entry_location = self.entry_locations.get(index)?;
    //     Some(self.load_entry_from_location(*entry_location))
    // }

    fn load_entry_from_location(&self, location: usize) -> DirectoryEntry {
        let bytes = self.block.data();
        unsafe { DirectoryEntry::from_bytes(&bytes[location..]) }
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = DirectoryEntry> {
        self.entry_locations
            .iter()
            .map(|&location| self.load_entry_from_location(location))
    }
}

#[derive(Debug)]
pub(super) struct DirectoryEntry<'a> {
    bytes: &'a [u8],
}

impl<'a> DirectoryEntry<'a> {
    /// # Safety
    ///
    /// The caller must ensure that the beginning of the given slice is a valid
    /// starting point for a directory entry.
    unsafe fn from_bytes(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn header(&self) -> &DirectoryEntryHeader {
        unsafe {
            let ptr = self.bytes.as_ptr().cast::<DirectoryEntryHeader>();
            ptr.as_ref().expect("DirectoryEntryHeader pointer is null")
        }
    }

    pub(super) fn name(&self) -> &str {
        let name_start = core::mem::size_of::<DirectoryEntryHeader>();
        let name_end = name_start + self.header().name_len as usize;
        let name_slice = &self.bytes[name_start..name_end];
        core::str::from_utf8(name_slice).unwrap_or("<invalid utf8>")
    }

    pub(super) fn inode_number(&self) -> InodeNumber {
        self.header().inode
    }

    pub(super) fn is_file(&self) -> bool {
        self.header().file_type == DirectoryEntryFileType::RegularFile
    }

    pub(super) fn is_dir(&self) -> bool {
        self.header().file_type == DirectoryEntryFileType::Directory
    }
}

/// See <https://www.nongnu.org/ext2-doc/ext2.html#linked-directory-entry-structure>
#[repr(C, packed)]
#[derive(Debug)]
struct DirectoryEntryHeader {
    inode: InodeNumber,
    rec_len: u16,
    name_len: u8,
    file_type: DirectoryEntryFileType,
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
enum DirectoryEntryFileType {
    Unknown = 0,
    RegularFile = 1,
    Directory = 2,
    CharacterDevice = 3,
    BlockDevice = 4,
    Fifo = 5,
    Socket = 6,
    SymbolicLink = 7,
}
