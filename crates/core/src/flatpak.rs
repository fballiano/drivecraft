//! Differences inside a Flatpak sandbox.
//!
//! Flatpak points `XDG_CONFIG_HOME` and `XDG_DATA_HOME` into
//! `~/.var/app/<app id>`. The settings of DriveCraft can stay there, but the
//! configuration of other cloud clients and the Trash stay in the folders of
//! the host.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// True when DriveCraft runs in a Flatpak sandbox.
pub fn is_sandboxed() -> bool {
    Path::new("/.flatpak-info").exists()
}

/// The configuration folder of the host, usually `~/.config`.
/// Outside a sandbox, this is `dirs::config_dir`.
pub fn host_config_dir(home: &Path) -> Option<PathBuf> {
    if !is_sandboxed() {
        return dirs::config_dir();
    }
    // Flatpak keeps the value of the host in HOST_XDG_CONFIG_HOME.
    Some(host_dir(
        std::env::var_os("HOST_XDG_CONFIG_HOME"),
        home,
        ".config",
    ))
}

/// The folder in `var` if it is an absolute path, otherwise `home/default`.
fn host_dir(var: Option<OsString>, home: &Path, default: &str) -> PathBuf {
    var.map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(default))
}

/// Moves `path` to the Trash of the host through the Trash portal.
/// `gio trash` uses the portal in a sandbox. The trash crate cannot: it
/// would move the folder into the hidden Trash of the sandbox.
pub fn trash(path: &Path) -> Result<()> {
    let out = std::process::Command::new("gio")
        .arg("trash")
        .arg("--")
        .arg(path)
        .output()
        .context("cannot start gio")?;
    if !out.status.success() {
        bail!(
            "gio trash failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Flatpak exists only on Linux. On Windows, "/data/config" is not absolute.
    #[cfg(unix)]
    #[test]
    fn host_dir_uses_an_absolute_value_or_the_default() {
        let home = Path::new("/home/demo");
        assert_eq!(
            host_dir(Some("/data/config".into()), home, ".config"),
            PathBuf::from("/data/config")
        );
        assert_eq!(
            host_dir(None, home, ".config"),
            PathBuf::from("/home/demo/.config")
        );
        // The XDG specification ignores a relative path.
        assert_eq!(
            host_dir(Some("config".into()), home, ".config"),
            PathBuf::from("/home/demo/.config")
        );
    }
}
