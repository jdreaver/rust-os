use core::arch::asm;

/// Kernel function that is called when we are starting a userspace task. This
/// is the "entrypoint" to a userspace task, and performs some setup before
/// actually jumping to userspace.
pub(super) extern "C" fn task_userspace_setup(_arg: *const ()) {}

/// Function to go to userspace for the first time in a task.
#[naked]
pub(super) unsafe extern "C" fn _jump_to_userspace(
    user_instruction_pointer: usize,
    user_stack_pointer: usize,
) {
    unsafe {
        asm!(
            "mov rcx, rdi",    // First argument, new instruction pointer
            "mov rsp, rsi",    // Second argument, new stack pointer
            "mov r11, 0x0202", // eflags
            "sysretq",
            options(noreturn),
        )
    }
}
