use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::{gdt, serial_println};

lazy_static! {
    static ref IDT: Mutex<InterruptDescriptorTable> = Mutex::new({
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

        idt
    });

    static ref NEXT_OPEN_INTERRUPT_INDEX: Mutex<u8> = Mutex::new(APIC_INTERRUPT_START_OFFSET);
}

pub(crate) fn init_idt() {
    unsafe {
        IDT.lock().load_unsafe();
    };
    disable_pic();
    x86_64::instructions::interrupts::enable();
}

// Even though we are disabling the PIC in lieu of the APIC, we still need to
// set the offsets to avoid CPU exceptions. We should not use indexes 32 through 47
// for APIC interrupts so we avoid spurious PIC interrupts.
const MASTER_PIC_OFFSET: u8 = 32;
const SLAVE_PIC_OFFSET: u8 = MASTER_PIC_OFFSET + 8;
const APIC_INTERRUPT_START_OFFSET: u8 = SLAVE_PIC_OFFSET + 8;

/// Must disable the legacy PIC if we are using API. We do this by both masking
/// all of the interrupts and remapping all of the IRQs to be above 32 to avoid
/// spurious PIC interrupts masquerading as CPU exceptions. See
/// <https://wiki.osdev.org/8259_PIC#Disabling> and
/// <https://wiki.osdev.org/APIC>.
fn disable_pic() {
    unsafe {
        let mut pic = pic8259::ChainedPics::new(MASTER_PIC_OFFSET, SLAVE_PIC_OFFSET);
        pic.disable();
    };
}

/// Send spurious interrupts to a high index that we won't use.
pub(crate) const SPURIOUS_INTERRUPT_VECTOR_INDEX: u8 = 0xFF;

/// Install an interrupt handler in the IDT. Uses the next open interrupt index
/// and returns the used index.
pub(crate) fn install_interrupt(handler: extern "x86-interrupt" fn(InterruptStackFrame)) -> u8 {
    let interrupt_index = {
        let mut next_index = NEXT_OPEN_INTERRUPT_INDEX.lock();
        let index = *next_index;
        *next_index += 1;
        index
    };
    assert!(
        interrupt_index < SPURIOUS_INTERRUPT_VECTOR_INDEX,
        "Ran out of interrupt vectors"
    );

    let mut idt = IDT.lock();
    idt[interrupt_index as usize].set_handler_fn(handler);

    interrupt_index
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
