use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

use spin::Mutex;

use crate::acpi::ACPIInfo;
use crate::hpet::Milliseconds;
use crate::sync::{AtomicU8Enum, InitCell};
use crate::{apic, serial_println, tick};

static TASKS: InitCell<Mutex<Tasks>> = InitCell::new();

fn tasks_mutex() -> &'static Mutex<Tasks> {
    TASKS.get().expect("tasks not initialized")
}

/// Holds all tasks in the kernel.
struct Tasks {
    /// Next ID to use when creating a new task.
    next_task_id: TaskId,

    /// All tasks by ID
    tasks: BTreeMap<TaskId, Task>,

    /// Currently running tasks, indexed by CPU (via LAPIC ID)
    running_tasks_by_cpu: Vec<TaskId>,

    /// Idle tasks, indexed by CPU (via LAPIC ID)
    idle_tasks_by_cpu: Vec<TaskId>,

    /// All tasks that are not running, except for the idle tasks.
    pending_tasks: VecDeque<TaskId>,
}

impl Tasks {
    fn new(max_lapic_id: u8) -> Self {
        // Populate the idle tasks.
        let mut next_task_id = TaskId(1);
        let mut tasks = BTreeMap::new();
        let mut running_tasks_by_cpu = Vec::with_capacity(max_lapic_id as usize + 1);
        let mut idle_tasks_by_cpu = Vec::with_capacity(max_lapic_id as usize + 1);
        for _ in 0..=max_lapic_id {
            let id = next_task_id;
            let task = Task::new_idle_task(id);
            tasks.insert(id, task);
            running_tasks_by_cpu.push(id);
            idle_tasks_by_cpu.push(id);
            next_task_id.0 += 1;
        }

        Self {
            next_task_id,
            tasks,
            running_tasks_by_cpu,
            idle_tasks_by_cpu,
            pending_tasks: VecDeque::new(),
        }
    }

    fn new_task(
        &mut self,
        name: &'static str,
        start_fn: KernelTaskStartFunction,
        arg: *const (),
    ) -> TaskId {
        let id = self.next_task_id;
        self.next_task_id.0 += 1;

        assert!(
            !self.tasks.contains_key(&id),
            "task ID {id:?} already exists"
        );

        let task = Task::new(id, name, start_fn, arg);
        self.tasks.insert(id, task);
        self.pending_tasks.push_back(id);
        id
    }

    fn get_task_assert(&self, id: TaskId) -> &Task {
        self.tasks.get(&id).map_or_else(
            || panic!("tried to fetch task ID {id:?} but it does not exist"),
            |task| task,
        )
    }

    /// Gets the currently running task on the current CPU.
    fn current_task(&self) -> &Task {
        let id = self.current_task_id();
        self.get_task_assert(id)
    }

    fn current_task_id(&self) -> TaskId {
        // Assert that interrupts are disabled. Otherwise, we could get
        // rescheduled onto another CPU and the LAPIC ID could change. xv6 does
        // this, but is it necessary?
        assert!(
            !x86_64::instructions::interrupts::are_enabled(),
            "tried to access current CPU task while interrupts are enabled"
        );

        let lapic_id = apic::lapic_id();
        *self
            .running_tasks_by_cpu
            .get(lapic_id as usize)
            .expect("could not get running CPU task for the current LAPIC ID")
    }

    fn put_current_task_id(&mut self, id: TaskId) {
        // Assert that interrupts are disabled. Otherwise, we could get
        // rescheduled onto another CPU and the LAPIC ID could change. xv6 does
        // this, but is it necessary?
        assert!(
            !x86_64::instructions::interrupts::are_enabled(),
            "tried to access current CPU task while interrupts are enabled"
        );

        let lapic_id = apic::lapic_id();
        assert!(
            lapic_id < self.running_tasks_by_cpu.len() as u8,
            "lapic_id {lapic_id} out of range"
        );
        self.running_tasks_by_cpu[lapic_id as usize] = id;
    }

    fn current_cpu_idle_task(&self) -> TaskId {
        let lapic_id = apic::lapic_id();
        *self
            .idle_tasks_by_cpu
            .get(lapic_id as usize)
            .expect("could not get idle CPU task for the current LAPIC ID")
    }

