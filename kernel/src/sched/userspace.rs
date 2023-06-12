use core::arch::asm;

use x86_64::registers::rflags::RFlags;
use x86_64::structures::paging::mapper::MapToError;
use x86_64::structures::paging::{Page, PageTableFlags, PhysFrame};
use x86_64::VirtAddr;

use crate::{gdt, memory};

use super::syscall;

static DUMMY_USERSPACE_STACK: &[u8] = &[0; 4096];

/// Kernel function that is called when we are starting a userspace task. This
/// is the "entrypoint" to a userspace task, and performs some setup before
/// actually jumping to userspace.
pub(crate) extern "C" fn task_userspace_setup(_arg: *const ()) {
    // TODO: This is currently a big fat hack

    let instruction_ptr = dummy_hacky_userspace_task as usize;

    // Map our fake code to userspace addresses that userspace has access to
    let instruction_ptr_virt = VirtAddr::new(instruction_ptr as u64);
    let instruction_ptr_phys = memory::translate_addr(instruction_ptr_virt).unwrap();
    let instruction_ptr_page_start = VirtAddr::new(0x2_0000_0000);
    let instruction_ptr_page = Page::containing_address(instruction_ptr_page_start);
    let instruction_ptr_frame = PhysFrame::containing_address(instruction_ptr_phys);
    let flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    match memory::map_page_to_frame(instruction_ptr_page, instruction_ptr_frame, flags) {
        Ok(_) | Err(MapToError::PageAlreadyMapped(_)) => {}
        Err(e) => panic!("failed to map instruction page: {:?}", e),
    }

    let instruction_virt_offset =
        instruction_ptr_phys.as_u64() - instruction_ptr_frame.start_address().as_u64();
    let instruction_virt = instruction_ptr_page_start + instruction_virt_offset;

    let stack_ptr_start = VirtAddr::new(DUMMY_USERSPACE_STACK.as_ptr() as u64);
    let stack_ptr_phys = memory::translate_addr(stack_ptr_start).unwrap();
    let stack_start_virt = VirtAddr::new(0x2_1000_0000);
    let stack_ptr_page = Page::containing_address(stack_start_virt);
    let stack_ptr_frame = PhysFrame::containing_address(stack_ptr_phys);
    let flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    match memory::map_page_to_frame(stack_ptr_page, stack_ptr_frame, flags) {
        Ok(_) | Err(MapToError::PageAlreadyMapped(_)) => {}
        Err(e) => panic!("failed to map stack page: {:?}", e),
    }

    let stack_end_virt = VirtAddr::new(0x2_1000_0000 + DUMMY_USERSPACE_STACK.len() as u64);
    let stack_ptr = stack_end_virt;

    let user_code_segment_idx: u64 = u64::from(gdt::selectors().user_code_selector.0);
    let user_data_segment_idx: u64 = u64::from(gdt::selectors().user_data_selector.0);

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
) {
    unsafe {
        asm!(
            // Store the kernel stack
            "mov [{kernel_stack}], rsp",
            // Set up and execute iretq
            "push rcx",      // Fourth arg, stack segment
            "push rsi",      // Second arg, stack pointer
            "push {rflags}", // rflags
            "push rdx",      // Third arg, code segment
            "push rdi",      // First arg, instruction pointer
            "iretq",
            kernel_stack = sym syscall::KERNEL_STACK,
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
