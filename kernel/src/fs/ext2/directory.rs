use alloc::string::String;

use super::InodeNumber;

/// See <https://www.nongnu.org/ext2-doc/ext2.html#linked-directories>
#[derive(Debug, Clone)]
pub(crate) struct DirectoryBlock<'a>(pub(crate) &'a [u8]);

impl DirectoryBlock<'_> {
    pub(crate) fn iter(&self) -> DirectoryBlockIterator<'_> {
        DirectoryBlockIterator {
            block: self.clone(),
            offset: 0,
        }
    }
}

pub(crate) struct DirectoryBlockIterator<'a> {
    block: DirectoryBlock<'a>,
    offset: usize,
}

impl Iterator for DirectoryBlockIterator<'_> {
    type Item = DirectoryEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.block.0.len() {
            return None;
        }

        let header = unsafe {
            let ptr = self.block.0.as_ptr().add(self.offset);
            ptr.cast::<DirectoryEntryHeader>().read()
        };

        let name_start = self.offset + core::mem::size_of::<DirectoryEntryHeader>();
        let name_end = name_start + header.name_len as usize;
        let name_slice = &self.block.0[name_start..name_end];
        let name = core::str::from_utf8(name_slice).unwrap_or("<invalid UTF-8>");
        let name = String::from(name);

        self.offset += header.rec_len as usize;

        let entry = DirectoryEntry { header, name };

        Some(entry)
    }
}

#[derive(Debug)]
pub(crate) struct DirectoryEntry {
    pub(crate) header: DirectoryEntryHeader,
    pub(crate) name: String,
}

impl DirectoryEntry {
    pub(crate) fn is_file(&self) -> bool {
        self.header.file_type == DirectoryEntryFileType::RegularFile
    }

    pub(crate) fn is_dir(&self) -> bool {
        self.header.file_type == DirectoryEntryFileType::Directory
    }
}

/// See <https://www.nongnu.org/ext2-doc/ext2.html#linked-directory-entry-structure>
#[repr(C, packed)]
#[derive(Debug)]
pub(crate) struct DirectoryEntryHeader {
    pub(crate) inode: InodeNumber,
    pub(crate) rec_len: u16,
    pub(crate) name_len: u8,
    pub(crate) file_type: DirectoryEntryFileType,
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum DirectoryEntryFileType {
    Unknown = 0,
    RegularFile = 1,
    Directory = 2,
    CharacterDevice = 3,
    BlockDevice = 4,
    Fifo = 5,
    Socket = 6,
    SymbolicLink = 7,
}