    /// Moves the currently running task to the end of the pending task queue
    /// and moves the next task in the queue (that can run) to the current
    /// running CPU slot. Returns references to the previous task and the next
    /// task.
    fn round_robin_current_cpu_tasks(&mut self) -> (&Task, &Task) {
        assert!(
            !x86_64::instructions::interrupts::are_enabled(),
            "tried to run scheduler with interrupts enabled"
        );

        let idle_task_id = self.current_cpu_idle_task();

        let mut sleeping_tasks = VecDeque::new();
        let next_task_id: TaskId = loop {
            let Some(next_task_id) = self.pending_tasks.pop_front() else {
                break idle_task_id;
            };

            let next_task = self.get_task_assert(next_task_id);
            match next_task.state.get() {
                // If it is ready to run, select it
                TaskState::ReadyToRun => break next_task_id,
                // Push sleeping task to the back of the queue
                TaskState::Sleeping => sleeping_tasks.push_back(next_task_id),
                // Let killed task drop
                TaskState::Killed => {
                    serial_println!("Task {} was killed", next_task.name);
                }
            }
        };
        self.pending_tasks.append(&mut sleeping_tasks);

        // Move the previous task to the end of the queue and get a reference to
        // it.
        let prev_task_id = self.current_task_id();
        self.put_current_task_id(next_task_id);
        if prev_task_id != idle_task_id {
            self.pending_tasks.push_back(prev_task_id);
        }

        // Get references to the tasks
        let prev_task = self.get_task_assert(prev_task_id);
        let next_task = self.get_task_assert(next_task_id);

        (prev_task, next_task)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(u32);

pub(crate) fn init(acpi_info: &ACPIInfo) {
    let processor_info = acpi_info.processor_info();
    let max_lapic_id = processor_info
        .application_processors
        .iter()
        .map(|info| info.local_apic_id)
        .max()
        .expect("no processors found!");
    let max_lapic_id = u8::try_from(max_lapic_id).expect("LAPIC ID too large!");
    TASKS.init(Mutex::new(Tasks::new(max_lapic_id)));
}

/// Pushes a task onto the task queue.
pub(crate) fn push_task(
    name: &'static str,
    start_fn: KernelTaskStartFunction,
    arg: *const (),
) -> TaskId {
    tasks_mutex().lock().new_task(name, start_fn, arg)
}

static MULTITASKING_STARTED: AtomicBool = AtomicBool::new(false);

/// Switches from the bootstrap code, which isn't a task, to the first actual
/// kernel task.
pub(crate) fn start_multitasking() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let tasks = tasks_mutex().lock();

        MULTITASKING_STARTED.store(true, Ordering::SeqCst);

        // Just a dummy location for switch_to_task to store the previous stack
        // pointer.
        let dummy_stack_ptr = TaskKernelStackPointer(0);
        let prev_stack_ptr = core::ptr::addr_of!(dummy_stack_ptr);
        let next_stack_ptr = tasks.current_task().kernel_stack_pointer;
        unsafe {
            switch_to_task(prev_stack_ptr, next_stack_ptr);
        }
    });
}

pub(crate) fn run_scheduler() {
    assert!(
        MULTITASKING_STARTED.load(Ordering::Relaxed),
        "multi-tasking not initialized, but run_scheduler called"
    );

    // Disable interrupts and take a lock on the the run queue. When a task is
    // started for the very first time, `task_setup` handles re-enabling these.
    // Otherwise, they will be re-enabled by the next task when `run_scheduler`
    // is exited.
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut tasks = tasks_mutex().lock();

        let (prev_task, next_task) = tasks.round_robin_current_cpu_tasks();

        if prev_task.id == next_task.id {
            // Nothing to schedule. Return
            return;
        };
        let prev_stack_ptr = core::ptr::addr_of!(prev_task.kernel_stack_pointer);
        let next_stack_ptr = next_task.kernel_stack_pointer;

        unsafe {
            if *prev_stack_ptr == next_stack_ptr {
                // We're already running the next task, so just return.
                serial_println!("WARNING: Tried to switch to the same task!");
                return;
            }
            serial_println!(
                "SCHEDULER: Switching from '{}' {:?} SP: {:x?} (@ {prev_stack_ptr:?}) to '{}' {:?} SP: {next_stack_ptr:x?}",
                prev_task.name,
                prev_task.id,
                *prev_stack_ptr,
                next_task.name,
                next_task.id,
            );
            switch_to_task(prev_stack_ptr, next_stack_ptr);
        }
    });
}

/// A `Task` is a unit of work that can be scheduled, like a thread or a process.
#[derive(Debug)]
pub(crate) struct Task {
    id: TaskId,
    name: &'static str,
    kernel_stack_pointer: TaskKernelStackPointer,
    state: AtomicU8Enum<TaskState>,
    _kernel_stack: Box<[u8; KERNEL_STACK_SIZE]>,
}

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
type KernelTaskStartFunction = extern "C" fn(*const ()) -> ();

impl Task {
    /// Create a new task with the given ID and kernel stack pointer.
    pub(crate) fn new(
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
            state: AtomicU8Enum::new(TaskState::ReadyToRun),
            _kernel_stack: kernel_stack,
        }
    }

    fn new_idle_task(id: TaskId) -> Self {
        Self::new(id, "__IDLE_TASK__", idle_task_start, core::ptr::null())
    }
}

extern "C" fn idle_task_start(_arg: *const ()) {
    loop {
        // TODO: Once we have preemption, remove this explicit call to run_scheduler.
        run_scheduler();
        x86_64::instructions::hlt();
    }
}

#[repr(transparent)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
struct TaskKernelStackPointer(usize);

#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
enum TaskState {
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
extern "C" fn task_setup(task_fn: KernelTaskStartFunction, arg: *const ()) {
    // Release the scheduler lock
    unsafe {
        tasks_mutex().force_unlock();
    };

    // Re-enable interrupts. Interrupts are disabled in `run_scheduler`. Ensure
    // that we re-enable them.
    x86_64::instructions::interrupts::enable();

    task_fn(arg);

    serial_println!("task_setup: task_fn returned, halting");

    // Mark the current task as dead and run the scheduler.
    x86_64::instructions::interrupts::without_interrupts(|| {
        let lock = tasks_mutex().lock();
        let current_task = lock.current_task();
        current_task.state.swap(TaskState::Killed);
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

/// Puts the current task to sleep for the given number of milliseconds.
pub(crate) fn sleep(timeout: Milliseconds) {
    let task_id = x86_64::instructions::interrupts::without_interrupts(|| {
        let lock = tasks_mutex().lock();
        let current_task = lock.current_task();
        current_task.state.swap(TaskState::Sleeping);
        current_task.id
    });

    tick::add_relative_timer(timeout, move || {
        // N.B. timers run in an interrupt context, so interrupts are already
        // disabled.
        let lock = tasks_mutex().lock();
        let task = lock.get_task_assert(task_id);
        serial_println!("sleep: waking up task");
        task.state.swap(TaskState::ReadyToRun);
        serial_println!(
            "sleep task status: {task_id:?}, {}, {:?}",
            task.name,
            task.state.get()
        );
    });
    run_scheduler();
}
