use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use spin::mutex::SpinMutex;

use crate::acpi::ACPIInfo;
use crate::hpet::Milliseconds;
use crate::sync::InitCell;
use crate::{apic, serial_println, tick};

use super::task::{
    switch_to_task, KernelTaskStartFunction, Task, TaskId, TaskKernelStackPointer, TaskState,
};

static TASKS: InitCell<SpinMutex<Tasks>> = InitCell::new();

fn tasks_mutex() -> &'static SpinMutex<Tasks> {
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
            let task = Task::new(id, "__IDLE_TASK__", idle_task_start, core::ptr::null());
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

    /// Removes all killed tasks from the pending task list. It is important we
    /// don't remove a task that is killed but is still marked as running on a
    /// CPU, because the task's stack might be in use!
    fn remove_killed_pending_tasks(&mut self) {
        let mut remaining_pending_tasks = VecDeque::new();
        for id in &self.pending_tasks {
            let task = self.get_task_assert(*id);
            if task.state.load() == TaskState::Killed {
                self.tasks.remove(id);
            } else {
                remaining_pending_tasks.push_back(*id);
            }
        }

        self.pending_tasks = remaining_pending_tasks;
    }

    /// Finds the next task that is ready and removes it from the pending task
    /// list.
    fn pop_next_ready_pending_task(&mut self) -> Option<TaskId> {
        let mut non_ready_tasks = VecDeque::new();
        let next_task_id: Option<TaskId> = loop {
            let Some(next_task_id) = self.pending_tasks.pop_front() else {
                // No tasks are ready
                break None;
            };

            let next_task = self.get_task_assert(next_task_id);
            if next_task.state.load() == TaskState::ReadyToRun {
                // Found a ready task
                break Some(next_task_id);
            }
            non_ready_tasks.push_back(next_task_id);
        };
        self.pending_tasks.append(&mut non_ready_tasks);
        next_task_id
    }
}

extern "C" fn idle_task_start(_arg: *const ()) {
    loop {
        // TODO: Once we have preemption, remove this explicit call to run_scheduler.
        run_scheduler();
        x86_64::instructions::hlt();
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
    TASKS.init(SpinMutex::new(Tasks::new(max_lapic_id)));
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

        tasks.remove_killed_pending_tasks();

        let idle_task_id = tasks.current_cpu_idle_task();
        let prev_task = tasks.current_task();
        let prev_task_id = prev_task.id;
        let prev_task_state = prev_task.state.load();
        let next_task_id = match tasks.pop_next_ready_pending_task() {
            Some(id) => id,
            None => {
                // If we are not on the idle task, and if our current task is
                // not ready, let's switch to the idle task.
                if prev_task_state != TaskState::ReadyToRun && prev_task_id != idle_task_id {
                    idle_task_id
                } else {
                    // Otherwise, just return. We won't do a switch.
                    return;
                }
            }
        };
        tasks.put_current_task_id(next_task_id);

        // Store the previous task ID in pending task list, unless it is the
        // idle task.
        if prev_task_id != idle_task_id {
            tasks.pending_tasks.push_back(prev_task_id);
        }

        let prev_task = tasks.get_task_assert(prev_task_id);
        let prev_stack_ptr = core::ptr::addr_of!(prev_task.kernel_stack_pointer);
        let next_task = tasks.get_task_assert(next_task_id);
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
            task.state.load()
        );
    });
    run_scheduler();
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
    // Release the scheduler lock
    unsafe {
        tasks_mutex().force_unlock();
    };

    // Re-enable interrupts. Interrupts are disabled in `run_scheduler`. Ensure
    // that we re-enable them.
    x86_64::instructions::interrupts::enable();

    task_fn(arg);

    // Mark the current task as dead and run the scheduler.
    x86_64::instructions::interrupts::without_interrupts(|| {
        let lock = tasks_mutex().lock();
        let current_task = lock.current_task();
        serial_println!(
            "task_setup: task {} {:?} task_fn returned, halting",
            current_task.name,
            current_task.id
        );
        current_task.state.swap(TaskState::Killed);
    });

    run_scheduler();

    panic!("somehow returned to task_setup for dead task after running scheduler");
}
