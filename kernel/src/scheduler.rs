use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::arch::asm;

use spin::Mutex;

use crate::{hlt_loop, serial_println};

static RUN_QUEUE: Mutex<RunQueue> = Mutex::new(RunQueue::new());

struct RunQueue {
    running_task: Option<Task>,
    pending_tasks: VecDeque<Task>,
}

impl RunQueue {
    const fn new() -> Self {
        Self {
            running_task: None,
            pending_tasks: VecDeque::new(),
        }
    }

    /// Moves the currently running task to the back of the queue (if it
    /// exists). Returns the previously running tasks and next task to run.
    fn set_next_pending_task_running(&mut self) -> (Option<&Task>, &Task) {
        let prev_task = self.running_task.take();
        let next_task = self
            .pending_tasks
            .pop_front()
            .expect("no tasks to schedule!");
        if let Some(prev_task) = prev_task {
            self.pending_tasks.push_back(prev_task);
        }
        self.running_task.replace(next_task);
        let prev_task = self.pending_tasks.back();
        let next_task = self.running_task.as_ref().expect("no running task!");
        (prev_task, next_task)
    }
}

/// Pushes a task onto the task queue.
pub(crate) fn push_task(name: &'static str, start_fn: fn() -> ()) {
    let task = Task::new(name, start_fn);
    RUN_QUEUE.lock().pending_tasks.push_back(task);
}

pub fn start_multitasking() {
    fn dummy_task_fn() {
        serial_println!("FATAL: Dummy task was switched back to!");
        hlt_loop();
    }

    // Create a dummy task that we can switch away from. We will never return
    // here, so the values don't matter.
    //
    // TODO: The dummy task stack that we have to create in a Box never gets
    // dropped because we never return here. This is a memory leak.
    let current_task = Task::new("__START_MULTITASKING__", dummy_task_fn);

    // Just pick the next task in the queue and switch to it.
    let next_task_ptr = {
        // This lock must be dropped before we call `switch_to_task` or else
        // we'll deadlock.
        let mut queue = RUN_QUEUE.lock();
        let (_, next_task) = queue.set_next_pending_task_running();
        next_task.kernel_stack_pointer
    };
    let current_task_ptr = core::ptr::addr_of!(current_task.kernel_stack_pointer);
    unsafe {
        switch_to_task(current_task_ptr, next_task_ptr);
    }
}

pub(crate) fn run_scheduler() {
    // Disable interrupts and take a lock on the the run queue. When a task is
    // started for the very first time, `task_setup` handles re-enabling these.
    // Otherwise, they will be re-enabled by the next task when `run_scheduler`
    // is exited.
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut queue = RUN_QUEUE.lock();

        let (prev_task, next_task) = queue.set_next_pending_task_running();
        let prev_task = prev_task.expect("no previous task");
        let prev_stack_ptr = core::ptr::addr_of!(prev_task.kernel_stack_pointer);
        let next_stack_ptr = next_task.kernel_stack_pointer;

        unsafe {
            if *prev_stack_ptr == next_stack_ptr {
                // We're already running the next task, so just return.
                serial_println!("WARNING: Tried to switch to the same task!");
                return;
            }
            serial_println!(
                "SCHEDULER: Switching from '{}' SP: {:x?} (@ {:?}) to '{}' SP: {:x?}",
                prev_task.name,
                *prev_stack_ptr,
                prev_stack_ptr,
                next_task.name,
                next_stack_ptr
            );
            switch_to_task(prev_stack_ptr, next_stack_ptr);
        }
    });
}

/// A `Task` is a unit of work that can be scheduled, like a thread or a process.
#[derive(Debug)]
pub(crate) struct Task {
    name: &'static str,
    kernel_stack_pointer: TaskKernelStackPointer,
    _kernel_stack: Box<[u8; KERNEL_STACK_SIZE]>,
}

/// All kernel stacks have the same, constant size.
///
/// TODO: This is quite large, but it is necessary even for extremely simple
/// tasks because in debug mode we apparently use the stack a ton.
const KERNEL_STACK_SIZE: usize = 4096 * 4;

