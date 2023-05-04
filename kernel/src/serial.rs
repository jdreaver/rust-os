use core::fmt::Write;

use lazy_static::lazy_static;
use spin::Mutex;
use uart_16550::SerialPort;

lazy_static! {
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
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
        serial_port.write_str("\x1B[0m\x1B[H\x1B[J").expect("Failed to set colors");

        Mutex::new(serial_port)
    };
}

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use x86_64::instructions::interrupts;

    // Disable interrupts while taking mutex lock so we don't deadlock if an
    // interrupt occurs and the interrupt handler tries to take the same lock.
    interrupts::without_interrupts(|| {
        SERIAL1
            .lock()
            .write_fmt(args)
            .expect("Printing to serial failed");
    });
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
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(
        concat!($fmt, "\n"), $($arg)*));
}
