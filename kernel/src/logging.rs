use crate::sync::SpinLock;
use crate::{ansiterm, serial_println};

/// Dummy type to help us implement a logger using the `log` crate.
struct Logger {
    writer: SpinLock<LogSerialWriter>,
}

static LOGGER: Logger = Logger {
    writer: SpinLock::new(LogSerialWriter),
};

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.writer.lock_disable_interrupts().print_record(record);
        }
    }

    fn flush(&self) {}
}

/// Dummy type just so we can put something in a `SpinLock` and ensure that we
/// don't interleave log messages.
struct LogSerialWriter;

impl LogSerialWriter {
    #[allow(clippy::unused_self)]
    fn print_record(&self, record: &log::Record) {
        let color = match record.level() {
            log::Level::Error => ansiterm::Color::Red,
            log::Level::Warn => ansiterm::Color::Yellow,
            log::Level::Info => ansiterm::Color::Green,
            // White is actually kinda grey. Bright white is white.
            log::Level::Debug | log::Level::Trace => ansiterm::Color::White,
        };
        let color_code = ansiterm::AnsiEscapeSequence::SelectGraphicRendition(
            ansiterm::SelectGraphicRendition::ForegroundColor(color),
        );
        let clear = ansiterm::CLEAR_FORMAT;

        serial_println!("{color_code}[{}]{clear} {}", record.level(), record.args());
    }
}

pub(crate) fn init() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::debug!("Logging initialized");
}
