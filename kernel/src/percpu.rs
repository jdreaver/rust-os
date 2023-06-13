//! Per CPU storage using the `gs` segment register. See:
//!
//! - <https://www.kernel.org/doc/Documentation/this_cpu_ops.txt>
//! - <https://elixir.bootlin.com/linux/latest/source/include/linux/percpu.h>
//! - <https://elixir.bootlin.com/linux/latest/source/arch/x86/include/asm/percpu.h>

use alloc::boxed::Box;
use core::arch::asm;
use core::mem::offset_of;

use x86_64::VirtAddr;

#[derive(Debug)]
// repr(C) so offsets are stable, align(64) to prevent sharing cache lines
#[repr(C, align(64))]
pub(crate) struct PerCPUVars {
    pub(crate) test: u64,
}

/// Initializes per CPU storage on the current CPU.
pub(crate) fn init_current_cpu() {
    let vars = Box::new(PerCPUVars { test: 0xdead_beef });
    let addr = VirtAddr::new(Box::leak(vars) as *mut PerCPUVars as u64);
    x86_64::registers::model_specific::GsBase::write(addr);
}

pub(crate) fn get_current_cpu_test() -> u64 {
    let test: u64;
    unsafe {
        asm!(
            "mov {0}, gs:{1}",
            out(reg) test,
            const offset_of!(PerCPUVars, test),
            options(nomem, nostack, preserves_flags),
        );
    }
    test
}

pub(crate) fn inc_current_cpu_test() {
    unsafe {
        asm!(
            "inc qword ptr gs:{}",
            const offset_of!(PerCPUVars, test),
            options(nomem, nostack, preserves_flags),
        );
    }
}
