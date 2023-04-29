use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

use crate::{gdt, print, println};

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        // idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        unsafe {
            // set_stack_index is unsafe because the caller must ensure that the
            // used index is valid and not already used for another exception.
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt[InterruptIndex::Timer.into()].set_handler_fn(timer_interrupt_handler);

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
}

impl From<InterruptIndex> for u8 {
    fn from(index: InterruptIndex) -> Self {
        match index {
            InterruptIndex::Timer => InterruptIndex::Timer as u8,
        }
    }
}

impl From<InterruptIndex> for usize {
    fn from(index: InterruptIndex) -> Self {
        match index {
            InterruptIndex::Timer => InterruptIndex::Timer as usize,
        }
    }
}

pub fn init_idt() {
    IDT.load();

    // Enable PIC and interrupts
    unsafe { PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

// extern "x86-interrupt" fn page_fault_handler(
//     stack_frame: InterruptStackFrame,
//     _error_code: PageFaultErrorCode,
// ) {
//     println!("EXCEPTION: PAGE FAULT\n{:#?}", stack_frame);
// }

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    println!(
        "EXCEPTION: GENERAL PROTECTION FAULT\nerror_code:{error_code}\n{:#?}",
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
    print!(".");

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.into());
    }
}
