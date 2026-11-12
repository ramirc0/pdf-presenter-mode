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
/// Each failing version of the files logs one warning. The thread ends when the receiver is dropped.
///
/// # Panics
///
/// Panics if the operating system cannot start a thread.
pub fn watch<T: Send + 'static>(
    paths: Vec<PathBuf>,
    every: Duration,
    open: impl Fn() -> anyhow::Result<T> + Send + 'static,
    notify: impl Fn() + Send + 'static,
) -> Receiver<T> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("watch".into())
        .spawn(move || {
            let mut loaded = stamps(&paths);
            let mut warned = None;
            loop {
                std::thread::sleep(every);
                let now = stamps(&paths);
                if now == loaded {
                    continue;
                }
                match open() {
                    Ok(value) => {
                        if sender.send(value).is_err() {
                            return;
                        }
                        loaded = now;
                        notify();
                    }
                    Err(err) if warned.as_ref() != Some(&now) => {
                        log::warn!("keeping the old deck: {err:#}");
                        warned = Some(now);
                    }
                    Err(_) => {}
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
        assert_eq!(reloads.recv_timeout(WAIT).unwrap(), "three");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_new_file_counts_as_a_change() {
        let dir = scratch("new");
        let sidecar = dir.join("deck.pdfpc");
        let reloads = watch(vec![sidecar.clone()], TICK, read(sidecar.clone()), || {});
        std::thread::sleep(TICK * 3);
        save(&sidecar, "notes");
        assert_eq!(reloads.recv_timeout(WAIT).unwrap(), "notes");
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
        assert_eq!(reloads.recv_timeout(WAIT).unwrap(), "whole");
        assert_eq!(attempts.load(Ordering::Relaxed), 4);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
