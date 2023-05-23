use core::fmt;

use bitfield_struct::bitfield;

use crate::acpi::ACPIInfo;
use crate::register_struct;
use crate::registers::RegisterRW;

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
    pub(crate) id: u8,
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
