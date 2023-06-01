use alloc::boxed::Box;
use core::arch::asm;

use crate::sync::AtomicEnum;

use super::schedcore::task_setup;

/// A `Task` is a unit of work that can be scheduled, like a thread or a process.
#[derive(Debug)]
pub(crate) struct Task {
    pub(super) id: TaskId,
    pub(super) name: &'static str,
    pub(super) kernel_stack_pointer: TaskKernelStackPointer,
    pub(super) state: AtomicEnum<u8, TaskState>,
    _kernel_stack: Box<[u8; KERNEL_STACK_SIZE]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TaskId(pub(super) u32);

/// All kernel stacks have the same, constant size.
///
/// TODO: This is quite large, but it is necessary even for extremely simple
/// tasks because in debug mode we apparently use the stack a ton.
const KERNEL_STACK_SIZE: usize = 4096 * 4;

/// Function to run when starting a kernel task.
///
/// We use the C calling convention because I don't trust the unspecified Rust
/// calling convention to work when we cast a 64 bit integer to a function
/// pointer.
///
/// Each kernel task has an optional argument. This is a pointer to some data
/// that the task needs to run. This is passed to the task when it is started.
pub(crate) type KernelTaskStartFunction = extern "C" fn(*const ()) -> ();

impl Task {
    /// Create a new task with the given ID and kernel stack pointer.
    pub(super) fn new(
        id: TaskId,
        name: &'static str,
        start_fn: KernelTaskStartFunction,
        arg: *const (),
    ) -> Self {
        // Allocate a kernel stack
        let mut kernel_stack = Box::new([0; KERNEL_STACK_SIZE]);

        // We need to push many values onto the stack to set up the stack frame
        // for when we run switch_to_task. The general purpose registers don't
        // matter, but the rip register must point to where we want to start
        // execution.
        //
        // TODO: This would be a lot easier if we used an actual struct for this.

        // Push the RIP for the task_setup.
        let task_setup_rip_bytes_end = KERNEL_STACK_SIZE;
        let task_setup_rip_bytes_start = KERNEL_STACK_SIZE - 8;
        let task_setup_address = task_setup as usize;
        kernel_stack[task_setup_rip_bytes_start..task_setup_rip_bytes_end]
            .copy_from_slice(&(task_setup_address as u64).to_le_bytes());

        // Set rsi, which will end up as the second argument to task_setup when
        // we `ret` to it in `switch_to_task` (this is the C calling
        // convention).
        let task_rdi_bytes_end = KERNEL_STACK_SIZE - (6 * 8);
        let task_rdi_bytes_start = KERNEL_STACK_SIZE - (7 * 8);
        let task_rdi = arg as usize;
        kernel_stack[task_rdi_bytes_start..task_rdi_bytes_end]
            .copy_from_slice(&task_rdi.to_le_bytes());

        // Set rdi, which will end up as the first argument to task_setup when
        // we `ret` to it in `switch_to_task` (this is the C calling
        // convention).
        let task_rdi_bytes_end = KERNEL_STACK_SIZE - (7 * 8);
        let task_rdi_bytes_start = KERNEL_STACK_SIZE - (8 * 8);
        let task_rdi = start_fn as usize;
        kernel_stack[task_rdi_bytes_start..task_rdi_bytes_end]
            .copy_from_slice(&task_rdi.to_le_bytes());

        let num_general_purpose_registers = 15; // Ensure this matches `switch_to_task`!!!
        let num_stored_registers = num_general_purpose_registers + 1; // +1 for task_setup RIP
        let kernel_stack_pointer = TaskKernelStackPointer(
            // * 8 is because each register is 8 bytes
            kernel_stack.as_ptr() as usize + KERNEL_STACK_SIZE - num_stored_registers * 8,
        );

        Self {
            id,
            name,
            kernel_stack_pointer,
            state: AtomicEnum::new(TaskState::ReadyToRun),
            _kernel_stack: kernel_stack,
        }
    }
}

#[repr(transparent)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub(super) struct TaskKernelStackPointer(pub(super) usize);

#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum TaskState {
    /// ReadyToRun covers both a running task and a task that is currently
    /// running.
    ReadyToRun,
    Sleeping,
    Killed,
}

impl TryFrom<u8> for TaskState {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::ReadyToRun),
            1 => Ok(Self::Sleeping),
            2 => Ok(Self::Killed),
            _ => Err(()),
        }
    }
}

impl From<TaskState> for u8 {
    fn from(value: TaskState) -> Self {
        value as Self
    }
}

/// Architecture-specific assembly code to switch from one task to another.
#[naked]
pub(super) unsafe extern "C" fn switch_to_task(
    previous_task_stack_pointer: *const TaskKernelStackPointer,
    next_task_stack_pointer: TaskKernelStackPointer,
) {
    unsafe {
        asm!(
            // Save the previous task's general purpose registers by pushing
            // them onto the stack. Next time we switch to this task, we simply
            // pop them off the stack.
            //
            // TODO: If we assume a C calling convention, we can decide to just
            // save the callee-saved registers.
            "push rax",
            "push rbx",
            "push rcx",
            "push rdx",
            "push rbp",
            "push rsi",
            "push rdi",
            "push r8",
            "push r9",
            "push r10",
            "push r11",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            // Save the previous task's stack pointer in the task struct. (First
            // param of this function is in rdi)
            "mov [rdi], rsp",
            // Restore the next task's stack pointer from the task struct.
            // (Second param of this function is in rsi)
            "mov rsp, rsi",
            // Pop the next task's saved general purpose registers. Remember,
            // the only way we could have gotten to this point in the old task
            // is if it called this function itself, so we know that the next
            // task's registers are already saved on the stack.
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop r11",
            "pop r10",
            "pop r9",
            "pop r8",
            "pop rdi",
            "pop rsi",
            "pop rbp",
            "pop rdx",
            "pop rcx",
            "pop rbx",
            "pop rax",
            "ret",
            options(noreturn),
        );
    }
}
