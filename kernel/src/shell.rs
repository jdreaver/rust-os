use spin::Mutex;

use crate::serial;
use crate::{serial_print, serial_println};

static NEXT_COMMAND_BUFFER: Mutex<ShellBuffer<64>> = Mutex::new(ShellBuffer::new());

struct ShellBuffer<const N: usize> {
    buffer: [u8; N],
    index: usize,
}

impl<const N: usize> ShellBuffer<N> {
    const fn new() -> Self {
        Self {
            buffer: [0; N],
            index: 0,
        }
    }

    fn add_char(&mut self, c: u8) -> bool {
        if self.index < N {
            self.buffer[self.index] = c;
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn delete_char(&mut self) -> bool {
        if self.index > 0 {
            self.index -= 1;
            true
        } else {
            false
        }
    }

    fn clear(&mut self) {
        self.index = 0;
    }

    fn buffer_slice(&mut self) -> &mut [u8] {
        &mut self.buffer[..self.index]
    }
}

pub(crate) fn run_serial_shell() -> ! {
    let mut buffer = NEXT_COMMAND_BUFFER.lock();

    loop {
        serial_print!("ksh > ");
        loop {
            let c = serial::serial1_read_byte();
            match c {
                b'\n' | b'\r' => {
                    serial_println!();
                    let command = next_command(buffer.buffer_slice());
                    match command {
                        Some(Command::Help) => {
                            serial_println!("help - print this help");
                        }
                        Some(Command::Invalid) => {
                            serial_println!("Invalid command");
                        }
                        Some(Command::Unknown(command)) => {
                            serial_println!("Unknown command: {}", command);
                        }
                        None => {
                            serial_println!();
                        }
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
                _ => {
                    serial_println!("Don't know what to do with: {} ({})", c as char, c);
                }
            }
        }
    }
}

enum Command<'a> {
    Help,
    Invalid,
    Unknown(&'a str),
}

fn next_command(buffer: &mut [u8]) -> Option<Command> {
    let command_str = core::str::from_utf8(buffer);
    let Ok(command_str) = command_str else { return Some(Command::Invalid); };

    match command_str.trim() {
        "" => None,
        "help" => Some(Command::Help),
        s => Some(Command::Unknown(s)),
    }
}
