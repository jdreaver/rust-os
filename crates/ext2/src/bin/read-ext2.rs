use std::fs::File;
use std::os::unix::prelude::FileExt;

pub struct FileBlockReader(File);

impl ext2::BlockReader for FileBlockReader {
    fn read_num_bytes(&mut self, addr: ext2::OffsetBytes, num_bytes: usize) -> Vec<u8> {
        let mut buf = vec![0; num_bytes];
        self.0
            .read_exact_at(&mut buf, addr.0)
            .expect("failed to read bytes");
        buf
    }
}

fn main() {
    // Get command line arguments, including a FAT disk file
    let args: Vec<String> = std::env::args().collect();
    let disk_file = match &args[..] {
        [_, disk_file] => disk_file,
        _ => {
            eprintln!("Usage: {} <disk_file>", args[0]);
            std::process::exit(1);
        }
    };

    // Read a FAT disk file
    println!("Reading ext2 disk file: {}", disk_file);
    let file = File::open(disk_file).expect("failed to open disk file");
    let block_reader = FileBlockReader(file);
    let mut reader = ext2::FilesystemReader::read(block_reader).expect("failed to read disk file");

    let superblock = reader.superblock();
    println!("{:#X?}", superblock);
    println!("Num block groups: {}", superblock.num_block_groups());
    println!("Block size: {:#X?}", superblock.block_size());

    let root_inode = reader.read_root();
    println!("{:#X?}", root_inode);
    reader.iter_directory(&root_inode, |dir_entry| {
        println!("{:#X?}", dir_entry);
    });

    let hello_inode = reader.read_inode(ext2::InodeNumber(12)).expect("failed to find hello");
    println!("{:#X?}", hello_inode);

    print!("hello content: ");
    reader.iter_file_blocks(&hello_inode, |blocks| {
        print!("{}", String::from_utf8_lossy(&blocks));
    });
    println!();
}
