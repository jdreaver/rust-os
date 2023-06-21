//! Per CPU storage using the `gs` segment register. See:
//!
//! - <https://www.kernel.org/doc/Documentation/this_cpu_ops.txt>
//! - <https://elixir.bootlin.com/linux/latest/source/include/linux/percpu.h>
//! - <https://elixir.bootlin.com/linux/latest/source/mm/percpu.c>
//! - <https://elixir.bootlin.com/linux/latest/source/arch/x86/include/asm/percpu.h>
//! - <https://elixir.bootlin.com/linux/latest/source/arch/x86/kernel/setup_percpu.c>

use core::arch::asm;
use core::mem::offset_of;

use paste::paste;
use x86_64::VirtAddr;

use crate::apic::ProcessorID;
use crate::barrier::barrier;
use crate::memory::HIGHER_HALF_START;

//
// Experiment using linker variables for percpu area
//

extern "C" {
    static _percpu_start: u8;
    static _percpu_end: u8;
    static _percpu_load: u8;
}

#[allow(dead_code)]
pub(crate) fn test_linker_percpu() {
    log::warn!("_percpu_load: {:p}", unsafe {
        core::ptr::addr_of!(_percpu_load)
    });
    log::warn!("_percpu_start: {:p}", unsafe {
        core::ptr::addr_of!(_percpu_start)
    });
    log::warn!("MY_PERCPU_U8 = {:p}", &MY_PERCPU_U8);
    log::warn!("MY_PERCPU_U16 = {:p}", &MY_PERCPU_U16);
    log::warn!("MY_PERCPU_U32 = {:p}", &MY_PERCPU_U32);
    log::warn!("MY_PERCPU_I64 = {:p}", &MY_PERCPU_I64);
    log::warn!("MY_PERCPU_U64 = {:p}", &MY_PERCPU_U64);
    log::warn!("_percpu_end: {:p}", unsafe {
        core::ptr::addr_of!(_percpu_end)
    });
}

macro_rules! raw_define_per_cpu {
    ($name:ident, $type:ty, $mov_size:literal, $inc_dec_size:literal, $reg_class:ident) => {
        #[link_section = ".percpu"]
        static $name: $type = 0;

        paste! {
            #[allow(dead_code)]
            #[allow(non_snake_case)]
            fn [<get_per_cpu_ $name>]() -> $type {
                let val: $type;
                unsafe {
                    asm!(
                        concat!("mov {0:", $mov_size, "}, gs:{1}"),
                        out($reg_class) val,
                        sym $name,
                        options(nomem, nostack, preserves_flags),
                    );
                }
                val
            }
        }

        paste! {
            #[allow(dead_code)]
            #[allow(non_snake_case)]
            fn [<set_per_cpu_ $name>](x: $type) {
                unsafe {
                    asm!(
                        concat!("mov gs:{0}, {1:", $mov_size, "}"),
                        sym $name,
                        in($reg_class) x,
                        options(nomem, nostack, preserves_flags),
                    );
                }
            }
        }

        paste! {
            #[allow(dead_code)]
            #[allow(non_snake_case)]
            fn [<inc_per_cpu_ $name>]() {
                unsafe {
                    asm!(
                        concat!("inc ", $inc_dec_size, " ptr gs:{}"),
                        sym $name,
                        options(nomem, nostack, preserves_flags),
                    );
                }
            }
        }

        paste! {
            #[allow(dead_code)]
            #[allow(non_snake_case)]
            fn [<dec_per_cpu_ $name>]() {
                unsafe {
                    asm!(
                        concat!("dec ", $inc_dec_size, " ptr gs:{}"),
                        sym $name,
                        options(nomem, nostack, preserves_flags),
                    );
                }
            }
        }
    };
}

macro_rules! raw_define_per_cpu_1 {
    ($name:ident, $type:ty) => {
        raw_define_per_cpu!($name, $type, "", "byte", reg_byte);
    };
}

macro_rules! raw_define_per_cpu_2 {
    ($name:ident, $type:ty) => {
        raw_define_per_cpu!($name, $type, "x", "word", reg);
    };
}

macro_rules! raw_define_per_cpu_4 {
    ($name:ident, $type:ty) => {
        raw_define_per_cpu!($name, $type, "e", "dword", reg);
    };
}

