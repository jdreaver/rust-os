use alloc::vec::Vec;
use core::fmt;

use uefi::table::{Runtime, SystemTable};

use crate::hpet::Milliseconds;
use crate::sync::SpinLock;
use crate::{
    acpi, boot_info, pci, sched, serial, serial_print, serial_println, tests, tick, virtio,
};

static NEXT_COMMAND_BUFFER: SpinLock<ShellBuffer> = SpinLock::new(ShellBuffer::new());

struct ShellBuffer {
    buffer: Vec<u8>,
}

impl ShellBuffer {
    const MAX_SIZE: usize = 512;

    const fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    fn add_char(&mut self, c: u8) -> bool {
        if self.buffer.len() < Self::MAX_SIZE - 1 {
            self.buffer.push(c);
            true
        } else {
            false
        }
    }

    fn delete_char(&mut self) -> bool {
        self.buffer.pop().is_some()
    }

    fn clear(&mut self) {
        self.buffer.clear();
    }

    fn redraw_buffer(&self) {
        reset_terminal_line();
        serial_print!("ksh > ");
        for c in &self.buffer {
            serial::serial1_write_byte(*c);
        }
    }
}

pub(crate) extern "C" fn run_serial_shell(_arg: *const ()) {
    serial_println!("Welcome to Rust OS! Here is a shell for you to use.");
    let mut buffer = NEXT_COMMAND_BUFFER.lock();

    loop {
        buffer.redraw_buffer();
        loop {
            let c = serial::serial1_read_byte();
            match c {
                b'\n' | b'\r' => {
                    serial_println!();
                    let command = next_command(&buffer.buffer);
                    if let Some(command) = command {
                        run_command(&command);
                    }
                    buffer.clear();
                    break;
                }
                // DEL char
                127 => {
                    if buffer.delete_char() {
                        // Send backspace, space, and backspace again
                        serial::serial1_write_byte(8);
                        serial::serial1_write_byte(b' ');
                        serial::serial1_write_byte(8);
                    }
                }
                // Printable char range
                32..=126 => {
                    if buffer.add_char(c) {
                        serial::serial1_write_byte(c);
                    }
                }
                // End of text
                3 => {
                    serial_println!("^C");
                    buffer.clear();
                    break;
                }
                _ => {
                    reset_terminal_line();
                    serial_println!("Don't know what to do with ASCII char: {c}");
                    break;
                }
            }
        }
    }
}

fn reset_terminal_line() {
    // Clears line (with ESC[2K) and returns cursor to start of line (with \r)
    serial::serial1_write_bytes(b"\x1B[2K\r");
}

enum Command<'a> {
    TestMisc,
    TestHPET,
    ListPCI,
    ListVirtIO,
    BootInfo,
    PrintACPI,
    RNG(u32),
    VirtIOBlockList,
    VirtIOBlockRead { device_id: usize, sector: u64 },
    VirtIOBlockID { device_id: usize },
    FATBIOS { device_id: usize },
    EXT2Superblock { device_id: usize },
    Timer(Milliseconds),
    Sleep(Milliseconds),
    PrimeSync(usize),
    PrimeAsync(usize),
    Invalid,
    Unknown(&'a str),
}

