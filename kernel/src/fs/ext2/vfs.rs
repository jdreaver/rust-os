use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Debug;

use crate::block::{BlockDevice, BlockDeviceDriver};
use crate::sync::SpinLock;
use crate::vfs;

use super::file_system::FileSystem;
use super::inode::Inode;
use super::superblock::{InodeNumber, OffsetBytes};

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
        let (inode, inode_number) = self.reader.lock_disable_interrupts().read_root();
        let reader = self.reader.clone();
        let vfs_inode = Box::new(VFSInode {
            reader,
            inode_number,
            inode,
        });
        let inode_type = if vfs_inode.inode.is_file() {
            vfs::InodeType::File(vfs_inode)
        } else if vfs_inode.inode.is_dir() {
            vfs::InodeType::Directory(vfs_inode)
        } else {
            panic!("unexpected inode type: {:?}", vfs_inode.inode);
        };
        vfs::Inode { inode_type }
    }
}

#[derive(Debug)]
pub(crate) struct VFSInode<D> {
    reader: Arc<SpinLock<FileSystem<D>>>,
    inode_number: InodeNumber,
    inode: Inode,
}

impl<D: Debug + BlockDeviceDriver + 'static> vfs::FileInode for VFSInode<D> {
    fn read(&mut self) -> Vec<u8> {
        assert!(
            self.inode.is_file(),
            "expected file inode but found {:?}",
            self.inode
        );

        let mut reader = self.reader.lock_disable_interrupts();

        let block_size = usize::from(u16::from(reader.superblock().block_size()));
        let file_size = reader.superblock().inode_size(&self.inode) as usize;
        let mut data = vec![0; file_size];
        reader.iter_file_blocks(&self.inode, |OffsetBytes(start_bytes), block_buf| {
            let start_bytes = start_bytes as usize;
            let slice_end = file_size - start_bytes;
            let slice_end = core::cmp::min(slice_end, block_size);
            let end_bytes = start_bytes + slice_end;

            let block_data = block_buf.data();
            data[start_bytes..end_bytes].copy_from_slice(&block_data[..slice_end]);

            true
        });
        data
    }

    fn write(&mut self, data: &[u8]) -> bool {
        assert!(
            self.inode.is_file(),
            "expected file inode but found {:?}",
            self.inode
        );

        let mut lock = self.reader.lock_disable_interrupts();

        let mut written_bytes = 0;
        lock.iter_file_blocks(&self.inode, |_, mut block_buf| {
            if written_bytes >= data.len() {
                return false;
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
            true
        });

        assert!(
            written_bytes == data.len(),
            "couldn't write all bytes to file! implement adding new blocks"
        );

        // Write inode back
        self.inode.size_low = data.len() as u32;
        lock.write_inode(self.inode.clone(), self.inode_number);

        true
    }
}

impl<D: Debug + BlockDeviceDriver + 'static> vfs::DirectoryInode for VFSInode<D> {
    fn subdirectories(&mut self) -> Vec<Box<dyn vfs::DirectoryEntry>> {
        assert!(
            self.inode.is_dir(),
            "expected directory inode but found {:?}",
            self.inode
        );

        let mut entries: Vec<Box<dyn vfs::DirectoryEntry>> = Vec::new();
        self.reader
            .lock_disable_interrupts()
            .iter_directory_blocks(&self.inode, |block| {
                for entry in block.iter() {
                    let reader = self.reader.clone();
                    let entry_type = if entry.is_file() {
                        vfs::DirectoryEntryType::File
                    } else if entry.is_dir() {
                        vfs::DirectoryEntryType::Directory
                    } else {
                        panic!("unexpected directory entry type: {:?}", entry);
                    };
                    entries.push(Box::new(EXT2DirectoryEntry {
                        reader,
                        inode_number: entry.inode_number(),
                        name: String::from(entry.name()),
                        entry_type,
                    }));
                }
                true
            });
        entries
    }

    fn create_file(&mut self, name: &str) -> Option<Box<dyn vfs::FileInode>> {
        assert!(
            self.inode.is_dir(),
            "expected directory inode but found {:?}",
            self.inode
        );

        let mut lock = self.reader.lock_disable_interrupts();
        let (inode, inode_number) = lock.create_file(&self.inode, self.inode_number, name)?;
        let reader = self.reader.clone();
        Some(Box::new(Self {
            reader,
            inode_number,
            inode,
        }))
    }
}

#[derive(Debug)]
pub(crate) struct EXT2DirectoryEntry<D> {
    reader: Arc<SpinLock<FileSystem<D>>>,
    inode_number: InodeNumber,
    name: String,
    entry_type: vfs::DirectoryEntryType,
}

impl<D: Debug + BlockDeviceDriver + 'static> vfs::DirectoryEntry for EXT2DirectoryEntry<D> {
    fn name(&self) -> String {
        String::from(&self.name)
    }

    fn entry_type(&self) -> vfs::DirectoryEntryType {
        self.entry_type
    }

    fn get_inode(&mut self) -> vfs::Inode {
        let inode_number = self.inode_number;
        let Some(inode) = self.reader.lock_disable_interrupts().read_inode(inode_number) else {
            panic!("couldn't read inode {inode_number:?} inside EXT2DiretoryEntry::get_inode");
        };
        let reader = self.reader.clone();
        let vfs_inode = Box::new(VFSInode {
            reader,
            inode_number,
            inode,
        });
        let inode_type = if vfs_inode.inode.is_file() {
            vfs::InodeType::File(vfs_inode)
        } else if vfs_inode.inode.is_dir() {
            vfs::InodeType::Directory(vfs_inode)
        } else {
            panic!("unexpected inode type: {:?}", vfs_inode.inode);
        };
        vfs::Inode { inode_type }
    }
}
