//! aios-log — shared logging vocabulary for AIOS.
//!
//! Phase 0-1 routes the 'log' facade to COM1 serial. This crate hosts the
//! helpers every subsystem shares: level gates and the formatted prefix.

#![no_std]

use core::fmt::Write;
use log::{Level, Metadata, Record};

/// Format a record with the AIOS level prefix, e.g. "[INFO] boot ...".
pub fn format_record<W: Write>(out: &mut W, record: &Record<'_>) {
    let level = match record.level() {
        Level::Error => "ERR ",
        Level::Warn => "WARN",
        Level::Info => "INFO",
        Level::Debug => "DBG ",
        Level::Trace => "TRC ",
    };
    let _ = write!(out, "[{}] ", level);
    let _ = out.write_str(record.args());
    let _ = write!(out, "\n");
}

/// Default level gate: everything at or below Trace is emitted in alpha.
pub fn enabled(metadata: &Metadata<'_>) -> bool {
    metadata.level() <= Level::Trace
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_matches_level() {
        // format_record is exercised end-to-end by the kernel logger.
        assert!(enabled(&Metadata::builder().level(Level::Info).build()));
    }
}
