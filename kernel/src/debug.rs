use core::arch::asm;

use crate::boot_info;

/// Generates a stack trace by iterating over the stack frame pointers. Requires
/// `-C force-frame-pointers=yes` to be passed to rustc, otherwise Rust tends to
/// treat `rbp` as a general purpose register.
///
/// See:
/// - <https://techno-coder.github.io/example_os/2018/06/04/A-stack-trace-for-your-OS.html>
/// - <https://doc.rust-lang.org/rustc/codegen-options/index.html#force-frame-pointers>
pub(crate) fn print_stack_trace() {
    let boot_info_data = boot_info::boot_info();

    log::warn!("Stack trace:");
    let mut rbp: *const u64;
    unsafe {
        asm!("mov {}, rbp", out(reg) rbp);
    }
    while !rbp.is_null() {
        let return_address = unsafe { *(rbp.offset(1)) };
        let location = find_symbol_in_map_file(boot_info_data, return_address).unwrap_or("???");
        log::warn!("  {return_address:#x} [{location}]");
        rbp = unsafe { *(rbp) as *const u64 };
    }
}

fn find_symbol_in_map_file(
    boot_info_data: &boot_info::BootInfo,
    address: u64,
) -> Option<&'static str> {
    let map_file = boot_info_data.kernel_symbol_map_file.as_ref()?;
    map_file.find_function_symbol_for_instruction_address(address)
}
