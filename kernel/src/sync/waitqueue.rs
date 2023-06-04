use alloc::vec::Vec;

use super::{once_channel, OnceSender, SpinLock};

/// A value that can be waited on by tasks. Tasks sleep while they wait, and
/// they are woken up when the value is written. Each waiting task is given a
/// copy of the value. It is common to use `Arc` as the value type, to make
/// copies cheap.
#[derive(Debug)]
pub(crate) struct WaitQueue<T> {
    /// Holds the sending side of all the channels we need to send values to.
    channel_senders: SpinLock<Vec<OnceSender<T>>>,
}

impl<T> WaitQueue<T> {
    pub(crate) const fn new() -> Self {
        Self {
            channel_senders: SpinLock::new(Vec::new()),
        }
    }

    /// Sends value to just the first waiting task and wakes it up. Use
    /// `put_value` to send to all sleeping tasks.
    pub(crate) fn _send_single_consumer(&self, val: T) {
        let mut senders = self.channel_senders.lock_disable_interrupts();
        if let Some(sender) = senders.pop() {
            sender.send(val);
        }
    }

    /// Waits until the value is initialized, sleeping if necessary.
    pub(crate) fn wait_sleep(&self) -> T {
        // Create a new channel and add it to the list of channels to send
        // values to.
        let (sender, receiver) = once_channel();
        self.channel_senders.lock_disable_interrupts().push(sender);

        receiver.wait_sleep()
    }
}

impl<T: Clone> WaitQueue<T> {
    /// Sends value to all waiting tasks and wakes them up.
    pub(crate) fn send_all_consumers(&self, val: T) {
        let mut senders = self.channel_senders.lock_disable_interrupts();
        for sender in senders.drain(..) {
            sender.send(val.clone());
        }
    }
}
