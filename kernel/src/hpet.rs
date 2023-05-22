use bitfield_struct::bitfield;

use crate::registers::{RegisterRO, RegisterRW};
use crate::{register_struct, serial_println};

pub(crate) unsafe fn init(hpet_apic_base_address: usize) {
    let hpet = unsafe { HPET::from_base_address(hpet_apic_base_address) };
    serial_println!("HPET: {:#x?}", hpet);

    let first_timer = hpet.timer_registers(0);
    serial_println!("Timer 0: {:#x?}", first_timer);
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
