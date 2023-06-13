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
    pub(crate) current_task_id: u32,
    pub(crate) idle_task_id: u32,
    pub(crate) syscall_top_of_kernel_stack: u64,
}

/// Initializes per CPU storage on the current CPU.
pub(crate) fn init_current_cpu() {
    let vars = Box::new(PerCPUVars {
        current_task_id: 0,
        idle_task_id: 0,
        syscall_top_of_kernel_stack: 0,
    });
    let addr = VirtAddr::new(Box::leak(vars) as *mut PerCPUVars as u64);
    x86_64::registers::model_specific::GsBase::write(addr);
}

macro_rules! get_per_cpu {
    ($field:ident, $size:literal, $type:ty) => {
        paste! {
            pub(crate) fn [<get_per_cpu_ $field>]() -> $type {
                let field: $type;
                unsafe {
                    asm!(
                        concat!("mov {0:", $size, "}, gs:{1}"),
                        out(reg) field,
                        const offset_of!(PerCPUVars, $field),
                        options(nomem, nostack, preserves_flags),
                    );
                }
                field
            }
        }
    };
}

macro_rules! set_per_cpu {
    ($field:ident, $size:literal, $type:ty) => {
        paste! {
            pub(crate) fn [<set_per_cpu_ $field>](x: $type) {
                unsafe {
                    asm!(
                        concat!("mov gs:{0}, {1:", $size, "}"),
                        const offset_of!(PerCPUVars, $field),
                        in(reg) x,
                        options(nomem, nostack, preserves_flags),
                    );
                }
            }
        }
    };
}

macro_rules! get_per_cpu_4 {
    ($field:ident, $type:ty) => {
        get_per_cpu!($field, "e", $type);
    };
}

macro_rules! set_per_cpu_4 {
    ($field:ident, $type:ty) => {
        set_per_cpu!($field, "e", $type);
    };
}

get_per_cpu_4!(current_task_id, u32);
set_per_cpu_4!(current_task_id, u32);

get_per_cpu_4!(idle_task_id, u32);
set_per_cpu_4!(idle_task_id, u32);

pub(crate) const PER_CPU_SYSCALL_TOP_OF_KERNEL_STACK: u64 =
    offset_of!(PerCPUVars, syscall_top_of_kernel_stack) as u64;
// get_per_cpu!(syscall_top_of_kernel_stack, u64);
// set_per_cpu!(syscall_top_of_kernel_stack, u64);

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
