use core::ops::{Deref, DerefMut};

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