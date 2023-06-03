use std::fs::File;
use std::os::unix::prelude::FileExt;

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

    let superblock: ext2::Superblock = read_bytes(&mut file, ext2::Superblock::OFFSET_BYTES as u64);
    println!("{:#X?}", superblock);
    println!("Num block groups: {}", superblock.num_block_groups());
    println!("Block size: {:#X?}", superblock.block_size());

    let block_group_descriptor_0: ext2::BlockGroupDescriptor =
        read_bytes(&mut file, superblock.block_descriptor_offset(0).0);
    println!("{:#X?}", block_group_descriptor_0);

    let block_group_descriptor_1: ext2::BlockGroupDescriptor =
        read_bytes(&mut file, superblock.block_descriptor_offset(1).0);
    println!("{:#X?}", block_group_descriptor_1);
}

fn read_bytes<T>(file: &mut File, offset: u64) -> T {
    let mut buf = vec![0; std::mem::size_of::<T>()];
    file.read_exact_at(&mut buf, offset)
        .expect("failed to read bytes");
    unsafe { buf.as_ptr().cast::<T>().read() }
}
