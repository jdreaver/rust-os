//! Global tick system that runs every `TICK_HZ` times per second.

use crate::{hpet, interrupts, ioapic};

/// Frequency of the global tick system.
const TICK_HZ: u64 = 10;

#[allow(clippy::assertions_on_constants)]
pub(crate) fn init() {
    assert!(
        1000 % TICK_HZ == 0,
        "TICK_HZ must be a divisor of 1000 so we can evenly divide milliseconds into ticks"
    );

    let tick_millis = hpet::Milliseconds::new(1000 / TICK_HZ);
    hpet::enable_periodic_timer_handler(
        0,
        tick_handler,
        ioapic::IOAPICIRQNumber::Tick,
        hpet::HPETTimerNumber::Tick,
        &tick_millis,
    );
}

fn tick_handler(_vector: u8, _handler_id: interrupts::InterruptHandlerID) {}
