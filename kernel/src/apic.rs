use bitfield_struct::bitfield;
use core::fmt;

use crate::acpi::ACPIInfo;
use crate::interrupts::SPURIOUS_INTERRUPT_VECTOR_INDEX;
use crate::register_struct;
use crate::registers::{RegisterRO, RegisterRW, RegisterWO};

#[derive(Debug, Clone)]
pub(crate) struct LocalAPIC {
    registers: LocalAPICRegisters,
}

impl LocalAPIC {
    pub(crate) fn from_acpi_info(acpi_info: &ACPIInfo) -> Self {
        let apic_info = acpi_info.apic_info();
        let registers =
            unsafe { LocalAPICRegisters::from_address(apic_info.local_apic_address as usize) };
        Self { registers }
    }

    pub(crate) fn enable(&mut self) {
        self.registers.spurious_interrupt_vector().modify_mut(
            |vec: &mut SpuriousInterruptVector| {
                vec.set_vector(SPURIOUS_INTERRUPT_VECTOR_INDEX);
                vec.set_apic_enabled(true);
            },
        );
    }
}

register_struct!(
    /// See "11.4.1 The Local APIC Block Diagram", specifically "Table 11-1. Local
    /// APIC Register Address Map" in the Intel 64 Manual Volume 3. Also see
    /// <https://wiki.osdev.org/APIC>.
    pub(crate) LocalAPICRegisters {
        0x20 => local_apic_id: RegisterRW<APICId>,
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
pub(crate) struct APICId {
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

/// See <https://wiki.osdev.org/IOAPIC>
#[derive(Clone)]
pub(crate) struct IOAPIC {
    id: u8,
    global_system_interrupt_base: u32,
    registers: IOAPICRegisters,
}

impl IOAPIC {
    pub(crate) fn from_acpi_info(acpi_info: &ACPIInfo) -> Self {
        let apic_info = acpi_info.apic_info();
        let io_apic = apic_info
            .io_apics
            .get(0)
            .expect("no IOAPICS found from ACPI");
        let registers = unsafe { IOAPICRegisters::from_address(io_apic.address as usize) };
        Self {
            id: io_apic.id,
            global_system_interrupt_base: io_apic.global_system_interrupt_base,
            registers,
        }
    }

    /// Reads an IOAPIC register by selecting it and then reading the IO window.
    fn read_32_bit_register(&self, register: u8) -> u32 {
        self.registers.io_register_select().write(register);
        self.registers.io_window().read()
    }

    /// Reads a 64 IOAPIC register by reading two 32 bit registers.
    fn read_64_bit_register(&self, register: u8) -> u64 {
        let low = self.read_32_bit_register(register);
        let high = self.read_32_bit_register(register + 1);
        (u64::from(high) << 32) | u64::from(low)
    }

    /// Writes an IOAPIC register by selecting it and then writing the IO window.
    fn write_32_bit_register(&self, register: u8, value: u32) {
        self.registers.io_register_select().write(register);
        self.registers.io_window().write(value);
    }

    /// Writes a 64 IOAPIC register by writing two 32 bit registers.
    fn write_64_bit_register(&self, register: u8, value: u64) {
        let low = value as u32;
        let high = (value >> 32) as u32;
        self.write_32_bit_register(register, low);
        self.write_32_bit_register(register + 1, high);
    }

    /// See "3.2.1.IOAPICID—IOAPIC IDENTIFICATION REGISTER". This register
    /// contains the 4-bit APIC ID. The ID serves as a physical name of the
    /// IOAPIC. All APIC devices using the APIC bus should have a unique APIC
    /// ID. The APIC bus arbitration ID for the I/O unit is also writtten during
    /// a write to the APICID Register (same data is loaded into both). This
    /// register must be programmed with the correct ID value before using the
    /// IOAPIC for message transmission.
    pub(crate) fn ioapic_id(&self) -> IOAPICID {
        let raw = self.read_32_bit_register(IOAPIC_ID_REGISTER_OFFSET);
        IOAPICID::from(raw)
    }

    /// See "3.2.2. IOAPICVER—IOAPIC VERSION REGISTER"
    pub(crate) fn ioapic_version(&self) -> IOAPICVersion {
        let raw = self.read_32_bit_register(IOAPIC_VERSION_REGISTER_OFFSET);
        IOAPICVersion::from(raw)
    }

    /// See "3.2.4. 82093AA (IOAPIC) IOREDTBL[23:0]—I/O REDIRECTION TABLE REGISTERS"
    pub(crate) fn read_ioredtbl(&self, entry: u8) -> IOAPICRedirectionTableRegister {
        // Intel IOAPIC only has 24 entries
        assert!(entry < 24, "Intel IOAPIC only has 24 entries!");
        let offset = IOAPIC_REDIRECTION_TABLE_REGISTER_OFFSET + (entry * 2);
        let raw = self.read_64_bit_register(offset);
        IOAPICRedirectionTableRegister::from(raw)
    }

    /// See "3.2.4. 82093AA (IOAPIC) IOREDTBL[23:0]—I/O REDIRECTION TABLE REGISTERS"
    pub(crate) fn write_ioredtbl(&self, entry: u8, value: IOAPICRedirectionTableRegister) {
        // Intel IOAPIC only has 24 entries
        assert!(entry < 24, "Intel IOAPIC only has 24 entries!");
        let offset = IOAPIC_REDIRECTION_TABLE_REGISTER_OFFSET + (entry * 2);
        self.write_64_bit_register(offset, value.into());
    }
}

impl fmt::Debug for IOAPIC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IOAPIC")
            .field("id", &self.id)
            .field(
                "global_system_interrupt_base",
                &self.global_system_interrupt_base,
            )
            .field("ioapic_id", &self.ioapic_id())
            .field("ioapic_version", &self.ioapic_version())
            .finish()
    }
}

register_struct!(
    /// See <https://wiki.osdev.org/IOAPIC> and "82093AA I/O ADVANCED
    /// PROGRAMMABLE INTERRUPT CONTROLLER (IOAPIC) (1996)"
    pub(crate) IOAPICRegisters {
        0x00 => io_register_select: RegisterRW<u8>,
        0x10 => io_window: RegisterRW<u32>,
    }
);

const IOAPIC_ID_REGISTER_OFFSET: u8 = 0x00;

#[bitfield(u32)]
/// See "3.2.1.IOAPICID—IOAPIC IDENTIFICATION REGISTER"
pub(crate) struct IOAPICID {
    #[bits(24)]
    __reserved: u32,
    #[bits(4)]
    id: u8,
    #[bits(4)]
    __reserved: u8,
}

const IOAPIC_VERSION_REGISTER_OFFSET: u8 = 0x01;

#[bitfield(u32)]
/// See "3.2.2. IOAPICVER—IOAPIC VERSION REGISTER"
pub(crate) struct IOAPICVersion {
    version: u8,
    __reserved: u8,
    max_redirection_entry: u8,
    __reserved: u8,
}

const IOAPIC_REDIRECTION_TABLE_REGISTER_OFFSET: u8 = 0x10;

#[bitfield(u64)]
/// See "3.2.4. 82093AA (IOAPIC) IOREDTBL[23:0]—I/O REDIRECTION TABLE REGISTERS"
pub(crate) struct IOAPICRedirectionTableRegister {
    pub(crate) interrupt_vector: u8,
    #[bits(3)]
    pub(crate) delivery_mode: u8,
    pub(crate) destination_mode: bool,
    pub(crate) delivery_status: bool,
    pub(crate) interrupt_input_pin_polarity: bool,
    pub(crate) remote_irr: bool,
    pub(crate) trigger_mode: bool,
    pub(crate) interrupt_mask: bool,
    #[bits(39)]
    __reserved: u64,
    pub(crate) destination_field: u8,
}
