//! Where ppm's files live: the XDG base directories it writes to, and paths shown from `~`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::Context;

/// `$XDG_DATA_HOME`, by default `~/.local/share`.
///
/// # Errors
///
/// Fails if `XDG_DATA_HOME` is unset or unusable and `HOME` is not set.
pub fn data_home() -> anyhow::Result<PathBuf> {
    base_dir(
        std::env::var_os("XDG_DATA_HOME"),
        std::env::var_os("HOME"),
        ".local/share",
    )
}

/// `$XDG_STATE_HOME`, by default `~/.local/state`.
///
/// # Errors
///
/// Fails if `XDG_STATE_HOME` is unset or unusable and `HOME` is not set.
pub fn state_home() -> anyhow::Result<PathBuf> {
    base_dir(
        std::env::var_os("XDG_STATE_HOME"),
        std::env::var_os("HOME"),
        ".local/state",
    )
}

/// `path` with the home directory written as `~`.
pub fn tilde_home(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}

/// The directory `value` names, or else `fallback` under `home`.
/// The XDG spec says to ignore a value that is not an absolute path, the empty one included.
fn base_dir(
    value: Option<OsString>,
    home: Option<OsString>,
    fallback: &str,
) -> anyhow::Result<PathBuf> {
    if let Some(dir) = value.map(PathBuf::from).filter(|dir| dir.is_absolute()) {
        return Ok(dir);
    }
    Ok(PathBuf::from(home.context("HOME is not set")?).join(fallback))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(value: Option<&str>, home: Option<&str>) -> anyhow::Result<PathBuf> {
        base_dir(value.map(Into::into), home.map(Into::into), ".local/state")
    }

    #[test]
    fn an_absolute_value_wins() {
        assert_eq!(
            dir(Some("/var/state"), Some("/home/me")).unwrap(),
            PathBuf::from("/var/state")
        );
    }

    #[test]
    fn unset_empty_and_relative_values_fall_back_to_home() {
        for value in [None, Some(""), Some("state")] {
            assert_eq!(
                dir(value, Some("/home/me")).unwrap(),
                PathBuf::from("/home/me/.local/state"),
                "{value:?}"
            );
        }
    }

    #[test]
    fn without_home_there_is_no_fallback() {
        assert!(dir(None, None).is_err());
    }
}
