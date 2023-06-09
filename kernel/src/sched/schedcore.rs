use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use core::arch::asm;
use x86_64::PhysAddr;

use crate::gdt::set_tss_rsp0;
use crate::hpet::Milliseconds;
use crate::sync::SpinLock;
use crate::{define_per_cpu_u32, define_per_cpu_u8};
use crate::{percpu, tick};

use super::preempt::{get_preempt_count_no_guard, set_preempt_count};
use super::syscall::set_per_cpu_TOP_OF_KERNEL_STACK;
use super::task::{DesiredTaskState, KernelTaskStartFunction, Task, TaskExitCode, TaskId, TASKS};
use super::{stack, syscall};

static RUN_QUEUE: SpinLock<RunQueue> = SpinLock::new(RunQueue::new());

/// Force unlocks the scheduler and re-enables interrupts. This is necessary in
/// contexts where we switched to a task in the scheduler but we can't release
/// the lock.
pub(super) unsafe fn force_unlock_scheduler() {
    RUN_QUEUE.force_unlock();
    // N.B. Ordering is important. Don't re-enable interrupts until the spinlock
    // is released or else we could get an interrupt + a deadlock.
    x86_64::instructions::interrupts::enable();
    set_preempt_count(0);
}

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
        run_scheduler_if_needed();
        x86_64::instructions::hlt();
    }
}

pub(crate) fn global_init() {
    stack::stack_init();
}

define_per_cpu_u32!(
    /// The `TaskId` of the currently running task.
    CURRENT_TASK_ID
);

define_per_cpu_u32!(
    /// The `TaskId` for the idle task for the current CPU. Every CPU has its
    /// own idle task.
    IDLE_TASK_ID
);

pub(crate) fn per_cpu_init() {
    syscall::syscall_init();

    // Set the current CPU's idle task.
    let processor_id = percpu::get_processor_id_no_guard();
    let idle_task_id = TASKS.lock_disable_interrupts().new_task(
        format!("CPU {processor_id:?} __IDLE_TASK__"),
        idle_task_start,
        core::ptr::null(),
    );
    set_per_cpu_IDLE_TASK_ID(idle_task_id.0);
    set_per_cpu_CURRENT_TASK_ID(idle_task_id.0);
}

pub(crate) fn current_task_id() -> TaskId {
    // No guard is okay because task ID will live on stack of current task, and
    // preemption wouldn't change that.
    let id = get_per_cpu_no_guard_CURRENT_TASK_ID();
    TaskId(id)
}

pub(crate) fn current_task() -> Arc<Task> {
    TASKS
        .lock_disable_interrupts()
        .get_task_assert(current_task_id())
}

/// Switches from the bootstrap code, which isn't a task, to the first actual
/// kernel task.
pub(crate) fn start_scheduler() -> ! {
    // Just a dummy location for switch_to_task to store the previous stack
    // pointer.
    let dummy_stack_ptr = 0;
    let prev_stack_ptr = core::ptr::addr_of!(dummy_stack_ptr);
    let current_task = current_task();
    let next_stack_ptr = current_task.registers.rsp;
    let next_page_table = current_task.page_table.lock().physical_address();

    // Drop to decrement reference count or else we will leak because
    // switch_to_task will never return
    drop(current_task);

    unsafe {
        switch_to_task(prev_stack_ptr, next_stack_ptr, next_page_table);
    }

    panic!("ERROR: returned from switch_to_task in start_scheduler");
}

