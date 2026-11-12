//! Warnings and errors, printed to stderr and kept in a log file for bug reports,
//! with ppm's own info lines in the file only.
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

/// Prints warnings and errors to stderr, copies them and ppm's info lines to the log file
/// once there is one, and remembers whether eframe logged an error.
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

/// Sends `log` records to stderr, and panics to the log file before the default hook prints them.
/// Call it before anything logs.
///
/// # Errors
///
/// Fails if a logger is installed already.
pub fn install() -> anyhow::Result<()> {
    log::set_logger(&LOGGER)?;
    log::set_max_level(log::LevelFilter::Info);
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        LOGGER.append(format_args!("PANIC in thread {name}: {info}"));
        default(info);
    }));
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

    /// Writes `line` to the log file after the time, if there is a file.
    fn append(&self, line: std::fmt::Arguments) {
        // A poisoned lock only means a write panicked midway. Logging must not panic too.
        if let Some(mut file) = self.file.get().and_then(|log| log.file.lock().ok()) {
            let time = jiff::Zoned::now().strftime("%H:%M:%S");
            let _ = writeln!(file, "{time} {line}");
        }
    }
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let target = metadata.target();
        match metadata.level() {
            // egui_commonmark installs image loaders even with its image features off,
            // so egui_extras would warn on every launch that it has none.
            log::Level::Error | log::Level::Warn => target != "egui_extras::loaders",
            log::Level::Info => target == "ppm" || target.starts_with("ppm::"),
            log::Level::Debug | log::Level::Trace => false,
        }
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if record.level() == log::Level::Error && record.target().starts_with("eframe") {
            self.failed.store(true, Ordering::Relaxed);
        }
        if record.level() <= log::Level::Warn {
            eprintln!("{}: {}: {}", record.level(), record.target(), record.args());
        }
        self.append(format_args!(
            "{}: {}: {}",
            record.level(),
            record.target(),
            record.args()
        ));
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
    fn info_from_ppm_is_kept_and_from_other_crates_dropped() {
        let log = Logger::new();
        let at = |level, target| log::Metadata::builder().level(level).target(target).build();
        assert!(log.enabled(&at(log::Level::Info, "ppm")));
        assert!(log.enabled(&at(log::Level::Info, "ppm::watch")));
        assert!(!log.enabled(&at(log::Level::Info, "ppmx")));
        assert!(!log.enabled(&at(log::Level::Info, "eframe::native::run")));
        assert!(!log.enabled(&at(log::Level::Debug, "ppm")));
        assert!(log.enabled(&at(log::Level::Warn, "eframe::native::run")));
    }

    #[test]
    fn info_from_ppm_reaches_the_file() {
        let dir = scratch("info");
        let log = Logger::new();
        assert!(log.file.set(open_log(&dir).unwrap().unwrap()).is_ok());
        record(log::Level::Info, "ppm::deck", &log);
        let text = std::fs::read_to_string(dir.join("ppm.log")).unwrap();
        assert!(text.ends_with(" INFO: ppm::deck: message\n"), "{text}");
        std::fs::remove_dir_all(dir).unwrap();
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
