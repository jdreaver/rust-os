use alloc::vec::Vec;

use zerocopy::{AsBytes, FromBytes, FromZeroes, LayoutVerified};

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
    /// Create from a directory block that is known to have entries in it. For
    /// example, a block that was just loaded from a known good ext2 disk.
    pub(super) fn from_existing_block(mut block: BlockBuffer) -> Self {
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

    fn load_entry_from_location(&self, location: usize) -> DirectoryEntry {
        let bytes = self.block.data();
        unsafe { DirectoryEntry::from_bytes(&bytes[location..]) }
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = DirectoryEntry> {
        self.entry_locations
            .iter()
            .map(|&location| self.load_entry_from_location(location))
    }

    pub(super) fn insert_entry(
        &mut self,
        name: &str,
        inode_number: InodeNumber,
        entry_type: DirectoryEntryFileType,
    ) -> Option<DirectoryEntry> {
        let mut new_header = DirectoryEntryHeader::new(inode_number, name, entry_type);

        let (found_index, found_location) = self.iter().enumerate().find_map(|(i, entry)| {
            let header = entry.header();

            // Check if there is enough space
            if header.rec_len as usize - header.required_space() >= new_header.required_space() {
                Some((i, self.entry_locations[i]))
            } else {
                None
            }
        })?;

        // Do some surgery. Truncate the previous record, and insert the new one.
        let found_header = unsafe {
            let ptr = self.block.data_mut()[found_location..]
                .as_mut_ptr()
                .cast::<DirectoryEntryHeader>();
            ptr.as_mut().expect("DirectoryEntryHeader pointer is null")
        };
        let prev_rec_len = found_header.rec_len;
        found_header.rec_len = found_header.required_space() as u16;

        // Write the header and string
        let new_location = found_location + found_header.rec_len as usize;
        new_header.rec_len = prev_rec_len - found_header.rec_len;
        unsafe {
            let ptr = self.block.data_mut()[new_location..]
                .as_mut_ptr()
                .cast::<DirectoryEntryHeader>();
            ptr.write(new_header);
            let ptr = ptr.add(1).cast::<u8>();
            ptr.copy_from_nonoverlapping(name.as_ptr(), name.len());
        };

        // Insert the new header location and return the new entry
        self.entry_locations.insert(found_index, new_location);
        Some(self.load_entry_from_location(new_location))
    }

    pub(super) fn flush(&self) {
        self.block.flush();
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
        unsafe { DirectoryEntryHeader::from_bytes(self.bytes) }
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

    fn file_type(&self) -> DirectoryEntryFileType {
        self.header().raw_file_type.into()
    }

    pub(super) fn is_file(&self) -> bool {
        self.file_type() == DirectoryEntryFileType::RegularFile
    }

    pub(super) fn is_dir(&self) -> bool {
        self.file_type() == DirectoryEntryFileType::Directory
    }
}

/// See <https://www.nongnu.org/ext2-doc/ext2.html#linked-directory-entry-structure>
#[repr(C, packed)]
#[derive(Debug, FromZeroes, FromBytes, AsBytes)]
struct DirectoryEntryHeader {
    inode: InodeNumber,
    rec_len: u16,
    name_len: u8,
    raw_file_type: u8,
}

impl DirectoryEntryHeader {
    fn new(inode: InodeNumber, name: &str, file_type: DirectoryEntryFileType) -> Self {
        let name_len = name.len() as u8;
        Self {
            inode,
            rec_len: 0,
            name_len,
            raw_file_type: file_type as u8,
        }
    }

    unsafe fn from_bytes(bytes: &[u8]) -> &Self {
        LayoutVerified::<_, Self>::new_from_prefix(bytes)
            .expect("failed to cast DirectoryEntryHeader bytes")
            .0
            .into_ref()
    }

    /// From: https://www.nongnu.org/ext2-doc/ext2.html#ifdir-rec-len
    ///
    /// The directory entries must be aligned on 4 bytes boundaries and there
    /// cannot be any directory entry spanning multiple data blocks. If an entry
    /// cannot completely fit in one block, it must be pushed to the next data
    /// block and the rec_len of the previous entry properly adjusted.
    fn required_space(&self) -> usize {
        let space = core::mem::size_of::<Self>() + self.name_len as usize;
        space.next_multiple_of(4)
    }
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) enum DirectoryEntryFileType {
    Unknown = 0,
    RegularFile = 1,
    Directory = 2,
    CharacterDevice = 3,
    BlockDevice = 4,
    Fifo = 5,
    Socket = 6,
    SymbolicLink = 7,
}

#[allow(clippy::fallible_impl_from)]
impl From<u8> for DirectoryEntryFileType {
    fn from(raw: u8) -> Self {
        match raw {
            value if value == Self::Unknown as u8 => Self::Unknown,
            value if value == Self::RegularFile as u8 => Self::RegularFile,
            value if value == Self::Directory as u8 => Self::Directory,
            value if value == Self::CharacterDevice as u8 => Self::CharacterDevice,
            value if value == Self::BlockDevice as u8 => Self::BlockDevice,
            value if value == Self::Fifo as u8 => Self::Fifo,
            value if value == Self::Socket as u8 => Self::Socket,
            value if value == Self::SymbolicLink as u8 => Self::SymbolicLink,
            _ => panic!("invalid DirectoryEntryFileType value {raw}"),
        }
    }
}
