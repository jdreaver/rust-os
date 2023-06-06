use crate::{ansiterm, serial_println};

/// Dummy type to help us implement a logger using the `log` crate.
struct Logger;

static LOGGER: Logger = Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
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

    fn flush(&self) {}
}

pub(crate) fn init() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::debug!("Logging initialized");
}
