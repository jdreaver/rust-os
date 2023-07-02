use core::arch::asm;

use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

use crate::define_per_cpu_u64;
use crate::gdt::{USER_CODE_SELECTOR, USER_DATA_SELECTOR};
use crate::percpu::get_processor_id_no_guard;

use super::schedcore::{current_task_id, kill_current_task, run_scheduler};
use super::task::{TaskExitCode, TaskRegisters};

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
            "mov gs:{user_stack_scratch}, rsp",
            "mov rsp, gs:{kernel_stack}",

            // Construct a pointer to the syscall arguments on the stack. Must
            // match TaskRegisters struct order (in reverse).
            "push {user_data_selector}",    // ss
            "push gs:{user_stack_scratch}", // rsp
            "push r11",                     // rflags, part of syscall convention
            "push {user_code_selector}",    // cs
            "push rcx",                     // rip, part of syscall convention
            "push rdi",                     // syscall number
            // Callee-clobbered
            "push rdi",
            "push rsi",
            "push rdx",
            "push rcx",
            "push rax",
            "push r8",
            "push r9",
            "push r10",
            "push r11",
            // Callee-saved
            "push rbx",
            "push rbp",
            "push r12",
            "push r13",
            "push r14",
            "push r15",

            // First arg is pointer to syscall arguments on the stack
            "mov rdi, rsp",

            // Call the actual syscall handler
            "call {syscall_handler_inner}",

            // Restore registers and run systretq to get back to userland.
            // Callee-saved
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop rbp",
            "pop rbx",
            // Callee-clobbered
            "pop r11",
            "pop r10",
            "pop r9",
            "pop r8",
            "pop rax",
            "pop rcx",
            "pop rdx",
            "pop rsi",
            "pop rdi",
            // Syscall number
            "pop rdi",
            // iretq frame
            "pop rcx",
            "add rsp, 8", // cs, ignored
            "pop r11",
            "pop rax",    // rsp, putting in rax for now so we can put it in rsp later
            "add rsp, 8", // ss, ignored

            // Store kernel stack and restore user stack
            "mov gs:{kernel_stack}, rsp",
            "mov rsp, rax", // rsp was popped into rax earlier
            "swapgs",

            // Return to userspace
            "sysretq",
            kernel_stack = sym TOP_OF_KERNEL_STACK,
            user_data_selector = const USER_DATA_SELECTOR.0,
            user_code_selector = const USER_CODE_SELECTOR.0,
            user_stack_scratch = sym USER_STACK_SCRATCH,
            syscall_handler_inner = sym syscall_handler_inner,
            options(noreturn),
        )
    }
}

#[allow(clippy::similar_names)]
extern "C" fn syscall_handler_inner(registers: &mut TaskRegisters) {
    let processor_id = get_processor_id_no_guard();
    let task_id = current_task_id();
    log::warn!(
        "syscall handler! processor: {processor_id:?}, task: {task_id:?}, registers: {registers:x?}"
    );

    let syscall_num = registers.syscall_number_or_irq_or_error_code;

    let arg1 = registers.rsi;
    let arg2 = registers.rdx;
    let arg3 = registers.r10;
    let arg4 = registers.r8;
    let arg5 = registers.r9;

    let handler = SYSCALL_HANDLERS
        .get(syscall_num as usize)
        .into_iter()
        .flatten()
        .next();
    #[allow(clippy::option_if_let_else)]
    match handler {
        Some(handler) => handler(arg1, arg2, arg3, arg4, arg5),
        None => {
            log::warn!(
                "Unknown syscall {syscall_num} called with args ({arg1}, {arg2}, {arg3}, {arg4}, {arg5})",
            );
        }
    };

    // Run scheduler after syscalls
    run_scheduler();
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
