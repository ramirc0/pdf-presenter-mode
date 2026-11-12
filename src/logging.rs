//! Warnings and errors, printed to stderr and kept in a log file for bug reports.
//!
//! eframe reports a failed start, such as a missing GL driver, only through `log`,
//! so without a logger the app would exit silently with status 0.

use std::fs::{File, TryLockError};
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use anyhow::Context as _;

use crate::xdg;

/// Prints warnings and errors to stderr, copies them to the log file once there is one,
/// and remembers whether eframe logged an error.
pub struct Logger {
    failed: AtomicBool,
    file: OnceLock<LogFile>,
}

/// This run's log file, and the lock that makes it this run's.
struct LogFile {
    file: Mutex<File>,
    /// Held until ppm exits, so that a second ppm leaves the log alone.
    _lock: File,
}

static LOGGER: Logger = Logger::new();

/// Sends `log` records to stderr. Call it before anything logs.
///
/// # Errors
///
/// Fails if a logger is installed already.
pub fn install() -> anyhow::Result<()> {
    log::set_logger(&LOGGER)?;
    log::set_max_level(log::LevelFilter::Warn);
    Ok(())
}

/// Starts this run's log file, `$XDG_STATE_HOME/ppm/ppm.log`, after moving the last run's
/// to `ppm.log.1`. If another ppm holds the log, this run keeps none. Problems only warn,
/// since a talk does not need the file.
pub fn keep_file() {
    let opened = xdg::state_home().and_then(|state| open_log(&state.join("ppm")));
    match opened {
        Ok(Some(file)) => {
            let _ = LOGGER.file.set(file);
        }
        Ok(None) => log::warn!("another ppm is writing the log file, so this one keeps none"),
        Err(err) => log::warn!("cannot keep a log file: {err:#}"),
    }
}

/// Whether eframe has logged an error, which is how it reports a failed start.
/// Other crates' errors, such as a failed paste in egui-winit, do not count.
pub fn failed() -> bool {
    LOGGER.failed.load(Ordering::Relaxed)
}

/// Opens a new `ppm.log` in `dir`, unless another ppm holds `ppm.lock` there.
fn open_log(dir: &Path) -> anyhow::Result<Option<LogFile>> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let lock_path = dir.join("ppm.lock");
    let lock =
        File::create(&lock_path).with_context(|| format!("cannot open {}", lock_path.display()))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(err)) => {
            return Err(err).with_context(|| format!("cannot lock {}", lock_path.display()));
        }
    }
    let path = dir.join("ppm.log");
    match std::fs::rename(&path, dir.join("ppm.log.1")) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            return Err(err).with_context(|| format!("cannot move {}", path.display()));
        }
        _ => {}
    }
    let file = File::create(&path).with_context(|| format!("cannot create {}", path.display()))?;
    Ok(Some(LogFile {
        file: Mutex::new(file),
        _lock: lock,
    }))
}

impl Logger {
    const fn new() -> Self {
        Self {
            failed: AtomicBool::new(false),
            file: OnceLock::new(),
        }
    }
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // egui_commonmark installs image loaders even with its image features off,
        // so egui_extras would warn on every launch that it has none.
        metadata.level() <= log::Level::Warn && metadata.target() != "egui_extras::loaders"
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if record.level() == log::Level::Error && record.target().starts_with("eframe") {
            self.failed.store(true, Ordering::Relaxed);
        }
        eprintln!("{}: {}: {}", record.level(), record.target(), record.args());
        // A poisoned lock only means a write panicked midway. Logging must not panic too.
        if let Some(mut file) = self.file.get().and_then(|log| log.file.lock().ok()) {
            let time = jiff::Zoned::now().strftime("%H:%M:%S");
            let _ = writeln!(
                file,
                "{time} {}: {}: {}",
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

#[cfg(test)]
mod tests {
    use log::Log as _;

    use super::*;

    fn record(level: log::Level, target: &str, log: &Logger) {
        log.log(
            &log::Record::builder()
                .level(level)
                .target(target)
                .args(format_args!("message"))
                .build(),
        );
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ppm-log-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_error_from_eframe_marks_the_run_as_failed() {
        let log = Logger::new();
        record(log::Level::Warn, "eframe::native::run", &log);
        assert!(!log.failed.load(Ordering::Relaxed));
        record(log::Level::Error, "eframe::native::run", &log);
        assert!(log.failed.load(Ordering::Relaxed));
    }

    #[test]
    fn an_error_from_another_crate_does_not_fail_the_run() {
        let log = Logger::new();
        record(log::Level::Error, "egui_winit::clipboard", &log);
        assert!(!log.failed.load(Ordering::Relaxed));
    }

    #[test]
    fn info_and_debug_are_dropped() {
        let log = Logger::new();
        assert!(!log.enabled(&log::Metadata::builder().level(log::Level::Info).build()));
        assert!(log.enabled(&log::Metadata::builder().level(log::Level::Warn).build()));
    }

    #[test]
    fn the_image_loader_warning_is_dropped() {
        let log = Logger::new();
        let metadata = log::Metadata::builder()
            .level(log::Level::Warn)
            .target("egui_extras::loaders")
            .build();
        assert!(!log.enabled(&metadata));
    }

    #[test]
    fn the_last_run_moves_to_log_1() {
        let dir = scratch("rotate");
        std::fs::write(dir.join("ppm.log"), "the last run").unwrap();
        let log = open_log(&dir).unwrap();
        assert!(log.is_some());
        assert_eq!(
            std::fs::read_to_string(dir.join("ppm.log.1")).unwrap(),
            "the last run"
        );
        assert_eq!(std::fs::read_to_string(dir.join("ppm.log")).unwrap(), "");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_second_ppm_leaves_the_log_alone() {
        let dir = scratch("second");
        let first = open_log(&dir).unwrap();
        std::fs::write(dir.join("ppm.log"), "the first run").unwrap();
        assert!(open_log(&dir).unwrap().is_none());
        assert_eq!(
            std::fs::read_to_string(dir.join("ppm.log")).unwrap(),
            "the first run"
        );
        assert!(!dir.join("ppm.log.1").exists());
        drop(first);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_warning_reaches_the_file_after_its_time() {
        let dir = scratch("warning");
        let log = Logger::new();
        assert!(log.file.set(open_log(&dir).unwrap().unwrap()).is_ok());
        record(log::Level::Warn, "ppm::watch", &log);
        let text = std::fs::read_to_string(dir.join("ppm.log")).unwrap();
        let (time, rest) = text.split_once(' ').unwrap();
        assert!(
            jiff::civil::Time::strptime("%H:%M:%S", time).is_ok(),
            "{text}"
        );
        assert_eq!(rest, "WARN: ppm::watch: message\n");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