macro_rules! raw_define_per_cpu_8 {
    ($name:ident, $type:ty) => {
        raw_define_per_cpu!($name, $type, "", "qword", reg);
    };
}

macro_rules! define_per_cpu_u8 {
    ($name:ident) => {
        raw_define_per_cpu_1!($name, u8);
    };
}

macro_rules! define_per_cpu_u16 {
    ($name:ident) => {
        raw_define_per_cpu_2!($name, u16);
    };
}

macro_rules! define_per_cpu_u32 {
    ($name:ident) => {
        raw_define_per_cpu_4!($name, u32);
    };
}

macro_rules! define_per_cpu_i64 {
    ($name:ident) => {
        raw_define_per_cpu_8!($name, i64);
    };
}

macro_rules! define_per_cpu_u64 {
    ($name:ident) => {
        raw_define_per_cpu_8!($name, u64);
    };
}

define_per_cpu_u8!(MY_PERCPU_U8);
define_per_cpu_u16!(MY_PERCPU_U16);
define_per_cpu_u32!(MY_PERCPU_U32);
define_per_cpu_i64!(MY_PERCPU_I64);
define_per_cpu_u64!(MY_PERCPU_U64);

//
// End experiment
//

/// Maximum number of CPUs the kernel supports.
///
/// N.B. This is a u8 because LAPIC IDs are u8s, and we use those as processor
/// IDs (the Intel manual suggests this).
const MAX_CPUS: u8 = 8;

static mut GS_REGISTER_VARS: [GSRegisterVars; MAX_CPUS as usize] = [GSRegisterVars {
    processor_id: 0,
    needs_reschedule: 0,
    current_task_id: 0,
    idle_task_id: 0,
    preempt_count: 0,
    syscall_top_of_kernel_stack: 0,
}; MAX_CPUS as usize];

/// Variables that are stored on the GS register.
#[derive(Debug, Clone, Copy)]
// repr(C) so offsets are stable, align(64) to prevent sharing cache lines
#[repr(C, align(64))]
struct GSRegisterVars {
    /// The processor ID of the current CPU.
    pub(crate) processor_id: u8,

    /// When nonzero, the scheduler needs to run. This is set in contexts that
    /// can't run the scheduler (like interrupts), or in places that want to
    /// indicate the scheduler should run, but don't want it to run immediately.
    pub(crate) needs_reschedule: u8,

    /// The `TaskId` of the currently running task.
    pub(crate) current_task_id: u32,

    /// The `TaskId` for the idle task for the current CPU. Every CPU has its
    /// own idle task.
    pub(crate) idle_task_id: u32,

    /// When preempt_count > 0, preemption is disabled, which means the
    /// scheduler will not switch off the current task.
    pub(crate) preempt_count: i32,

    /// Used during syscalls to store and restore the top of the kernel stack.
    pub(crate) syscall_top_of_kernel_stack: u64,
}

/// Initializes per CPU storage on the current CPU.
pub(crate) fn init_current_cpu(processor_id: ProcessorID) {
    let vars = unsafe {
        GS_REGISTER_VARS
            .get_mut(processor_id.0 as usize)
            .unwrap_or_else(|| {
                panic!(
                    "Processor ID {} is too large for the maximum number of CPUs ({})",
                    processor_id.0, MAX_CPUS
                )
            })
    };
    let addr = VirtAddr::new(vars as *const GSRegisterVars as u64);
    x86_64::registers::model_specific::GsBase::write(addr);
    set_per_cpu_processor_id(processor_id.0);
}

macro_rules! get_per_cpu {
    ($vis:vis, $field:ident, $size:literal, $reg_class:ident, $type:ty) => {
        paste! {
            $vis fn [<get_per_cpu_ $field>]() -> $type {
                let field: $type;
                unsafe {
                    asm!(
                        concat!("mov {0:", $size, "}, gs:{1}"),
                        out($reg_class) field,
                        const offset_of!(GSRegisterVars, $field),
                        options(nomem, nostack, preserves_flags),
                    );
                }
                field
            }
        }
    };
}

macro_rules! set_per_cpu {
    ($vis:vis, $field:ident, $size:literal, $reg_class:ident, $type:ty) => {
        paste! {
            $vis fn [<set_per_cpu_ $field>](x: $type) {
                unsafe {
                    asm!(
                        concat!("mov gs:{0}, {1:", $size, "}"),
                        const offset_of!(GSRegisterVars, $field),
                        in($reg_class) x,
                        options(nomem, nostack, preserves_flags),
                    );
                }
            }
        }
    };
}

