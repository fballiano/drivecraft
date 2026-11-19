//! The check for a new version of DriveCraft.
//!
//! Only installs that no package manager updates check: a zip, an
//! installer, or a Linux tar.gz from the GitHub release page (and winget,
//! which uses the same installer). Homebrew, Scoop, Flatpak, and the AUR
//! update DriveCraft themselves.
//!
//! The check asks the GitHub API for the latest release, at most once a
//! day. It sends no data about the user or the accounts, and DriveCraft
//! has no server: only GitHub sees the request, as for a visit to the
//! release page.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

const LATEST: &str = "https://api.github.com/repos/fballiano/drivecraft/releases/latest";

/// The version of this build, for example "0.9.0".
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// A newer release.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    /// For example "0.9.1".
    pub version: String,
    /// The release page.
    pub url: String,
}

/// True when this install must check for updates itself: no package
/// manager updates it, and it is not a development build.
pub fn wanted() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let exe = exe.canonicalize().unwrap_or(exe);
    !managed(&exe, crate::flatpak::is_sandboxed())
}

/// True when a package manager installed the executable `exe`.
fn managed(exe: &Path, flatpak: bool) -> bool {
    if flatpak {
        return true;
    }
    let path = exe.to_string_lossy().replace('\\', "/").to_lowercase();
    // Scoop: ~/scoop/apps/drivecraft/<version>/DriveCraft.exe.
    // AUR: /usr/lib/drivecraft/drivecraft.
    if path.contains("/scoop/apps/drivecraft/") || path.starts_with("/usr/lib/drivecraft/") {
        return true;
    }
    // Homebrew: the app is in /Applications, and the cask in the Caskroom.
    cfg!(target_os = "macos")
        && path.starts_with("/applications/drivecraft.app/")
        && ["/opt/homebrew", "/usr/local"]
            .iter()
            .any(|prefix| Path::new(prefix).join("Caskroom/drivecraft").is_dir())
}

/// The latest release, if it is newer than this build.
pub fn check() -> Result<Option<Update>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut resp = agent
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        // GitHub requires a User-Agent. It names only the app.
        .header("User-Agent", &format!("DriveCraft/{CURRENT}"))
        .call()
        .context("no connection to GitHub")?;
    let status = resp.status().as_u16();
    if status != 200 {
        bail!("GitHub answered with HTTP {status}");
    }
    let body: Value = resp.body_mut().read_json()?;
    Ok(newer(&body, CURRENT))
}

/// The release in `body` if it is newer than `current`.
fn newer(body: &Value, current: &str) -> Option<Update> {
    let tag = body["tag_name"].as_str()?;
    let version = tag.trim_start_matches('v');
    (parse(version)? > parse(current)?).then(|| Update {
        version: version.to_string(),
        url: body["html_url"]
            .as_str()
            .unwrap_or("https://github.com/fballiano/drivecraft/releases/latest")
            .to_string(),
    })
}

/// "0.9.1" as numbers. A pre-release ("1.0.0-rc1") is not an update.
fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|p| p.parse::<u64>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_a_newer_release_is_an_update() {
        let release = |tag: &str| json!({"tag_name": tag, "html_url": "https://x/r"});
        assert_eq!(
            newer(&release("v0.10.0"), "0.9.0"),
            Some(Update {
                version: "0.10.0".into(),
                url: "https://x/r".into()
            })
        );
        assert_eq!(newer(&release("v0.9.0"), "0.9.0"), None);
        assert_eq!(newer(&release("v0.8.9"), "0.9.0"), None);
        assert_eq!(newer(&release("v1.0.0-rc1"), "0.9.0"), None);
        assert_eq!(newer(&json!({}), "0.9.0"), None);
    }

    #[test]
    fn package_managers_update_their_installs() {
        assert!(managed(Path::new("/x"), true), "Flatpak");
        assert!(managed(
            Path::new(r"C:\Users\me\scoop\apps\drivecraft\0.9.0\DriveCraft.exe"),
            false
        ));
        assert!(managed(Path::new("/usr/lib/drivecraft/drivecraft"), false));
        assert!(!managed(
            Path::new(r"C:\Users\me\AppData\Local\Programs\DriveCraft\DriveCraft.exe"),
            false
        ));
        assert!(!managed(
            Path::new("/home/me/Downloads/DriveCraft/drivecraft"),
            false
        ));
    }
}