fn next_command(buffer: &[u8]) -> Option<Command> {
    let command_str = core::str::from_utf8(buffer);
    let Ok(command_str) = command_str else { return Some(Command::Invalid); };

    let words = command_str.split_whitespace().collect::<Vec<_>>();

    match &words[..] {
        [] => None,
        ["test", "misc"] => Some(Command::TestMisc),
        ["test", "hpet"] => Some(Command::TestHPET),
        ["list-pci"] => Some(Command::ListPCI),
        ["list-virtio"] => Some(Command::ListVirtIO),
        ["boot-info"] => Some(Command::BootInfo),
        ["print-acpi"] => Some(Command::PrintACPI),
        ["rng", num_bytes_str] => {
            let num_bytes = parse_or_print_error(num_bytes_str, "number of bytes")?;
            Some(Command::RNG(num_bytes))
        }
        ["virtio-block", "list"] => Some(Command::VirtIOBlockList),
        ["virtio-block", "read", device_id_str, sector_str] => {
            let device_id = parse_or_print_error(device_id_str, "device ID")?;
            let sector = parse_or_print_error(sector_str, "sector number")?;
            Some(Command::VirtIOBlockRead { device_id, sector })
        }
        ["virtio-block", "id", device_id_str] => {
            let device_id = parse_or_print_error(device_id_str, "device ID")?;
            Some(Command::VirtIOBlockID { device_id })
        }
        ["fat", "bios", device_id_str] => {
            let device_id = parse_or_print_error(device_id_str, "device ID")?;
            Some(Command::FATBIOS { device_id })
        }
        ["ext2", "superblock", device_id_str] => {
            let device_id = parse_or_print_error(device_id_str, "device ID")?;
            Some(Command::EXT2Superblock { device_id })
        }
        ["timer", milliseconds_str] => {
            let milliseconds = parse_or_print_error(milliseconds_str, "milliseconds")?;
            Some(Command::Timer(Milliseconds::new(milliseconds)))
        }
        ["sleep", milliseconds_str] => {
            let milliseconds = parse_or_print_error(milliseconds_str, "milliseconds")?;
            Some(Command::Sleep(Milliseconds::new(milliseconds)))
        }
        ["prime-sync", nth_prime_str] => {
            let nth_prime = parse_or_print_error(nth_prime_str, "prime number index")?;
            Some(Command::PrimeSync(nth_prime))
        }
        ["prime-async", nth_prime_str] => {
            let nth_prime = parse_or_print_error(nth_prime_str, "prime number index")?;
            Some(Command::PrimeAsync(nth_prime))
        }
        _ => Some(Command::Unknown(command_str)),
    }
}

fn parse_or_print_error<T>(s: &str, name: &str) -> Option<T>
where
    T: core::str::FromStr + fmt::Display,
    T::Err: fmt::Display,
{
    let parsed = s.parse::<T>();
    match parsed {
        Ok(parsed) => Some(parsed),
        Err(e) => {
            serial_println!("Invalid {name} {s}: {e}");
            None
        }
    }
}

