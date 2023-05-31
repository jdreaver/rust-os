use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::arch::asm;

use spin::Mutex;

use crate::acpi::ACPIInfo;
use crate::sync::{AtomicRef, InitCell};
use crate::{apic, hlt_loop, serial_println};

/// Currently running process on each CPU. The index is the CPU's LAPIC ID.
static RUNNING_CPU_TASKS: InitCell<RunningCPUTasks> = InitCell::new();

fn running_cpu_tasks() -> &'static RunningCPUTasks {
    RUNNING_CPU_TASKS
        .get()
        .expect("running CPU tasks not initialized")
}

/// All pending tasks that aren't running on a CPU
static RUN_QUEUE: Mutex<RunQueue> = Mutex::new(RunQueue::new());

struct RunningCPUTasks {
    tasks: Vec<AtomicRef<Task>>,
}

impl RunningCPUTasks {
    fn new(max_lapic_id: u8) -> Self {
        let mut tasks = Vec::with_capacity(max_lapic_id as usize + 1);
        for _ in 0..=max_lapic_id {
            tasks.push(AtomicRef::new());
        }

        Self { tasks }
    }

    fn cpu_task_ptr(&self) -> &AtomicRef<Task> {
        // TODO: Assert that interrupts are disabled. Otherwise, we could get
        // rescheduled onto another CPU and the LAPIC ID could change. xv6 does
        // this, but is it necessary?
        //
        // assert!(
        //     !x86_64::instructions::interrupts::are_enabled(),
        //     "tried to access current CPU task pointer while interrupts are enabled"
        // );

        let lapic_id = apic::lapic_id();
        self.tasks
            .get(lapic_id as usize)
            .expect("could not get running CPU task for the current LAPIC ID")
    }

    fn running_task(&self) -> Option<&Task> {
        self.cpu_task_ptr().get()
    }

    fn pop_running_task(&self) -> Option<Task> {
        self.cpu_task_ptr().pop()
    }

    fn swap_running_task(&self, new_task: Task) -> Option<Task> {
        self.cpu_task_ptr().swap(Some(new_task))
    }
}

#[derive(Debug)]
struct RunQueue {
    pending_tasks: VecDeque<Task>,
}

impl RunQueue {
    const fn new() -> Self {
        Self {
            pending_tasks: VecDeque::new(),
        }
    }

    fn pop_next_task(&mut self) -> Option<Task> {
        self.pending_tasks.pop_front()
    }

    fn push_task(&mut self, task: Task) {
        self.pending_tasks.push_back(task);
    }
}

pub(crate) fn init(acpi_info: &ACPIInfo) {
    let processor_info = acpi_info.processor_info();
    let max_lapic_id = processor_info
        .application_processors
        .iter()
        .map(|info| info.local_apic_id)
        .max()
        .expect("no processors found!");
    let max_lapic_id = u8::try_from(max_lapic_id).expect("LAPIC ID too large!");
    RUNNING_CPU_TASKS.init(RunningCPUTasks::new(max_lapic_id));
}

/// Pushes a task onto the task queue.
pub(crate) fn push_task(name: &'static str, start_fn: extern "C" fn() -> ()) {
    let task = Task::new(name, start_fn);
    RUN_QUEUE.lock().pending_tasks.push_back(task);
}

pub fn start_multitasking() {
    extern "C" fn dummy_task_fn() {
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
        let next_task = RUN_QUEUE
            .lock()
            .pop_next_task()
            .expect("failed to initialize multi-tasking: no tasks to run");
        let stack_ptr = next_task.kernel_stack_pointer;
        running_cpu_tasks().swap_running_task(next_task);
        stack_ptr
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
        // Move the current task to the back of the queue, pop the next task,
        // and mark the next task as the current task.

        let next_task = loop {
            let Some(next_task) = queue.pop_next_task() else {
                // No tasks to run, so just return.
                return;
            };

            // If the current task is killed, throw this task away and try
            // again.
            if !next_task.killed {
                break next_task;
            }

            serial_println!("Task {} was killed", next_task.name);
        };

        let Some(prev_task) = running_cpu_tasks().swap_running_task(next_task) else {
            panic!("tried switching tasks, but there was amazingly no currently running task on the CPU");
        };
        queue.push_task(prev_task);

        // Create new references to the next and previous tasks so we get stable
        // pointers to them.
        let next_task = running_cpu_tasks().running_task().expect("no running task");
        let prev_task = queue
            .pending_tasks
            .back()
            .expect("no previous task in the queue");

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
    killed: bool,
    _kernel_stack: Box<[u8; KERNEL_STACK_SIZE]>,
}

/// All kernel stacks have the same, constant size.
///
/// TODO: This is quite large, but it is necessary even for extremely simple
/// tasks because in debug mode we apparently use the stack a ton.
const KERNEL_STACK_SIZE: usize = 4096 * 4;

impl Task {
    /// Create a new task with the given ID and kernel stack pointer.
    pub(crate) fn new(name: &'static str, start_fn: extern "C" fn() -> ()) -> Self {
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
        let task_rdi = 0xdead_beef_u64;
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
            name,
            kernel_stack_pointer,
            killed: false,
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
extern "C" fn task_setup(task_fn: extern "C" fn() -> (), arg: u64) {
    serial_println!("task_setup arg: {arg:#x}");

    // Release the scheduler lock
    unsafe {
        RUN_QUEUE.force_unlock();
    };

    // Re-enable interrupts. Interrupts are disabled in `run_scheduler`. Ensure
    // that we re-enable them.
    x86_64::instructions::interrupts::enable();

    task_fn();

    serial_println!("task_setup: task_fn returned, halting");

    // Mark the current task as dead and run the scheduler.
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut current_task = running_cpu_tasks()
            .pop_running_task()
            .expect("no running task");
        current_task.killed = true;
        running_cpu_tasks().swap_running_task(current_task);
    });

    run_scheduler();

    panic!("somehow returned to task_setup for dead task after running scheduler");
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
