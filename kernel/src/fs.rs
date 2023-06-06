use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;
use core::fmt::Debug;

use crate::sync::SpinLock;
use crate::{vfs, virtio};

#[derive(Debug)]
pub(crate) struct EXT2FileSystem<R> {
    reader: Arc<SpinLock<ext2::FilesystemReader<R>>>,
}

impl<R: ext2::BlockReader> EXT2FileSystem<R> {
    pub(crate) fn read(reader: R) -> Self {
        let reader = ext2::FilesystemReader::read(reader).expect("couldn't read ext2 filesystem!");
        let reader = Arc::new(SpinLock::new(reader));
        Self { reader }
    }
}

impl<R: Debug + ext2::BlockReader + 'static> vfs::FileSystem for EXT2FileSystem<R> {
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
pub(crate) struct EXT2FileInode<R> {
    reader: Arc<SpinLock<ext2::FilesystemReader<R>>>,
    inode: ext2::Inode,
}

impl<R: Debug + ext2::BlockReader> vfs::FileInode for EXT2FileInode<R> {
    fn read(&mut self) -> Vec<u8> {
        let mut data = Vec::new();
        self.reader
            .lock_disable_interrupts()
            .iter_file_blocks(&self.inode, |block| {
                data.extend(block);
            });
        data
    }
}

#[derive(Debug)]
pub(crate) struct EXT2DirectoryInode<R> {
    reader: Arc<SpinLock<ext2::FilesystemReader<R>>>,
    inode: ext2::Inode,
}

impl<R: Debug + ext2::BlockReader + 'static> vfs::DirectoryInode for EXT2DirectoryInode<R> {
    fn subdirectories(&mut self) -> Vec<vfs::DirectoryEntry> {
        let mut entries: Vec<vfs::DirectoryEntry> = Vec::new();
        self.reader
            .lock_disable_interrupts()
            .iter_directory(&self.inode, |entry| {
                let name = &entry.name;
                let name = String::from(name);
                let reader = self.reader.clone();
                let entry_type: vfs::DirectoryEntryType = if entry.is_file() {
                    vfs::DirectoryEntryType::File(Box::new(EXT2FileDirectoryEntry {
                        reader,
                        entry,
                    }))
                } else if entry.is_dir() {
                    vfs::DirectoryEntryType::Directory(Box::new(EXT2DirectoryDirectoryEntry {
                        reader,
                        entry,
                    }))
                } else {
                    panic!("unexpected directory entry type: {:?}", entry);
                };
                entries.push(vfs::DirectoryEntry { name, entry_type });
                true
            });
        entries

        // self.reader
        //     .lock_disable_interrupts()
        //     .iter_directory(&self.inode, |entry| { entries.push(entry); true });

        // entries.into_iter().map(|entry| {
        //         let name = &entry.name;
        //         let name = String::from(name);
        //         let entry_type = if entry.is_file() {
        //             vfs::DirectoryEntryType::File(Box::new(EXT2FileDirectoryEntry {
        //                 reader: self.reader.clone(),
        //                 entry,
        //             }))
        //         } else if entry.is_dir() {
        //             vfs::DirectoryEntryType::Directory(Box::new(EXT2DirectoryDirectoryEntry {
        //                 reader: self.reader.clone(),
        //                 entry,
        //             }))
        //         } else {
        //             panic!("unexpected directory entry type: {:?}", entry);
        //         };
        //         vfs::DirectoryEntry { name, entry_type }
        //     })
        //     .collect()
    }
}

#[derive(Debug)]
pub(crate) struct EXT2FileDirectoryEntry<R> {
    reader: Arc<SpinLock<ext2::FilesystemReader<R>>>,
    entry: ext2::DirectoryEntry,
}

impl<R: Debug + ext2::BlockReader> vfs::FileDirectoryEntry for EXT2FileDirectoryEntry<R> {}

#[derive(Debug)]
pub(crate) struct EXT2DirectoryDirectoryEntry<R> {
    reader: Arc<SpinLock<ext2::FilesystemReader<R>>>,
    entry: ext2::DirectoryEntry,
}

impl<R: Debug + ext2::BlockReader> vfs::DirectoryDirectoryEntry for EXT2DirectoryDirectoryEntry<R> {}

#[derive(Debug)]
pub(crate) struct VirtioBlockReader {
    device_id: usize,
}

impl VirtioBlockReader {
    pub(crate) fn new(device_id: usize) -> Self {
        Self { device_id }
    }
}

impl ext2::BlockReader for VirtioBlockReader {
    fn read_num_bytes(&mut self, addr: ext2::OffsetBytes, num_bytes: usize) -> Vec<u8> {
        let sector = addr.0 / u64::from(virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES);
        let sector_offset = addr.0 as usize % virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES as usize;

        let total_bytes = sector_offset + num_bytes;
        let num_sectors = total_bytes.div_ceil(virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES as usize);

        let response =
            virtio::virtio_block_read(self.device_id, sector, num_sectors as u32).wait_sleep();
        let virtio::VirtIOBlockResponse::Read{ ref data } = response else {
            panic!("unexpected virtio block response: {:?}", response);
        };

        let mut data = data.clone();
        data.drain(0..sector_offset);
        data.drain(num_bytes..);
        data
    }
}
