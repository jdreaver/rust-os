use core::ops::{Deref, DerefMut};

use crate::barrier::barrier;
use crate::define_per_cpu_i64;

define_per_cpu_i64!(
    /// When preempt_count > 0, preemption is disabled, which means the
    /// scheduler will not switch off the current task.
    PREEMPT_COUNT
);

pub(super) fn get_preempt_count_no_guard() -> i64 {
    get_per_cpu_no_guard_PREEMPT_COUNT()
}

pub(super) fn set_preempt_count(count: i64) {
    set_per_cpu_PREEMPT_COUNT(count);
}

/// Simple type that disables preemption while it is alive, and re-enables it
/// when dropped.
#[derive(Debug, PartialOrd, Ord, PartialEq, Eq)]
pub(crate) struct PreemptGuard<T> {
    val: T,
}

impl<T: Copy> PreemptGuard<T> {
    #[allow(dead_code)]
    pub(crate) fn map<U, F: FnOnce(T) -> U>(self, f: F) -> PreemptGuard<U> {
        let val = self.val;
        PreemptGuard::new(f(val))
    }
}

impl<T> PreemptGuard<T> {
    pub(crate) fn new(val: T) -> Self {
        inc_per_cpu_PREEMPT_COUNT();
        barrier();
        Self { val }
    }
}

impl<T> Drop for PreemptGuard<T> {
    fn drop(&mut self) {
        barrier();
        dec_per_cpu_PREEMPT_COUNT();
    }
}

impl<T> Deref for PreemptGuard<T> {
    type Target = T;

    #[allow(clippy::explicit_deref_methods)]
    fn deref(&self) -> &T {
        &self.val
    }
}

impl<T> DerefMut for PreemptGuard<T> {
    #[allow(clippy::explicit_deref_methods)]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.val
    }
}
