use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::arch::asm;

use spin::Mutex;

use crate::{hlt_loop, serial_println};

static TASKS: Mutex<Option<VecDeque<Task>>> = Mutex::new(None);

/// Initialize the scheduling subsystem.
pub(crate) unsafe fn init() {
    TASKS.lock().replace(VecDeque::new());
}

/// Pushes a task onto the task queue.
pub(crate) fn push_task(name: &'static str, start_fn: fn() -> ()) {
    let mut tasks = TASKS.lock();
    let task = Task::new(name, start_fn);
    tasks.as_mut().unwrap().push_back(task);

    serial_println!("TASKS: {:x?}", tasks);
}

pub fn start_multitasking() {
    fn dummy_task_fn() {
        serial_println!("FATAL: Dummy task was switched back to!");
        hlt_loop();
    }

    // Create a dummy task that we can switch away from. We will never return
    // here, so the values don't matter.
    let current_task = Task::new("__START_MULTITASKING__", dummy_task_fn);

    // Just pick the next task in the queue and switch to it.
    let next_task = TASKS
        .lock()
        .as_mut()
        .expect("schedule not initialized!")
        .pop_front()
        .expect("no tasks to schedule!");
    // switch_to_task(&current_task, &next_task);

    let current_task_ptr = core::ptr::addr_of!(current_task.kernel_stack_pointer.0);
    unsafe {
        switch_to_task(current_task_ptr, next_task.kernel_stack_pointer.0);
    }
}

/// A `Task` is a unit of work that can be scheduled, like a thread or a process.
#[derive(Debug)]
pub(crate) struct Task {
    name: &'static str,
    kernel_stack_pointer: TaskKernelStackPointer,
    kernel_stack: Box<[u8; KERNEL_STACK_SIZE]>,
}

/// All kernel stacks have the same, constant size.
const KERNEL_STACK_SIZE: usize = 4096;

impl Task {
    /// Create a new task with the given ID and kernel stack pointer.
    pub(crate) fn new(name: &'static str, start_fn: fn() -> ()) -> Self {
        // Allocate a kernel stack
        let mut kernel_stack = Box::new([0; KERNEL_STACK_SIZE]);

        // We need to push many values onto the stack to set up the stack frame
        // for when we run switch_to_task. The general purpose registers don't
        // matter, but the rip register must point to where we want to start
        // execution.
        let rip_bytes_end = KERNEL_STACK_SIZE;
        let rip_bytes_start = KERNEL_STACK_SIZE - 8;
        let start_fn_address = start_fn as usize;
        kernel_stack[rip_bytes_start..rip_bytes_end]
            .copy_from_slice(&(start_fn_address as u64).to_le_bytes());

        let num_general_purpose_registers = 15; // Ensure this matches `switch_to_task`!!!
        let num_stored_registers = num_general_purpose_registers + 1; // +1 for RIP
        let kernel_stack_pointer = TaskKernelStackPointer(
            // * 8 is because each register is 8 bytes
            kernel_stack.as_ptr() as usize + KERNEL_STACK_SIZE - num_stored_registers * 8,
        );

        Self {
            name,
            kernel_stack_pointer,
            kernel_stack,
        }
    }
}

#[repr(transparent)]
#[derive(Debug)]
pub(crate) struct TaskKernelStackPointer(pub(crate) usize);

/// Architecture-specific assembly code to switch from one task to another.
#[naked]
unsafe extern "C" fn switch_to_task(previous_task_stack_pointer: *const usize, next_task_stack_pointer: usize) {
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
