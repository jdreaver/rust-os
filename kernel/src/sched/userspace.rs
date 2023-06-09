use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::arch::asm;

use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

use crate::memory::{
    allocate_and_map_pages, set_page_flags, Page, PageRange, PageSize, PageTableEntryFlags,
};
use crate::{elf, task_creator_box, vfs};

use super::schedcore::current_task;
use super::syscall::TOP_OF_KERNEL_STACK;
use super::task::TaskId;

/// Parameters to create a new process.
pub(crate) struct ExecParams {
    pub(crate) path: vfs::FilePath,
    pub(crate) args: Vec<String>,
}

pub(crate) fn new_userspace_task(params: ExecParams) -> TaskId {
    create_userspace_task(params.path.as_string(), Box::new(params))
}

task_creator_box!(create_userspace_task, ExecParams, task_userspace_setup);

/// Kernel function that is called when we are starting a userspace task. This
/// is the "entrypoint" to a userspace task, and performs some setup before
/// actually jumping to userspace.
extern "C" fn task_userspace_setup(params: Box<ExecParams>) {
    let path = &params.path;
    let inode = match vfs::get_path_inode(path) {
        Ok(inode) => inode,
        Err(e) => {
            log::warn!("Failed to get inode for path: {e:?}");
            return;
        }
    };

    let vfs::InodeType::File(mut file) = inode.inode_type else {
        log::warn!("Path {path} not a file");
        return;
    };

    let bytes = file.read_all();
    let elf_exe = match elf::ElfExecutableHeader::parse(&bytes) {
        Ok(exe) => exe,
        Err(e) => {
            log::warn!("Failed to parse ELF: {e:?}");
            return;
        }
    };

    let instruction_ptr = elf_exe.entrypoint;
    let stack_ptr = set_up_elf_segments(&elf_exe, &params);

    // N.B. It is important that jump_to_userspace is marked as returning !,
    // which means it never returns, because I _think_ that the compiler will
    // properly clean up all the other stuff in this function. Before I had `!`
    // I was getting some intermittent page faults.
    drop(params);
    drop(file);
    drop(elf_exe);
    drop(bytes);

    unsafe {
        jump_to_userspace(instruction_ptr, stack_ptr);
    };
}

// Separate function so we can clean up before jump_to_userspace, which never returns
fn set_up_elf_segments(elf_exe: &elf::ElfExecutableHeader, params: &ExecParams) -> VirtAddr {
    let task = current_task();
    let mut table = task.page_table.lock();

    // Map ELF segments to userspace addresses
    for segment in &elf_exe.loadable_segments {
        assert!(segment.alignment as usize == PageSize::Size4KiB.size_bytes());

        let segment_data = elf_exe
            .parsed
            .segment_data(&segment.raw_header)
            .expect("failed to get segment data");
        let start_page = Page::from_start_addr(segment.vaddr, PageSize::Size4KiB);
        let mut user_pages = PageRange::from_num_bytes(start_page, segment.mem_size as usize);

        let initial_flags = PageTableEntryFlags::PRESENT
            | PageTableEntryFlags::WRITABLE
            | PageTableEntryFlags::USER_ACCESSIBLE;

        allocate_and_map_pages(&mut table, user_pages.iter(), initial_flags)
            .expect("failed to map segment pages");
        user_pages.as_byte_slice()[..segment_data.len()].copy_from_slice(segment_data);

        let user_flags =
            segment.flags.page_table_entry_flags() | PageTableEntryFlags::USER_ACCESSIBLE;
        set_page_flags(&mut table, user_pages.iter(), user_flags)
            .expect("failed to set segment flags");
    }

    // Allocate a stack
    let stack_start = VirtAddr::new(0x2_1000_0000);
    let stack_page = Page::from_start_addr(stack_start, PageSize::Size4KiB);
    let stack_pages = PageRange::new(stack_page, 4);
    let stack_flags = PageTableEntryFlags::PRESENT
        | PageTableEntryFlags::WRITABLE
        | PageTableEntryFlags::USER_ACCESSIBLE;
    allocate_and_map_pages(&mut table, stack_pages.iter(), stack_flags)
        .expect("failed to map stack pages");

    // Initialize stack. See "3.4 Process Initialization" in the System V AMD64
    // ABI spec, and https://lwn.net/Articles/631631/ for a good explanation.

    let stack_ptr = stack_start + stack_pages.num_bytes();
    let mut stack_ptr = stack_ptr.as_mut_ptr::<u8>();

    // TODO: Add environment variables and auxiliary vector onto stack

    // Push args onto stack as nul-terminated strings (remember first arg is the program path)
    let first_arg = params
        .path
        .components
        .last()
        .map(|s| String::from(s.as_str()))
        .unwrap_or_default();
    let all_args = core::iter::once(&first_arg).chain(params.args.iter());
    let arg_locations = all_args
        .map(|arg| {
            // Write as nul-terminated string
            let arg_ptr = stack_ptr.wrapping_sub(arg.len() + 1);
            unsafe {
                arg_ptr.copy_from_nonoverlapping(arg.as_ptr(), arg.len());
                arg_ptr.add(arg.len()).write(0);
            }
            stack_ptr = arg_ptr;
            arg_ptr as usize
        })
        .collect::<Vec<usize>>();

    // Align stack pointer _down_ to usize alignment (stack grows down)
    let mut stack_ptr: *mut usize = unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        stack_ptr
            .sub(8)
            .add(stack_ptr.align_offset(8))
            .cast::<usize>()
    };
    assert!(
        stack_ptr as usize % core::mem::align_of::<usize>() == 0,
        "stack_ptr {stack_ptr:p} not aligned!"
    );

    // Push argv onto stack
    arg_locations.iter().rev().for_each(|arg_ptr| unsafe {
        stack_ptr = stack_ptr.sub(1);
        stack_ptr.write(*arg_ptr);
    });

    // Push argc onto stack
    unsafe {
        stack_ptr = stack_ptr.sub(1);
        stack_ptr.cast::<usize>().write(arg_locations.len());
    }

    VirtAddr::new(stack_ptr as u64)
}

#[naked]
pub(super) unsafe extern "C" fn jump_to_userspace(
    user_instruction_pointer: VirtAddr,
    user_stack_pointer: VirtAddr,
) {
    unsafe {
        asm!(
            // Store the kernel stack and switch to the user stack
            "mov gs:{kernel_stack}, rsp",
            "mov rsp, rsi",      // Second argument, new stack pointer
            // Set up sysretq arguments
            "mov rcx, rdi",      // First argument, new instruction pointer
            "mov r11, {rflags}", // rflags
            // Swap out the kernel GS base for the user's so userspace can't
            // mess with our GS base.
            "swapgs",
            // Jump to userspace
            "sysretq",
            kernel_stack = sym TOP_OF_KERNEL_STACK,
            rflags = const RFlags::INTERRUPT_FLAG.bits(),
            options(noreturn),
        )
    }
}
