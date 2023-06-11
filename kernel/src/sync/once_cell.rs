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
    pub(super) const fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            ready: AtomicBool::new(false),
        }
    }

    /// Write a value to the cell.
    ///
    /// # Safety
    ///
    /// This function can only be called once. This is important because writing
    /// a value discards the old value, and we will never drop the old value
    /// (this is a `MaybeUninit` feature/limitation). We panic if we call this
    /// function twice, but it is still marked unsafe so the caller is careful.
    pub(super) unsafe fn set(&self, message: T) {
        unsafe {
            self.message.get().write(MaybeUninit::new(message));
        };
        let old = self.ready.swap(true, Ordering::Release);
        assert!(!old, "ERROR: Tried to write cell value twice");
    }

    /// Extracts the value from the cell.
    ///
    /// # Safety
    ///
    /// This function can only be called once. It is itself "safe" because the
    /// invariant on `set` ensures we only call `set` once, and the function
    /// also uses the atomic bool `ready` to ensure we only read once. However,
    /// the caller should ensure that if `get_once` is ever called, then other
    /// functions that get references or copied to the value are _never_ called.
    pub(super) unsafe fn get_once(&self) -> Option<T> {
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

    /// Extracts a reference to the stored value.
    pub(super) fn get_ref(&self) -> Option<&T> {
        if self.ready.load(Ordering::Acquire) {
            let ptr_ref = unsafe { self.message.get().as_ref().expect("null pointer") };
            unsafe { Some(ptr_ref.assume_init_ref()) }
        } else {
            None
        }
    }
}

// impl<T: Copy> OnceCell<T> {
//     /// Extracts a copy of the stored value.
//     pub(super) unsafe fn get_copy(&self) -> Option<T> {
//         if self.ready.load(Ordering::Acquire) {
//             // Safety: We ensure that the type implements Copy, which is
//             // required by `assume_init_read`.
//             Some(self.message.get().read().assume_init_read())
//         } else {
//             None
//         }
//     }
// }

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
