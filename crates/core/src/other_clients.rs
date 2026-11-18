//! The folders of other OneDrive clients on this computer.
//!
//! DriveCraft refuses these folders: two clients that sync the same folder
//! can delete files. On macOS the official app keeps its folder in
//! `~/Library/CloudStorage`, which `check_local_dir` refuses already.

use std::path::{Path, PathBuf};

/// The folders that other OneDrive clients use:
/// - Windows: each account of the official app
///   (`HKCU\Software\Microsoft\OneDrive\Accounts\*\UserFolder`);
/// - Linux: the `sync_dir` of the `onedrive` client (abraunegg).
#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
pub fn folders(home: &Path) -> Vec<PathBuf> {
    #[allow(unused_mut)]
    let mut out = Vec::new();
    #[cfg(windows)]
    out.extend(windows_app_folders());
    #[cfg(target_os = "linux")]
    if let Some(dir) = dirs::config_dir() {
        out.extend(abraunegg_folder(&dir.join("onedrive"), home));
    }
    out
}

#[cfg(windows)]
fn windows_app_folders() -> Vec<PathBuf> {
    let Ok(accounts) =
        windows_registry::CURRENT_USER.open(r"Software\Microsoft\OneDrive\Accounts")
    else {
        return Vec::new();
    };
    let Ok(names) = accounts.keys() else {
        return Vec::new();
    };
    names
        .filter_map(|name| accounts.open(&name).ok()?.get_string("UserFolder").ok())
        .filter(|folder| !folder.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// The sync folder of the abraunegg client, if its settings folder exists.
/// Without a `sync_dir` line, the client uses `~/OneDrive`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn abraunegg_folder(config_dir: &Path, home: &Path) -> Option<PathBuf> {
    if !config_dir.is_dir() {
        return None;
    }
    let text = std::fs::read_to_string(config_dir.join("config")).unwrap_or_default();
    let value = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .find(|(key, _)| key.trim() == "sync_dir")
        .map(|(_, value)| value.trim().trim_matches('"').to_string())
        .unwrap_or_else(|| "~/OneDrive".into());
    match value.strip_prefix("~/") {
        Some(rest) => Some(home.join(rest)),
        None if value == "~" => Some(home.to_path_buf()),
        None => Some(PathBuf::from(value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn abraunegg_folder_reads_sync_dir_or_uses_the_default() {
        let base = std::env::temp_dir().join("drivecraft-abraunegg-test");
        let _ = fs::remove_dir_all(&base);
        let home = PathBuf::from("/home/demo");
        let config = base.join("onedrive");
        assert_eq!(abraunegg_folder(&config, &home), None, "not installed");

        fs::create_dir_all(&config).unwrap();
        assert_eq!(
            abraunegg_folder(&config, &home),
            Some(home.join("OneDrive"))
        );

        fs::write(
            config.join("config"),
            "# sync_dir = \"~/Old\"\nskip_file = \"~*\"\n  sync_dir = \"~/Cloud/OneDrive\"\n",
        )
        .unwrap();
        assert_eq!(
            abraunegg_folder(&config, &home),
            Some(home.join("Cloud/OneDrive"))
        );

        fs::write(config.join("config"), "sync_dir = \"/data/od\"\n").unwrap();
        assert_eq!(
            abraunegg_folder(&config, &home),
            Some(PathBuf::from("/data/od"))
        );
        let _ = fs::remove_dir_all(&base);
    }
}
