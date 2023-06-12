use core::arch::asm;

use x86_64::registers::rflags::RFlags;
use x86_64::structures::paging::{Page, PageTableFlags, PhysFrame};
use x86_64::VirtAddr;

use crate::memory;

static DUMMY_USERSPACE_STACK: &[u8] = &[0; 4096];

/// Kernel function that is called when we are starting a userspace task. This
/// is the "entrypoint" to a userspace task, and performs some setup before
/// actually jumping to userspace.
pub(crate) extern "C" fn task_userspace_setup(_arg: *const ()) {
    // TODO: This is currently a big fat hack

    let instruction_ptr = dummy_hacky_userspace_task as usize;
    log::warn!("instruction_ptr: {:#x}", instruction_ptr);

    // Map our fake code to userspace addresses that userspace has access to
    let instruction_ptr_virt = VirtAddr::new(instruction_ptr as u64);
    let instruction_ptr_phys = memory::translate_addr(instruction_ptr_virt).unwrap();
    let instruction_virt = VirtAddr::new(0x2_0000_0000);
    let instruction_ptr_page = Page::containing_address(instruction_virt);
    let instruction_ptr_frame = PhysFrame::containing_address(instruction_ptr_phys);
    let flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    memory::map_page_to_frame(instruction_ptr_page, instruction_ptr_frame, flags)
        .expect("failed to map instruction page");

    let stack_ptr_start = VirtAddr::new(DUMMY_USERSPACE_STACK.as_ptr() as u64);
    let stack_ptr_phys = memory::translate_addr(stack_ptr_start).unwrap();
    let stack_start_virt = VirtAddr::new(0x2_1000_0000);
    let stack_ptr_page = Page::containing_address(stack_start_virt);
    let stack_ptr_frame = PhysFrame::containing_address(stack_ptr_phys);
    let flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    memory::map_page_to_frame(stack_ptr_page, stack_ptr_frame, flags)
        .expect("failed to map stack page");

    let stack_ptr_end = VirtAddr::new(
        (DUMMY_USERSPACE_STACK.as_ptr() as usize + DUMMY_USERSPACE_STACK.len()) as u64,
    );
    let stack_ptr_phys = memory::translate_addr(stack_ptr_end).unwrap();
    let stack_end_virt = VirtAddr::new(0x2_1000_0000 + DUMMY_USERSPACE_STACK.len() as u64);
    let stack_ptr_page = Page::containing_address(stack_end_virt);
    let stack_ptr_frame = PhysFrame::containing_address(stack_ptr_phys);
    let flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    memory::map_page_to_frame(stack_ptr_page, stack_ptr_frame, flags)
        .expect("failed to map stack page");

    let stack_ptr = stack_end_virt;
    log::warn!("stack_ptr: {:#x}", stack_ptr);

    unsafe {
        jump_to_userspace(instruction_virt, stack_ptr);
    };
}

/// Function to go to userspace for the first time in a task.
#[naked]
pub(super) unsafe extern "C" fn jump_to_userspace(
    user_instruction_pointer: VirtAddr,
    user_stack_pointer: VirtAddr,
) {
    unsafe {
        asm!(
            "mov rcx, rdi",    // First argument, new instruction pointer
            "mov rsp, rsi",    // Second argument, new stack pointer
            "mov r11, {rflags}", // rflags
            "sysretq",
            rflags = const RFlags::INTERRUPT_FLAG.bits(),
            options(noreturn),
        )
    }
}

#[naked]
unsafe extern "C" fn dummy_hacky_userspace_task() {
    unsafe {
        asm!(
            "mov rdi, 0x1337",
            "mov rsi, 0x1338",
            "mov rdx, 0x1339",
            "mov r10, 0x133A",
            "syscall",
            options(noreturn),
        )
    }
}
