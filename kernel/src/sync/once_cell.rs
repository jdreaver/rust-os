use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

/// A thread-safe cell that can be written to only once.
///
/// Uses atomics for speed, but the tradeoff is we panic if the cell is written
/// to more than once. (If we used a spinlock we could ensure we only write once
/// and return an error instead of panicking.)
///
/// This should not be used directly in kernel code, but is instead a useful
/// primitive for other synchronization primitives that are safer.
#[derive(Debug)]
pub(super) struct OnceCell<T> {
    message: UnsafeCell<MaybeUninit<T>>,
    ready: AtomicBool,
}

unsafe impl<T> Sync for OnceCell<T> where T: Send {}

impl<T> OnceCell<T> {
    pub(super) fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::zeroed()),
            ready: AtomicBool::new(false),
        }
    }

    /// Write a value to the cell.
    ///
    /// # Safety
    ///
    /// This function should only be called once. This is important because
    /// writing a value discards the old value, and we will never drop the old
    /// value (this is a `MaybeUninit` feature/limitation). We panic if we call
    /// this function twice, but it is still marked unsafe so the caller is
    /// careful.
    pub(super) unsafe fn send(&self, message: T) {
        unsafe {
            self.message.get().write(MaybeUninit::new(message));
        };
        let old = self.ready.swap(true, Ordering::Release);
        assert!(!old, "ERROR: Tried to write cell value twice");
    }

    pub(super) fn receive(&self) -> Option<T> {
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

impl<T> Drop for OnceCell<T> {
    fn drop(&mut self) {
        if self.ready.load(Ordering::Acquire) {
            // Safety: We only ever store the message in `send`, which sets
            // `ready` to `true`. Therefore we can assume that this message has
            // been initialized.
            unsafe { self.message.get_mut().assume_init_drop() }
        }
    }
}
