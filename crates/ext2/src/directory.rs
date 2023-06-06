use crate::InodeNumber;

/// See <https://www.nongnu.org/ext2-doc/ext2.html#linked-directories>
#[derive(Debug, Clone)]
pub struct DirectoryBlock<'a>(pub &'a [u8]);

impl DirectoryBlock<'_> {
    pub fn iter(&self) -> DirectoryBlockIterator<'_> {
        DirectoryBlockIterator {
            block: self.clone(),
            offset: 0,
        }
    }
}

pub struct DirectoryBlockIterator<'a> {
    block: DirectoryBlock<'a>,
    offset: usize,
}

impl<'a> Iterator for DirectoryBlockIterator<'a> {
    type Item = DirectoryEntry<'a>;

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

        self.offset += header.rec_len as usize;

        let entry = DirectoryEntry { header, name };

        Some(entry)
    }
}

#[derive(Debug)]
pub struct DirectoryEntry<'a> {
    pub header: DirectoryEntryHeader,
    pub name: &'a str,
}

impl DirectoryEntry<'_> {
    pub fn is_dir(&self) -> bool {
        self.header.file_type == DirectoryEntryFileType::Directory
    }

    pub fn is_dot(&self) -> bool {
        self.name == "."
    }

    pub fn is_dotdot(&self) -> bool {
        self.name == ".."
    }
}

/// See <https://www.nongnu.org/ext2-doc/ext2.html#linked-directory-entry-structure>
#[repr(C, packed)]
#[derive(Debug)]
pub struct DirectoryEntryHeader {
    pub inode: InodeNumber,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: DirectoryEntryFileType,
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DirectoryEntryFileType {
    Unknown = 0,
    RegularFile = 1,
    Directory = 2,
    CharacterDevice = 3,
    BlockDevice = 4,
    Fifo = 5,
    Socket = 6,
    SymbolicLink = 7,
}
