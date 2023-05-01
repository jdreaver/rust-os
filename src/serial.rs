use lazy_static::lazy_static;
use spin::Mutex;
use uart_16550::SerialPort;

lazy_static! {
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        Mutex::new(serial_port)
    };
}

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
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

/// # Safety
///
/// If the string is not null-terminated, this will happily iterate through
/// memory and print garbage until it finds a null byte or we hit a protection
/// fault because we tried to ready a page we don't have access to.
pub unsafe fn print_null_terminated_string(ptr: *const u8) {
    let mut i = 0;
    loop {
        let c = *ptr.offset(i);
        if c == 0 {
            break;
        }
        serial_print!("{}", c as char);
        i += 1;
    }
}
