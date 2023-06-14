//! Global tick system that runs every `TICK_HZ` times per second.

use alloc::boxed::Box;
use alloc::collections::VecDeque;

use crate::hpet::Milliseconds;
use crate::interrupts::ReservedInterruptVectors;
use crate::sync::SpinLock;
use crate::{apic, hpet, interrupts, ioapic, sched};

/// Frequency of the global tick system.
const TICK_HZ: u64 = 20;

const TICK_MILLIS: Milliseconds = Milliseconds::new(1000 / TICK_HZ);

/// Global list of timers
static TIMERS: SpinLock<VecDeque<Timer>> = SpinLock::new(VecDeque::new());

#[allow(clippy::assertions_on_constants)]
pub(crate) fn global_init() {
    assert!(
        1000 % TICK_HZ == 0,
        "TICK_HZ must be a divisor of 1000 so we can evenly divide milliseconds into ticks"
    );

    hpet::enable_periodic_timer_handler(
        0,
        tick_broadcast_handler,
        ioapic::IOAPICIRQNumber::Tick,
        hpet::HPETTimerNumber::Tick,
        TICK_MILLIS,
    );
}

pub(crate) fn per_cpu_init() {
    interrupts::install_interrupt(
        Some(ReservedInterruptVectors::CPUTick as u8),
        0,
        cpu_tick_handler,
    );
}

/// Handler for tick from the HPET. Broadcasts to all CPUs.
fn tick_broadcast_handler(_vector: u8, _handler_id: interrupts::InterruptHandlerID) {
    // Iterate through all timers and fire off + remove ones that expired.
    TIMERS.lock().retain_mut(|timer| {
        if timer.expiration <= hpet::elapsed_milliseconds() {
            (timer.callback)();
            false
        } else {
            true
        }
    });

    // Send a tick to all CPUs
    apic::send_ipi_all_cpus(ReservedInterruptVectors::CPUTick as u8);
}

fn cpu_tick_handler(_vector: u8, _handler_id: interrupts::InterruptHandlerID) {
    // Let the scheduler do accounting
    sched::scheduler_tick(TICK_MILLIS);
}

struct Timer {
    /// Expiration time in milliseconds since boot.
    expiration: Milliseconds,

    /// Callback to call when the timer expires. This function is called in an
    /// interrupt context, so it must be fast and it must not sleep, block, or
    /// take spin locks that shouldn't be taken in an interrupt context!
    ///
    /// TODO: Implement something akin to linux softirq so we can be more
    /// flexible with our timers.
    callback: Box<dyn FnMut() + Send>,
}

/// Adds a timer to be called after the global milliseconds since boot reaches
/// the given number of milliseconds.
pub(crate) fn add_timer<F>(expiration: Milliseconds, callback: F)
where
    F: FnMut() + Send + 'static,
{
    let mut timers = TIMERS.lock();
    let timer = Timer {
        expiration,
        callback: Box::new(callback),
    };
    timers.push_back(timer);
}

/// Adds a timer to be called after the given number of milliseconds.
pub(crate) fn add_relative_timer<F>(timeout: Milliseconds, callback: F)
where
    F: FnMut() + Send + 'static,
{
    let current_millis = hpet::elapsed_milliseconds();
    let expiration = current_millis + timeout;
    add_timer(expiration, callback);
}
