use alloc::sync::Arc;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::sched;
use crate::sched::TaskId;

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
    let channel = Arc::new(OnceChannel::new());
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

/// A channel that can be written to once and read from once.
#[derive(Debug)]
pub(crate) struct OnceChannel<T> {
    message: UnsafeCell<MaybeUninit<T>>,
    ready: AtomicBool,
}

unsafe impl<T> Sync for OnceChannel<T> where T: Send {}

impl<T> OnceChannel<T> {
    fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::zeroed()),
            ready: AtomicBool::new(false),
        }
    }

    /// Write a value to the channel.
    ///
    /// # Safety
    ///
    /// This function should only be called once. This is important because
    /// writing a value discards the old value, and we will never drop the old
    /// value (this is a `MaybeUninit` feature/limitation). We panic if we call
    /// this function twice, but it is still marked unsafe so the caller is
    /// careful.
    unsafe fn send(&self, message: T) {
        unsafe {
            self.message.get().write(MaybeUninit::new(message));
        };
        let old = self.ready.swap(true, Ordering::Release);
        assert!(!old, "ERROR: Tried to send to a channel twice");
    }

    fn receive(&self) -> Option<T> {
        if self.ready.swap(false, Ordering::Acquire) {
            // Safety: We should only read a message once, since we are reifying
            // it from a single memory location. The swap above is an extra
            // safeguard to ensure we don't read the message twice.
            let message = unsafe { self.message.get().read().assume_init_read() };
            Some(message)
        } else {
            None
        }
    }
}

impl<T> Drop for OnceChannel<T> {
    fn drop(&mut self) {
        if self.ready.load(Ordering::Acquire) {
            // Safety: We only ever store the message in `send`, which sets
            // `ready` to `true`. Therefore we can assume that this message has
            // been initialized.
            unsafe { self.message.get_mut().assume_init_drop() }
        }
    }
}

/// Sender side of a `OnceChannel`.
#[derive(Debug)]
pub(crate) struct OnceSender<T> {
    channel: Arc<OnceChannel<T>>,
    receiver_task_id: TaskId,
}

impl<T> OnceSender<T> {
    /// Write a value to the channel so the receiver can read it. This can only
    /// be called once because it consumes `self`.
    pub(crate) fn send(self, message: T) {
        // Safety: We only call this function once, which is enforced by this
        // function consuming `self`.
        unsafe { self.channel.send(message) };
        sched::scheduler_lock().awaken_task(self.receiver_task_id);
    }
}

/// Receiver side of a `OnceChannel`.
#[derive(Debug)]
pub(crate) struct OnceReceiver<T> {
    channel: Arc<OnceChannel<T>>,

    /// This is a hack to make `OnceReceiver` not implement `Send`. This is
    /// necessary so the `TaskId` of the receiver doesn't change. If the
    /// `TaskId` changed, then the sender would wake up the wrong task.
    _no_send: PhantomData<*const ()>,
}

impl<T> OnceReceiver<T> {
    pub(crate) fn wait_sleep(&self) -> T {
        loop {
            if let Some(message) = self.channel.receive() {
                return message;
            }
            sched::scheduler_lock().go_to_sleep();
        }
    }
}
