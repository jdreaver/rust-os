use alloc::boxed::Box;
use core::arch::asm;

use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

use crate::memory::{MapError, Page, PageSize, PageTableEntryFlags, TranslateResult};
use crate::{elf, gdt, memory, vfs};

use super::syscall::TOP_OF_KERNEL_STACK;

static DUMMY_USERSPACE_STACK: &[u8] = &[0; 4096];

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

    // TODO: This is currently a big fat hack and this code is very fragile
    // because it relies on our hack "userspace" function fitting in a single
    // page. Adding code here can break that.

    let instruction_ptr = dummy_hacky_userspace_task as usize;

    // Map our fake code to userspace addresses that userspace has access to
    let instruction_ptr_virt = VirtAddr::new(instruction_ptr as u64);
    let TranslateResult::Mapped(instruction_mapping) = memory::translate_addr(instruction_ptr_virt) else {
        panic!("instruction pointer not mapped")
    };
    let instruction_ptr_phys = instruction_mapping.address();
    let instruction_ptr_page_start = VirtAddr::new(0x2_0000_0000);
    let instruction_ptr_phys_page =
        Page::containing_address(instruction_ptr_phys, PageSize::Size4KiB);

    // Map two of these pages to be safe
    for i in 0..=1 {
        let instruction_ptr_virt_page = Page::from_start_addr(
            instruction_ptr_page_start + i * PageSize::Size4KiB.size_bytes(),
            PageSize::Size4KiB,
        );
        let instruction_ptr_phys_page = Page::from_start_addr(
            instruction_ptr_phys_page.start_addr() + i * PageSize::Size4KiB.size_bytes(),
            PageSize::Size4KiB,
        );
        let flags = PageTableEntryFlags::PRESENT
            | PageTableEntryFlags::WRITABLE
            | PageTableEntryFlags::USER_ACCESSIBLE;
        match memory::map_page(instruction_ptr_virt_page, instruction_ptr_phys_page, flags) {
            Ok(_) | Err(MapError::PageAlreadyMapped { .. }) => {}
            Err(e) => panic!("failed to map instruction page: {:?}", e),
        }
    }

    let instruction_virt = instruction_ptr_page_start + instruction_mapping.offset;

    let stack_ptr_start = VirtAddr::new(DUMMY_USERSPACE_STACK.as_ptr() as u64);
    let TranslateResult::Mapped(stack_mapping) = memory::translate_addr(stack_ptr_start) else {
        panic!("stack pointer not mapped")
    };
    let stack_ptr_phys = stack_mapping.address();
    let stack_ptr_page_start = VirtAddr::new(0x2_1000_0000);
    let stack_ptr_virt_page = Page::from_start_addr(stack_ptr_page_start, PageSize::Size4KiB);
    let stack_ptr_phys_page = Page::containing_address(stack_ptr_phys, PageSize::Size4KiB);
    let flags = PageTableEntryFlags::PRESENT
        | PageTableEntryFlags::WRITABLE
        | PageTableEntryFlags::USER_ACCESSIBLE;
    match memory::map_page(stack_ptr_virt_page, stack_ptr_phys_page, flags) {
        Ok(_) | Err(MapError::PageAlreadyMapped { .. }) => {}
        Err(e) => panic!("failed to map stack page: {:?}", e),
    }

    let stack_end_virt = VirtAddr::new(0x2_1000_0000 + DUMMY_USERSPACE_STACK.len() as u64);
    let stack_ptr = stack_end_virt;

    let user_code_segment_idx: u64 = u64::from(gdt::selectors().user_code_selector.0);
    let user_data_segment_idx: u64 = u64::from(gdt::selectors().user_data_selector.0);

    // N.B. It is important that jump_to_userspace is marked as returning !,
    // which means it never returns, because I _think_ that the compiler will
    // properly clean up all the other stuff in this function. Before I had `!`
    // I was getting some intermittent page faults.
    unsafe {
        jump_to_userspace(
            instruction_virt,
            stack_ptr,
            user_code_segment_idx,
            user_data_segment_idx,
        );
    };
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

#[naked]
unsafe extern "C" fn dummy_hacky_userspace_task() {
    unsafe {
        asm!(
            // Call interrupt for fun
            "int3",
            // Call syscall
            "mov rdi, 0x1111",
            "mov rsi, 0x2222",
            "mov rdx, 0x3333",
            "mov r10, 0x4444",
            "mov r8, 0x5555",
            "mov r9, 0x6666",
            "syscall",
            options(noreturn),
        )
    }
}
