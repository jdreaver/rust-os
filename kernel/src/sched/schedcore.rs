use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};
use x86_64::PhysAddr;

use crate::hpet::Milliseconds;
use crate::sync::SpinLock;
use crate::{percpu, tick};

use super::task::{
    DesiredTaskState, KernelTaskStartFunction, Task, TaskExitCode, TaskId, TaskKernelStackPointer,
    TASKS,
};
use super::{stack, syscall};

static RUN_QUEUE: SpinLock<RunQueue> = SpinLock::new(RunQueue::new());

/// Used to protect against accidentally calling scheduling functions that
/// require a task context before we've started running tasks.
static MULTITASKING_STARTED: AtomicBool = AtomicBool::new(false);

/// Force unlocks the scheduler and re-enables interrupts. This is necessary in
/// contexts where we switched to a task in the scheduler but we can't release
/// the lock.
pub(super) unsafe fn force_unlock_scheduler() {
    RUN_QUEUE.force_unlock();
    // N.B. Ordering is important. Don't re-enable interrupts until the spinlock
    // is released or else we could get an interrupt + a deadlock.
    x86_64::instructions::interrupts::enable();
    percpu::set_per_cpu_preempt_count(0);
}

/// If set to true, then the scheduler should run next time it has a chance.
/// Used after exiting from IRQs and in other contexts that can't run the
/// scheduler but made a change (like changing a task's desired state) that
/// would require the scheduler to run.
static NEEDS_RESCHEDULE: AtomicBool = AtomicBool::new(false);

/// Stores pending tasks that may or may not want to be scheduled.
pub(crate) struct RunQueue {
    /// All tasks that are not running, except for the idle tasks.
    pending_tasks: VecDeque<TaskId>,
}

impl RunQueue {
    const fn new() -> Self {
        Self {
            pending_tasks: VecDeque::new(),
        }
    }

