use alloc::boxed::Box;
use core::fmt;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering};

use spin::mutex::{SpinMutex, SpinMutexGuard};

/// Wrapper around `spin::mutex::SpinMutex` with some added features, like
/// handling disabling and enabling interrupts.
#[derive(Debug)]
pub(crate) struct SpinLock<T> {
    mutex: SpinMutex<T>,
}

impl<T> SpinLock<T> {
    pub(crate) const fn new(data: T) -> Self {
        Self {
            mutex: SpinMutex::new(data),
        }
    }

    pub(crate) fn lock(&self) -> SpinLockGuard<'_, T> {
        SpinLockGuard {
            guard: self.mutex.lock(),
            enable_interrupts_on_drop: false,
        }
    }

    /// Locks the mutex and disables interrupts while the lock is held. Restores
    /// interrupts to their previous state (enabled or disabled) once the lock
    /// is released.
    pub(crate) fn lock_disable_interrupts(&self) -> SpinLockGuard<'_, T> {
        let saved_intpt_flag = x86_64::instructions::interrupts::are_enabled();

        // If interrupts are enabled, disable them for now. They will be
        // re-enabled when the guard drops.
        if saved_intpt_flag {
            x86_64::instructions::interrupts::disable();
        }

        SpinLockGuard {
            guard: self.mutex.lock(),
            enable_interrupts_on_drop: saved_intpt_flag,
        }
    }

    pub(crate) unsafe fn force_unlock(&self) {
        self.mutex.force_unlock();
    }
}

/// Wrapper around `spin::mutex::SpinMutexGuard`, used with `SpinLock`.
pub(crate) struct SpinLockGuard<'a, T: ?Sized + 'a> {
    guard: SpinMutexGuard<'a, T>,
    enable_interrupts_on_drop: bool,
}

impl<'a, T: ?Sized> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        if self.enable_interrupts_on_drop {
            x86_64::instructions::interrupts::enable();
        }
    }
}

impl<'a, T> Deref for SpinLockGuard<'a, T> {
    type Target = T;

    #[allow(clippy::explicit_deref_methods)]
    fn deref(&self) -> &T {
        self.guard.deref()
    }
}

impl<'a, T> DerefMut for SpinLockGuard<'a, T> {
    #[allow(clippy::explicit_deref_methods)]
    fn deref_mut(&mut self) -> &mut T {
        self.guard.deref_mut()
    }
}

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
        // This is safe because we only ever write to the pointer once, so the
        // lifetime of the value does indeed match the lifetime of the InitCell.
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
/// transparently converting to/from a specific type.
#[derive(Debug)]
pub(crate) struct AtomicInt<I, T>
where
    I: AtomicIntTrait,
{
    atom: I::Atomic,
    _phantom: PhantomData<T>,
}

impl<I, T> AtomicInt<I, T>
where
    I: AtomicIntTrait + fmt::Display + Copy,
    T: From<I> + Into<I>,
{
    pub(crate) fn new(val: T) -> Self {
        Self {
            atom: <I as AtomicIntTrait>::new(val.into()),
            _phantom: PhantomData,
        }
    }

    pub(crate) fn load(&self) -> T {
        let val = <I as AtomicIntTrait>::load(&self.atom, Ordering::SeqCst);
        T::from(val)
    }

    // pub(crate) fn store(&self, val: T) {
    //     <I as AtomicInt>::store(&self.atom, val.into(), Ordering::SeqCst);
    // }

    pub(crate) fn swap(&self, val: T) -> T {
        let old_val = <I as AtomicIntTrait>::swap(&self.atom, val.into(), Ordering::SeqCst);
        T::from(old_val)
    }
}

pub(crate) trait AtomicIntTrait {
    type Atomic;

    fn new(val: Self) -> Self::Atomic;
    fn load(atom: &Self::Atomic, order: Ordering) -> Self;
    fn store(atom: &Self::Atomic, val: Self, order: Ordering);
    fn swap(atom: &Self::Atomic, val: Self, order: Ordering) -> Self;
}

macro_rules! atomic_int_trait_impl {
    ($type:ty, $atom:ty) => {
        impl AtomicIntTrait for $type {
            type Atomic = $atom;

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
    };
}

atomic_int_trait_impl!(u8, AtomicU8);
atomic_int_trait_impl!(u16, AtomicU16);
atomic_int_trait_impl!(u32, AtomicU32);
atomic_int_trait_impl!(u64, AtomicU64);

/// Wrapper around `AtomicInt` that allows fallible conversion, which is super
/// useful for enums.
#[derive(Debug)]
pub(crate) struct AtomicEnum<I, T>
where
    I: AtomicIntTrait,
    I::Atomic: fmt::Debug,
{
    int: AtomicInt<I, I>,
    _phantom: PhantomData<T>,
}

impl<I, T> AtomicEnum<I, T>
where
    I: AtomicIntTrait + fmt::Display + Copy,
    I::Atomic: fmt::Debug,
    T: TryFrom<I> + Into<I>,
{
    pub(crate) fn new(val: T) -> Self {
        Self {
            int: AtomicInt::new(val.into()),
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
        let val = self.int.load();
        Self::convert_from_integer(val)
    }

    pub(crate) fn swap(&self, val: T) -> T {
        let old_val = self.int.swap(val.into());
        Self::convert_from_integer(old_val)
    }
}
