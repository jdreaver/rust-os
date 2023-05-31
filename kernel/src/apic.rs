use bitfield_struct::bitfield;

use crate::acpi::ACPIInfo;
use crate::interrupts::SPURIOUS_INTERRUPT_VECTOR_INDEX;
use crate::registers::{RegisterRO, RegisterRW, RegisterWO};
use crate::sync::InitCell;
use crate::{register_struct, serial_println};

/// Global static for the local APIC. Particularly useful for interrupt
/// handlers so they know where to send an End Of Interrupt (EOI).
///
/// It might seem weird to have a single global static because there is a
/// local APIC per CPU. However, since we never remap the local APIC
/// address, the address is the same for all CPUs.
static LOCAL_APIC: InitCell<LocalAPIC> = InitCell::new();

pub(crate) fn init_local_apic(acpi_info: &ACPIInfo) {
    let mut local_apic = LocalAPIC::from_acpi_info(acpi_info);
    local_apic.enable();
    serial_println!("DEBUG: Local APIC: {:#x?}", local_apic);
    LOCAL_APIC.init(local_apic);
}

/// See "11.8.5 Signaling Interrupt Servicing Completion" in the Intel 64 Manual
/// Volume 3.
pub(crate) fn end_of_interrupt() {
    LOCAL_APIC
        .get()
        .expect("Local APIC not initialized")
        .end_of_interrupt();
}

/// Get the local APIC ID for the current processor.
pub(crate) fn lapic_id() -> u8 {
    LOCAL_APIC
        .get()
        .expect("Local APIC not initialized")
        .registers
        .local_apic_id()
        .read()
        .id()
}

#[derive(Debug, Clone)]
struct LocalAPIC {
    registers: LocalAPICRegisters,
}

impl LocalAPIC {
    pub(crate) fn from_acpi_info(acpi_info: &ACPIInfo) -> Self {
        let apic_info = acpi_info.apic_info();
        let registers =
            unsafe { LocalAPICRegisters::from_address(apic_info.local_apic_address as usize) };
        Self { registers }
    }

    pub fn enable(&mut self) {
        self.registers.spurious_interrupt_vector().modify_mut(
            |vec: &mut SpuriousInterruptVector| {
                vec.set_vector(SPURIOUS_INTERRUPT_VECTOR_INDEX);
                vec.set_apic_enabled(true);
            },
        );
    }

    /// See "11.8.5 Signaling Interrupt Servicing Completion" in the Intel 64
    /// Manual Volume 3.
    pub(crate) fn end_of_interrupt(&self) {
        self.registers.end_of_interrupt().write(0);
    }
}

register_struct!(
    /// See "11.4.1 The Local APIC Block Diagram", specifically "Table 11-1. Local
    /// APIC Register Address Map" in the Intel 64 Manual Volume 3. Also see
    /// <https://wiki.osdev.org/APIC>.
    pub(crate) LocalAPICRegisters {
        0x20 => local_apic_id: RegisterRW<APICIdRegister>,
        0x30 => local_apic_version: RegisterRO<APICVersion>,
        0x80 => task_priority: RegisterRW<u32>,
        0x90 => arbitration_priority: RegisterRO<u32>,
        0xa0 => processor_priority: RegisterRO<u32>,
        0xb0 => end_of_interrupt: RegisterWO<u32>,
        0xc0 => remote_read: RegisterRO<u32>,
        0xd0 => logical_destination: RegisterRW<u32>,
        0xe0 => destination_format: RegisterRW<u32>,
        0xf0 => spurious_interrupt_vector: RegisterRW<SpuriousInterruptVector>,

        0x100 => in_service_0: RegisterRO<u32>,
        0x110 => in_service_1: RegisterRO<u32>,
        0x120 => in_service_2: RegisterRO<u32>,
        0x130 => in_service_3: RegisterRO<u32>,
        0x140 => in_service_4: RegisterRO<u32>,
        0x150 => in_service_5: RegisterRO<u32>,
        0x160 => in_service_6: RegisterRO<u32>,
        0x170 => in_service_7: RegisterRO<u32>,

        0x180 => trigger_mode_0: RegisterRO<u32>,
        0x190 => trigger_mode_1: RegisterRO<u32>,
        0x1a0 => trigger_mode_2: RegisterRO<u32>,
        0x1b0 => trigger_mode_3: RegisterRO<u32>,
        0x1c0 => trigger_mode_4: RegisterRO<u32>,
        0x1d0 => trigger_mode_5: RegisterRO<u32>,
        0x1e0 => trigger_mode_6: RegisterRO<u32>,
        0x1f0 => trigger_mode_7: RegisterRO<u32>,

        0x200 => interrupt_request_0: RegisterRO<u32>,
        0x210 => interrupt_request_1: RegisterRO<u32>,
        0x220 => interrupt_request_2: RegisterRO<u32>,
        0x230 => interrupt_request_3: RegisterRO<u32>,
        0x240 => interrupt_request_4: RegisterRO<u32>,
        0x250 => interrupt_request_5: RegisterRO<u32>,
        0x260 => interrupt_request_6: RegisterRO<u32>,
        0x270 => interrupt_request_7: RegisterRO<u32>,

        0x280 => error_status: RegisterRO<u32>,
        0x2f0 => lvt_corrected_machine_check_interrupt: RegisterRW<u32>,
        0x300 => interrupt_command_low_bits: RegisterRW<u32>,
        0x310 => interrupt_command_high_bits: RegisterRW<u32>,
        0x320 => lvt_timer: RegisterRW<u32>,
        0x330 => lvt_thermal_sensor: RegisterRW<u32>,
        0x340 => lvt_performance_monitoring_counters: RegisterRW<u32>,
        0x350 => lvt_lint0: RegisterRW<u32>,
        0x360 => lvt_lint1: RegisterRW<u32>,
        0x370 => lvt_error: RegisterRW<u32>,
        0x380 => initial_count: RegisterRW<u32>,
        0x398 => current_count: RegisterRO<u32>,
        0x3e0 => divide_configuration: RegisterRW<u32>,
    }
);

#[bitfield(u32)]
/// See "11.4.6 Local APIC ID" in the Intel 64 Manual Volume 3.
pub(crate) struct APICIdRegister {
    #[bits(24)]
    __reserved: u32,
    id: u8,
}

#[bitfield(u32)]
/// See "11.4.8 Local APIC Version Register" in the Intel 64 Manual Volume 3.
pub(crate) struct APICVersion {
    version: u8,
    __reserved: u8,
    max_lvt_entry: u8,
    support_suppress_eoi_broadcast: bool,
    #[bits(7)]
    __reserved2: u8,
}

#[bitfield(u32)]
/// See "11.9 SPURIOUS INTERRUPT" in the Intel 64 Manual Volume 3.
pub(crate) struct SpuriousInterruptVector {
    vector: u8,
    apic_enabled: bool,
    focus_processor_checking: bool,

    #[bits(2)]
    __reserved: u8,

    eoi_broadcast_suppression: bool,

    #[bits(19)]
    __reserved: u32,
}