    /// Removes all killed tasks from the pending task list. It is important we
    /// don't remove a task that is killed but is still marked as running on a
    /// CPU, because the task's stack might be in use!
    fn remove_killed_pending_tasks(&mut self) {
        let mut remaining_pending_tasks = VecDeque::new();
        for id in &self.pending_tasks {
            let task = TASKS.lock_disable_interrupts().get_task_assert(*id);
            if task.desired_state.load() == DesiredTaskState::Killed {
                TASKS.lock_disable_interrupts().delete_task(*id);
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

            let next_task = TASKS
                .lock_disable_interrupts()
                .get_task_assert(next_task_id);
            if next_task.desired_state.load() == DesiredTaskState::ReadyToRun {
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

pub(crate) fn global_init() {
    stack::stack_init();
}

pub(crate) fn per_cpu_init() {
    syscall::syscall_init();

    // Set the current CPU's idle task.
    let idle_task_id = TASKS.lock_disable_interrupts().new_task(
        "__IDLE_TASK__",
        idle_task_start,
        core::ptr::null(),
    );
    percpu::set_per_cpu_idle_task_id(idle_task_id.0);
    percpu::set_per_cpu_current_task_id(idle_task_id.0);
}

pub(crate) fn current_task_id() -> TaskId {
    let id = percpu::get_per_cpu_current_task_id();
    TaskId(id)
}

pub(crate) fn current_task() -> Arc<Task> {
    TASKS
        .lock_disable_interrupts()
        .get_task_assert(current_task_id())
}

/// Switches from the bootstrap code, which isn't a task, to the first actual
/// kernel task.
pub(crate) fn start_multitasking(
    init_task_name: &'static str,
    init_task_start_fn: KernelTaskStartFunction,
    init_task_arg: *const (),
) {
    MULTITASKING_STARTED.store(true, Ordering::Release);

    new_task(init_task_name, init_task_start_fn, init_task_arg);

    // Just a dummy location for switch_to_task to store the previous stack
    // pointer.
    let dummy_stack_ptr = TaskKernelStackPointer(0);
    let prev_stack_ptr = core::ptr::addr_of!(dummy_stack_ptr);
    let next_stack_ptr = current_task().kernel_stack_pointer;
    let next_page_table = current_task().page_table_addr;

    unsafe {
        switch_to_task(prev_stack_ptr, next_stack_ptr, next_page_table);
    }
}

pub(crate) fn new_task(
    name: &'static str,
    start_fn: KernelTaskStartFunction,
    arg: *const (),
) -> TaskId {
    assert!(
        MULTITASKING_STARTED.load(Ordering::Relaxed),
        "multi-tasking not initialized, but tasks_lock called"
    );

    let id = TASKS
        .lock_disable_interrupts()
        .new_task(name, start_fn, arg);
    RUN_QUEUE
        .lock_disable_interrupts()
        .pending_tasks
        .push_back(id);
    id
}

/// How much time a task gets to run before being preempted.
const DEFAULT_TIME_SLICE: Milliseconds = Milliseconds::new(100);

pub(crate) fn run_scheduler() {
    // Set needs_reschedule to false if it hasn't been set already.
    NEEDS_RESCHEDULE.store(true, Ordering::Relaxed);

    // Check preempt counter. If it's non-zero, then we're in a critical
    // section and we shouldn't preempt.
    //
    // TODO: For now this we compare against 1 because the scheduler is
    // itself in a spinlock. Once we move it out of a spinlock we should
    // compare to zero again.
    let preempt_count = percpu::get_per_cpu_preempt_count();
    assert!(
        preempt_count >= 0,
        "preempt_count is negative! Something bad happened"
    );
    // TODO: Re-enable this once preempt count issues are fixed.
    // if preempt_count > 0 {
    //     return;
    // }

    // If the previous task still has a time slice left, don't preempt it.
    // (Except for idle task. We don't care if that ran out of time.)
    let idle_task_id = TaskId(percpu::get_per_cpu_idle_task_id());
    let current_task = current_task();

    let is_idle = current_task.id == idle_task_id;
    let is_ready = current_task.desired_state.load() == DesiredTaskState::ReadyToRun;
    let is_expired = current_task.remaining_slice.load() == Milliseconds::new(0);
    if !is_idle && is_ready && !is_expired {
        return;
    }

    // Take a lock on the run queue and disable interrupts. It is very important
    // we hold this lock past switch_to_task!
    let mut run_queue = RUN_QUEUE.lock_disable_interrupts();

    run_queue.remove_killed_pending_tasks();
    let prev_task = current_task;
    let prev_task_id = prev_task.id;
    let prev_task_state = prev_task.desired_state.load();
    let next_task_id = match run_queue.pop_next_ready_pending_task() {
        Some(id) => id,
        None => {
            // If we are not on the idle task, and if our current task is
            // not ready, let's switch to the idle task.
            if prev_task_state != DesiredTaskState::ReadyToRun && prev_task_id != idle_task_id {
                idle_task_id
            } else {
                // Otherwise, just return. We won't do a switch.
                return;
            }
        }
    };
    percpu::set_per_cpu_current_task_id(next_task_id.0);

    // Store the previous task ID in pending task list, unless it is the
    // idle task.
    if prev_task_id != idle_task_id {
        run_queue.pending_tasks.push_back(prev_task_id);
    }

    let prev_task = TASKS
        .lock_disable_interrupts()
        .get_task_assert(prev_task_id);
    let prev_stack_ptr = core::ptr::addr_of!(prev_task.kernel_stack_pointer);
    let next_task = TASKS
        .lock_disable_interrupts()
        .get_task_assert(next_task_id);
    let next_stack_ptr = next_task.kernel_stack_pointer;
    let next_page_table = next_task.page_table_addr;

    // Give the next task some time slice
    next_task.remaining_slice.store(DEFAULT_TIME_SLICE);

    unsafe {
        if prev_task_id == next_task_id {
            // We're already running the next task, so just return.
            log::warn!("Tried to switch to the same task!: {prev_task_id:?} {next_task_id:?}");
            return;
        }
        log::info!(
                "SCHEDULER: Switching from '{}' {:?} SP: {:x?} (@ {prev_stack_ptr:?}) to '{}' {:?} SP: {next_stack_ptr:x?}",
                prev_task.name,
                prev_task.id,
                *prev_stack_ptr,
                next_task.name,
                next_task.id,
            );
        switch_to_task(prev_stack_ptr, next_stack_ptr, next_page_table);
    }
}

/// If the scheduler needs to run, then run it.
pub(crate) fn run_scheduler_if_needed() {
    if NEEDS_RESCHEDULE.load(Ordering::Acquire) {
        run_scheduler();
    }
}

/// Function to run every time the kernel tick system ticks.
pub(crate) fn scheduler_tick(time_between_ticks: Milliseconds) {
    if !MULTITASKING_STARTED.load(Ordering::Acquire) {
        return;
    }

    // Deduct time from the currently running task's time slice.
    let current_task = current_task();
    let slice = current_task.remaining_slice.load();
    let slice = slice.saturating_sub(time_between_ticks);
    current_task.remaining_slice.store(slice);

    // If the task has run out of time, we need to run the scheduler.
    if slice == Milliseconds::new(0) {
        NEEDS_RESCHEDULE.store(true, Ordering::Relaxed);
    }
}

/// Puts the current task to sleep for the given number of milliseconds.
pub(crate) fn sleep_timeout(timeout: Milliseconds) {
    let task_id = go_to_sleep_no_run_scheduler();
    tick::add_relative_timer(timeout, move || {
        awaken_task(task_id);
    });
    run_scheduler();
}

/// Puts the current task to sleep and runs the scheduler
pub(crate) fn go_to_sleep() {
    go_to_sleep_no_run_scheduler();
    run_scheduler();
}

pub(super) fn kill_current_task(exit_code: TaskExitCode) {
    let current_task = current_task();
    log::info!("killing task {} {:?}", current_task.name, current_task.id);
    current_task.desired_state.swap(DesiredTaskState::Killed);

    // Inform waiters that the task has exited.
    current_task.exit_wait_cell.send_all_consumers(exit_code);

    run_scheduler();
}

/// Puts the current task to sleep and returns the current task ID, but does
/// _not_ run the scheduler.
fn go_to_sleep_no_run_scheduler() -> TaskId {
    NEEDS_RESCHEDULE.store(true, Ordering::Relaxed);
    let current_task = current_task();
    current_task.desired_state.swap(DesiredTaskState::Sleeping);
    current_task.id
}

/// Awakens the given task and sets needs_reschedule to true.
pub(crate) fn awaken_task(task_id: TaskId) {
    let task = TASKS.lock_disable_interrupts().get_task_assert(task_id);
    task.desired_state.swap(DesiredTaskState::ReadyToRun);
    NEEDS_RESCHEDULE.store(true, Ordering::Relaxed);
}

/// Waits until the given task is finished.
pub(crate) fn wait_on_task(target_task_id: TaskId) -> Option<TaskExitCode> {
    let Some(target_task) = TASKS.lock_disable_interrupts().get_task(target_task_id) else { return None; };
    let exit_code = target_task.exit_wait_cell.wait_sleep();
    Some(exit_code)
}

/// Architecture-specific assembly code to switch from one task to another.
#[naked]
pub(super) unsafe extern "C" fn switch_to_task(
    previous_task_stack_pointer: *const TaskKernelStackPointer,
    next_task_stack_pointer: TaskKernelStackPointer,
    next_page_table: PhysAddr,
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
            // Restore the next task's CR3 from the task struct.
            // (Third param of this function is in rdx)
            //
            // TODO: Don't reload cr3 if it didn't actually change! Reloading
            // cr3 invalidates the page cache.
            "mov cr3, rdx",
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
