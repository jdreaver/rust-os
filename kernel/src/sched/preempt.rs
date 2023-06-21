use crate::barrier::barrier;
use crate::define_per_cpu_i64;

define_per_cpu_i64!(
    /// When preempt_count > 0, preemption is disabled, which means the
    /// scheduler will not switch off the current task.
    PREEMPT_COUNT
);

pub(super) fn get_preempt_count() -> i64 {
    get_per_cpu_PREEMPT_COUNT()
}

pub(super) fn set_preempt_count(count: i64) {
    set_per_cpu_PREEMPT_COUNT(count);
}

/// Simple type that disables preemption while it is alive, and re-enables it
/// when dropped.
pub(crate) struct PreemptGuard;

impl PreemptGuard {
    pub(crate) fn new() -> Self {
        inc_per_cpu_PREEMPT_COUNT();
        barrier();
        Self
    }
}

impl Drop for PreemptGuard {
    fn drop(&mut self) {
        barrier();
        dec_per_cpu_PREEMPT_COUNT();
    }
}
