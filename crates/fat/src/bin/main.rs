use std::fs::File;

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
    println!("Reading FAT disk file: {}", disk_file);
    let file = File::open(disk_file).expect("failed to open disk file");
    let mut reader = genio::std_impls::GenioIo::new(file);
    let bios_param_block: fat::BIOSParameterBlock =
        fat::zero_copy_read(&mut reader).expect("failed to read BIOS parameter block");

    println!("BIOS parameter block: {:#X?}", bios_param_block);
}
