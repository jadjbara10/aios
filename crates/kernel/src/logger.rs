//! Bridge between the 'log' crate facade and the COM1 serial port.

use core::fmt::Write;
use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::serial;

struct SerialLogger;

impl Log for SerialLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            Level::Error => "ERR ",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DBG ",
            Level::Trace => "TRC ",
        };
        let mut port = serial::SERIAL.lock();
        let _ = write!(port, "[{}] ", level);
        let _ = port.write_str(record.args());
        let _ = write!(port, "\r\n");
    }

    fn flush(&self) {}
}

static LOGGER: SerialLogger = SerialLogger;

/// Install the serial-backed logger. Call exactly once at boot.
pub fn init() {
    // SAFETY: 'set_logger' requires the logger to live for 'static.
    // 'LOGGER' is a 'static, so this holds.
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(LevelFilter::Trace);
    }
}
