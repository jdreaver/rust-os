use alloc::boxed::Box;
use core::fmt;
use core::marker::PhantomData;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU8, Ordering};

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
        let prev = self.ptr.swap(ptr, Ordering::SeqCst);
        assert!(
            prev.is_null(),
            "ERROR: InitCell already initialized, can't initialize again"
        );
    }

    pub(crate) fn get(&self) -> Option<&T> {
        unsafe { self.ptr.load(Ordering::SeqCst).as_ref() }
    }

    /// Wait (via a spin loop) until the value is initialized, then return a
    /// reference to it.
    pub(crate) fn wait_spin(&self) -> &T {
        loop {
            if let Some(value) = self.get() {
                return value;
            }
            core::hint::spin_loop();
        }
    }
}

impl<T> Drop for InitCell<T> {
    fn drop(&mut self) {
        // If the pointer is set, drop the value by converting back into a Box
        // and letting that drop.
        let ptr = self.ptr.load(Ordering::SeqCst);
        if !ptr.is_null() {
            unsafe { Box::from_raw(ptr) };
        }
    }
}

/// Wrapper around an atomic integer type (via `AtomicInt`) that supports
/// transparently converting to/from an enum.
#[derive(Debug)]
pub(crate) struct AtomicEnum<A, T> {
    val: A,
    _phantom: PhantomData<T>,
}

impl<A, T> AtomicEnum<A, T>
where
    A: AtomicInt,
    A::Integer: fmt::Display + Copy,
    T: TryFrom<A::Integer> + Into<A::Integer>,
{
    pub(crate) fn new(val: T) -> Self {
        Self {
            val: A::new(val.into()),
            _phantom: PhantomData,
        }
    }

    fn convert_from_integer(val: A::Integer) -> T {
        T::try_from(val).map_or_else(
            |_| {
                panic!("ERROR: Invalid enum value {val}");
            },
            |enum_val| enum_val,
        )
    }

    pub(crate) fn load(&self) -> T {
        let val = self.val.load(Ordering::SeqCst);
        Self::convert_from_integer(val)
    }

    pub(crate) fn swap(&self, val: T) -> T {
        let old_val = self.val.swap(val.into(), Ordering::SeqCst);
        Self::convert_from_integer(old_val)
    }
}

pub(crate) trait AtomicInt {
    type Integer;

    fn new(val: Self::Integer) -> Self;
    fn load(&self, order: Ordering) -> Self::Integer;
    fn store(&self, val: Self::Integer, order: Ordering);
    fn swap(&self, val: Self::Integer, order: Ordering) -> Self::Integer;
}

impl AtomicInt for AtomicU8 {
    type Integer = u8;

    fn new(val: Self::Integer) -> Self {
        Self::new(val)
    }

    fn load(&self, order: Ordering) -> Self::Integer {
        self.load(order)
    }

    fn store(&self, val: Self::Integer, order: Ordering) {
        self.store(val, order);
    }

    fn swap(&self, val: Self::Integer, order: Ordering) -> Self::Integer {
        self.swap(val, order)
    }
}
