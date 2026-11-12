//! The desktop entry, which lists ppm in app launchers and in "Open with" menus, and its icon.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::xdg;

/// The entry the Nix package installs. Its `Exec` starts `ppm` from the `PATH`.
const ENTRY: &str = include_str!("../assets/ppm.desktop");

/// The icon the entry names, `ppm`.
const ICON: &str = include_str!("../assets/ppm.svg");

/// Writes the desktop entry to `$XDG_DATA_HOME/applications`, by default `~/.local/share/applications`,
/// so that it starts this program by its full path, and the icon to the hicolor theme beside it.
/// Returns the paths written.
///
/// # Errors
///
/// Fails as [`xdg::data_home`] does, if this program's path is unknown,
/// or if a file cannot be written.
pub fn install() -> anyhow::Result<[PathBuf; 2]> {
    let program = std::env::current_exe().context("cannot find this program's path")?;
    install_into(&xdg::data_home()?, &program)
}

/// Writes the entry for `program` and the icon under `data`.
fn install_into(data: &Path, program: &Path) -> anyhow::Result<[PathBuf; 2]> {
    let entry = write(
        &data.join("applications"),
        "ppm.desktop",
        &entry_for(program),
    )?;
    let icon = write(&data.join("icons/hicolor/scalable/apps"), "ppm.svg", ICON)?;
    Ok([entry, icon])
}

/// Writes `contents` to the file `name` in `folder`, which it creates if needed.
fn write(folder: &Path, name: &str, contents: &str) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(folder)
        .with_context(|| format!("cannot create {}", folder.display()))?;
    let path = folder.join(name);
    std::fs::write(&path, contents).with_context(|| format!("cannot write {}", path.display()))?;
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
    fn the_entry_and_the_icon_are_written_under_the_data_folder() {
        let data = std::env::temp_dir().join(format!("ppm-desktop-{}", std::process::id()));
        let [entry, icon] = install_into(&data, Path::new("/usr/bin/ppm")).unwrap();
        assert_eq!(entry, data.join("applications/ppm.desktop"));
        assert!(
            std::fs::read_to_string(&entry)
                .unwrap()
                .contains("Exec=\"/usr/bin/ppm\" %f")
        );
        assert_eq!(icon, data.join("icons/hicolor/scalable/apps/ppm.svg"));
        assert_eq!(std::fs::read_to_string(&icon).unwrap(), ICON);
        std::fs::remove_dir_all(data).unwrap();
    }
}
