use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::fmt;

use spin::Mutex;
use uefi::table::{Runtime, SystemTable};

use crate::{acpi, boot_info, pci, serial, serial_print, serial_println, tests, virtio};

static NEXT_COMMAND_BUFFER: Mutex<ShellBuffer> = Mutex::new(ShellBuffer::new());

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

pub(crate) fn run_serial_shell() -> ! {
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
    TestScheduler,
    ListPCI,
    ListVirtIO,
    BootInfo,
    PrintACPI,
    RNG,
    VirtIOBlockList,
    VirtIOBlockRead { device_id: usize, sector: u64 },
    VirtIOBlockID { device_id: usize },
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
        ["test", "scheduler"] => Some(Command::TestScheduler),
        ["list-pci"] => Some(Command::ListPCI),
        ["list-virtio"] => Some(Command::ListVirtIO),
        ["boot-info"] => Some(Command::BootInfo),
        ["print-acpi"] => Some(Command::PrintACPI),
        ["rng"] => Some(Command::RNG),
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

fn run_command(command: &Command) {
    match command {
        Command::TestMisc => {
            tests::run_misc_tests();
        }
        Command::TestHPET => {
            tests::test_hpet();
        }
        Command::TestScheduler => {
            tests::test_scheduler();
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
        Command::RNG => {
            serial_println!("Generating random numbers...");

            let buffer: Arc<Mutex<Option<Box<[u8]>>>> = Arc::new(Mutex::new(None));

            let closure_buf = buffer.clone();
            virtio::request_random_numbers(move |buf| {
                x86_64::instructions::interrupts::without_interrupts(|| {
                    closure_buf.lock().replace(buf);
                });
            });

            loop {
                // Disable interrupts so RNG IRQ doesn't deadlock the mutex
                let done = x86_64::instructions::interrupts::without_interrupts(|| {
                    let guard = buffer.lock();
                    guard.is_some()
                });

                if done {
                    let buf = buffer.lock().take().unwrap();
                    serial_println!("Got RNG buffer: {buf:x?}");
                    return;
                }
                core::hint::spin_loop();
            }
        }
        Command::VirtIOBlockList => {
            virtio::virtio_block_print_devices();
        }
        Command::VirtIOBlockRead { device_id, sector } => {
            serial_println!("Reading VirtIO block sector {sector}...");
            virtio::virtio_block_read(*device_id, *sector);
        }
        Command::VirtIOBlockID { device_id } => {
            serial_println!("Reading VirtIO block device ID...");
            virtio::virtio_block_get_id(*device_id);
        }
        Command::Unknown(command) => {
            serial_println!("Unknown command: {}", command);
        }
    }
}
