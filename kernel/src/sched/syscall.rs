use core::arch::asm;

use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

pub(super) fn syscall_init() {
    // N.B. There is some other initialization done when setting up the GDT for
    // the STAR register to set user and kernel mode segments. See gdt.rs for
    // more details.

    // Enable System Call Extensions (SCE) to allow userspace to use the syscall
    // instruction.
    unsafe {
        x86_64::registers::model_specific::Efer::update(|efer| {
            *efer |= x86_64::registers::model_specific::EferFlags::SYSTEM_CALL_EXTENSIONS;
        });
    }

    // Use SFMASK register to disable interrupts when executing a syscall
    x86_64::registers::model_specific::SFMask::write(RFlags::INTERRUPT_FLAG);

    // Set syscall handler address via LSTAR register
    let syscall_handler_addr = VirtAddr::new(syscall_handler as usize as u64);
    x86_64::registers::model_specific::LStar::write(syscall_handler_addr);
}

#[naked]
pub(super) unsafe extern "C" fn syscall_handler() {
    unsafe {
        asm!(
            // Back up registers for sysret. rcx holds caller's userspace RIP
            // and r11 holds rflags.
            "push rcx",
            "push r11",
            // Callee-saved registers.
            "push rbp",
            "push rbx",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            // Call the actual syscall handler
            "call {syscall_handler_inner}",
            // Restore registers and run systretq to get back to userland.
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop rbx",
            "pop rbp",
            "pop r11",
            "pop rcx",
            "sysretq",
            syscall_handler_inner = sym syscall_handler_inner,
            options(noreturn),
        )
    }
}

#[allow(clippy::similar_names)]
extern "C" fn syscall_handler_inner(rdi: u64, rsi: u64, rdx: u64, r10: u64) {
    // TODO: Set CR3 to kernel page table

    log::warn!("syscall handler! rdi: {rdi:#x}, rsi: {rsi:#x}, rdx: {rdx:#x}, r10: {r10:#x}");

    // TODO: Set CR3 back to task's userspace page table
}
