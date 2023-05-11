use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::{gdt, serial_print, serial_println};

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        unsafe {
            // set_stack_index is unsafe because the caller must ensure that the
            // used index is valid and not already used for another exception.
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt[InterruptIndex::Timer.into()].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.into()].set_handler_fn(keyboard_interrupt_handler);

        idt
    };
}

// Set PIC offset to 32 b/c 0-31 are usually existing interrupts.
const PIC_1_OFFSET: u8 = 32;
const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

static PICS: spin::Mutex<pic8259::ChainedPics> =
    spin::Mutex::new(unsafe { pic8259::ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
}

impl From<InterruptIndex> for u8 {
    fn from(index: InterruptIndex) -> Self {
        index as Self
    }
}

impl From<InterruptIndex> for usize {
    fn from(index: InterruptIndex) -> Self {
        index as Self
    }
}

pub(crate) fn init_idt() {
    IDT.load();

    // Enable PIC and interrupts
    unsafe {
        let mut pic = PICS.lock();
        pic.initialize();

        // Limine masks all legacy PIC and APIC IRQs. We have to enable the ones
        // we want. I got these values by calling `pic.read_masks()` using GRUB
        // instead of limine.
        pic.write_masks(0b1011_1000, 0b1000_1110); // 184 and 142 in decimal
    };
    x86_64::instructions::interrupts::enable();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    serial_println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    serial_println!("EXCEPTION: PAGE FAULT");
    serial_println!("Accessed Address: {:?}", Cr2::read());
    serial_println!("Error Code: {:?}", error_code);
    serial_println!("{:#?}", stack_frame);

    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    serial_println!(
        "EXCEPTION: GENERAL PROTECTION FAULT\nerror_code:{}\n{:#?}",
        error_code,
        stack_frame
    );
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    serial_print!(".");

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.into());
    }
}

// https://wiki.osdev.org/%228042%22_PS/2_Controller#PS.2F2_Controller_IO_Ports
const KEYBOARD_PORT: u16 = 0x60;

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
    use spin::Mutex;
    use x86_64::instructions::port::Port;

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
                DecodedKey::Unicode(character) => serial_print!("{}", character),
                DecodedKey::RawKey(key) => serial_print!("{:?}", key),
            }
        }
    }
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.into());
    }
}
