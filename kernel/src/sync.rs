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
pub(crate) struct AtomicEnum<I, T>
where
    I: AtomicInt,
{
    atom: I::Atomic,
    _phantom: PhantomData<T>,
}

impl<I, T> AtomicEnum<I, T>
where
    I: AtomicInt + fmt::Display + Copy,
    T: TryFrom<I> + Into<I>,
{
    pub(crate) fn new(val: T) -> Self {
        Self {
            atom: <I as AtomicInt>::new(val.into()),
            _phantom: PhantomData,
        }
    }

    fn convert_from_integer(val: I) -> T {
        T::try_from(val).map_or_else(
            |_| {
                panic!("ERROR: Invalid enum value {val}");
            },
            |enum_val| enum_val,
        )
    }

    pub(crate) fn load(&self) -> T {
        let val = <I as AtomicInt>::load(&self.atom, Ordering::SeqCst);
        Self::convert_from_integer(val)
    }

    // pub(crate) fn store(&self, val: T) {
    //     <I as AtomicInt>::store(&self.atom, val.into(), Ordering::SeqCst);
    // }

    pub(crate) fn swap(&self, val: T) -> T {
        let old_val = <I as AtomicInt>::swap(&self.atom, val.into(), Ordering::SeqCst);
        Self::convert_from_integer(old_val)
    }
}

pub(crate) trait AtomicInt {
    type Atomic;

    fn new(val: Self) -> Self::Atomic;
    fn load(atom: &Self::Atomic, order: Ordering) -> Self;
    fn store(atom: &Self::Atomic, val: Self, order: Ordering);
    fn swap(atom: &Self::Atomic, val: Self, order: Ordering) -> Self;
}

impl AtomicInt for u8 {
    type Atomic = AtomicU8;

    fn new(val: Self) -> Self::Atomic {
        Self::Atomic::new(val)
    }

    fn load(atom: &Self::Atomic, order: Ordering) -> Self {
        atom.load(order)
    }

    fn store(atom: &Self::Atomic, val: Self, order: Ordering) {
        atom.store(val, order);
    }

    fn swap(atom: &Self::Atomic, val: Self, order: Ordering) -> Self {
        atom.swap(val, order)
    }
}
