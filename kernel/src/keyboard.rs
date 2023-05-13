use lazy_static::lazy_static;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use spin::Mutex;
use x86_64::instructions::port::Port;
use x86_64::structures::idt::InterruptStackFrame;

use crate::{apic, interrupts, serial_println};

pub(crate) fn init_keyboard(ioapic: &apic::IOAPIC) {
    let kbd_irq = interrupts::install_interrupt(keyboard_interrupt_handler);
    let kbd_ioredtbl = apic::IOAPICRedirectionTableRegister::new()
        .with_interrupt_vector(kbd_irq)
        .with_interrupt_mask(false)
        .with_delivery_mode(0) // Fixed
        .with_destination_mode(false) // Physical
        .with_delivery_status(false)
        .with_destination_field(ioapic.ioapic_id().id());

    ioapic.write_ioredtbl(KEYBOARD_IOAPIC_REDTBL_INDEX, kbd_ioredtbl);
    serial_println!(
        "Keyboard IOREDTBL: {:#x?}",
        ioapic.read_ioredtbl(KEYBOARD_IOAPIC_REDTBL_INDEX)
    );
}

/// Assumes that the keyboard IRQ for the IOAPIC is 1, which is the same as if
/// we were using the 8259 PIC. If we wanted to determine this dynamically, we
/// could read the IOAPIC redirection table entry for IRQ 1, or if that doesn't
/// exist I think we need to parse some ACPI AML.
const KEYBOARD_IOAPIC_REDTBL_INDEX: u8 = 1;

pub extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
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
                    serial_println!("FOUND UNICODE CHAR {}", character);
                }
                DecodedKey::RawKey(key) => serial_println!("FOUND RAW CHAR {:?}", key),
            }
        }
    }

    apic::end_of_interrupt();
}