use core::ops::{Deref, DerefMut};

use spin::mutex::{SpinMutex, SpinMutexGuard};

use crate::percpu::PreemptGuard;

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
        // Ordering is important! Disable preemption before taking the lock.
        let preempt_guard = PreemptGuard::new();
        SpinLockGuard {
            guard: self.mutex.lock(),
            _interrupt_guard: InterruptGuard {
                needs_enabling: false,
            },
            _preempt_guard: preempt_guard,
        }
    }

    /// Locks the mutex and disables interrupts while the lock is held. Restores
    /// interrupts to their previous state (enabled or disabled) once the lock
    /// is released.
    pub(crate) fn lock_disable_interrupts(&self) -> SpinLockGuard<'_, T> {
        // Ordering is important! Disable preemption before taking the lock.
        let preempt_guard = PreemptGuard::new();

        let saved_intpt_flag = x86_64::instructions::interrupts::are_enabled();

        // If interrupts are enabled, disable them for now. They will be
        // re-enabled when the guard drops.
        if saved_intpt_flag {
            x86_64::instructions::interrupts::disable();
        }

        SpinLockGuard {
            guard: self.mutex.lock(),
            _interrupt_guard: InterruptGuard {
                needs_enabling: saved_intpt_flag,
            },
            _preempt_guard: preempt_guard,
        }
    }

    pub(crate) unsafe fn force_unlock(&self) {
        self.mutex.force_unlock();
    }
}

/// Wrapper around `spin::mutex::SpinMutexGuard`, used with `SpinLock`.
pub(crate) struct SpinLockGuard<'a, T: ?Sized + 'a> {
    guard: SpinMutexGuard<'a, T>,
    // Note: ordering is very important here! We want to restore interrupts to
    // their previous state (enabled or disabled) _after_ the spinlock guard is
    // dropped. Rust drops fields in order.
    _interrupt_guard: InterruptGuard,
    // We want to drop preemption after dropping the lock and enabling
    // interrupts.
    _preempt_guard: PreemptGuard,
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
