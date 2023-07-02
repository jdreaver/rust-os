use alloc::vec::Vec;

use crate::sched;
use crate::sched::TaskId;

use super::once_cell::OnceCell;
use super::spin_lock::SpinLock;

/// A value that can be waited on by tasks. Tasks sleep while they wait, and
/// they are woken up when the value is written. Each waiting task is given a
/// copy of the value. It is common to use `Arc` as the value type, to make
/// copies cheap.
#[derive(Debug)]
pub(crate) struct WaitCell<T> {
    cell: OnceCell<T>,
    waiting_tasks: SpinLock<Vec<TaskId>>,
}

impl<T: Clone> WaitCell<T> {
    pub(crate) fn new() -> Self {
        Self {
            cell: OnceCell::new(),
            waiting_tasks: SpinLock::new(Vec::new()),
        }
    }

    /// Sends value to all waiting tasks and wakes them up.
    pub(crate) fn send_all_consumers(&self, val: T) {
        self.cell.set(val);
        let mut task_ids = self.waiting_tasks.lock_disable_interrupts();
        for task_id in task_ids.drain(..) {
            sched::awaken_task(task_id);
        }
    }

    /// Waits until the value is initialized, sleeping if necessary.
    pub(crate) fn wait_sleep(&self) -> T {
        loop {
            // Set desired_state to sleeping before checking value to avoid race
            // condition where we get woken up before we go to sleep.
            let task_id = sched::prepare_to_sleep();

            // TODO: If we have a spurious wakeup we might add ourselves twice
            // because we would have never been removed from waiting_tasks in
            // the first place. Seems fine for now.
            self.waiting_tasks.lock_disable_interrupts().push(task_id);

            let message = self.cell.get_clone();
            if let Some(message) = message {
                sched::awaken_task(task_id);
                return message;
            }
            sched::run_scheduler();
        }
    }
}
