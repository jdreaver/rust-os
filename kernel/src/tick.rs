//! Global tick system that runs every `TICK_HZ` times per second.

use alloc::boxed::Box;
use alloc::collections::VecDeque;

use spin::mutex::SpinMutex;

use crate::hpet::Milliseconds;
use crate::{hpet, interrupts, ioapic};

/// Frequency of the global tick system.
const TICK_HZ: u64 = 10;

/// Global list of timers
static TIMERS: SpinMutex<VecDeque<Timer>> = SpinMutex::new(VecDeque::new());

#[allow(clippy::assertions_on_constants)]
pub(crate) fn init() {
    assert!(
        1000 % TICK_HZ == 0,
        "TICK_HZ must be a divisor of 1000 so we can evenly divide milliseconds into ticks"
    );

    let tick_millis = Milliseconds::new(1000 / TICK_HZ);
    hpet::enable_periodic_timer_handler(
        0,
        tick_handler,
        ioapic::IOAPICIRQNumber::Tick,
        hpet::HPETTimerNumber::Tick,
        tick_millis,
    );
}

fn tick_handler(_vector: u8, _handler_id: interrupts::InterruptHandlerID) {
    // Iterate through all timers and fire off + remove ones that expired.
    TIMERS.lock().retain_mut(|timer| {
        if timer.expiration <= hpet::elapsed_milliseconds() {
            (timer.callback)();
            false
        } else {
            true
        }
    });
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
