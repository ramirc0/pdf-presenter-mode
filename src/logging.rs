//! Warnings and errors from our dependencies, printed to stderr.
//!
//! eframe reports a failed start, such as a missing GL driver, only through `log`,
//! so without a logger the app would exit silently with status 0.

use std::sync::atomic::{AtomicBool, Ordering};

/// Prints warnings and errors to stderr and remembers whether any error was logged.
pub struct StderrLog {
    failed: AtomicBool,
}

impl StderrLog {
    /// A logger that has seen no errors yet.
    pub const fn new() -> Self {
        Self {
            failed: AtomicBool::new(false),
        }
    }

    /// Whether an error has been logged.
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }
}

impl log::Log for StderrLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // egui_commonmark installs image loaders even with its image features off,
        // so egui_extras would warn on every launch that it has none.
        metadata.level() <= log::Level::Warn && metadata.target() != "egui_extras::loaders"
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if record.level() == log::Level::Error {
            self.failed.store(true, Ordering::Relaxed);
        }
        eprintln!("{}: {}: {}", record.level(), record.target(), record.args());
    }

    fn flush(&self) {}
}

#[cfg(test)]
mod tests {
    use log::Log as _;

    use super::*;

    fn record(level: log::Level, log: &StderrLog) {
        log.log(
            &log::Record::builder()
                .level(level)
                .target("test")
                .args(format_args!("message"))
                .build(),
        );
    }

    #[test]
    fn an_error_marks_the_run_as_failed() {
        let log = StderrLog::new();
        record(log::Level::Warn, &log);
        assert!(!log.failed());
        record(log::Level::Error, &log);
        assert!(log.failed());
    }

    #[test]
    fn info_and_debug_are_dropped() {
        let log = StderrLog::new();
        assert!(!log.enabled(&log::Metadata::builder().level(log::Level::Info).build()));
        assert!(log.enabled(&log::Metadata::builder().level(log::Level::Warn).build()));
    }

    #[test]
    fn the_image_loader_warning_is_dropped() {
        let log = StderrLog::new();
        let metadata = log::Metadata::builder()
            .level(log::Level::Warn)
            .target("egui_extras::loaders")
            .build();
        assert!(!log.enabled(&metadata));
    }
}
