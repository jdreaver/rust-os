use crate::register_struct;
use crate::registers::{RegisterRO, RegisterRW, RegisterWO};

register_struct!(
    /// See "11.4.1 The Local APIC Block Diagram", specifically "Table 11-1. Local
    /// APIC Register Address Map" in the Intel 64 Manual Volume 3. Also see
    /// <https://wiki.osdev.org/APIC>.
    pub(crate) LocalAPICRegisters {
        0x20 => local_apic_id: RegisterRW<u32>,
        0x30 => local_apic_version: RegisterRO<u32>,
        0x80 => task_priority: RegisterRW<u32>,
        0x90 => arbitration_priority: RegisterRO<u32>,
        0xa0 => processor_priority: RegisterRO<u32>,
        0xb0 => end_of_interrupt: RegisterWO<u32>,
        0xc0 => remote_read: RegisterRO<u32>,
        0xd0 => logical_destination: RegisterRW<u32>,
        0xe0 => destination_format: RegisterRW<u32>,
        0xf0 => spurious_interrupt_vector: RegisterRW<u32>,

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
