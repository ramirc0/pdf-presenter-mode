//! Reopening a deck when its files change on disk.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, SystemTime};

/// What identifies one version of a file: its modification time and length, or `None` while missing.
type Stamp = Option<(SystemTime, u64)>;

fn stamps(paths: &[PathBuf]) -> Vec<Stamp> {
    paths
        .iter()
        .map(|path| {
            let meta = std::fs::metadata(path).ok()?;
            Some((meta.modified().ok()?, meta.len()))
        })
        .collect()
}

/// Checks `paths` every `every` and calls `open` after any of them changes, appears or
/// disappears. Each value `open` returns arrives on the receiver, followed by a call to `notify`.
///
/// Checking each path anew, instead of watching the file, also catches an editor or compiler
/// that replaces the file by renaming a new one over it. If `open` fails, as it does on a
/// half-written PDF, the old value stays in use and `open` runs again at the next check.
/// A file still being written changes by then. A version of the files that fails twice in a row
/// is broken instead: its error arrives once, also logged as a warning.
/// The thread ends when the receiver is dropped.
///
/// # Panics
///
/// Panics if the operating system cannot start a thread.
pub fn watch<T: Send + 'static>(
    paths: Vec<PathBuf>,
    every: Duration,
    open: impl Fn() -> anyhow::Result<T> + Send + 'static,
    notify: impl Fn() + Send + 'static,
) -> Receiver<anyhow::Result<T>> {
    let (sender, receiver) = mpsc::channel();
    // Stamped here, not in the thread, so a change right after the caller opened the files counts.
    let mut loaded = stamps(&paths);
    std::thread::Builder::new()
        .name("watch".into())
        .spawn(move || {
            // The version of the files that keeps failing, and how many times in a row it did.
            let mut failing: Option<(Vec<Stamp>, u32)> = None;
            loop {
                std::thread::sleep(every);
                let now = stamps(&paths);
                if now == loaded {
                    continue;
                }
                match open() {
                    Ok(value) => {
                        if sender.send(Ok(value)).is_err() {
                            return;
                        }
                        loaded = now;
                        failing = None;
                        notify();
                    }
                    Err(err) => {
                        let times = match &mut failing {
                            Some((version, times)) if *version == now => {
                                *times += 1;
                                *times
                            }
                            _ => {
                                failing = Some((now, 1));
                                1
                            }
                        };
                        if times == 2 {
                            log::warn!("keeping the old deck: {err:#}");
                            if sender.send(Err(err)).is_err() {
                                return;
                            }
                            notify();
                        }
                    }
                }
            }
        })
        .expect("cannot start the watch thread");
    receiver
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const TICK: Duration = Duration::from_millis(10);
    const WAIT: Duration = Duration::from_secs(5);

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ppm-watch-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(path: PathBuf) -> impl Fn() -> anyhow::Result<String> {
        move || Ok(std::fs::read_to_string(&path)?)
    }

    /// Writes `text` to `path` the way editors and Typst save: whole, then renamed into place.
    /// A plain write could show the watch an empty file for a moment.
    fn save(path: &Path, text: &str) {
        let partial = path.with_extension("partial");
        std::fs::write(&partial, text).unwrap();
        std::fs::rename(partial, path).unwrap();
    }

    #[test]
    fn a_change_reopens() {
        let dir = scratch("change");
        let file = dir.join("deck");
        std::fs::write(&file, "one").unwrap();
        let reloads = watch(vec![file.clone()], TICK, read(file.clone()), || {});
        std::thread::sleep(TICK * 3);
        save(&file, "three");
        assert_eq!(reloads.recv_timeout(WAIT).unwrap().unwrap(), "three");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_change_right_after_the_watch_starts_reopens() {
        let dir = scratch("early");
        let file = dir.join("deck");
        save(&file, "one");
        let reloads = watch(vec![file.clone()], TICK, read(file.clone()), || {});
        save(&file, "three");
        assert_eq!(reloads.recv_timeout(WAIT).unwrap().unwrap(), "three");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_new_file_counts_as_a_change() {
        let dir = scratch("new");
        let sidecar = dir.join("deck.pdfpc");
        let reloads = watch(vec![sidecar.clone()], TICK, read(sidecar.clone()), || {});
        std::thread::sleep(TICK * 3);
        save(&sidecar, "notes");
        assert_eq!(reloads.recv_timeout(WAIT).unwrap().unwrap(), "notes");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_open_is_retried_until_it_works() {
        let dir = scratch("retry");
        let file = dir.join("deck");
        std::fs::write(&file, "one").unwrap();
        let attempts = Arc::new(AtomicUsize::new(0));
        let open = {
            let attempts = attempts.clone();
            move || {
                if attempts.fetch_add(1, Ordering::Relaxed) < 3 {
                    anyhow::bail!("half written");
                }
                Ok("whole")
            }
        };
        let reloads = watch(vec![file.clone()], TICK, open, || {});
        std::thread::sleep(TICK * 3);
        save(&file, "two");
        // The second failure of the same version is reported, once.
        let error = reloads.recv_timeout(WAIT).unwrap().unwrap_err();
        assert_eq!(error.to_string(), "half written");
        assert_eq!(reloads.recv_timeout(WAIT).unwrap().unwrap(), "whole");
        assert_eq!(attempts.load(Ordering::Relaxed), 4);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_version_that_fails_once_and_changes_is_not_reported() {
        let dir = scratch("half");
        let file = dir.join("deck");
        save(&file, "one");
        let attempts = Arc::new(AtomicUsize::new(0));
        let open = {
            let attempts = attempts.clone();
            let file = file.clone();
            move || {
                attempts.fetch_add(1, Ordering::Relaxed);
                let text = std::fs::read_to_string(&file)?;
                if text == "half" {
                    anyhow::bail!("half written");
                }
                Ok(text)
            }
        };
        // A slow tick leaves time to finish the file between two checks.
        let tick = Duration::from_millis(200);
        let reloads = watch(vec![file.clone()], tick, open, || {});
        save(&file, "half");
        while attempts.load(Ordering::Relaxed) == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        save(&file, "whole");
        assert_eq!(reloads.recv_timeout(WAIT).unwrap().unwrap(), "whole");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
