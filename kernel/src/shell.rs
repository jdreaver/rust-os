use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use uefi::table::{Runtime, SystemTable};

use crate::block;
use crate::elf;
use crate::fs::{ext2, sysfs};
use crate::hpet::Milliseconds;
use crate::sync::SpinLock;
use crate::vfs::FilePath;
use crate::{
    acpi, ansiterm, boot_info, pci, sched, serial, serial_print, serial_println, tests, tick, vfs,
    virtio,
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
        serial_print!(
            "{}{}ksh > {}",
            ansiterm::GREEN,
            ansiterm::BOLD,
            ansiterm::CLEAR_FORMAT
        );
        for c in &self.buffer {
            serial::serial1_write_byte(*c);
        }
    }
}

pub(crate) extern "C" fn run_serial_shell(_arg: *const ()) {
    serial_println!("Welcome to Rust OS! Here is a shell for you to use.");
    loop {
        NEXT_COMMAND_BUFFER.lock().redraw_buffer();
        loop {
            let c = serial::serial1_read_byte();
            match c {
                b'\n' | b'\r' => {
                    serial_println!();
                    let command = {
                        let buffer = NEXT_COMMAND_BUFFER.lock();
                        parse_command(&buffer.buffer)
                    };
                    if let Some(command) = command {
                        run_command(&command);
                    }
                    NEXT_COMMAND_BUFFER.lock().buffer.clear();
                    break;
                }
                // DEL char
                127 => {
                    if NEXT_COMMAND_BUFFER.lock().delete_char() {
                        // Send backspace, space, and backspace again
                        serial::serial1_write_byte(8);
                        serial::serial1_write_byte(b' ');
                        serial::serial1_write_byte(8);
                    }
                }
                // Printable char range
                32..=126 => {
                    if NEXT_COMMAND_BUFFER.lock().add_char(c) {
                        serial::serial1_write_byte(c);
                    }
                }
                // End of text
                3 => {
                    serial_println!("^C");
                    NEXT_COMMAND_BUFFER.lock().clear();
                    break;
                }
                ansiterm::ANSI_ESCAPE => {
                    handle_ansi_escape_sequence();
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
    // Clears line and returns cursor to start of line
    serial_print!("{}\r", ansiterm::AnsiEscapeSequence::ClearEntireLine);
}

/// Handle ANSI escape sequences we care about. This isn't intended to be
/// exhaustive.
fn handle_ansi_escape_sequence() {
    let left_bracket = serial::serial1_read_byte();
    if left_bracket != b'[' {
        serial_println!("invalid escape sequence: {}", left_bracket);
        return;
    }
    let escaped_char = serial::serial1_read_byte();
    serial_println!("\ngot ANSI escape char: {}", escaped_char);
}

#[derive(Debug)]
enum Command {
    TestMisc,
    ListPCI,
    ListVirtIO,
    BootInfo,
    PrintACPI,
    RNG(u32),
    VirtIOBlock(VirtIOBlockCommand),
    Mount(MountTarget),
    Unmount,
    Ls(FilePath),
    Cat(FilePath),
    Elf(FilePath),
    Exec,
    WriteToFile { path: FilePath, content: String },
    FATBIOS { device_id: usize },
    Timer(Milliseconds),
    Sleep(Milliseconds),
    Prime(PrimeCommand),
}

#[derive(Debug)]
enum VirtIOBlockCommand {
    List,
    Read {
        device_id: usize,
        sector: u64,
    },
    Write {
        device_id: usize,
        sector: u64,
        data: u64,
    },
    ID {
        device_id: usize,
    },
}

#[derive(Debug)]
enum MountTarget {
    Device { device_id: usize },
    Sysfs,
}

#[derive(Debug)]
struct PrimeCommand {
    sync: bool,
    nth_prime: usize,
}

#[allow(clippy::too_many_lines)]
fn parse_command(buffer: &[u8]) -> Option<Command> {
    let command_str = core::str::from_utf8(buffer);
    let Ok(command_str) = command_str else {
        serial_println!("Invalid UTF-8 in command: {:?}", buffer);
        return None;
    };

    let mut words = command_str.split_whitespace();

    #[allow(clippy::single_match_else)]
    let command = match words.next()? {
        "test" => match words.next() {
            Some("misc") => Some(Command::TestMisc),
            _ => {
                serial_println!("Usage: test [misc|hpet]");
                None
            }
        },
        "list-pci" => Some(Command::ListPCI),
        "list-virtio" => Some(Command::ListVirtIO),
        "boot-info" => Some(Command::BootInfo),
        "print-acpi" => Some(Command::PrintACPI),
        "rng" => {
            let num_bytes = parse_next_word(&mut words, "num bytes", "rng <num_bytes>")?;
            Some(Command::RNG(num_bytes))
        }
        "block" => match words.next() {
            Some("list") => Some(Command::VirtIOBlock(VirtIOBlockCommand::List)),
            Some("read") => {
                let usage = "block read <device_id> <sector>";
                let device_id = parse_next_word(&mut words, "device ID", usage)?;
                let sector = parse_next_word(&mut words, "sector number", usage)?;
                Some(Command::VirtIOBlock(VirtIOBlockCommand::Read {
                    device_id,
                    sector,
                }))
            }
            Some("write") => {
                let usage = "block write <device_id> <sector> <number>";
                let device_id = parse_next_word(&mut words, "device ID", usage)?;
                let sector = parse_next_word(&mut words, "sector number", usage)?;
                let data = parse_next_word(&mut words, "number", usage)?;
                Some(Command::VirtIOBlock(VirtIOBlockCommand::Write {
                    device_id,
                    sector,
                    data,
                }))
            }
            Some("id") => {
                let device_id = parse_next_word(&mut words, "device ID", "block id <device_id>")?;
                Some(Command::VirtIOBlock(VirtIOBlockCommand::ID { device_id }))
            }
            _ => {
                serial_println!("Usage: block [list|read|id]");
                None
            }
        },
        "mount" => {
            let usage = "mount <device_id> | sysfs";
            match words.next() {
                Some("sysfs") => Some(Command::Mount(MountTarget::Sysfs)),
                Some(device_id_str) => {
                    let device_id = parse_word(device_id_str, "device ID")?;
                    Some(Command::Mount(MountTarget::Device { device_id }))
                }
                None => {
                    serial_println!("Usage: {usage}");
                    None
                }
            }
        }
        "umount" => Some(Command::Unmount),
        "ls" => {
            let path = parse_next_word(&mut words, "path", "ls <path>")?;
            Some(Command::Ls(path))
        }
        "cat" => {
            let path = parse_next_word(&mut words, "path", "cat <path>")?;
            Some(Command::Cat(path))
        }
        "elf" => {
            let path = parse_next_word(&mut words, "path", "elf <path>")?;
            Some(Command::Elf(path))
        }
        "exec" => Some(Command::Exec),
        "write-to-file" => {
            let path = parse_next_word(&mut words, "path", "write-to-file <path> <content>")?;
            let content = parse_next_word(&mut words, "content", "write-to-file <path> <content>")?;
            Some(Command::WriteToFile { path, content })
        }
        "fat" => match words.next() {
            Some("bios") => {
                let device_id = parse_next_word(&mut words, "device ID", "fat bios <device_id>")?;
                Some(Command::FATBIOS { device_id })
            }
            _ => {
                serial_println!("Usage: fat [bios]");
                None
            }
        },
        "timer" => {
            let milliseconds = parse_next_word(&mut words, "milliseconds", "timer <milliseconds>")?;
            Some(Command::Timer(Milliseconds::new(milliseconds)))
        }
        "sleep" => {
            let milliseconds = parse_next_word(&mut words, "milliseconds", "sleep <milliseconds>")?;
            Some(Command::Sleep(Milliseconds::new(milliseconds)))
        }
        "prime" => {
            let usage = "prime <sync|async> <nth_prime>";
            let sync = match words.next() {
                Some("sync") => true,
                Some("async") => false,
                _ => {
                    serial_println!("Usage: {usage}");
                    return None;
                }
            };
            let nth_prime = parse_next_word(&mut words, "prime number index", usage)?;
            Some(Command::Prime(PrimeCommand { sync, nth_prime }))
        }
        _ => {
            serial_println!("Unknown command: {:?}", command_str);
            None
        }
    };

    let command = command?;

    let mut words = words.peekable();
    if words.peek().is_some() {
        let remaining = words.collect::<Vec<_>>().join(" ");
        serial_println!(
            "Too many arguments. Parsed command: {command:?}, remaining args: {remaining}"
        );
        None
    } else {
        Some(command)
    }
}

fn parse_next_word<'a, T>(
    words: &mut impl Iterator<Item = &'a str>,
    name: &str,
    usage_msg: &str,
) -> Option<T>
where
    T: core::str::FromStr + fmt::Display,
    T::Err: fmt::Display,
{
    let val = words.next().and_then(|word| parse_word(word, name));
    if val.is_none() {
        serial_println!("Usage: {usage_msg}");
    }
    val
}

fn parse_word<T>(word: &str, name: &str) -> Option<T>
where
    T: core::str::FromStr + fmt::Display,
    T::Err: fmt::Display,
{
    let parsed = word.parse::<T>();
    match parsed {
        Ok(parsed) => Some(parsed),
        Err(e) => {
            serial_println!("Invalid {name}: {word}, error: {e}");
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
        Command::VirtIOBlock(VirtIOBlockCommand::List) => {
            virtio::virtio_block_print_devices();
        }
        Command::VirtIOBlock(VirtIOBlockCommand::Read { device_id, sector }) => {
            serial_println!("Reading VirtIO block sector {sector}...");
            let cell = virtio::virtio_block_read(*device_id, *sector, 1);
            let response = cell.wait_sleep();
            let virtio::VirtIOBlockResponse::Read{ ref data } = response else {
                log::error!("Unexpected response from block request: {response:x?}");
                return;
            };
            serial_println!("Got block data: {data:x?}");
        }
        Command::VirtIOBlock(VirtIOBlockCommand::Write {
            device_id,
            sector,
            data,
        }) => {
            serial_println!("Reading VirtIO block sector {sector}...");
            let data = data.to_le_bytes();
            let cell = virtio::virtio_block_write(*device_id, *sector, &data);
            let response = cell.wait_sleep();
            let virtio::VirtIOBlockResponse::Write = response else {
                log::error!("Unexpected response from block request: {response:x?}");
                return;
            };
            serial_println!("Write success");
        }
        Command::VirtIOBlock(VirtIOBlockCommand::ID { device_id }) => {
            serial_println!("Reading VirtIO block device ID...");
            let cell = virtio::virtio_block_get_id(*device_id);
            let response = cell.wait_sleep();
            let virtio::VirtIOBlockResponse::GetID{ ref id } = response else {
                log::error!("Unexpected response from block request: {response:x?}");
                return;
            };
            serial_println!("Got block ID: {id}");
        }
        Command::Mount(target) => {
            let filesystem: Box<dyn vfs::FileSystem + Send + 'static> = match target {
                MountTarget::Device { device_id } => {
                    serial_println!(
                        "Mounting ext2 filesystem from VirtIO block device {device_id}"
                    );
                    let device = block::virtio_block_device(*device_id);
                    Box::new(ext2::VFSFileSystem::read(device))
                }
                MountTarget::Sysfs => {
                    serial_println!("Mounting sysfs filesystem");
                    Box::new(sysfs::Sysfs)
                }
            };
            vfs::mount_root_filesystem(filesystem);
        }
        Command::Unmount => {
            vfs::unmount_root_filesystem();
            serial_println!("Unmounted filesystem");
        }
        Command::Ls(path) => {
            serial_println!("ls: {path:?}");
            let Some(inode) = get_path_inode(path) else { return; };

            let vfs::InodeType::Directory(mut dir) = inode.inode_type else {
                serial_println!("Not a directory");
                return;
            };

            dir.subdirectories().iter().for_each(|entry| {
                let trailing_slash = if entry.entry_type() == vfs::DirectoryEntryType::Directory {
                    "/"
                } else {
                    ""
                };
                serial_println!("{}{}", entry.name(), trailing_slash);
            });
        }
        Command::Cat(path) => {
            serial_println!("cat: {path:?}");
            let Some(inode) = get_path_inode(path) else { return; };

            let vfs::InodeType::File(mut file) = inode.inode_type else {
                serial_println!("Not a file");
                return;
            };

            let bytes = file.read();
            serial_println!("{}", String::from_utf8_lossy(&bytes));
        }
        Command::Elf(path) => {
            serial_println!("elf: {path:?}");
            let Some(inode) = get_path_inode(path) else { return; };

            let vfs::InodeType::File(mut file) = inode.inode_type else {
                serial_println!("Not a file");
                return;
            };

            let bytes = file.read();
            let elf_exe = match elf::ElfExecutableHeader::parse(&bytes) {
                Ok(exe) => exe,
                Err(e) => {
                    serial_println!("Failed to parse ELF: {e:?}");
                    return;
                }
            };
            serial_println!("ELF header: {:#?}", elf_exe);
        }
        Command::Exec => {
            let task_id = sched::new_task(
                "dummy userspace",
                sched::task_userspace_setup,
                core::ptr::null(),
            );
            serial_println!("Waiting for userspace task {task_id:?} to finish...");
            let exit_code = sched::wait_on_task(task_id);
            serial_println!("Task {task_id:?} finished! Exit code: {exit_code:?}");
        }
        Command::WriteToFile { path, content } => {
            let mut file = if let Some(inode) = get_path_inode(path) {
                let vfs::InodeType::File(file) = inode.inode_type else {
                    serial_println!("Not a file");
                    return;
                };
                file
            } else {
                // File not found. Need to create an inode.
                let Some((parent_path, filename)) = path.split_dirname_filename() else {
                    serial_println!("Parent directory path doesn't exist. Did you try to write to the root directory?");
                    return;
                };

                let Some(parent_inode) = get_path_inode(&parent_path)  else {
                    serial_println!("Parent directory '{parent_path}' not found");
                    return;
                };

                let vfs::InodeType::Directory(mut parent_dir) = parent_inode.inode_type else {
                    serial_println!("Parent path '{parent_path}' is not a directory");
                    return;
                };

                let Some(file) = parent_dir.create_file(filename.as_str()) else {
                    serial_println!("Failed to create inode");
                    return;
                };
                file
            };

            file.write(content.as_bytes());
        }
        Command::FATBIOS { device_id } => {
            let response = virtio::virtio_block_read(*device_id, 0, 1).wait_sleep();
            let virtio::VirtIOBlockResponse::Read{ ref data } = response else {
                log::error!("Unexpected response from block request: {response:x?}");
                return;
            };
            let bios_param_block: fat::BIOSParameterBlock =
                unsafe { data.as_ptr().cast::<fat::BIOSParameterBlock>().read() };
            serial_println!("BIOS Parameter Block: {:#x?}", bios_param_block);
        }
        Command::Timer(ms) => {
            let inner_ms = *ms;
            tick::add_relative_timer(*ms, move || {
                log::info!("Timer that lasted {inner_ms} expired!");
            });
            serial_println!("Created a timer for {ms} from now");
        }
        Command::Sleep(ms) => {
            serial_println!("Sleeping for {ms}");
            sched::sleep_timeout(*ms);
            serial_println!("Slept for {ms}");
        }
        Command::Prime(PrimeCommand { sync, nth_prime }) => {
            let task_id = sched::new_task(
                "calculate prime",
                calculate_prime_task,
                *nth_prime as *const (),
            );
            if *sync {
                serial_println!("Waiting for task {task_id:?} to finish...");
                let exit_code = sched::wait_on_task(task_id);
                serial_println!("Task {task_id:?} finished! Exit code: {exit_code:?}");
            } else {
                serial_println!("Task {task_id:?} is running in the background");
                sched::run_scheduler();
            }
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

fn get_path_inode(path: &vfs::FilePath) -> Option<vfs::Inode> {
    let mut lock = vfs::root_filesystem_lock();
    let Some(filesystem) = lock.as_mut() else {
        serial_println!("No filesystem mounted. Run 'mount <device_id>' first.");
        return None;
    };
    if !path.absolute {
        serial_println!("Path must be absolute. Got {}", path);
        return None;
    }

    let Some(inode) = filesystem.traverse_path(path) else {
        serial_println!("No such file or directory: {}", path);
        return None;
    };
    Some(inode)
}
