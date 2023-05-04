use zerocopy::{AsBytes, FromBytes};

/// The BIOS parameter block is the first part of the boot sector.
#[derive(Debug, AsBytes, FromBytes)]
#[repr(C, packed)]
pub struct BIOSParameterBlock {
    jmp_boot: [u8; 3],
    oem_name: [u8; 8],
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    fat_count: u8,
    root_dir_entries: u16,
    total_sectors: u16,
    media_descriptor: u8,
    sectors_per_fat: u16,
    sectors_per_track: u16,
    head_count: u16,
    hidden_sectors: u32,
    total_sectors_large: u32,
}