#[allow(clippy::too_many_lines)]
fn run_command(command: &Command) {
    match command {
        Command::TestMisc => {
            tests::run_misc_tests();
        }
        Command::TestHPET => {
            tests::test_hpet();
        }
        Command::Invalid => {
            serial_println!("Invalid command");
        }
        Command::ListPCI => {
            serial_println!("Listing PCI devices...");
            let acpi_info = acpi::acpi_info();
            let pci_config_region_base_address = acpi_info.pci_config_region_base_address();
            pci::for_pci_devices_brute_force(pci_config_region_base_address, |device| {
                serial_println!("Found PCI device: {device:#x?}");
            });
        }
        Command::ListVirtIO => {
            serial_println!("Listing virtio devices...");
            let acpi_info = acpi::acpi_info();
            let pci_config_region_base_address = acpi_info.pci_config_region_base_address();
            pci::for_pci_devices_brute_force(pci_config_region_base_address, |device| {
                let Some(device) = virtio::VirtIODeviceConfig::from_pci_config(device) else { return; };
                serial_println!("Found VirtIO device: {device:#x?}");
            });
        }
        Command::BootInfo => {
            serial_println!("Printing boot info...");
            let boot_info_data = boot_info::boot_info();
            serial_println!("limine boot info:\n{boot_info_data:#x?}");
            boot_info::print_limine_memory_map();

            if let Some(system_table_addr) = boot_info_data.efi_system_table_address {
                unsafe {
                    let system_table =
                        SystemTable::<Runtime>::from_ptr(system_table_addr.as_mut_ptr())
                            .expect("failed to create EFI system table");
                    serial_println!(
                        "EFI runtime services:\n{:#?}",
                        system_table.runtime_services()
                    );

                    for entry in system_table.config_table() {
                        if entry.guid == uefi::table::cfg::ACPI2_GUID {
                            // This should match the limine RSDP address
                            serial_println!("EFI config table ACPI2 entry: {entry:#X?}");
                        }
                    }
                };
            }
        }
        Command::PrintACPI => {
            serial_println!("Printing ACPI info...");
            acpi::print_acpi_info();
        }
        Command::RNG(num_bytes) => {
            serial_println!("Generating random numbers...");
            let cell = virtio::request_random_numbers(*num_bytes);
            let buffer = cell.wait_sleep();
            serial_println!("Got RNG buffer: {buffer:x?}");
        }
        Command::VirtIOBlockList => {
            virtio::virtio_block_print_devices();
        }
        Command::VirtIOBlockRead { device_id, sector } => {
            serial_println!("Reading VirtIO block sector {sector}...");
            let cell = virtio::virtio_block_read(*device_id, *sector, 2);
            let response = cell.wait_sleep();
            let virtio::VirtIOBlockResponse::Read{ ref data } = *response else {
                serial_println!("Unexpected response from block request: {response:x?}");
                return;
            };
            serial_println!("Got block data: {data:x?}");
        }
        Command::VirtIOBlockID { device_id } => {
            serial_println!("Reading VirtIO block device ID...");
            let cell = virtio::virtio_block_get_id(*device_id);
            let response = cell.wait_sleep();
            let virtio::VirtIOBlockResponse::GetID{ ref id } = *response else {
                serial_println!("Unexpected response from block request: {response:x?}");
                return;
            };
            serial_println!("Got block ID: {id}");
        }
        Command::FATBIOS { device_id } => {
            let response = virtio::virtio_block_read(*device_id, 0, 1).wait_sleep();
            let virtio::VirtIOBlockResponse::Read{ ref data } = *response else {
                serial_println!("Unexpected response from block request: {response:x?}");
                return;
            };
            let bios_param_block: fat::BIOSParameterBlock =
                unsafe { data.as_ptr().cast::<fat::BIOSParameterBlock>().read() };
            serial_println!("BIOS Parameter Block: {:#x?}", bios_param_block);
        }
        Command::EXT2Superblock { device_id } => {
            let sector = ext2::Superblock::OFFSET_BYTES as u64
                / u64::from(virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES);
            let num_sectors = core::mem::size_of::<ext2::Superblock>()
                .div_ceil(virtio::VIRTIO_BLOCK_SECTOR_SIZE_BYTES as usize);
            let response =
                virtio::virtio_block_read(*device_id, sector, num_sectors as u32).wait_sleep();
            let virtio::VirtIOBlockResponse::Read{ ref data } = *response else {
                serial_println!("Unexpected response from block request: {response:x?}");
                return;
            };

            let superblock: ext2::Superblock =
                unsafe { data.as_ptr().cast::<ext2::Superblock>().read() };
            if superblock.magic_valid() {
                serial_println!("Found ext2 superblock: {:#x?}", superblock);
            } else {
                let magic = superblock.magic;
                serial_println!("No ext2 superblock found. Magic value was: {:x?}", magic);
            }
        }
        Command::Timer(ms) => {
            let inner_ms = *ms;
            tick::add_relative_timer(*ms, move || {
                serial_println!("Timer that lasted {inner_ms} expired!");
            });
            serial_println!("Created a timer for {ms} from now");
        }
        Command::Sleep(ms) => {
            serial_println!("Sleeping for {ms}");
            sched::scheduler_lock().sleep_timeout(*ms);
            serial_println!("Slept for {ms}");
        }
        Command::PrimeSync(n) => {
            let task_id = sched::scheduler_lock().new_task(
                "calculate prime",
                calculate_prime_task,
                *n as *const (),
            );
            sched::wait_on_task(task_id);
        }
        Command::PrimeAsync(n) => {
            sched::scheduler_lock().new_task(
                "calculate prime",
                calculate_prime_task,
                *n as *const (),
            );
            sched::scheduler_lock().run_scheduler();
        }
        Command::Unknown(command) => {
            serial_println!("Unknown command: {}", command);
        }
    }
}

extern "C" fn calculate_prime_task(arg: *const ()) {
    let n = arg as usize;
    let p = naive_nth_prime(n);
    serial_println!("calculate_prime_task DONE: {n}th prime: {p}");
}

fn naive_nth_prime(n: usize) -> usize {
    fn is_prime(x: usize) -> bool {
        for i in 2..x {
            if x % i == 0 {
                return false;
            }
        }
        true
    }

    let mut i = 2;
    let mut found_primes = 0;
    loop {
        i += 1;
        if is_prime(i) {
            found_primes += 1;
            if found_primes == n {
                return i;
            }
        }
    }
}
