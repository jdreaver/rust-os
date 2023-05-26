use alloc::vec::Vec;
use spin::Mutex;

use crate::{acpi, pci, serial, serial_print, serial_println, tests, virtio};

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
    Help,
    Tests,
    ListPCI,
    ListVirtIO,
    PrintACPI,
    RNG,
    VirtIOBlockRead { sector: u64 },
    VirtIOBlockID,
    Invalid,
    Unknown(&'a str),
}

fn next_command(buffer: &[u8]) -> Option<Command> {
    let command_str = core::str::from_utf8(buffer);
    let Ok(command_str) = command_str else { return Some(Command::Invalid); };

    let words = command_str.split_whitespace().collect::<Vec<_>>();

    match &words[..] {
        [""] => None,
        ["help"] => Some(Command::Help),
        ["tests"] => Some(Command::Tests),
        ["list-pci"] => Some(Command::ListPCI),
        ["list-virtio"] => Some(Command::ListVirtIO),
        ["print-acpi"] => Some(Command::PrintACPI),
        ["rng"] => Some(Command::RNG),
        ["virtio-block-read", sector_str] => {
            let sector = sector_str.parse::<u64>();
            match sector {
                Ok(sector) => Some(Command::VirtIOBlockRead { sector }),
                Err(e) => {
                    serial_println!("Invalid sector number {sector_str}: {e}");
                    None
                }
            }
        }
        ["virtio-block-id"] => Some(Command::VirtIOBlockID),
        _ => Some(Command::Unknown(command_str)),
    }
}

fn run_command(command: &Command) {
    match command {
        Command::Help => {
            serial_println!("help - print this help");
        }
        Command::Tests => {
            serial_println!("Running tests...");
            tests::run_tests();
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
        Command::PrintACPI => {
            serial_println!("Printing ACPI info...");
            acpi::print_acpi_info();
        }
        Command::RNG => {
            serial_println!("Generating random numbers...");
            virtio::request_random_numbers();
        }
        Command::VirtIOBlockRead { sector } => {
            serial_println!("Reading VirtIO block sector {sector}...");
            virtio::virtio_block_read(*sector);
        }
        Command::VirtIOBlockID => {
            serial_println!("Reading VirtIO block device ID...");
            virtio::virtio_block_get_id();
        }
        Command::Unknown(command) => {
            serial_println!("Unknown command: {}", command);
        }
    }
}
