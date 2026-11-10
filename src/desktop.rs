//! The desktop entry, which lists ppm in app launchers and in "Open with" menus.

use std::path::{Path, PathBuf};

use anyhow::Context;

/// The entry the Nix package installs. Its `Exec` starts `ppm` from the `PATH`.
const ENTRY: &str = include_str!("../assets/ppm.desktop");

/// Writes the desktop entry to `$XDG_DATA_HOME/applications`, by default `~/.local/share/applications`,
/// so that it starts this program by its full path. Returns the path written.
///
/// # Errors
///
/// Fails if neither `XDG_DATA_HOME` nor `HOME` is set, if this program's path is unknown,
/// or if the file cannot be written.
pub fn install() -> anyhow::Result<PathBuf> {
    let data = match std::env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => {
            PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".local/share")
        }
    };
    let program = std::env::current_exe().context("cannot find this program's path")?;
    install_into(&data, &program)
}

/// Writes the entry for `program` into the `applications` folder of `data`.
fn install_into(data: &Path, program: &Path) -> anyhow::Result<PathBuf> {
    let folder = data.join("applications");
    std::fs::create_dir_all(&folder)
        .with_context(|| format!("cannot create {}", folder.display()))?;
    let path = folder.join("ppm.desktop");
    std::fs::write(&path, entry_for(program))
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(path)
}

/// The entry with `Exec` starting `program`, quoted and escaped as the Desktop Entry
/// specification asks.
fn entry_for(program: &Path) -> String {
    let mut quoted = String::new();
    for c in program.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    // String values unescape backslashes before the quoting rules apply, so each doubles again.
    let quoted = quoted.replace('\\', "\\\\");
    ENTRY.replace("Exec=ppm %f", &format!("Exec=\"{quoted}\" %f"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_starts_the_program_by_its_full_path() {
        let entry = entry_for(Path::new("/home/me/.cargo/bin/ppm"));
        assert!(
            entry.contains("\nExec=\"/home/me/.cargo/bin/ppm\" %f\n"),
            "{entry}"
        );
        assert_eq!(entry.replace("\"/home/me/.cargo/bin/ppm\"", "ppm"), ENTRY);
    }

    #[test]
    fn odd_characters_in_the_path_are_escaped() {
        let entry = entry_for(Path::new("/home/a b/$x/100%/ppm"));
        assert!(
            entry.contains("Exec=\"/home/a b/\\\\$x/100%%/ppm\" %f"),
            "{entry}"
        );
    }

    #[test]
    fn the_entry_is_written_under_applications() {
        let data = std::env::temp_dir().join(format!("ppm-desktop-{}", std::process::id()));
        let path = install_into(&data, Path::new("/usr/bin/ppm")).unwrap();
        assert_eq!(path, data.join("applications/ppm.desktop"));
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("Exec=\"/usr/bin/ppm\" %f")
        );
        std::fs::remove_dir_all(data).unwrap();
    }
}