pub(crate) fn new_task(name: String, start_fn: KernelTaskStartFunction, arg: *const ()) -> TaskId {
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

define_per_cpu_u8!(
    /// When nonzero, the scheduler needs to run. This is set in contexts that
    /// can't run the scheduler (like interrupts), or in places that want to
    /// indicate the scheduler should run, but don't want it to run immediately.
    NEEDS_RESCHEDULE
);

pub(crate) fn run_scheduler() {
    // Set needs_reschedule to false if it hasn't been set already.
    set_per_cpu_NEEDS_RESCHEDULE(0);

    // N.B. This function is split up into sub functions to ensure values are
    // dropped before we hit `switch_to_task`.

    if !check_current_task_needs_preemption() {
        return;
    }

    // Take a lock on the run queue and disable interrupts. It is very important
    // we hold this lock past switch_to_task!
    let mut run_queue = RUN_QUEUE.lock_disable_interrupts();

    let Some((prev_stack_ptr, next_stack_ptr, next_page_table)) = task_swap_parameters(&mut run_queue) else {
        return;
    };

    unsafe {
        switch_to_task(prev_stack_ptr, next_stack_ptr, next_page_table);
    }
}

fn check_current_task_needs_preemption() -> bool {
    // no guard because we are in scheduler, and preemption is disabled
    let processor_id = percpu::get_processor_id_no_guard();
    let idle_task_id = TaskId(get_per_cpu_no_guard_IDLE_TASK_ID());

    let preempt_count = get_preempt_count_no_guard();
    let current_task = current_task();
    let task_id = current_task.id;

    // Check preempt counter. If it's non-zero, then we're in a critical
    // section and we shouldn't preempt.
    assert!(
        preempt_count >= 0,
        "preempt_count is negative! Something bad happened"
    );
    if preempt_count > 0 {
        log::warn!(
            "CPU {processor_id:?}: {task_id:?} preempt_count is {}, not preempting",
            preempt_count
        );
        return false;
    }

    // If the previous task still has a time slice left, don't preempt it.
    // (Except for idle task. We don't care if that ran out of time.)
    let is_idle = current_task.id == idle_task_id;
    let is_ready = current_task.desired_state.load() == DesiredTaskState::ReadyToRun;
    let is_expired = current_task.remaining_slice.load() == Milliseconds::new(0);
    if !is_idle && is_ready && !is_expired {
        return false;
    }

    true
}

fn task_swap_parameters(run_queue: &mut RunQueue) -> Option<(*const u64, u64, PhysAddr)> {
    let processor_id = percpu::get_processor_id_no_guard();
    let idle_task_id = TaskId(get_per_cpu_no_guard_IDLE_TASK_ID());

    run_queue.remove_killed_pending_tasks();
    let prev_task = current_task();
    let prev_task_id = prev_task.id;
    let prev_task_state = prev_task.desired_state.load();
    let next_task_id = if let Some(id) = run_queue.pop_next_ready_pending_task() {
        id
    } else {
        // No other task to switch to. If we are on the idle task, or if the
        // current task is ReadyToRun, don't switch tasks.
        if prev_task_id == idle_task_id || prev_task_state == DesiredTaskState::ReadyToRun {
            return None;
        }

        // Otherwise, switch to the idle task.
        idle_task_id
    };
    set_per_cpu_CURRENT_TASK_ID(next_task_id.0);

    // Store the previous task ID in pending task list, unless it is the
    // idle task.
    if prev_task_id != idle_task_id {
        run_queue.pending_tasks.push_back(prev_task_id);
    }

    let prev_task = TASKS
        .lock_disable_interrupts()
        .get_task_assert(prev_task_id);
    let prev_stack_ptr = core::ptr::addr_of!(prev_task.registers.rsp);
    let next_task = TASKS
        .lock_disable_interrupts()
        .get_task_assert(next_task_id);
    let next_stack_ptr = next_task.registers.rsp;
    let next_page_table = next_task.page_table.lock().physical_address();

    // Give the next task some time slice
    next_task.remaining_slice.store(DEFAULT_TIME_SLICE);

    if prev_task_id == next_task_id {
        // We're already running the next task, so just return.
        log::warn!("Tried to switch to the same task!: {prev_task_id:?} {next_task_id:?}");
        return None;
    }
    log::info!(
        "SCHEDULER: (CPU {:?}) Switching from '{}' {:?} to '{}' {:?}",
        processor_id,
        prev_task.name,
        prev_task.id,
        next_task.name,
        next_task.id,
    );

    // Reset per CPU kernel stack pointer and TSS rsp0
    set_per_cpu_TOP_OF_KERNEL_STACK(next_task.kernel_stack.top_addr().as_u64());
    set_tss_rsp0(processor_id, next_task.kernel_stack.top_addr());

    Some((prev_stack_ptr, next_stack_ptr, next_page_table))
}

/// If the scheduler needs to run, then run it.
pub(crate) fn run_scheduler_if_needed() {
    if get_per_cpu_no_guard_NEEDS_RESCHEDULE() > 0 {
        run_scheduler();
    }
}

/// Function to run every time the kernel tick system ticks.
pub(crate) fn scheduler_tick(time_between_ticks: Milliseconds) {
    // Deduct time from the currently running task's time slice.
    let current_task = current_task();
    let slice = current_task.remaining_slice.load();
    let slice = slice.saturating_sub(time_between_ticks);
    current_task.remaining_slice.store(slice);

    // If the task has run out of time, we need to run the scheduler.
    if slice == Milliseconds::new(0) {
        set_per_cpu_NEEDS_RESCHEDULE(1);
    }
}

/// Puts the current task to sleep for the given number of milliseconds.
pub(crate) fn sleep_timeout(timeout: Milliseconds) {
    let task_id = prepare_to_sleep();
    tick::add_relative_timer(timeout, move || {
        awaken_task(task_id);
    });
    run_scheduler();
}

pub(super) fn kill_current_task(exit_code: TaskExitCode) {
    let current_task = current_task();
    log::info!(
        "killing task {} {:?} with code {exit_code:?}",
        current_task.name,
        current_task.id
    );
    current_task.desired_state.swap(DesiredTaskState::Killed);

    // Inform waiters that the task has exited.
    current_task.exit_wait_cell.send_all_consumers(exit_code);

    // Drop to decrement reference count or else we will leak because
    // run_scheduler will never return
    drop(current_task);

    run_scheduler();

    panic!("run_scheduler in kill_current_task returned");
}

/// Puts the current task to sleep and returns the current task ID, but does
/// _not_ run the scheduler.
pub(crate) fn prepare_to_sleep() -> TaskId {
    set_per_cpu_NEEDS_RESCHEDULE(1);
    let current_task = current_task();
    current_task.desired_state.swap(DesiredTaskState::Sleeping);
    current_task.id
}

/// Awakens the given task and sets needs_reschedule to true if it wasn't
/// already ready to run.
pub(crate) fn awaken_task(task_id: TaskId) {
    let task = TASKS.lock_disable_interrupts().get_task_assert(task_id);
    let old_state = task.desired_state.swap(DesiredTaskState::ReadyToRun);
    if old_state != DesiredTaskState::ReadyToRun {
        set_per_cpu_NEEDS_RESCHEDULE(1);
    }
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
    previous_task_stack_pointer: *const u64,
    next_task_stack_pointer: u64,
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
