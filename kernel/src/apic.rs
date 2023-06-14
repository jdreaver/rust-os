use bitfield_struct::bitfield;

use crate::acpi::ACPIInfo;
use crate::interrupts::SPURIOUS_INTERRUPT_VECTOR_INDEX;
use crate::register_struct;
use crate::registers::{RegisterRO, RegisterRW, RegisterWO};
use crate::sync::InitCell;

/// Global static for the local APIC. Particularly useful for interrupt
/// handlers so they know where to send an End Of Interrupt (EOI).
///
/// It might seem weird to have a single global static because there is a
/// local APIC per CPU. However, since we never remap the local APIC
/// address, the address is the same for all CPUs.
static LOCAL_APIC: InitCell<LocalAPIC> = InitCell::new();

pub(crate) fn global_init(acpi_info: &ACPIInfo) {
    let local_apic = LocalAPIC::from_acpi_info(acpi_info);
    LOCAL_APIC.init(local_apic);
}

pub(crate) fn per_cpu_init() {
    LOCAL_APIC
        .get()
        .expect("Local APIC not initialized")
        .enable();
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
#[allow(dead_code)]
pub(crate) fn lapic_id() -> ProcessorID {
    let id = LOCAL_APIC
        .get()
        .expect("Local APIC not initialized")
        .registers
        .local_apic_id()
        .read()
        .id();
    ProcessorID(id)
}

/// Broadcast an interprocessor interrupt (IPI) to all processors.
///
/// See "11.6 ISSUING INTERPROCESSOR INTERRUPTS"
pub(crate) fn send_ipi_all_cpus(vector: u8) {
    LOCAL_APIC
        .get()
        .expect("Local APIC not initialized")
        .registers
        .interrupt_command_low_bits()
        .write(
            InterruptCommandLowBits::new()
                .with_delivery_mode(InterruptCommandDeliveryMode::Fixed)
                .with_destination_mode(InterruptCommandDestinationMode::Logical)
                .with_destination_shorthand(InterruptCommandDestinationShorthand::AllIncludingSelf)
                .with_level(InterruptCommandLevel::Assert)
                .with_vector(vector),
        );
}

/// Both a LAPIC ID and a processor ID. See the Intel manual:
///
/// 11.4.6 Local APIC ID
///
/// At power up, system hardware assigns a unique APIC ID to each local APIC on
/// the system bus. ... In MP systems, the local APIC ID is also used as a
/// processor ID by the BIOS and the operating system.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub(crate) struct ProcessorID(pub(crate) u8);

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

    pub fn enable(&self) {
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
    LocalAPICRegisters {
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
        0x300 => interrupt_command_low_bits: RegisterRW<InterruptCommandLowBits>,
        0x310 => interrupt_command_high_bits: RegisterRW<InterruptCommandHighBits>,
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
struct APICIdRegister {
    #[bits(24)]
    __reserved: u32,
    id: u8, // ProcessorID, but bitfield_struct doesn't support newtypes
}

#[bitfield(u32)]
/// See "11.4.8 Local APIC Version Register" in the Intel 64 Manual Volume 3.
struct APICVersion {
    version: u8,
    __reserved: u8,
    max_lvt_entry: u8,
    support_suppress_eoi_broadcast: bool,
    #[bits(7)]
    __reserved2: u8,
}

#[bitfield(u32)]
/// See "11.9 SPURIOUS INTERRUPT" in the Intel 64 Manual Volume 3.
struct SpuriousInterruptVector {
    vector: u8,
    apic_enabled: bool,
    focus_processor_checking: bool,

    #[bits(2)]
    __reserved: u8,

    eoi_broadcast_suppression: bool,

    #[bits(19)]
    __reserved: u32,
}

#[bitfield(u32)]
/// See "11.6.1 Interrupt Command Register (ICR)" in the Intel 64 Manual Volume 3.
struct InterruptCommandLowBits {
    vector: u8,

    #[bits(3)]
    delivery_mode: InterruptCommandDeliveryMode,

    #[bits(1)]
    destination_mode: InterruptCommandDestinationMode,

    #[bits(1)]
    delivery_status: InterruptCommandDeliveryStatus,

    #[bits(1)]
    __reserved: u8,

    #[bits(1)]
    level: InterruptCommandLevel,

    #[bits(1)]
    trigger_mode: InterruptCommandTriggerMode,

    #[bits(2)]
    __reserved: u8,

    #[bits(2)]
    destination_shorthand: InterruptCommandDestinationShorthand,

    #[bits(12)]
    __reserved: u16,
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum InterruptCommandDeliveryMode {
    Fixed = 0b000,
    LowestPriority1 = 0b001,
    SMI = 0b010,
    Reserved1 = 0b011,
    NMI = 0b100,
    INIT = 0b101,
    StartUp = 0b110,
    Reserved2 = 0b111,
}

#[allow(clippy::fallible_impl_from)]
impl From<u32> for InterruptCommandDeliveryMode {
    fn from(value: u32) -> Self {
        match value {
            value if value == Self::Fixed as u32 => Self::Fixed,
            value if value == Self::LowestPriority1 as u32 => Self::LowestPriority1,
            value if value == Self::SMI as u32 => Self::SMI,
            value if value == Self::Reserved1 as u32 => Self::Reserved1,
            value if value == Self::NMI as u32 => Self::NMI,
            value if value == Self::INIT as u32 => Self::INIT,
            value if value == Self::StartUp as u32 => Self::StartUp,
            value if value == Self::Reserved2 as u32 => Self::Reserved2,
            _ => panic!("Invalid InterruptCommandDeliveryMode: {}", value),
        }
    }
}

impl From<InterruptCommandDeliveryMode> for u32 {
    fn from(value: InterruptCommandDeliveryMode) -> Self {
        value as Self
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum InterruptCommandDestinationMode {
    Physical = 0,
    Logical = 1,
}

#[allow(clippy::fallible_impl_from)]
impl From<u32> for InterruptCommandDestinationMode {
    fn from(value: u32) -> Self {
        match value {
            value if value == Self::Physical as u32 => Self::Physical,
            value if value == Self::Logical as u32 => Self::Logical,
            _ => panic!("Invalid InterruptCommandDestinationMode: {}", value),
        }
    }
}

impl From<InterruptCommandDestinationMode> for u32 {
    fn from(value: InterruptCommandDestinationMode) -> Self {
        value as Self
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum InterruptCommandDeliveryStatus {
    Idle = 0,
    Pending = 1,
}

#[allow(clippy::fallible_impl_from)]
impl From<u32> for InterruptCommandDeliveryStatus {
    fn from(value: u32) -> Self {
        match value {
            value if value == Self::Idle as u32 => Self::Idle,
            value if value == Self::Pending as u32 => Self::Pending,
            _ => panic!("Invalid InterruptCommandDeliveryStatus: {}", value),
        }
    }
}

impl From<InterruptCommandDeliveryStatus> for u32 {
    fn from(value: InterruptCommandDeliveryStatus) -> Self {
        value as Self
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum InterruptCommandLevel {
    DeAssert = 0,
    Assert = 1,
}

#[allow(clippy::fallible_impl_from)]
impl From<u32> for InterruptCommandLevel {
    fn from(value: u32) -> Self {
        match value {
            value if value == Self::DeAssert as u32 => Self::DeAssert,
            value if value == Self::Assert as u32 => Self::Assert,
            _ => panic!("Invalid InterruptCommandLevel: {}", value),
        }
    }
}

impl From<InterruptCommandLevel> for u32 {
    fn from(value: InterruptCommandLevel) -> Self {
        value as Self
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum InterruptCommandTriggerMode {
    Edge = 0,
    Level = 1,
}

#[allow(clippy::fallible_impl_from)]
impl From<u32> for InterruptCommandTriggerMode {
    fn from(value: u32) -> Self {
        match value {
            value if value == Self::Edge as u32 => Self::Edge,
            value if value == Self::Level as u32 => Self::Level,
            _ => panic!("Invalid InterruptCommandTriggerMode: {}", value),
        }
    }
}

impl From<InterruptCommandTriggerMode> for u32 {
    fn from(value: InterruptCommandTriggerMode) -> Self {
        value as Self
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum InterruptCommandDestinationShorthand {
    NoShorthand = 0b00,
    DestSelf = 0b01,
    AllIncludingSelf = 0b10,
    AllExcludingSelf = 0b11,
}

#[allow(clippy::fallible_impl_from)]
impl From<u32> for InterruptCommandDestinationShorthand {
    fn from(value: u32) -> Self {
        match value {
            value if value == Self::NoShorthand as u32 => Self::NoShorthand,
            value if value == Self::DestSelf as u32 => Self::DestSelf,
            value if value == Self::AllIncludingSelf as u32 => Self::AllIncludingSelf,
            value if value == Self::AllExcludingSelf as u32 => Self::AllExcludingSelf,
            _ => panic!("Invalid InterruptCommandDestinationShorthand: {}", value),
        }
    }
}

impl From<InterruptCommandDestinationShorthand> for u32 {
    fn from(value: InterruptCommandDestinationShorthand) -> Self {
        value as Self
    }
}

#[bitfield(u32)]
/// See "11.6.1 Interrupt Command Register (ICR)" in the Intel 64 Manual Volume 3.
struct InterruptCommandHighBits {
    #[bits(24)]
    __reserved: u32,
    destination: u8,
}
