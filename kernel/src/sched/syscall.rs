use core::arch::asm;

use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

use crate::define_per_cpu_u64;
use crate::percpu::get_processor_id_no_guard;

use super::schedcore::{current_task_id, kill_current_task};
use super::task::TaskExitCode;

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

    // Use SFMASK register to disable interrupts when executing a syscall. This
    // is important because we use swapgs and we mess with the stack.
    x86_64::registers::model_specific::SFMask::write(RFlags::INTERRUPT_FLAG);

    // Set syscall handler address via LSTAR register
    let syscall_handler_addr = VirtAddr::new(syscall_handler as usize as u64);
    x86_64::registers::model_specific::LStar::write(syscall_handler_addr);
}

define_per_cpu_u64!(
    /// Used during syscalls to store and restore the top of the kernel stack.
    pub(super) TOP_OF_KERNEL_STACK
);

define_per_cpu_u64!(
    /// Used during syscalls to store and restore the user stack
    pub(super) USER_STACK_SCRATCH
);

#[naked]
pub(super) unsafe extern "C" fn syscall_handler() {
    unsafe {
        asm!(
            // Swap out the user GS base for the kernel GS base and restore the
            // kernel stack.
            "swapgs",
            "mov gs:{user_stack}, rsp",
            "mov rsp, gs:{kernel_stack}",

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

            // Set up syscall handler arguments. We use the Linux x86_64 syscall
            // calling convention, which uses rdi, rsi, rdx, r10, r8, and r9.
            // The standard x86_64 C calling convention uses rdi, rsi, rdx, rcx,
            // r8, r9, for arguments; note that r10 and rcx are different for
            // the 4th arg. This is because the syscall instruction clobbers
            // rcx. Therefore, we just set rcx to r10, and then our syscall
            // handler will use r10 for the 4th arg.
            "mov rcx, r10",

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

            // Restore user stack
            "mov gs:{kernel_stack}, rsp",
            "mov rsp, gs:{user_stack}",
            "swapgs",

            // Return to userspace
            "sysretq",
            kernel_stack = sym TOP_OF_KERNEL_STACK,
            user_stack = sym USER_STACK_SCRATCH,
            syscall_handler_inner = sym syscall_handler_inner,
            options(noreturn),
        )
    }
}

#[allow(clippy::similar_names)]
extern "C" fn syscall_handler_inner(rdi: u64, rsi: u64, rdx: u64, r10: u64, r8: u64, r9: u64) {
    let processor_id = get_processor_id_no_guard();
    let task_id = current_task_id();
    log::warn!("syscall handler! processor: {processor_id:?}, task: {task_id:?}, rdi: {rdi:#x}, rsi: {rsi:#x}, rdx: {rdx:#x}, r10: {r10:#x}, r8: {r8:#x}, r9: {r9:#x}");

    let syscall_num = rdi;

    let handler = SYSCALL_HANDLERS
        .get(syscall_num as usize)
        .into_iter()
        .flatten()
        .next();
    #[allow(clippy::option_if_let_else)]
    match handler {
        Some(handler) => handler(rsi, rdx, r10, r8, r9),
        None => {
            log::warn!(
                "Unknown syscall {syscall_num} called with args ({rsi}, {rdx}, {r10}, {r8}, {r9})"
            );
        }
    };
}

type SyscallHandler = fn(u64, u64, u64, u64, u64);

static SYSCALL_HANDLERS: [Option<SyscallHandler>; 2] = [
    Some(syscall_exit), // 0
    Some(syscall_print),
];

fn syscall_exit(exit_code: u64, _: u64, _: u64, _: u64, _: u64) {
    kill_current_task(TaskExitCode::from(exit_code));
}

fn syscall_print(data_ptr: u64, data_len: u64, _: u64, _: u64, _: u64) {
    let s = unsafe { core::slice::from_raw_parts(data_ptr as *const u8, data_len as usize) };
    let s = core::str::from_utf8(s).unwrap();
    log::info!("PRINT SYSCALL: {}", s);
}
