use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

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
    let mut file = File::open(disk_file).expect("failed to open disk file");
    let seek = SeekFrom::Start(ext2::Superblock::OFFSET_BYTES as u64);
    file.seek(seek).expect("failed to seek to superblock");

    let mut bytes: [u8; 1024] = [0; 1024];
    file.read_exact(&mut bytes)
        .expect("failed to read superblock bytes");

    let superblock: ext2::Superblock = unsafe { bytes.as_ptr().cast::<ext2::Superblock>().read() };
    println!("{:#X?}", superblock);
    println!("Block size: {}", superblock.block_size());
}
