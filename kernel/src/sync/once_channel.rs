use alloc::sync::Arc;
use core::marker::PhantomData;

use crate::sched;
use crate::sched::TaskId;

use super::once_cell::OnceCell;

/// Creates a `OnceSender` and `OnceReceiver` pair. The sender can send a single
/// value (hence the "once") to the receiver, and the receiver can wait for the
/// value to be sent. While waiting, the receiver is put to sleep, and the
/// sender ensures the receiver is woken up when the value is sent.
///
/// Note that this function must be called on the receiver's thread and the
/// receiver can't be moved to another thread, since the `ThreadId` for the
/// receiver is stored in `OnceSender`. This is enforced by `OnceReceiver` _not_
/// implementing `Send`.
pub(crate) fn once_channel<T>() -> (OnceSender<T>, OnceReceiver<T>) {
    let receiver_task_id = sched::scheduler_lock().current_task_id();
    let channel = Arc::new(OnceCell::new());
    let sender = OnceSender {
        channel: channel.clone(),
        receiver_task_id,
    };
    let receiver = OnceReceiver {
        channel,
        _no_send: PhantomData,
    };
    (sender, receiver)
}

/// Sender side of a `once_channel`.
#[derive(Debug)]
pub(crate) struct OnceSender<T> {
    channel: Arc<OnceCell<T>>,
    receiver_task_id: TaskId,
}

impl<T> OnceSender<T> {
    /// Write a value to the channel so the receiver can read it. This can only
    /// be called once because it consumes `self`.
    pub(crate) fn send(self, message: T) {
        // Safety: We only call this function once, which is enforced by this
        // function consuming `self`.
        unsafe { self.channel.set(message) };
        sched::scheduler_lock().awaken_task(self.receiver_task_id);
    }
}

/// Receiver side of a `once_channel`.
#[derive(Debug)]
pub(crate) struct OnceReceiver<T> {
    channel: Arc<OnceCell<T>>,

    /// This is a hack to make `OnceReceiver` not implement `Send`. This is
    /// necessary so the `TaskId` of the receiver doesn't change. If the
    /// `TaskId` changed, then the sender would wake up the wrong task.
    _no_send: PhantomData<*const ()>,
}

impl<T> OnceReceiver<T> {
    pub(crate) fn wait_sleep(&self) -> T {
        loop {
            let message = unsafe { self.channel.get_once() };
            if let Some(message) = message {
                return message;
            }
            sched::scheduler_lock().go_to_sleep();
        }
    }
}
