use std::fs::File;
use std::os::unix::prelude::FileExt;

use ext2::InodeBitmap;

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

    let block_group_descriptor_0_offset =
        superblock.block_descriptor_offset(ext2::BlockGroupIndex(0));
    let block_group_descriptor_0: ext2::BlockGroupDescriptor =
        read_bytes(&mut file, block_group_descriptor_0_offset.0);
    println!("{:#X?}", block_group_descriptor_0);

    let root_inode = lookup_inode(&mut file, &superblock, ext2::ROOT_DIRECTORY);
    println!("{:#X?}", root_inode);

    let hello_inode = lookup_inode(&mut file, &superblock, ext2::InodeNumber(12));
    println!("{:#X?}", hello_inode);
}

fn read_bytes<T>(file: &mut File, offset: u64) -> T {
    let buf = read_n_bytes(file, offset, std::mem::size_of::<T>());
    unsafe { buf.as_ptr().cast::<T>().read() }
}

fn read_n_bytes(file: &mut File, offset: u64, n: usize) -> Vec<u8> {
    let mut buf = vec![0; n];
    file.read_exact_at(&mut buf, offset)
        .expect("failed to read bytes");
    buf
}

fn lookup_inode(
    file: &mut File,
    superblock: &ext2::Superblock,
    inode_number: ext2::InodeNumber,
) -> ext2::Inode {
    let (block_group_index, local_inode_index) = superblock.inode_location(inode_number);
    let block_group_offset = superblock.block_descriptor_offset(block_group_index);
    let block_group_descriptor: ext2::BlockGroupDescriptor = read_bytes(file, block_group_offset.0);

    let inode_bitmap_block_address = block_group_descriptor.inode_bitmap;
    let inode_bitmap_offset = superblock.block_address_bytes(inode_bitmap_block_address);
    let inode_bitmap_buf = read_n_bytes(
        file,
        inode_bitmap_offset.0,
        superblock.block_size().0 as usize,
    );
    let inode_bitmap = InodeBitmap(&inode_bitmap_buf);
    assert!(inode_bitmap
        .is_used(local_inode_index)
        .expect("inode doesn't exist"));

    let inode_table_block_address = block_group_descriptor.inode_table;
    let inode_offset = superblock.inode_offset(inode_table_block_address, local_inode_index);
    read_bytes(file, inode_offset.0)
}
