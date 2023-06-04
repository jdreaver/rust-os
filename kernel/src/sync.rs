use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering};

use spin::mutex::{SpinMutex, SpinMutexGuard};

use crate::sched;
use crate::sched::TaskId;

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
        }
    }

    /// Locks the mutex and disables interrupts while the lock is held. Restores
    /// interrupts to their previous state (enabled or disabled) once the lock
    /// is released.
    pub(crate) fn lock_disable_interrupts(&self) -> SpinLockInterruptGuard<'_, T> {
        let saved_intpt_flag = x86_64::instructions::interrupts::are_enabled();

        // If interrupts are enabled, disable them for now. They will be
        // re-enabled when the guard drops.
        if saved_intpt_flag {
            x86_64::instructions::interrupts::disable();
        }

        SpinLockInterruptGuard {
            guard: self.mutex.lock(),
            _interrupt_guard: InterruptGuard {
                needs_enabling: saved_intpt_flag,
            },
        }
    }

    pub(crate) unsafe fn force_unlock(&self) {
        self.mutex.force_unlock();
    }
}

/// Wrapper around `spin::mutex::SpinMutexGuard`, used with `SpinLock`.
pub(crate) struct SpinLockGuard<'a, T: ?Sized + 'a> {
    guard: SpinMutexGuard<'a, T>,
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

/// Similar to `SpinLockGuard`, except it also handles disabling and enabling
/// interrupts.
pub(crate) struct SpinLockInterruptGuard<'a, T: ?Sized + 'a> {
    guard: SpinMutexGuard<'a, T>,
    // Note: ordering is very important here! We want to restore interrupts to
    // their previous state (enabled or disabled) _after_ the spinlock guard is
    // dropped. Rust drops fields in order.
    _interrupt_guard: InterruptGuard,
}

impl<'a, T> Deref for SpinLockInterruptGuard<'a, T> {
    type Target = T;

    #[allow(clippy::explicit_deref_methods)]
    fn deref(&self) -> &T {
        self.guard.deref()
    }
}

impl<'a, T> DerefMut for SpinLockInterruptGuard<'a, T> {
    #[allow(clippy::explicit_deref_methods)]
    fn deref_mut(&mut self) -> &mut T {
        self.guard.deref_mut()
    }
}

struct InterruptGuard {
    needs_enabling: bool,
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        if self.needs_enabling {
            x86_64::instructions::interrupts::enable();
        }
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

    /// Wait (via a spin loop) until the value is initialized, then return a
    /// reference to it.
    pub(crate) fn _wait_spin(&self) -> &T {
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
        let ptr = self.ptr.load(Ordering::Acquire);
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
        let val = <I as AtomicIntTrait>::load(&self.atom, Ordering::Acquire);
        T::from(val)
    }

    pub(crate) fn store(&self, val: T) {
        <I as AtomicIntTrait>::store(&self.atom, val.into(), Ordering::Release);
    }

    pub(crate) fn swap(&self, val: T) -> T {
        let old_val = <I as AtomicIntTrait>::swap(&self.atom, val.into(), Ordering::Acquire);
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

/// A value that can be waited on by tasks. Tasks sleep while they wait, and
/// they are woken up when the value is written. Each waiting task is given a
/// copy of the value. It is common to use `Arc` as the value type, to make
/// copies cheap.
#[derive(Debug)]
pub(crate) struct WaitQueue<T>(SpinLock<WaitQueueInner<T>>);

#[derive(Debug)]
struct WaitQueueInner<T> {
    value: Option<T>,

    /// The tasks waiting for the value to change.
    task_ids: Vec<TaskId>, // N.B. Vec faster than HashSet for small sets
}

impl<T: Clone> WaitQueue<T> {
    pub(crate) const fn new() -> Self {
        Self(SpinLock::new(WaitQueueInner {
            value: None,
            task_ids: Vec::new(),
        }))
    }

    /// Stores the value and wakes up the sleeping tasks.
    pub(crate) fn put_value(&self, val: T) {
        let mut inner = self.0.lock_disable_interrupts();
        inner.value.replace(val);
        for task_id in inner.task_ids.drain(..) {
            sched::awaken_task(task_id);
        }
    }

    /// Waits until the value is initialized, sleeping if necessary.
    pub(crate) fn wait_sleep(&self) -> T {
        let task_id = sched::scheduler_lock().current_task_id();
        self.0.lock_disable_interrupts().task_ids.push(task_id);

        loop {
            {
                let mut inner = self.0.lock_disable_interrupts();

                if let Some(value) = &inner.value {
                    let value = value.clone();

                    // Remove task ID from the list of waiting tasks.
                    let index = inner.task_ids.iter().position(|id| *id == task_id);
                    if let Some(index) = index {
                        inner.task_ids.swap_remove(index);
                    }

                    return value;
                }

                // Value isn't present. Go back to sleep. It is important we do
                // this while the lock is still taken or else a producer might
                // write the value before this line and we may never wake up.
                sched::scheduler_lock().go_to_sleep();
            }

            // Important to run the scheduler outside of the lock, otherwise
            // we can deadlock.
            sched::scheduler_lock().run_scheduler();
        }
    }
}
