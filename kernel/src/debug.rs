use core::arch::asm;

/// Generates a stack trace by iterating over the stack frame pointers. Requires
/// `-C force-frame-pointers=yes` to be passed to rustc, otherwise Rust tends to
/// treat `rbp` as a general purpose register.
///
/// See:
/// - <https://techno-coder.github.io/example_os/2018/06/04/A-stack-trace-for-your-OS.html>
/// - <https://doc.rust-lang.org/rustc/codegen-options/index.html#force-frame-pointers>
pub(crate) fn print_stack_trace() {
    log::warn!("Stack trace:");
    let mut rbp: *const u64;
    unsafe {
        asm!("mov {}, rbp", out(reg) rbp);
    }
    while !rbp.is_null() {
        let return_address = unsafe { *(rbp.offset(1)) };
        log::warn!("        {:#x}", return_address);
        rbp = unsafe { *(rbp) as *const u64 };
    }
}
