//! Per CPU storage using the `gs` segment register. See:
//!
//! - <https://www.kernel.org/doc/Documentation/this_cpu_ops.txt>
//! - <https://elixir.bootlin.com/linux/latest/source/include/linux/percpu.h>
//! - <https://elixir.bootlin.com/linux/latest/source/mm/percpu.c>
//! - <https://elixir.bootlin.com/linux/latest/source/arch/x86/include/asm/percpu.h>
//! - <https://elixir.bootlin.com/linux/latest/source/arch/x86/kernel/setup_percpu.c>

use x86_64::VirtAddr;

use crate::apic::ProcessorID;

/// Macro to create a per CPU variable with various functions to access it.
#[macro_export]
macro_rules! raw_define_per_cpu {
    ($(#[$($attrss:tt)*])* $name:ident, $type:ty, $mov_size:literal, $inc_dec_size:literal, $reg_class:ident) => {
        #[link_section = ".percpu"]
        $(#[$($attrss)*])*
        pub(super) static $name: $type = 0;

        ::paste::paste! {
            #[allow(dead_code)]
            #[allow(non_snake_case)]
            fn [<get_per_cpu_ $name>]() -> $type {
                let val: $type;
                unsafe {
                    core::arch::asm!(
                        concat!("mov {0:", $mov_size, "}, gs:{1}"),
                        out($reg_class) val,
                        sym $name,
                        options(nomem, nostack, preserves_flags),
                    );
                }
                val
            }
        }

        ::paste::paste! {
            #[allow(dead_code)]
            #[allow(non_snake_case)]
            fn [<set_per_cpu_ $name>](x: $type) {
                unsafe {
                    core::arch::asm!(
                        concat!("mov gs:{0}, {1:", $mov_size, "}"),
                        sym $name,
                        in($reg_class) x,
                        options(nomem, nostack, preserves_flags),
                    );
                }
            }
        }

        ::paste::paste! {
            #[allow(dead_code)]
            #[allow(non_snake_case)]
            fn [<inc_per_cpu_ $name>]() {
                unsafe {
                    core::arch::asm!(
                        concat!("inc ", $inc_dec_size, " ptr gs:{}"),
                        sym $name,
                        options(nomem, nostack, preserves_flags),
                    );
                }
            }
        }

        ::paste::paste! {
            #[allow(dead_code)]
            #[allow(non_snake_case)]
            fn [<dec_per_cpu_ $name>]() {
                unsafe {
                    core::arch::asm!(
                        concat!("dec ", $inc_dec_size, " ptr gs:{}"),
                        sym $name,
                        options(nomem, nostack, preserves_flags),
                    );
                }
            }
        }
    };
}

//
// Macros to call raw_define_per_cpu with various sizes
//

#[macro_export]
macro_rules! raw_define_per_cpu_1 {
    ($(#[$($attrss:tt)*])* $name:ident, $type:ty) => {
        $crate::raw_define_per_cpu!($(#[$($attrss)*])* $name, $type, "", "byte", reg_byte);
    };
}

#[macro_export]
macro_rules! raw_define_per_cpu_2 {
    ($(#[$($attrss:tt)*])* $name:ident, $type:ty) => {
        $crate::raw_define_per_cpu!($(#[$($attrss)*])* $name, $type, "x", "word", reg);
    };
}

#[macro_export]
macro_rules! raw_define_per_cpu_4 {
    ($(#[$($attrss:tt)*])* $name:ident, $type:ty) => {
        $crate::raw_define_per_cpu!($(#[$($attrss)*])* $name, $type, "e", "dword", reg);
    };
}

#[macro_export]
macro_rules! raw_define_per_cpu_8 {
    ($(#[$($attrss:tt)*])* $name:ident, $type:ty) => {
        $crate::raw_define_per_cpu!($(#[$($attrss)*])* $name, $type, "", "qword", reg);
    };
}

//
// Exported macros using concrete types
//

#[macro_export]
macro_rules! define_per_cpu_u8 {
    ($(#[$($attrss:tt)*])* $name:ident) => {
        $crate::raw_define_per_cpu_1!($(#[$($attrss)*])* $name, u8);
    };
}

#[macro_export]
macro_rules! define_per_cpu_u16 {
    ($(#[$($attrss:tt)*])* $name:ident) => {
        $crate::raw_define_per_cpu_2!($(#[$($attrss)*])* $name, u16);
    };
}

#[macro_export]
macro_rules! define_per_cpu_u32 {
    ($(#[$($attrss:tt)*])* $name:ident) => {
        $crate::raw_define_per_cpu_4!($(#[$($attrss)*])* $name, u32);
    };
}

#[macro_export]
macro_rules! define_per_cpu_i64 {
    ($(#[$($attrss:tt)*])* $name:ident) => {
        $crate::raw_define_per_cpu_8!($(#[$($attrss)*])* $name, i64);
    };
}

#[macro_export]
macro_rules! define_per_cpu_u64 {
    ($(#[$($attrss:tt)*])* $name:ident) => {
        $crate::raw_define_per_cpu_8!($(#[$($attrss)*])* $name, u64);
    };
}

define_per_cpu_u8!(MY_PERCPU_U8);
define_per_cpu_u16!(MY_PERCPU_U16);
define_per_cpu_u32!(MY_PERCPU_U32);
define_per_cpu_i64!(MY_PERCPU_I64);
define_per_cpu_u64!(MY_PERCPU_U64);

/// Maximum number of CPUs the kernel supports.
///
/// N.B. This is a u8 because LAPIC IDs are u8s, and we use those as processor
/// IDs (the Intel manual suggests this).
const MAX_CPUS: u8 = 8;

const X86_64_CACHE_LINE_SIZE_BYTES: usize = 64;

/// How large to make each per-CPU area.
///
/// N.B. This should always be a multiple of the cache line size so we don't
/// share cache lines between CPUs.
#[allow(clippy::identity_op)]
const PER_CPU_AREA_SIZE_BYTES: usize = 1 * X86_64_CACHE_LINE_SIZE_BYTES;

/// Memory locations to store per CPU variables.
///
/// TODO: In the future we could dynamically allocate these, but it is a pain
/// before the physical memory allocator is initialized and it is nice not to
/// depend on it.
static mut PER_CPU_AREAS: [[u8; PER_CPU_AREA_SIZE_BYTES]; MAX_CPUS as usize] =
    [[0; PER_CPU_AREA_SIZE_BYTES]; MAX_CPUS as usize];

extern "C" {
    static _percpu_start: u8;
    static _percpu_end: u8;
    static _percpu_load: u8;
}

/// Initializes per CPU storage on the current CPU.
pub(crate) fn init_current_cpu(processor_id: ProcessorID) {
    // Get this CPU's per-CPU area.
    let this_cpu_area = unsafe {
        PER_CPU_AREAS
            .get(processor_id.0 as usize)
            .unwrap_or_else(|| {
                panic!(
                    "Processor ID {} is too large for the maximum number of CPUs ({})",
                    processor_id.0, MAX_CPUS
                )
            })
    };

    // Ensure the per-CPU area is large enough
    let per_cpu_start = unsafe { core::ptr::addr_of!(_percpu_start) as usize };
    let per_cpu_end = unsafe { core::ptr::addr_of!(_percpu_end) as usize };
    let per_cpu_area_size = per_cpu_end - per_cpu_start;
    assert!(
        this_cpu_area.len() <= per_cpu_area_size,
        "Per CPU area is too small. Need to increase PER_CPU_AREA_SIZE_BYTES"
    );

    // Store the per-CPU area in the GSBASE register so `gs:{offset}` points to
    // per-CPU variables.
    let addr = VirtAddr::new(core::ptr::addr_of!(this_cpu_area) as u64);
    x86_64::registers::model_specific::GsBase::write(addr);
    set_per_cpu_PROCESSOR_ID(processor_id.0);
}

define_per_cpu_u8!(
    /// The processor ID of the current CPU.
    PROCESSOR_ID
);

pub(crate) fn get_processor_id() -> ProcessorID {
    ProcessorID(get_per_cpu_PROCESSOR_ID())
}
