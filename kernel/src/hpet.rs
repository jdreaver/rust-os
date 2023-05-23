use bitfield_struct::bitfield;
use spin::RwLock;

use crate::interrupts::{InterruptHandler, InterruptHandlerID};
use crate::registers::{RegisterRO, RegisterRW};
use crate::{interrupts, ioapic, register_struct, serial_println};

static HPET: RwLock<Option<HPET>> = RwLock::new(None);

pub(crate) unsafe fn init(hpet_apic_base_address: usize) {
    let hpet = unsafe { HPET::from_base_address(hpet_apic_base_address) };
    serial_println!("HPET: {:#x?}", hpet);
    HPET.write().replace(hpet);
}

pub(crate) fn enable_periodic_timer_handler(
    handler_id: InterruptHandlerID,
    handler: InterruptHandler,
    ioapic_irq_number: u8,
    timer_number: u8,
    interval: &Milliseconds,
) {
    let interrupt_vector = interrupts::install_interrupt(handler_id, handler);
    ioapic::install_irq(interrupt_vector, ioapic_irq_number);

    let lock = HPET.read();
    let hpet = lock.as_ref().expect("HPET not initialized");

    let interval_femtoseconds = interval.femtoseconds();
    hpet.enable_periodic_timer(timer_number, ioapic_irq_number, interval_femtoseconds);

    let timer = hpet.timer_registers(timer_number);
    serial_println!("intalled HPET timer {timer_number}: {timer:#x?}");
}

pub(crate) struct Milliseconds(u64);

impl Milliseconds {
    pub(crate) fn new(milliseconds: u64) -> Self {
        Self(milliseconds)
    }

    fn femtoseconds(&self) -> u64 {
        self.0 * 1_000_000_000_000
    }
}

/// High Precision Event Timer. See <https://wiki.osdev.org/HPET>
#[derive(Debug)]
struct HPET {
    registers: HpetRegisters,
}

register_struct!(
    HpetRegisters {
        0x00 => general_capabilities_and_id: RegisterRO<GeneralCapabilitiesAndID>,
        0x10 => general_configuration: RegisterRW<GeneralConfiguration>,
        0x20 => general_interrupt_status: RegisterRW<u64>,
        0xF0 => main_counter_value: RegisterRW<u64>,
    }
);

impl HPET {
    /// Constructs an `HPET` from the given base address, which can be found in
    /// the HPET ACPI table.
    unsafe fn from_base_address(address: usize) -> Self {
        Self {
            registers: HpetRegisters::from_address(address),
        }
    }

    fn timer_registers(&self, timer_number: u8) -> TimerRegisters {
        let offset: usize = 0x100 + timer_number as usize * 0x20;
        unsafe { TimerRegisters::from_address(self.registers.address + offset) }
    }

    /// Enables the given timer to fire interrupts periodically to the given
    /// IO/APIC interrupt number with the given interval.
    fn enable_periodic_timer(
        &self,
        timer_number: u8,
        ioapic_interrupt_number: u8,
        interval_femtoseconds: u64,
    ) {
        let hpet_caps = self.registers.general_capabilities_and_id().read();

        let num_timers = hpet_caps.number_of_timers();
        assert!(
            timer_number < hpet_caps.number_of_timers(),
            "HPET only has {num_timers} timers but got timer number {timer_number}"
        );

        // Ensure HPET is enabled (TODO: Should we do this on `init()`?)
        self.registers.general_configuration().modify_mut(|conf| {
            conf.set_enabled(true);
        });

        // Configure timer

        let timer = self.timer_registers(timer_number);
        timer.config_and_cap().modify_mut(|conf| {
            assert!(
                conf.periodic_interrupt_capable(),
                "tried to enable_periodic_timer on timer {timer_number} but it does not support periodic mode"
            );

            conf.set_is_periodic(true);
            conf.set_interrupt_enabled(true);
            conf.set_interrupt_route(ioapic_interrupt_number);
        });

        let hpet_period = hpet_caps.counter_clock_period();
        let comparator_value = interval_femtoseconds / u64::from(hpet_period);
        timer.comparator_value().write(comparator_value);
    }
}

#[bitfield(u64)]
struct GeneralCapabilitiesAndID {
    /// Indicates which revision of the function is implemented; must not be 0.
    revision_id: u8,

    /// The number of comparators (i.e. timers) that the HPET supports, minus 1.
    #[bits(5)]
    number_of_timers: u8,

    /// If this bit is 1, HPET main counter is capable of operating in 64 bit mode.
    counter_width: bool,

    __reserved: bool,

    /// If this bit is 1, HPET is capable of using "legacy replacement" mapping.
    legacy_replacement_route: bool,

    /// This field should be interpreted similarly to PCI's vendor ID.
    vendor_id: u16,

    /// Main counter tick period in femtoseconds (10^-15 seconds). Must not be
    /// zero, must be less or equal to 0x05F5E100, or 100 nanoseconds.
    counter_clock_period: u32,
}

#[bitfield(u64)]
struct GeneralConfiguration {
    /// Overall enable.
    /// - 0: main counter is halted, timer interrupts are disabled
    /// - 1: main counter is running, timer interrupts are allowed if enabled
    enabled: bool,
    legacy_replacement_enabled: bool,

    #[bits(62)]
    __reserved: u64,
}

register_struct!(
    /// Configuration for a specific timer.
    TimerRegisters {
        0x00 => config_and_cap: RegisterRW<TimerConfigAndCapabilities>,
        0x08 => comparator_value: RegisterRW<u64>,
        0x10 => fsb_interrupt_route: RegisterRW<u64>,
    }
);

#[bitfield(u64)]
/// See "2.3.8 Timer N Configuration and Capabilities Register" in the HPET spec.
struct TimerConfigAndCapabilities {
    __reserved: bool,

    /// If `false`, then edge_triggered.
    is_level_triggered: bool,
    interrupt_enabled: bool,
    is_periodic: bool, // TODO: Rename to is_periodic?
    periodic_interrupt_capable: bool,

    /// `false` means 32 bits
    is_64_bits: bool,

    /// Software uses this read/write bit only for timers that have been set to
    /// periodic mode. By writing this bit to a 1, the software is then allowed
    /// to directly set a periodic timer’s accumulator. Software does NOT have
    /// to write this bit back to 0 (it automatically clears).
    timer_value_set: bool,
    __reserved: bool,
    set_32_bit_mode: bool,

    /// This 5-bit read/write field indicates the routing for the interrupt to
    /// the I/O APIC. A maximum value of 32 interrupts are supported. Default is
    /// 00h Software writes to this field to select which interrupt in the I/O
    /// (x) will be used for this timer’s interrupt. If the value is not
    /// supported by this prarticular timer, then the value read back will not
    /// match what is written. The software must only write valid values.
    #[bits(5)]
    interrupt_route: u8,

    fsb_interrupt_enable: bool,
    fsb_interrupt_delivery: bool,

    __reserved: u16,

    interrupt_routing_capability: u32,
}
