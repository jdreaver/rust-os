use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::fmt;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};
use core::ptr;
use core::sync::atomic::{
    AtomicBool, AtomicPtr, AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering,
};

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
        sched::awaken_task(self.receiver_task_id);
    }
}

/// Receiver side of a `OnceChannel`.
#[derive(Debug)]
pub(crate) struct OnceReceiver<T> {
    channel: Arc<OnceChannel<T>>,

    // This is a hack to make `OnceReceiver` not implement `Send`. This is
    // necessary so the `TaskId` of the receiver doesn't change. If the `TaskId`
    // changed, then the sender would wake up the wrong task.
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
