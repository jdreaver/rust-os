use alloc::sync::Arc;

use crate::hpet::Milliseconds;
use crate::sched::force_unlock_scheduler;
use crate::serial_println;
use crate::sync::{AtomicEnum, AtomicInt, WaitQueue};

use super::schedcore::scheduler_lock;
use super::stack;

/// A `Task` is a unit of work that can be scheduled, like a thread or a process.
#[derive(Debug)]
pub(crate) struct Task {
    pub(super) id: TaskId,
    pub(super) name: &'static str,
    pub(super) kernel_stack_pointer: TaskKernelStackPointer,
    pub(super) state: AtomicEnum<u8, TaskState>,
    pub(super) exit_wait_queue: Arc<WaitQueue<TaskExitCode>>,

    /// How much longer the task can run before it is preempted.
    pub(super) remaining_slice: AtomicInt<u64, Milliseconds>,
    pub(super) kernel_stack: stack::KernelStack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TaskId(pub(super) u32);

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
        let kernel_stack = stack::allocate_stack();

        // We need to push many values onto the stack to set up the stack frame
        // for when we run switch_to_task. The general purpose registers don't
        // matter, but the rip register must point to where we want to start
        // execution.
        //
        // TODO: This would be a lot easier if we used an actual struct for this.

        let stack_top = unsafe {
            // -7 because we need to align to a u64.
            #[allow(clippy::cast_ptr_alignment)]
            let stack_top_pointer = kernel_stack
                .top_addr()
                .as_mut_ptr::<u8>()
                .sub(7)
                .cast::<usize>();
            assert!(stack_top_pointer as usize % 8 == 0, "stack top not aligned");

            // Push the RIP for the task_setup.
            *stack_top_pointer = task_setup as usize;

            // Set rsi, which will end up as the second argument to task_setup when
            // we `ret` to it in `switch_to_task` (this is the C calling
            // convention).
            *stack_top_pointer.sub(6) = arg as usize;

            // Set rdi, which will end up as the first argument to task_setup when
            // we `ret` to it in `switch_to_task` (this is the C calling
            // convention).
            *stack_top_pointer.sub(7) = start_fn as usize;

            // N.B. The stack_top already accounts for the task_setup RIP, so we
            // don't need to add +1 here.
            let num_general_purpose_registers = 15; // Ensure this matches `switch_to_task`!!!
            stack_top_pointer.sub(num_general_purpose_registers) as usize
        };

        Self {
            id,
            name,
            kernel_stack_pointer: TaskKernelStackPointer(stack_top),
            state: AtomicEnum::new(TaskState::ReadyToRun),
            remaining_slice: AtomicInt::new(Milliseconds::new(0)),
            exit_wait_queue: Arc::new(WaitQueue::new()),
            kernel_stack,
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum TaskExitCode {
    ExitSuccess,
    // TODO: Add failure codes here
}

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
/// `switch_to_task`, and we need to pass in arguments via the known C calling
/// convention registers.
pub(super) extern "C" fn task_setup(task_fn: KernelTaskStartFunction, arg: *const ()) {
    // Release the scheduler lock. Normally, when we switch to a task, the task
    // exits `run_scheduler` and the lock would be released. However, the first
    // time we switch to a task we won't be exiting from `run_scheduler`, so we
    // need to manually release the lock here.
    unsafe {
        force_unlock_scheduler();
    };

    task_fn(arg);

    // Mark the current task as dead and run the scheduler.
    let wait_queue = {
        let lock = scheduler_lock();
        let current_task = lock.current_task();
        serial_println!(
            "task_setup: task {} {:?} task_fn returned, halting",
            current_task.name,
            current_task.id
        );
        current_task.state.swap(TaskState::Killed);
        current_task.exit_wait_queue.clone()
    };

    // Inform waiters that the task has exited.
    wait_queue.put_value(TaskExitCode::ExitSuccess);

    scheduler_lock().run_scheduler();

    panic!("somehow returned to task_setup for dead task after running scheduler");
}
