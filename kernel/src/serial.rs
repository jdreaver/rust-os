use core::fmt::Write;

use lazy_static::lazy_static;
use x86_64::instructions::port::{PortRead, PortWrite};

/// Serial port that can be written to, but not read from. This is useful for
/// printing to the host from the guest without having to worry about
/// synchronization, disabling interrupts, locking, etc.
///
/// See <https://wiki.osdev.org/Serial_Ports>
struct WriteOnlySerialPort {
    // data is technically read/write, but we only use it for writing
    data: u16,
    int_en: u16,
    fifo_ctrl: u16,
    line_ctrl: u16,
    modem_ctrl: u16,
    line_sts: u16,
}

impl WriteOnlySerialPort {
    const COM1_PORT: u16 = 0x3F8;

    const LINE_STATUS_DATA_READY: u8 = 1 << 0;
    const LINE_STATUS_TRANSMITTER_EMPTY: u8 = 1 << 5;

    const fn new() -> Self {
        Self {
            data: Self::COM1_PORT,
            int_en: Self::COM1_PORT + 1,
            fifo_ctrl: Self::COM1_PORT + 2,
            line_ctrl: Self::COM1_PORT + 3,
            modem_ctrl: Self::COM1_PORT + 4,
            line_sts: Self::COM1_PORT + 5,
        }
    }

    /// See <https://wiki.osdev.org/Serial_Ports> for init options
    fn init(&self) {
        unsafe {
            // Disable interrupts
            u8::write_to_port(self.int_en, 0x00);

            // Enable DLAB
            u8::write_to_port(self.line_ctrl, 0x80);

            // Set maximum speed to 38400 bps by configuring DLL and DLM
            u8::write_to_port(self.data, 0x03); // Low byte
            u8::write_to_port(self.int_en, 0x00); // High byte

            // Disable DLAB and set data word length to 8 bits, no parity, one
            // stop bit
            u8::write_to_port(self.line_ctrl, 0x03);

            // Enable FIFO, clear them, with 14-byte threshold
            u8::write_to_port(self.fifo_ctrl, 0xC7);

            // Mark data terminal ready, signal request to send
            // and enable auxilliary output #2 (used as interrupt line for CPU)
            u8::write_to_port(self.modem_ctrl, 0x0B);

            // Enable interrupts
            //
            // TODO: Do we even need interrupts?
            u8::write_to_port(self.int_en, 0x01);
        }
    }

    fn is_transmit_empty(&self) -> bool {
        unsafe { u8::read_from_port(self.line_sts) & Self::LINE_STATUS_TRANSMITTER_EMPTY != 0 }
    }

    fn write(&self, byte: u8) {
        // Wait for line to clear
        while !self.is_transmit_empty() {
            core::hint::spin_loop();
        }

        unsafe {
            u8::write_to_port(self.data, byte);
        }
    }

    fn write_str(&self, s: &str) {
        for byte in s.bytes() {
            self.write(byte);
        }
    }

    fn is_data_ready(&self) -> bool {
        unsafe { u8::read_from_port(self.line_sts) & Self::LINE_STATUS_DATA_READY != 0 }
    }

    fn read(&self) -> u8 {
        while !self.is_data_ready() {
            core::hint::spin_loop();
        }

        unsafe { u8::read_from_port(self.data) }
    }
}

/// This type exists just so we can use the `Write` trait. Useful for use with
/// the `write!` macro. We don't want to implement `Write` directly on
/// `WriteOnlySerialPort` because we don't want to have to make a global mutable
/// reference to it.
pub(crate) struct SerialWriter();

impl Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        SERIAL1.write_str(s);
        Ok(())
    }
}

lazy_static! {
    static ref SERIAL1: WriteOnlySerialPort = {
        let serial_port = WriteOnlySerialPort::new();
        serial_port.init();

        // When running in UEFI, the OVMF firmware prints a ton of crap
        // formatting characters to the serial port. This clears the screen
        // before printing so we don't have to look at that. Here it is for
        // posterity:
        //
        // [2J[01;01H[=3h[2J[01;01H[0m[35m[40m[0m[37m[40mBdsDxe: failed to load Boot0001 "UEFI QEMU DVD-ROM QM00005 " from PciRoot(0x0)/Pci(0x1F,0x2)/Sata(0x2,0xFFFF,0x0): Not Found
        // BdsDxe: loading Boot0002 "UEFI QEMU HARDDISK QM00001 " from PciRoot(0x0)/Pci(0x1F,0x2)/Sata(0x0,0xFFFF,0x0)
        // BdsDxe: starting Boot0002 "UEFI QEMU HARDDISK QM00001 " from PciRoot(0x0)/Pci(0x1F,0x2)/Sata(0x0,0xFFFF,0x0)
        // [2J[01;01H[01;01H[2J[01;01H[01;01H
        //
        // TODO: Find a way to clear the serial port so qemu doesn't see that
        // stuff in the first place.
        //
        // See https://gist.github.com/fnky/458719343aabd01cfb17a3a4f7296797 for
        // escape codes:
        //
        // - `[0m` resets all styles and colors
        // - `[H` moves the cursor to the top left
        // - `[J` clears the screen from the cursor down
        serial_port.write_str("\x1B[0m\x1B[H\x1B[J");

        serial_port
    };
}

/// Fetch the global serial writer for use in `write!` macros.
///
/// # Examples
///
/// ```
/// writeln!(serial1_writer(), "Hello, world!");
/// ```
pub(crate) fn serial1_writer() -> SerialWriter {
    SerialWriter()
}

#[doc(hidden)]
pub(crate) fn _print(args: ::core::fmt::Arguments) {
    serial1_writer()
        .write_fmt(args)
        .expect("Printing to serial failed");
}

/// Prints to the host through the serial interface.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*))
    };
}

/// Prints to the host through the serial interface, appending a newline.
#[macro_export]
macro_rules! serial_println {
    () => {
        $crate::serial_print!("\n")
    };

    ($fmt:expr) => {
        {
            $crate::serial_print!($fmt);
            $crate::serial_print!("\n");
        }
    };

    ($fmt:expr, $($arg:tt)*) => {
        {
            $crate::serial_print!($fmt, $( $arg )*);
            $crate::serial_print!("\n");
        }
    };
}

pub(crate) fn serial1_write_byte(byte: u8) {
    SERIAL1.write(byte);
}

pub(crate) fn serial1_write_bytes(bytes: &[u8]) {
    for byte in bytes {
        serial1_write_byte(*byte);
    }
}

/// Read the next byte from the serial port.
pub(crate) fn serial1_read_byte() -> u8 {
    SERIAL1.read()
}
