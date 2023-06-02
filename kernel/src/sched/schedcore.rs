use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::acpi::ACPIInfo;
use crate::hpet::Milliseconds;
use crate::sync::{InitCell, SpinLock};
use crate::{apic, serial_println, tick};

use super::task::{KernelTaskStartFunction, Task, TaskId, TaskKernelStackPointer, TaskState};

static TASKS: InitCell<SpinLock<Tasks>> = InitCell::new();

/// Used to protect against accidentally calling scheduling functions that
/// require a task context before we've started running tasks.
static MULTITASKING_STARTED: AtomicBool = AtomicBool::new(false);

pub(super) fn tasks_lock() -> &'static SpinLock<Tasks> {
    assert!(
        MULTITASKING_STARTED.load(Ordering::Relaxed),
        "multi-tasking not initialized, but tasks_lock called"
    );

    TASKS.get().expect("tasks not initialized")
}

/// Holds all tasks in the kernel.
pub(super) struct Tasks {
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

    fn get_task(&self, id: TaskId) -> Option<&Task> {
        self.tasks.get(&id)
    }

    fn get_task_assert(&self, id: TaskId) -> &Task {
        self.get_task(id).map_or_else(
            || panic!("tried to fetch task ID {id:?} but it does not exist"),
            |task| task,
        )
    }

    /// Gets the currently running task on the current CPU.
    pub(super) fn current_task(&self) -> &Task {
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
    TASKS.init(SpinLock::new(Tasks::new(max_lapic_id)));
}

/// Pushes a task onto the task queue.
pub(crate) fn push_task(
    name: &'static str,
    start_fn: KernelTaskStartFunction,
    arg: *const (),
) -> TaskId {
    tasks_lock()
        .lock_disable_interrupts()
        .new_task(name, start_fn, arg)
}

/// Switches from the bootstrap code, which isn't a task, to the first actual
/// kernel task.
pub(crate) fn start_multitasking(
    init_task_name: &'static str,
    init_task_start_fn: KernelTaskStartFunction,
    init_task_arg: *const (),
) {
    MULTITASKING_STARTED.store(true, Ordering::Release);

    let mut tasks = tasks_lock().lock_disable_interrupts();
    tasks.new_task(init_task_name, init_task_start_fn, init_task_arg);

    // Just a dummy location for switch_to_task to store the previous stack
    // pointer.
    let dummy_stack_ptr = TaskKernelStackPointer(0);
    let prev_stack_ptr = core::ptr::addr_of!(dummy_stack_ptr);
    let next_stack_ptr = tasks.current_task().kernel_stack_pointer;
    unsafe {
        switch_to_task(prev_stack_ptr, next_stack_ptr);
    }
}

pub(crate) fn current_task_id() -> TaskId {
    tasks_lock().lock_disable_interrupts().current_task_id()
}

/// How much time a task gets to run before being preempted.
const DEFAULT_TIME_SLICE: Milliseconds = Milliseconds::new(100);

pub(crate) fn run_scheduler() {
    // Set NEEDS_RESCHEDULE to false if it hasn't been set already.
    NEEDS_RESCHEDULE.swap(false, Ordering::Acquire);

    // Disable interrupts and take a lock on the the run queue. When a task is
    // started for the very first time, `task_setup` handles re-enabling these.
    // Otherwise, they will be re-enabled by the next task when `run_scheduler`
    // is exited.
    let mut tasks = tasks_lock().lock_disable_interrupts();

    // If the previous task still has a time slice left, don't preempt it.
    // (Except for idle task. We don't care if that ran out of time.)
    let idle_task_id = tasks.current_cpu_idle_task();
    let current_task = tasks.current_task();

    let is_idle = current_task.id == idle_task_id;
    let is_ready = current_task.state.load() == TaskState::ReadyToRun;
    let is_expired = current_task.remaining_slice.load() == Milliseconds::new(0);
    if !is_idle && is_ready && !is_expired {
        return;
    }

    tasks.remove_killed_pending_tasks();
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

    // Give the next task some time slice
    next_task.remaining_slice.store(DEFAULT_TIME_SLICE);

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
}

/// Function to run every time the kernel tick system ticks.
pub(crate) fn scheduler_tick(time_between_ticks: Milliseconds) {
    if !MULTITASKING_STARTED.load(Ordering::Acquire) {
        return;
    }

    let tasks = tasks_lock().lock_disable_interrupts();

    // Deduct time from the currently running task's time slice.
    let current_task = tasks.current_task();
    let slice = current_task.remaining_slice.load();
    let slice = slice.saturating_sub(time_between_ticks);
    current_task.remaining_slice.store(slice);

    // If the task has run out of time, we need to run the scheduler.
    if slice == Milliseconds::new(0) {
        NEEDS_RESCHEDULE.store(true, Ordering::Release);
    }
}

/// If set to true, then the scheduler should reschedule as soon as possible.
/// Used after exiting from IRQs and in other contexts that would
/// opportunistically trigger the scheduler if appropriate.
static NEEDS_RESCHEDULE: AtomicBool = AtomicBool::new(false);

/// If the scheduler needs to run, then run it.
pub(crate) fn run_scheduler_if_needed() {
    if NEEDS_RESCHEDULE.swap(false, Ordering::Acquire) {
        run_scheduler();
    }
}

/// Puts the current task to sleep and returns the current task ID.
pub(crate) fn go_to_sleep() -> TaskId {
    let lock = tasks_lock().lock_disable_interrupts();
    let current_task = lock.current_task();
    current_task.state.swap(TaskState::Sleeping);
    NEEDS_RESCHEDULE.store(true, Ordering::Release);
    current_task.id
}

/// Puts the current task to sleep for the given number of milliseconds.
pub(crate) fn sleep_timeout(timeout: Milliseconds) {
    let task_id = go_to_sleep();
    tick::add_relative_timer(timeout, move || {
        awaken_task(task_id);
    });
    run_scheduler();
}

/// Awakens the given task and sets NEEDS_RESCHEDULE to true.
pub(crate) fn awaken_task(task_id: TaskId) {
    let lock = tasks_lock().lock_disable_interrupts();
    let task = lock.get_task_assert(task_id);
    task.state.swap(TaskState::ReadyToRun);
    NEEDS_RESCHEDULE.store(true, Ordering::Release);
}

/// Waits until the given task is finished.
pub(crate) fn wait_on_task(target_task_id: TaskId, sleep_interval: Milliseconds) {
    loop {
        {
            let lock = tasks_lock().lock_disable_interrupts();

            // TODO: Set current task to sleeping. We can't do this until we
            // have a reliable way to wake it up from outside of this function.

            // If target task doesn't exist, assume it is done
            let Some(target_task) = lock.get_task(target_task_id) else { break; };
            // If target task was killed, assume it is done
            if target_task.state.load() == TaskState::Killed {
                break;
            }
        }

        // TODO: Instead of sleeping for a set interval, put the current task to
        // sleep and find a reliable way to wake it up once the target task is
        // done. This logic might not belong in the scheduler; it might need to
        // be a wrapper function. For example, spawn a task in a function, and
        // then create some kind of condvar of signal that the parent task can
        // wait on.
        sleep_timeout(sleep_interval);
        run_scheduler();
    }

    run_scheduler();
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