macro_rules! inc_per_cpu {
    ($vis:vis, $field:ident, $size:literal, $type:ty) => {
        paste! {
            $vis fn [<inc_per_cpu_ $field>]() {
                unsafe {
                    asm!(
                        concat!("inc ", $size, " ptr gs:{}"),
                        const offset_of!(GSRegisterVars, $field),
                        options(nomem, nostack, preserves_flags),
                    );
                }
            }
        }
    };
}

macro_rules! dec_per_cpu {
    ($vis:vis, $field:ident, $size:literal, $type:ty) => {
        paste! {
            $vis fn [<dec_per_cpu_ $field>]() {
                unsafe {
                    asm!(
                        concat!("dec ", $size, " ptr gs:{}"),
                        const offset_of!(GSRegisterVars, $field),
                        options(nomem, nostack, preserves_flags),
                    );
                }
            }
        }
    };
}

macro_rules! get_per_cpu_1 {
    ($vis:vis, $field:ident, $type:ty) => {
        get_per_cpu!($vis, $field, "", reg_byte, $type);
    };
}

macro_rules! set_per_cpu_1 {
    ($vis:vis, $field:ident, $type:ty) => {
        set_per_cpu!($vis, $field, "", reg_byte, $type);
    };
}

macro_rules! get_per_cpu_4 {
    ($vis:vis, $field:ident, $type:ty) => {
        get_per_cpu!($vis, $field, "e", reg, $type);
    };
}

macro_rules! set_per_cpu_4 {
    ($vis:vis, $field:ident, $type:ty) => {
        set_per_cpu!($vis, $field, "e", reg, $type);
    };
}

macro_rules! inc_per_cpu_4 {
    ($vis:vis, $field:ident, $type:ty) => {
        inc_per_cpu!($vis, $field, "dword", $type);
    };
}

macro_rules! dec_per_cpu_4 {
    ($vis:vis, $field:ident, $type:ty) => {
        dec_per_cpu!($vis, $field, "dword", $type);
    };
}

get_per_cpu_1!(pub(crate), processor_id, u8);
set_per_cpu_1!(pub(crate), processor_id, u8);

get_per_cpu_1!(pub(crate), needs_reschedule, u8);
set_per_cpu_1!(pub(crate), needs_reschedule, u8);

get_per_cpu_4!(pub(crate), current_task_id, u32);
set_per_cpu_4!(pub(crate), current_task_id, u32);

get_per_cpu_4!(pub(crate), idle_task_id, u32);
set_per_cpu_4!(pub(crate), idle_task_id, u32);

get_per_cpu_4!(pub(crate), preempt_count, i32);
set_per_cpu_4!(pub(crate), preempt_count, i32);
inc_per_cpu_4!(pub(crate), preempt_count, i32);
dec_per_cpu_4!(pub(crate), preempt_count, i32);

/// Simple type that disables preemption while it is alive, and re-enables it
/// when dropped.
pub(crate) struct PreemptGuard;

impl PreemptGuard {
    pub(crate) fn new() -> Self {
        inc_per_cpu_preempt_count();
        barrier();
        Self
    }
}

impl Drop for PreemptGuard {
    fn drop(&mut self) {
        barrier();
        dec_per_cpu_preempt_count();
    }
}

pub(crate) const PER_CPU_SYSCALL_TOP_OF_KERNEL_STACK: u64 =
    offset_of!(GSRegisterVars, syscall_top_of_kernel_stack) as u64;
// get_per_cpu!(syscall_top_of_kernel_stack, u64);
// set_per_cpu!(syscall_top_of_kernel_stack, u64);

/// Tests if the current gsbase is the kernel's gsbase. This is needed in
/// exception handlers, which can be called from userspace, so they know to do
/// swapgs.
///
/// See <https://elixir.bootlin.com/linux/v6.3.7/source/Documentation/x86/entry_64.rst>
pub(crate) fn gsbase_is_kernel() -> bool {
    // Assume that if the virtual address for GSBASE is above
    // `HIGHER_HALF_START` (which should be 0xffff_8000_0000_0000) then we are
    // in the kernel.
    let gsbase = x86_64::registers::model_specific::GsBase::read();
    gsbase >= VirtAddr::new(HIGHER_HALF_START)
}
