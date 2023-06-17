use x86_64::instructions::port::Port;

/// Custom exit codes because QEMU does a binary OR with 0x1
#[derive(Debug, Clone, Copy)]
#[repr(u32)] // Must match `iosize` for the `isa-debug-exit` device
pub(crate) enum QEMUExitCode {
    Success = 0x10,
    // Failed = 0x11,
}

/// Exit QEMU with the given exit code
///
/// Device must be created with `isa-debug-exit,iobase=0xf4,iosize=0x04`
const QEMU_EXIT_PORT: u16 = 0xf4;

pub(crate) fn exit_qemu(exit_code: QEMUExitCode) {
    unsafe {
        let mut port = Port::new(QEMU_EXIT_PORT);
        port.write(exit_code as u32);
    }
    log::error!("Exiting QEMU failed! Is the device `isa-debug-exit` missing?");
}
