use lazy_static::lazy_static;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use spin::Mutex;
use x86_64::instructions::port::Port;

use crate::interrupts::InterruptHandlerID;
use crate::{interrupts, ioapic, serial_println};

pub(crate) fn init_keyboard() {
    let interrupt_vector = interrupts::install_interrupt(1, keyboard_interrupt_handler);
    ioapic::install_irq(interrupt_vector, KEYBOARD_IOAPIC_REDTBL_INDEX);
}

/// Assumes that the keyboard IRQ for the IOAPIC is 1, which is the same as if
/// we were using the 8259 PIC. If we wanted to determine this dynamically, we
/// could read the IOAPIC redirection table entry for IRQ 1, or if that doesn't
/// exist I think we need to parse some ACPI AML.
const KEYBOARD_IOAPIC_REDTBL_INDEX: u8 = 1;

fn keyboard_interrupt_handler(_vector: u8, _handler_id: InterruptHandlerID) {
    // https://wiki.osdev.org/%228042%22_PS/2_Controller#PS.2F2_Controller_IO_Ports
    const KEYBOARD_PORT: u16 = 0x60;

    lazy_static! {
        static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(
            Keyboard::new(layouts::Us104Key, ScancodeSet1, HandleControl::Ignore)
        );
    }

    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(KEYBOARD_PORT);

    // KEYBOARD has an internal state machine that processes e.g. modifier keys
    // like shift and caps lock. It needs to be fed with the scancodes of the
    // pressed keys. If the scancode is a valid key, the keyboard crate will
    // eventually return a `DecodedKey`.
    let scancode: u8 = unsafe { port.read() };
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => {
                    serial_println!("FOUND UNICODE CHAR {character}");
                }
                DecodedKey::RawKey(key) => serial_println!("FOUND RAW CHAR {key:?}"),
            }
        }
    }
}
