//! The folders of other cloud clients on this computer.
//!
//! DriveCraft refuses these folders: two clients that sync the same folder
//! can delete files. On macOS the current apps of OneDrive, Google Drive,
//! Dropbox, and Box keep their folders in `~/Library/CloudStorage`, which
//! `check_local_dir` refuses already.

use std::path::{Path, PathBuf};

/// The folders that other cloud clients use:
/// - OneDrive on Windows: each account of the official app
///   (`HKCU\Software\Microsoft\OneDrive\Accounts\*\UserFolder`);
/// - OneDrive on Linux: the `sync_dir` of the `onedrive` client (abraunegg);
/// - Dropbox: the folders in its `info.json`, on all systems;
/// - Box Drive and the old Google "Backup and Sync" app on macOS and
///   Windows: `~/Box` and `~/Google Drive`;
/// - the default folders of pCloud Drive, kDrive, and Yandex Disk.
pub fn folders(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(windows)]
    out.extend(windows_app_folders());
    #[cfg(target_os = "linux")]
    if let Some(dir) = dirs::config_dir() {
        out.extend(abraunegg_folder(&dir.join("onedrive"), home));
    }
    for info in dropbox_info_files(home) {
        if let Ok(text) = std::fs::read_to_string(info) {
            out.extend(dropbox_folders(&text));
        }
    }
    if cfg!(any(target_os = "macos", windows)) {
        out.push(home.join("Box"));
        out.push(home.join("Google Drive"));
    }
    // The default folders of the apps of pCloud, kDrive, and Yandex Disk.
    for name in [
        "pCloud Drive",
        "kDrive",
        "Yandex.Disk",
        "Yandex.Disk.localized",
    ] {
        out.push(home.join(name));
    }
    out
}

/// The places where the Dropbox app writes `info.json`.
fn dropbox_info_files(home: &Path) -> Vec<PathBuf> {
    let mut files = vec![home.join(".dropbox/info.json")];
    if cfg!(windows) {
        for var in ["APPDATA", "LOCALAPPDATA"] {
            if let Some(dir) = std::env::var_os(var) {
                files.push(PathBuf::from(dir).join("Dropbox").join("info.json"));
            }
        }
    }
    files
}

/// The folders in a Dropbox `info.json`: one for each account, for example
/// `{"personal": {"path": "/Users/me/Dropbox", ...}, "business": {...}}`.
fn dropbox_folders(text: &str) -> Vec<PathBuf> {
    let Ok(serde_json::Value::Object(accounts)) = serde_json::from_str(text) else {
        return Vec::new();
    };
    accounts
        .values()
        .filter_map(|a| a["path"].as_str())
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(windows)]
fn windows_app_folders() -> Vec<PathBuf> {
    let Ok(accounts) = windows_registry::CURRENT_USER.open(r"Software\Microsoft\OneDrive\Accounts")
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
    fn dropbox_folders_come_from_info_json() {
        let text = r#"{"personal": {"path": "/Users/me/Dropbox", "host": 1},
                       "business": {"path": "/Users/me/Dropbox (Acme)"}}"#;
        let mut found = dropbox_folders(text);
        found.sort();
        assert_eq!(
            found,
            vec![
                PathBuf::from("/Users/me/Dropbox"),
                PathBuf::from("/Users/me/Dropbox (Acme)")
            ]
        );
        assert!(dropbox_folders("not json").is_empty());
        assert!(dropbox_folders("{\"personal\": {}}").is_empty());
    }

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
