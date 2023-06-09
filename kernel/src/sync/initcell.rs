use alloc::boxed::Box;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

/// A cell that can be initialized only once. This is useful because we can
/// share it between multiple threads without having to use a mutex, and since
/// the value can only be written once, we don't need a mutable reference to
/// write to it, so we can store this value as e.g. a static.
#[derive(Debug)]
pub(crate) struct InitCell<T> {
    ptr: AtomicPtr<T>,
}

impl<T> InitCell<T> {
    pub(crate) const fn new() -> Self {
        Self {
            ptr: AtomicPtr::new(ptr::null_mut()),
        }
    }

    pub(crate) fn init(&self, value: T) {
        let ptr = Box::into_raw(Box::new(value));
        let prev = self.ptr.swap(ptr, Ordering::Acquire);
        assert!(
            prev.is_null(),
            "ERROR: InitCell already initialized, can't initialize again"
        );
    }

    pub(crate) fn get(&self) -> Option<&T> {
        // This is safe because we only ever write to the pointer once, so the
        // lifetime of the value does indeed match the lifetime of the InitCell.
        unsafe { self.ptr.load(Ordering::Acquire).as_ref() }
    }
}

impl<T> Drop for InitCell<T> {
    fn drop(&mut self) {
        // If the pointer is set, drop the value by converting back into a Box
        // and letting that drop.
        let ptr = self.ptr.load(Ordering::Acquire);
        if !ptr.is_null() {
            unsafe { Box::from_raw(ptr) };
        }
    }
}
