use spin::Mutex;

use crate::{serial, serial_print, serial_println, tests};

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

    fn redraw_buffer(&self) {
        reset_terminal_line();
        serial_print!("ksh > ");
        for i in 0..self.index {
            serial::serial1_write_byte(self.buffer[i]);
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
                    let command = next_command(buffer.buffer_slice());
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
    Invalid,
    Unknown(&'a str),
}

fn next_command(buffer: &mut [u8]) -> Option<Command> {
    let command_str = core::str::from_utf8(buffer);
    let Ok(command_str) = command_str else { return Some(Command::Invalid); };

    match command_str.trim() {
        "" => None,
        "help" => Some(Command::Help),
        "tests" => Some(Command::Tests),
        s => Some(Command::Unknown(s)),
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
        Command::Unknown(command) => {
            serial_println!("Unknown command: {}", command);
        }
    }
}
