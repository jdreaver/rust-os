use core::arch::asm;
use core::sync::atomic::AtomicU64;

use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

use crate::sched::{self, TaskExitCode};

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

// TODO: Kernel stack location per CPU instead of a single global
pub(super) static KERNEL_STACK: AtomicU64 = AtomicU64::new(0);

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
            // Save user stack and restore kernel stack
            "mov r12, rsp", // r12 is callee-saved, and should be preserved
            "mov rsp, [{kernel_stack}]",
            // Set up syscall handler arguments. We use the Linux x86_64 syscall
            // calling convention, which uses rdi, rsi, rdx, r10, r8, and r9.
            // The standard x86_64 C calling convention uses rdi, rsi, rdx, rcx,
            // r8, r9, for arguments; note that r10 and rcx are different for
            // the 4th arg. This is because the syscall instruction clobbers
            // rcx. Therefore, we just set rcx to r10, and then our syscall
            // handler will use r10 for the 4th arg.
            "mov rcx, r10",
            // TODO: Switch to a fresh kernel stack?
            // Call the actual syscall handler
            "call {syscall_handler_inner}",
            // Restore user stack (stored in r12 earlier)
            "mov [{kernel_stack}], rsp",
            "mov rsp, r12",
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
            kernel_stack = sym KERNEL_STACK,
            syscall_handler_inner = sym syscall_handler_inner,
            options(noreturn),
        )
    }
}

#[allow(clippy::similar_names)]
extern "C" fn syscall_handler_inner(rdi: u64, rsi: u64, rdx: u64, r10: u64, r8: u64, r9: u64) {
    log::warn!("syscall handler! rdi: {rdi:#x}, rsi: {rsi:#x}, rdx: {rdx:#x}, r10: {r10:#x}, r8: {r8:#x}, r9: {r9:#x}");

    // Kill the task for now.
    sched::scheduler_lock().kill_current_task(TaskExitCode::ExitSuccess);
}