impl Task {
    /// Create a new task with the given ID and kernel stack pointer.
    pub(crate) fn new(name: &'static str, start_fn: fn() -> ()) -> Self {
        // Allocate a kernel stack
        let mut kernel_stack = Box::new([0; KERNEL_STACK_SIZE]);

        // We need to push many values onto the stack to set up the stack frame
        // for when we run switch_to_task. The general purpose registers don't
        // matter, but the rip register must point to where we want to start
        // execution.
        //
        // TODO: This would be a lot easier if we used an actual struct for this.

        // Push the RIP for the given start_fn function onto the stack.
        let start_fn_rip_bytes_end = KERNEL_STACK_SIZE;
        let start_fn_rip_bytes_start = KERNEL_STACK_SIZE - 8;
        let start_fn_address = start_fn as usize;
        kernel_stack[start_fn_rip_bytes_start..start_fn_rip_bytes_end]
            .copy_from_slice(&(start_fn_address as u64).to_le_bytes());

        // Push the RIP for the task_setup.
        let task_setup_rip_bytes_end = KERNEL_STACK_SIZE - 8;
        let task_setup_rip_bytes_start = KERNEL_STACK_SIZE - 16;
        let task_setup_address = task_setup as usize;
        kernel_stack[task_setup_rip_bytes_start..task_setup_rip_bytes_end]
            .copy_from_slice(&(task_setup_address as u64).to_le_bytes());

        // Set rdi, which will end up as the first argument to task_setup when
        // we `ret` to it in `switch_to_task` (this is the C calling
        // convention).
        let task_rdi_bytes_end = KERNEL_STACK_SIZE - (8 * 8);
        let task_rdi_bytes_start = KERNEL_STACK_SIZE - (9 * 8);
        let task_rdi = 0xdead_beef_u64;
        kernel_stack[task_rdi_bytes_start..task_rdi_bytes_end]
            .copy_from_slice(&task_rdi.to_le_bytes());

        let num_general_purpose_registers = 15; // Ensure this matches `switch_to_task`!!!
        let num_stored_registers = num_general_purpose_registers + 2; // +1 for start_fn_rip, +1 for task_setup RIP
        let kernel_stack_pointer = TaskKernelStackPointer(
            // * 8 is because each register is 8 bytes
            kernel_stack.as_ptr() as usize + KERNEL_STACK_SIZE - num_stored_registers * 8,
        );

        Self {
            name,
            kernel_stack_pointer,
            _kernel_stack: kernel_stack,
        }
    }
}

#[repr(transparent)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
struct TaskKernelStackPointer(usize);

/// Architecture-specific assembly code that is run when a task is switched to
/// for the very first time.
///
/// This is similar to Linux's
/// [`ret_from_fork`](https://elixir.bootlin.com/linux/v6.3.2/source/arch/x86/entry/entry_64.S#L279)
/// and
/// [`schedule_tail`](https://elixir.bootlin.com/linux/v6.3.2/source/kernel/sched/core.c#L5230)
/// functions, as well as xv6's
/// [`forkret`](https://github.com/IamAdiSri/xv6/blob/4cee212b832157fde3289f2088eb5a9d8713d777/proc.c#L406-L425).
///
/// `extern "C"` is important here. We get to this function via a `ret` in
/// `switch_to_task`, and we need to pass in an argument via the rdi register.
extern "C" fn task_setup(test_value: u64) {
    serial_println!("task_setup test_value: {:#x}", test_value);

    // Release the scheduler lock
    unsafe {
        RUN_QUEUE.force_unlock();
    };

    // Re-enable interrupts. Interrupts are disabled in `run_scheduler`. Ensure
    // that we re-enable them.
    x86_64::instructions::interrupts::enable();

    // When we return, we will pop the RIP and jump to the task's actual start function.
}

/// Architecture-specific assembly code to switch from one task to another.
#[naked]
unsafe extern "C" fn switch_to_task(
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
