use lazy_static::lazy_static;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use x86_64::instructions::port::Port;

use crate::interrupts::{InterruptHandlerID, InterruptVector};
use crate::sync::SpinLock;
use crate::{interrupts, ioapic};

pub(crate) fn init_keyboard() {
    let interrupt_vector = interrupts::install_interrupt(None, 1, keyboard_interrupt_handler);
    ioapic::install_irq(interrupt_vector, ioapic::IOAPICIRQNumber::Keyboard);
}

fn keyboard_interrupt_handler(_vector: InterruptVector, _handler_id: InterruptHandlerID) {
    // https://wiki.osdev.org/%228042%22_PS/2_Controller#PS.2F2_Controller_IO_Ports
    const KEYBOARD_PORT: u16 = 0x60;

    lazy_static! {
        static ref KEYBOARD: SpinLock<Keyboard<layouts::Us104Key, ScancodeSet1>> = SpinLock::new(
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
                    log::info!("FOUND UNICODE CHAR {character}");
                }
                DecodedKey::RawKey(key) => log::info!("FOUND RAW CHAR {key:?}"),
            }
        }
    }
}
