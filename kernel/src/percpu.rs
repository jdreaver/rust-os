//! Per CPU storage using the `gs` segment register. See:
//!
//! - <https://www.kernel.org/doc/Documentation/this_cpu_ops.txt>
//! - <https://elixir.bootlin.com/linux/latest/source/include/linux/percpu.h>
//! - <https://elixir.bootlin.com/linux/latest/source/mm/percpu.c>
//! - <https://elixir.bootlin.com/linux/latest/source/arch/x86/include/asm/percpu.h>
//! - <https://elixir.bootlin.com/linux/latest/source/arch/x86/kernel/setup_percpu.c>

use alloc::boxed::Box;
use core::arch::asm;
use core::mem::offset_of;

use paste::paste;
use x86_64::VirtAddr;

#[derive(Debug)]
// repr(C) so offsets are stable, align(64) to prevent sharing cache lines
#[repr(C, align(64))]
pub(crate) struct PerCPUVars {
    pub(crate) test: u64,
}

/// Initializes per CPU storage on the current CPU.
pub(crate) fn init_current_cpu() {
    let vars = Box::new(PerCPUVars {
        test: 0xdead_beef,
    });
    let addr = VirtAddr::new(Box::leak(vars) as *mut PerCPUVars as u64);
    x86_64::registers::model_specific::GsBase::write(addr);
}

macro_rules! get_per_cpu {
    ($field:ident, $type:ty) => {
        paste! {
            pub(crate) fn [<get_per_cpu_ $field>]() -> $type {
                let field: $type;
                unsafe {
                    asm!(
                        "mov {0}, gs:{1}",
                        out(reg) field,
                        const offset_of!(PerCPUVars, $field),
                        options(nomem, nostack, preserves_flags),
                    );
                }
                field
            }
        }
    }
}

macro_rules! set_per_cpu {
    ($field:ident, $type:ty) => {
        paste! {
            pub(crate) fn [<set_per_cpu_ $field>](x: $type) {
                unsafe {
                    asm!(
                        "mov gs:{0}, {1}",
                        const offset_of!(PerCPUVars, $field),
                        in(reg) x,
                        options(nomem, nostack, preserves_flags),
                    );
                }
            }
        }
    }
}

get_per_cpu!(test, u64);
set_per_cpu!(test, u64);

// Useful if we ever want counters. I'm only keeping this around because it took
// forever to figure out the `inc qword ptr gs:{}` syntax and I don't want to
// relive that.
//
// pub(crate) fn inc_per_cpu_test() {
//     unsafe {
//         asm!(
//             "inc qword ptr gs:{}",
//             const offset_of!(PerCPUVars, test),
//             options(nomem, nostack, preserves_flags),
//         );
//     }
// }
