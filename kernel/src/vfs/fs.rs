use core::fmt::Debug;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use super::FilePath;

/// Top level VFS abstraction for an underlying filesystem.
pub(crate) trait FileSystem {
    fn read_root(&mut self) -> Inode;

    // fn traverse_path(&mut self, path: &FilePath) -> Option<Inode> {
    //     let mut inode = self.read_root();
    //     for component in &path.components {
    //         let mut found_inode_number = None;
    //         self.iter_directory(&*inode, |entry| {
    //             if entry.name() == component.as_str() {
    //                 found_inode_number.replace(entry.inode_number());
    //                 return false;
    //             }
    //             true
    //         });
    //         if let Some(found_inode_number) = found_inode_number {
    //             inode = self
    //                 .read_inode(found_inode_number)
    //                 .expect("ERROR: found inode {found_inode_number} but failed to read it");
    //         } else {
    //             return None;
    //         }
    //     }
    //     Some(inode)
    // }
}

#[derive(Debug)]
pub(crate) struct Inode {
    pub(crate) inode_type: InodeType,
}

#[derive(Debug)]
pub(crate) enum InodeType {
    File(Box<dyn FileInode>),
    Directory(Box<dyn DirectoryInode>),
}

pub(crate) trait FileInode: Debug {
    fn read(&mut self) -> Vec<u8>;
}

pub(crate) trait DirectoryInode: Debug {
    fn subdirectories(&mut self) -> Vec<DirectoryEntry>;
}

#[derive(Debug)]
pub(crate) struct DirectoryEntry {
    pub(crate) name: String,
    pub(crate) entry_type: DirectoryEntryType,
}

#[derive(Debug)]
pub(crate) enum DirectoryEntryType {
    File(Box<dyn FileDirectoryEntry>),
    Directory(Box<dyn DirectoryDirectoryEntry>),
}

pub(crate) trait FileDirectoryEntry: Debug {}

pub(crate) trait DirectoryDirectoryEntry: Debug {}

// /// Top level VFS abstraction for an underlying filesystem.
// pub(crate) trait FileSystem {
//     fn superblock(&self) -> Box<dyn Superblock>;
//     fn read_root(&mut self) -> Box<dyn Inode>;
//     fn read_inode(&mut self, inode_number: InodeNumber) -> Option<Box<dyn Inode>>;
//     fn iter_directory<F>(&mut self, inode: &dyn Inode, func: F)
//     where
//         F: FnMut(Box<dyn DirectoryEntry>) -> bool;

//     fn traverse_path<R: ext2::BlockReader>(&mut self, path: &FilePath) -> Option<Box<dyn Inode>> {
//         let mut inode = self.read_root();
//         for component in &path.components {
//             let mut found_inode_number = None;
//             self.iter_directory(&*inode, |entry| {
//                 if entry.name() == component.as_str() {
//                     found_inode_number.replace(entry.inode_number());
//                     return false;
//                 }
//                 true
//             });
//             if let Some(found_inode_number) = found_inode_number {
//                 inode = self
//                     .read_inode(found_inode_number)
//                     .expect("ERROR: found inode {found_inode_number} but failed to read it");
//             } else {
//                 return None;
//             }
//         }
//         Some(inode)
//     }
// }

// pub(crate) trait Superblock: Debug + Any {}

// pub(crate) trait Inode: Debug + Any {}

// pub(crate) trait DirectoryEntry {
//     fn name(&self) -> &str;
//     fn inode_number(&self) -> InodeNumber;
// }

// /// "Global" inode number within the filesystem.
// #[derive(Debug, Copy, Clone)]
// pub struct InodeNumber(pub u32);
