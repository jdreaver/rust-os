use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;

use crate::sched::TaskId;
use crate::{sched, vfs};

#[derive(Debug)]
pub(crate) struct Sysfs;

impl vfs::FileSystem for Sysfs {
    fn read_root(&mut self) -> vfs::Inode {
        vfs::Inode {
            inode_type: vfs::InodeType::Directory(Box::new(VFSRootInode)),
        }
    }
}

#[derive(Debug)]
struct VFSRootInode;

impl vfs::DirectoryInode for VFSRootInode {
    fn subdirectories(&mut self) -> alloc::vec::Vec<alloc::boxed::Box<dyn vfs::DirectoryEntry>> {
        vec![Box::new(VFSTasksDirectory)]
    }
}

/// Holds a subdirectory per running task.
#[derive(Debug)]
struct VFSTasksDirectory;

impl vfs::DirectoryEntry for VFSTasksDirectory {
    fn name(&self) -> String {
        String::from("tasks")
    }

    fn entry_type(&self) -> vfs::DirectoryEntryType {
        vfs::DirectoryEntryType::Directory
    }

    fn get_inode(&mut self) -> vfs::Inode {
        vfs::Inode {
            inode_type: vfs::InodeType::Directory(Box::new(Self)),
        }
    }
}

impl vfs::DirectoryInode for VFSTasksDirectory {
    fn subdirectories(&mut self) -> alloc::vec::Vec<alloc::boxed::Box<dyn vfs::DirectoryEntry>> {
        sched::TASKS
            .lock()
            .task_ids()
            .into_iter()
            .map(|task_id| Box::new(VFSTaskDirectory { task_id }) as Box<dyn vfs::DirectoryEntry>)
            .collect()
    }
}

/// Subdirectory for a specific task.
#[derive(Debug, Clone)]
struct VFSTaskDirectory {
    task_id: TaskId,
}

impl vfs::DirectoryEntry for VFSTaskDirectory {
    fn name(&self) -> String {
        format!("{}", u32::from(self.task_id))
    }

    fn entry_type(&self) -> vfs::DirectoryEntryType {
        vfs::DirectoryEntryType::Directory
    }

    fn get_inode(&mut self) -> vfs::Inode {
        vfs::Inode {
            inode_type: vfs::InodeType::Directory(Box::new(self.clone())),
        }
    }
}

impl vfs::DirectoryInode for VFSTaskDirectory {
    fn subdirectories(&mut self) -> alloc::vec::Vec<alloc::boxed::Box<dyn vfs::DirectoryEntry>> {
        vec![Box::new(VFSTaskInfoFile {
            task_id: self.task_id,
        })]
    }
}

/// General info about a task
#[derive(Debug)]
struct VFSTaskInfoFile {
    task_id: TaskId,
}

impl VFSTaskInfoFile {
    fn data(&self) -> String {
        sched::TASKS
            .lock_disable_interrupts()
            .get_task(self.task_id)
            .map_or_else(
                || format!("task not found..."),
                |task| format!("{:#X?}", task),
            )
    }
}

impl vfs::DirectoryEntry for VFSTaskInfoFile {
    fn name(&self) -> String {
        String::from("info")
    }

    fn entry_type(&self) -> vfs::DirectoryEntryType {
        vfs::DirectoryEntryType::File
    }

    fn get_inode(&mut self) -> vfs::Inode {
        vfs::Inode {
            inode_type: vfs::InodeType::File(Box::new(Self {
                task_id: self.task_id,
            })),
        }
    }
}

impl vfs::FileInode for VFSTaskInfoFile {
    fn read(&mut self, buffer: &mut [u8], offset: usize) -> vfs::FileInodeReadResult {
        sysfs_read_file(&self.data(), buffer, offset)
    }
}

/// Generic code to implement a sysfs file read that just reads from a string.
fn sysfs_read_file(
    file_content: &str,
    buffer: &mut [u8],
    offset: usize,
) -> vfs::FileInodeReadResult {
    let data = file_content.as_bytes();
    let start = offset.min(data.len());
    let end = (offset + buffer.len()).min(data.len());
    let copy_data = &data[start..end];
    buffer[..copy_data.len()].copy_from_slice(copy_data);
    if end == data.len() {
        vfs::FileInodeReadResult::Done {
            bytes_read: file_content.len(),
        }
    } else {
        vfs::FileInodeReadResult::Success
    }
}
