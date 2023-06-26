use core::fmt::Debug;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::sync::{Mutex, MutexGuard};

use super::FilePath;

static MOUNTED_ROOT_FILE_SYSTEM: Mutex<Option<Box<dyn FileSystem + Send>>> = Mutex::new(None);

pub(crate) fn mount_root_filesystem(fs: Box<dyn FileSystem + Send>) {
    MOUNTED_ROOT_FILE_SYSTEM.lock().replace(fs);
}

pub(crate) fn unmount_root_filesystem() {
    MOUNTED_ROOT_FILE_SYSTEM.lock().take();
}

pub(crate) fn root_filesystem_lock(
) -> MutexGuard<'static, Option<Box<dyn FileSystem + Send + 'static>>> {
    MOUNTED_ROOT_FILE_SYSTEM.lock()
}

/// Top level VFS abstraction for an underlying filesystem.
pub(crate) trait FileSystem {
    fn read_root(&mut self) -> Inode;

    fn traverse_path(&mut self, path: &FilePath) -> Option<Inode> {
        let mut inode = self.read_root();
        for component in &path.components {
            let InodeType::Directory(mut dir) = inode.inode_type else {
                log::warn!("traverse_path: expected directory but found {:?}", inode.inode_type);
                return None;
            };

            let mut entry = dir
                .subdirectories()
                .into_iter()
                .find(|entry| entry.name() == component.as_str())?;
            inode = entry.get_inode();
        }
        Some(inode)
    }
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
    fn read(&mut self, buffer: &mut [u8], offset: usize) -> FileInodeReadResult;

    fn read_all(&mut self) -> Vec<u8> {
        let mut buffer = Vec::new();
        let mut offset = 0;
        loop {
            let mut read_buffer = vec![0u8; 4096];
            match self.read(&mut read_buffer, offset) {
                FileInodeReadResult::Success => {
                    buffer.extend_from_slice(&read_buffer);
                    offset += read_buffer.len();
                }
                FileInodeReadResult::Done { bytes_read } => {
                    buffer.extend_from_slice(&read_buffer[..bytes_read]);
                    break;
                }
            }
        }
        buffer
    }

    fn write(&mut self, _data: &[u8]) -> bool {
        false
    }
}

pub(crate) enum FileInodeReadResult {
    /// Success means entire buffer was filled.
    Success,

    /// Done means the file has been fully read. Not all bytes may have been
    /// read, so we return how many were read.
    Done { bytes_read: usize },
}

pub(crate) trait DirectoryInode: Debug {
    // TODO: Return an iterator instead of a Vec (probably a dyn for some
    // iterator type to avoid an impl in the return position).
    fn subdirectories(&mut self) -> Vec<Box<dyn DirectoryEntry>>;

    fn create_file(&mut self, _name: &str) -> Option<Box<dyn FileInode>> {
        log::warn!("create_file: not implemented for {:?}", self);
        None
    }
}

pub(crate) trait DirectoryEntry: Debug {
    fn name(&self) -> String;
    fn entry_type(&self) -> DirectoryEntryType;
    fn get_inode(&mut self) -> Inode;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum DirectoryEntryType {
    File,
    Directory,
}

pub(crate) fn get_path_inode(path: &FilePath) -> Result<Inode, String> {
    let mut lock = root_filesystem_lock();
    let Some(filesystem) = lock.as_mut() else {
        return Err(String::from("No filesystem mounted. Run 'mount <device_id>' first."));
    };
    if !path.absolute {
        return Err(format!("Path must be absolute. Got {path}"));
    }

    let Some(inode) = filesystem.traverse_path(path) else {
        return Err(format!("No such file or directory: {path}"));
    };
    Ok(inode)
}
