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
        // Lock task_ids _before_ setting the value, or else a task might add
        // itself to the list of waiting tasks after we awaken tasks and never
        // get woken up.
        let mut task_ids = self.waiting_tasks.lock_disable_interrupts();
        self.cell.set(val);
        for task_id in task_ids.drain(..) {
            sched::scheduler_lock().awaken_task(task_id);
        }
        // N.B. Cannot call run_scheduler here because this might be running in
        // an interrupt context.
    }

    /// Waits until the value is initialized, sleeping if necessary.
    pub(crate) fn wait_sleep(&self) -> T {
        // Add ourselves to the list of waiting tasks. It is important this is
        // done before we check for the value being set, and that this is done
        // with a lock on `waiting_tasks` (which is also taken in
        // `send_all_consumers`), otherwise we might miss the value being set
        // and go to sleep forever because no one will wake us up.
        let task_id = sched::scheduler_lock().current_task_id();
        self.waiting_tasks.lock_disable_interrupts().push(task_id);

        loop {
            let message = self.cell.get_clone();
            if let Some(message) = message {
                return message;
            }
            sched::scheduler_lock().go_to_sleep();
        }
    }
}
