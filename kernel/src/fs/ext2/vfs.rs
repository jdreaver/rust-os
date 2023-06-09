use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::Debug;

use crate::block::{BlockDevice, BlockDeviceDriver};
use crate::sync::SpinLock;
use crate::vfs;

use super::directory::DirectoryEntry;
use super::file_system::FileSystem;
use super::inode::Inode;

/// VFS interface into an ext2 file system.
#[derive(Debug)]
pub(crate) struct VFSFileSystem<D> {
    reader: Arc<SpinLock<FileSystem<D>>>,
}

unsafe impl<D: BlockDeviceDriver + Send> Send for VFSFileSystem<D> {}

impl<D: BlockDeviceDriver + 'static> VFSFileSystem<D> {
    pub(crate) fn read(device: BlockDevice<D>) -> Self {
        let reader = FileSystem::read(device).expect("couldn't read ext2 filesystem!");
        let reader = Arc::new(SpinLock::new(reader));
        Self { reader }
    }
}

impl<D: Debug + BlockDeviceDriver + 'static> vfs::FileSystem for VFSFileSystem<D> {
    fn read_root(&mut self) -> vfs::Inode {
        let inode = self.reader.lock_disable_interrupts().read_root();
        let reader = self.reader.clone();
        let inode_type = if inode.is_file() {
            vfs::InodeType::File(Box::new(EXT2FileInode { reader, inode }))
        } else if inode.is_dir() {
            vfs::InodeType::Directory(Box::new(EXT2DirectoryInode { reader, inode }))
        } else {
            panic!("unexpected inode type: {:?}", inode);
        };
        vfs::Inode { inode_type }
    }
}

#[derive(Debug)]
// TODO: Perhaps combine EXT2FileNode with EXT2DirectoryNode? They are both just
// inodes. We can use assertions to ensure we picked the right one if needed.
// (Same with DirectoryEntry below)
pub(crate) struct EXT2FileInode<D> {
    reader: Arc<SpinLock<FileSystem<D>>>,
    inode: Inode,
}

impl<D: Debug + BlockDeviceDriver + 'static> vfs::FileInode for EXT2FileInode<D> {
    fn read(&mut self) -> Vec<u8> {
        let mut data = Vec::new();
        self.reader
            .lock_disable_interrupts()
            .iter_file_data(&self.inode, |block_data| {
                data.extend(block_data);
            });
        data
    }

    fn write(&mut self, data: &[u8]) -> bool {
        let mut lock = self.reader.lock_disable_interrupts();

        let mut written_bytes = 0;
        lock.iter_file_blocks(&self.inode, |block_buf| {
            if written_bytes >= data.len() {
                return; // TODO: Allow early termination
            }

            let block_data = block_buf.data_mut();
            for byte in block_data.iter_mut() {
                *byte = if written_bytes < data.len() {
                    written_bytes += 1;
                    data[written_bytes - 1]
                } else {
                    0
                };
            }
            block_buf.flush();
        });

        assert!(
            written_bytes == data.len(),
            "couldn't write all bytes to file! implement adding new blocks"
        );

        true
    }
}

#[derive(Debug)]
pub(crate) struct EXT2DirectoryInode<D> {
    reader: Arc<SpinLock<FileSystem<D>>>,
    inode: Inode,
}

impl<D: Debug + BlockDeviceDriver + 'static> vfs::DirectoryInode for EXT2DirectoryInode<D> {
    fn subdirectories(&mut self) -> Vec<Box<dyn vfs::DirectoryEntry>> {
        let mut entries: Vec<Box<dyn vfs::DirectoryEntry>> = Vec::new();
        self.reader
            .lock_disable_interrupts()
            .iter_directory(&self.inode, |entry| {
                let reader = self.reader.clone();
                entries.push(Box::new(EXT2DirectoryEntry { reader, entry }));
                true
            });
        entries
    }
}

#[derive(Debug)]
pub(crate) struct EXT2DirectoryEntry<D> {
    reader: Arc<SpinLock<FileSystem<D>>>,
    entry: DirectoryEntry,
}

impl<D: Debug + BlockDeviceDriver + 'static> vfs::DirectoryEntry for EXT2DirectoryEntry<D> {
    fn name(&self) -> String {
        String::from(&self.entry.name)
    }

    fn entry_type(&self) -> vfs::DirectoryEntryType {
        if self.entry.is_file() {
            vfs::DirectoryEntryType::File
        } else if self.entry.is_dir() {
            vfs::DirectoryEntryType::Directory
        } else {
            panic!("unexpected directory entry type: {:?}", self.entry);
        }
    }

    fn get_inode(&mut self) -> vfs::Inode {
        let inode_number = self.entry.header.inode;
        let Some(inode) = self.reader.lock_disable_interrupts().read_inode(inode_number) else {
            panic!("couldn't read inode {inode_number:?} inside EXT2DiretoryEntry::get_inode");
        };
        let reader = self.reader.clone();
        let inode_type = if inode.is_file() {
            vfs::InodeType::File(Box::new(EXT2FileInode { reader, inode }))
        } else if inode.is_dir() {
            vfs::InodeType::Directory(Box::new(EXT2DirectoryInode { reader, inode }))
        } else {
            panic!("unexpected inode type: {:?}", inode);
        };
        vfs::Inode { inode_type }
    }
}
