use alloc::boxed::Box;
use core::arch::asm;

use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

use crate::memory::{
    allocate_and_map_pages, set_page_flags, Page, PageRange, PageSize, PageTableEntryFlags,
};
use crate::{elf, gdt, vfs};

use super::schedcore::current_task;
use super::syscall::TOP_OF_KERNEL_STACK;

/// Kernel function that is called when we are starting a userspace task. This
/// is the "entrypoint" to a userspace task, and performs some setup before
/// actually jumping to userspace.
pub(crate) extern "C" fn task_userspace_setup(arg: *const ()) {
    let path: Box<vfs::FilePath> = unsafe { Box::from_raw(arg as *mut vfs::FilePath) };

    let inode = match vfs::get_path_inode(&path) {
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

    let bytes = file.read();
    let elf_exe = match elf::ElfExecutableHeader::parse(&bytes) {
        Ok(exe) => exe,
        Err(e) => {
            log::warn!("Failed to parse ELF: {e:?}");
            return;
        }
    };
    log::info!("ELF header: {:#?}", elf_exe);

    let instruction_ptr = elf_exe.entrypoint;
    let stack_ptr = set_up_elf_segments(&elf_exe);

    let user_code_segment_idx: u64 = u64::from(gdt::selectors().user_code_selector.0);
    let user_data_segment_idx: u64 = u64::from(gdt::selectors().user_data_selector.0);

    // N.B. It is important that jump_to_userspace is marked as returning !,
    // which means it never returns, because I _think_ that the compiler will
    // properly clean up all the other stuff in this function. Before I had `!`
    // I was getting some intermittent page faults.
    drop(path);
    drop(file);
    drop(elf_exe);
    drop(bytes);
    unsafe {
        jump_to_userspace(
            instruction_ptr,
            stack_ptr,
            user_code_segment_idx,
            user_data_segment_idx,
        );
    };
}

// Separate function so we can clean up before jump_to_userspace, which never returns
fn set_up_elf_segments(elf_exe: &elf::ElfExecutableHeader) -> VirtAddr {
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

    #[allow(clippy::let_and_return)]
    let stack_ptr = stack_start + stack_pages.num_bytes();
    stack_ptr
}

/// Function to go to userspace for the first time in a task.
#[naked]
pub(super) unsafe extern "C" fn jump_to_userspace(
    user_instruction_pointer: VirtAddr,
    user_stack_pointer: VirtAddr,
    user_code_segment_idx: u64,
    user_data_segment_idx: u64,
) -> ! {
    unsafe {
        asm!(
            // Store the kernel stack
            "mov gs:{kernel_stack}, rsp",
            // Swap out the kernel GS base for the user's so userspace can't
            // mess with our GS base.
            "swapgs",
            // Set up and execute iretq
            "push rcx",      // Fourth arg, data segment
            "push rsi",      // Second arg, stack pointer
            "push {rflags}", // rflags
            "push rdx",      // Third arg, code segment
            "push rdi",      // First arg, instruction pointer
            "iretq",
            kernel_stack = sym TOP_OF_KERNEL_STACK,
            rflags = const RFlags::INTERRUPT_FLAG.bits(),
            options(noreturn),
        )
    }
}

// TODO: For some reason I couldn't get sysretq to work for the first jump to
// userspace. I had to use iretq instead.
//
// #[naked]
// pub(super) unsafe extern "C" fn jump_to_userspace(
//     user_instruction_pointer: VirtAddr,
//     user_stack_pointer: VirtAddr,
// ) {
//     unsafe {
//         asm!(
//             "mov rcx, rdi",    // First argument, new instruction pointer
//             "mov rsp, rsi",    // Second argument, new stack pointer
//             "mov r11, {rflags}", // rflags
//             "sysretq",
//             rflags = const RFlags::INTERRUPT_FLAG.bits(),
//             options(noreturn),
//         )
//     }
// }
