use core::cell::{Ref, RefCell};
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
    message: RefCell<Option<T>>,
    ready: AtomicBool,
}

unsafe impl<T> Sync for OnceCell<T> where T: Send {}

impl<T> OnceCell<T> {
    pub(super) const fn new() -> Self {
        Self {
            message: RefCell::new(None),
            ready: AtomicBool::new(false),
        }
    }

    /// Write a value to the cell.
    ///
    /// This function can only be called once. This is important because writing
    /// a value discards the old value, and we will never drop the old value
    /// (this is a `MaybeUninit` feature/limitation). We panic if we call this
    /// function twice.
    pub(super) fn set(&self, message: T) {
        let _ = self.message.replace(Some(message));
        let old = self.ready.swap(true, Ordering::Release);
        assert!(!old, "ERROR: Tried to write cell value twice");
    }

    /// Extracts the value from the cell.
    ///
    /// This function can only be called once. It is itself "safe" because the
    /// invariant on `set` ensures we only call `set` once, and the function
    /// also uses the atomic bool `ready` to ensure we only read once. However,
    /// the caller should ensure that if `get_once` is ever called, then other
    /// functions that get references or copied to the value are _never_ called.
    pub(super) fn get_once(&self) -> Option<T> {
        if self.ready.swap(false, Ordering::Acquire) {
            // Safety: We should only read a message once, since we are reifying
            // it from a single memory location. The swap above is an extra
            // safeguard to ensure we don't read the message twice.
            self.message.replace(None)
        } else {
            None
        }
    }

    /// Extracts a reference to the stored value.
    pub(super) fn get_ref(&self) -> Option<&T> {
        if self.ready.load(Ordering::Acquire) {
            // NOTE: We use `Ref::leak` here to avoid needing to wrap in `Ref`.
            // Using `leak` means writing to the `RefCell` will panic, which is
            // preferred to unsafe behavior.
            Ref::leak(self.message.borrow()).as_ref()
        } else {
            None
        }
    }
}

impl<T: Clone> OnceCell<T> {
    /// Extracts a clone of the stored value.
    pub(super) fn get_clone(&self) -> Option<T> {
        if self.ready.load(Ordering::Acquire) {
            self.message.borrow().clone()
        } else {
            None
        }
    }
}
