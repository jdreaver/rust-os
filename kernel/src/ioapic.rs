use core::fmt;

use bitfield_struct::bitfield;

use crate::acpi::ACPIInfo;
use crate::interrupts::InterruptVector;
use crate::register_struct;
use crate::registers::RegisterRW;
use crate::sync::InitCell;

static IOAPIC: InitCell<IOAPIC> = InitCell::new();

pub(crate) fn init(acpi_info: &ACPIInfo) {
    let ioapic = IOAPIC::from_acpi_info(acpi_info);
    IOAPIC.init(ioapic);
}

pub(crate) fn install_irq(interrupt_vector: InterruptVector, irq_entry: IOAPICIRQNumber) {
    let ioapic = IOAPIC.get().expect("IOAPIC not initialized!");

    ioapic.write_ioredtbl(
        irq_entry as u8,
        IOAPICRedirectionTableRegister::new()
            .with_interrupt_vector(interrupt_vector.0)
            .with_interrupt_mask(false)
            .with_delivery_mode(0) // Fixed
            .with_destination_mode(false) // Physical
            .with_delivery_status(false)
            .with_destination_field(ioapic.ioapic_id().id()),
    );
}

/// Global list of registered IOAPIC IRQs to ensure we don't have collisions.
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub(crate) enum IOAPICIRQNumber {
    _Reserved = 0,

    /// Assumes that the keyboard IRQ for the IOAPIC is 1, which is the same as
    /// if we were using the 8259 PIC. If we wanted to determine this
    /// dynamically, we could read the IOAPIC redirection table entry for IRQ 1,
    /// or if that doesn't exist I think we need to parse some ACPI AML.
    Keyboard = 1,

    // Some reserved numbers in the middle. I don't trust that these aren't
    // already taken.
    Tick = 9,
    TestHPET = 10,
}

/// See <https://wiki.osdev.org/IOAPIC>
#[derive(Clone)]
struct IOAPIC {
    id: u8,
    global_system_interrupt_base: u32,
    registers: IOAPICRegisters,
}

impl IOAPIC {
    fn from_acpi_info(acpi_info: &ACPIInfo) -> Self {
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
    #[allow(dead_code)] // TODO: Remove once used
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
    fn ioapic_id(&self) -> IOAPICID {
        let raw = self.read_32_bit_register(IOAPIC_ID_REGISTER_OFFSET);
        IOAPICID::from(raw)
    }

    /// See "3.2.2. IOAPICVER—IOAPIC VERSION REGISTER"
    fn ioapic_version(&self) -> IOAPICVersion {
        let raw = self.read_32_bit_register(IOAPIC_VERSION_REGISTER_OFFSET);
        IOAPICVersion::from(raw)
    }

    /// See "3.2.4. 82093AA (IOAPIC) IOREDTBL\[23:0\]—I/O REDIRECTION TABLE REGISTERS"
    #[allow(dead_code)] // TODO: Remove once used
    fn read_ioredtbl(&self, entry: u8) -> IOAPICRedirectionTableRegister {
        // Intel IOAPIC only has 24 entries
        assert!(entry < 24, "Intel IOAPIC only has 24 entries!");
        let offset = IOAPIC_REDIRECTION_TABLE_REGISTER_OFFSET + (entry * 2);
        let raw = self.read_64_bit_register(offset);
        IOAPICRedirectionTableRegister::from(raw)
    }

    /// See "3.2.4. 82093AA (IOAPIC) IOREDTBL\[23:0\]—I/O REDIRECTION TABLE REGISTERS"
    fn write_ioredtbl(&self, entry: u8, value: IOAPICRedirectionTableRegister) {
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
    IOAPICRegisters {
        0x00 => io_register_select: RegisterRW<u8>,
        0x10 => io_window: RegisterRW<u32>,
    }
);

const IOAPIC_ID_REGISTER_OFFSET: u8 = 0x00;

#[bitfield(u32)]
/// See "3.2.1.IOAPICID—IOAPIC IDENTIFICATION REGISTER"
struct IOAPICID {
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
struct IOAPICVersion {
    version: u8,
    __reserved: u8,
    max_redirection_entry: u8,
    __reserved: u8,
}

const IOAPIC_REDIRECTION_TABLE_REGISTER_OFFSET: u8 = 0x10;

#[bitfield(u64)]
/// See "3.2.4. 82093AA (IOAPIC) IOREDTBL\[23:0\]—I/O REDIRECTION TABLE REGISTERS"
struct IOAPICRedirectionTableRegister {
    interrupt_vector: u8,
    #[bits(3)]
    delivery_mode: u8,
    destination_mode: bool,
    delivery_status: bool,
    interrupt_input_pin_polarity: bool,
    remote_irr: bool,
    trigger_mode: bool,
    interrupt_mask: bool,
    #[bits(39)]
    __reserved: u64,
    destination_field: u8,
}
