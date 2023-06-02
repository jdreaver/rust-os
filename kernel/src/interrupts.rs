use core::sync::atomic::{AtomicU8, Ordering};

use lazy_static::lazy_static;
use paste::paste;
use seq_macro::seq;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::sync::SpinLock;
use crate::{apic, gdt, sched, serial_println};

/// CPU exception interrupt vectors stop at 32.
const FIRST_EXTERNAL_INTERRUPT_VECTOR: usize = 32;

/// This is how many interrupt vectors we have available in the IDT on x86 systems.
const NUM_INTERRUPT_VECTORS: usize = 256;

/// Macro to generate a stub interrupt handler for external interrupts that just
/// calls the common interrupt handler with the vector.
macro_rules! external_stub_interrupt_handler {
    ($idt:ident $vector:literal) => {
        paste! {
            extern "x86-interrupt" fn [<_idt_entry_ $vector>](_: InterruptStackFrame) {
                common_external_interrupt_handler($vector);
            }

            $idt[$vector].set_handler_fn([<_idt_entry_ $vector>]);
        }
    };
}

/// Common entrypoint to all external interrupts. The kernel creates stub
/// handlers using `external_stub_interrupt_handler!` for every external
/// interrupt vector, and those handlers call into this function with the
/// `vector`. This gives us a layer of indirection that allows us to reuse
/// handler code while still being able to identify the source interrupt vector.
///
/// For reference, here is how Linux does things:
///
/// - For CPU exceptions (vectors < 32), they have a hard-coded handler in the IDT
/// - For external interrupts (starting at 32) Linux pre-populates a stub interrupt handler for every vector (256 - 32 of them on x86_64) that simply calls `common_interrupt` with the vector number.
///   - [This is the code](https://elixir.bootlin.com/linux/v6.3/source/arch/x86/include/asm/idtentry.h#L483) where they create the stubs
///   - [`DECLARE_IDTENTRY` definition](https://elixir.bootlin.com/linux/v6.3/source/arch/x86/include/asm/idtentry.h#L17), which [is used](https://elixir.bootlin.com/linux/v6.3/source/arch/x86/include/asm/idtentry.h#L636) (via one intermediate macro in the same file) to create `asm_common_interrupt`, which is what the stub jumps to.
/// - [Definition for `common_interrupt`](https://elixir.bootlin.com/linux/v6.3/source/arch/x86/kernel/irq.c#L240)
///   - [`DEFINE_IDTENTRY_IRQ` def](https://elixir.bootlin.com/linux/v6.3/source/arch/x86/include/asm/idtentry.h#L191)
///
fn common_external_interrupt_handler(vector: u8) {
    let &(interrupt_id, handler) = EXTERNAL_INTERRUPT_HANDLERS
        .lock()
        .get(vector as usize)
        .expect("Invalid interrupt vector");
    handler(vector, interrupt_id);
    apic::end_of_interrupt();

    // Now that we have signaled the end of the interrupt, we are out of the
    // interrupt context. If we need to call the scheduler, do it.
    sched::scheduler_lock().run_scheduler_if_needed();
}

fn default_external_interrupt_handler(vector: u8, interrupt_id: InterruptHandlerID) {
    panic!("Unhandled external interrupt: vector: {vector}, interrupt_id: {interrupt_id}");
}

/// This is passed to interrupt handler functions to disambiguate multiple IRQs
/// using the same function.
pub(crate) type InterruptHandlerID = u32;

/// Function called by `common_external_interrupt_handler`. Installed with
/// `install_interrupt`.
pub(crate) type InterruptHandler = fn(vector: u8, InterruptHandlerID);

/// Holds the interrupt handlers for external interrupts. This is a static
/// because we need to be able to access it from the interrupt handlers, which
/// are `extern "x86-interrupt"`.
static EXTERNAL_INTERRUPT_HANDLERS: SpinLock<
    [(InterruptHandlerID, InterruptHandler); NUM_INTERRUPT_VECTORS],
> = SpinLock::new([(0, default_external_interrupt_handler); NUM_INTERRUPT_VECTORS]);

lazy_static!(
    static ref IDT: InterruptDescriptorTable = {
        // TODO: Set up a handler for all of the possible CPU exceptions!
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault
            .set_handler_fn(general_protection_fault_handler);
        unsafe {
            // set_stack_index is unsafe because the caller must ensure that the
            // used index is valid and not already used for another exception.
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }

        // Set up stub handlers for all external interrupts.
        seq!(N in 32..255 {
            external_stub_interrupt_handler!(idt N);
        });

        idt
    };
);

pub(crate) fn init_interrupts() {
    IDT.load();
    disable_pic();
    x86_64::instructions::interrupts::enable();
}

// Even though we are disabling the PIC in lieu of the APIC, we still need to
// set the offsets to avoid CPU exceptions. We should not use indexes 32 through 47
// for APIC interrupts so we avoid spurious PIC interrupts.
const MASTER_PIC_OFFSET: u8 = FIRST_EXTERNAL_INTERRUPT_VECTOR as u8;
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

static NEXT_OPEN_INTERRUPT_INDEX: AtomicU8 = AtomicU8::new(APIC_INTERRUPT_START_OFFSET);

/// Install an interrupt handler in the IDT. Uses the next open interrupt index
/// and returns the used index.
pub(crate) fn install_interrupt(interrupt_id: InterruptHandlerID, handler: InterruptHandler) -> u8 {
    let interrupt_index = NEXT_OPEN_INTERRUPT_INDEX.fetch_add(1, Ordering::Relaxed);
    assert!(
        interrupt_index < SPURIOUS_INTERRUPT_VECTOR_INDEX,
        "Ran out of interrupt vectors"
    );

    EXTERNAL_INTERRUPT_HANDLERS.lock()[interrupt_index as usize] = (interrupt_id, handler);

    interrupt_index
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    serial_println!("EXCEPTION: BREAKPOINT");
    serial_println!("{stack_frame:#?}");
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    serial_println!("EXCEPTION: PAGE FAULT");
    serial_println!("Accessed Address (CR2): {:x?}", Cr2::read_raw());
    serial_println!("Error Code: {error_code:?}");
    serial_println!("{stack_frame:#?}");

    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    serial_println!("EXCEPTION: GENERAL PROTECTION FAULT");
    serial_println!("error_code: {error_code}");
    serial_println!("{stack_frame:#?}");
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}
