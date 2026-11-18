//! User settings and the selective sync rules.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// An rclone binary to use instead of the bundled one, for tests only.
    /// Empty means the bundled binary.
    pub rclone_path: String,
    /// The rclone remote for the OneDrive account, without the colon.
    pub remote: Option<String>,
    /// The local OneDrive folder. It holds real copies of the synced folders
    /// and of the files in the OneDrive root.
    pub local_dir: PathBuf,
    pub sync_interval_minutes: u64,
    /// The folders that sync to this computer, as paths from the OneDrive root.
    pub pinned: BTreeSet<String>,
    /// Changes that the next sync applies.
    pub pending_pin: BTreeSet<String>,
    pub pending_unpin: BTreeSet<String>,
    /// True when the next bisync must use --resync.
    pub needs_resync: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Settings {
            rclone_path: String::new(),
            remote: None,
            local_dir: default_local_dir(&home),
            sync_interval_minutes: 10,
            pinned: BTreeSet::new(),
            pending_pin: BTreeSet::new(),
            pending_unpin: BTreeSet::new(),
            needs_resync: true,
        }
    }
}

/// `~/OneDrive`, or `~/OneDrive (DriveCraft)` if that name is used.
/// The official OneDrive app makes `~/OneDrive` as a link to its own folder.
fn default_local_dir(home: &Path) -> PathBuf {
    let plain = home.join("OneDrive");
    if fs::symlink_metadata(&plain).is_err() {
        plain
    } else {
        home.join("OneDrive (DriveCraft)")
    }
}

/// Refuses local folders that must not hold a synced copy.
pub fn check_local_dir(dir: &Path, home: &Path) -> Result<()> {
    if !dir.is_absolute() {
        bail!("Select a full folder path.");
    }
    let real = resolve(dir);
    let real_home = resolve(home);
    if real.parent().is_none() || real == real_home {
        bail!(
            "Select a folder inside your home folder, not {}.",
            dir.display()
        );
    }
    if real.starts_with(real_home.join("Library/CloudStorage")) {
        bail!(
            "{} belongs to a different cloud app (macOS CloudStorage). Select a different folder.",
            dir.display()
        );
    }
    if real.starts_with(resolve(&Settings::dir())) {
        bail!("Select a folder outside the DriveCraft settings folder.");
    }
    Ok(())
}

/// The real path, with symbolic links resolved, also if the end of the
/// path does not exist yet.
fn resolve(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while fs::symlink_metadata(&existing).is_err() {
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut real = fs::canonicalize(&existing).unwrap_or(existing);
    for name in rest.into_iter().rev() {
        real.push(name);
    }
    real.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect()
}

/// How the UI shows the sync switch of one folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeepState {
    NotKept,
    Kept,
    /// A parent folder syncs, so this folder syncs too.
    Inherited,
    /// The user changed the switch. The next sync applies the change.
    Pending {
        keep: bool,
    },
}

impl KeepState {
    pub fn is_on(self) -> bool {
        matches!(
            self,
            KeepState::Kept | KeepState::Inherited | KeepState::Pending { keep: true }
        )
    }
}

impl Settings {
    pub fn dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("drivecraft")
    }

    fn file() -> PathBuf {
        Self::dir().join("settings.toml")
    }

    pub fn filters_file() -> PathBuf {
        Self::dir().join("filters.txt")
    }

    pub fn session_file() -> PathBuf {
        Self::dir().join("rclone-session.json")
    }

    /// The list of the synced local files after the last sync.
    pub fn snapshot_file() -> PathBuf {
        Self::dir().join("snapshot.json")
    }

    pub fn log_file() -> PathBuf {
        Self::dir().join("rclone.log")
    }

    pub fn load() -> Result<Settings> {
        let path = Self::file();
        if !path.exists() {
            return Ok(Settings::default());
        }
        let text = fs::read_to_string(&path)?;
        toml::from_str(&text).with_context(|| format!("cannot read {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(Self::dir())?;
        let tmp = Self::file().with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        fs::rename(&tmp, Self::file())?;
        Ok(())
    }

    pub fn keep_state(&self, path: &str) -> KeepState {
        let parent_kept = self
            .pinned
            .iter()
            .filter(|p| !self.pending_unpin.contains(*p))
            .chain(self.pending_pin.iter())
            .any(|p| is_strictly_under(path, p));
        if parent_kept {
            KeepState::Inherited
        } else if self.pending_unpin.contains(path) {
            KeepState::Pending { keep: false }
        } else if self.pending_pin.contains(path) {
            KeepState::Pending { keep: true }
        } else if self.pinned.contains(path) {
            KeepState::Kept
        } else {
            KeepState::NotKept
        }
    }

    /// Records a switch change. The next sync applies it.
    pub fn set_keep(&mut self, path: &str, keep: bool) {
        if keep {
            if !self.pending_unpin.remove(path) && !self.pinned.contains(path) {
                self.pending_pin.insert(path.to_string());
            }
        } else if !self.pending_pin.remove(path) && self.pinned.contains(path) {
            self.pending_unpin.insert(path.to_string());
        }
    }

    pub fn has_pending(&self) -> bool {
        !self.pending_pin.is_empty() || !self.pending_unpin.is_empty()
    }

    /// Moves the pending changes into `pinned`. Returns the folders that no
    /// longer sync, so that the caller can remove their local copies.
    pub fn apply_pending(&mut self) -> Vec<String> {
        let removed: Vec<String> = std::mem::take(&mut self.pending_unpin)
            .into_iter()
            .collect();
        for p in &removed {
            self.pinned.remove(p);
        }
        self.pinned.append(&mut self.pending_pin);
        // A folder inside a kept folder needs no entry of its own.
        let all = self.pinned.clone();
        self.pinned
            .retain(|p| !all.iter().any(|a| is_strictly_under(p, a)));
        self.needs_resync = true;
        removed
    }

    /// True for a local file path (relative, "/" separators) that syncs:
    /// a file in the root, or a file in a synced folder.
    pub fn is_synced_path(&self, rel: &str) -> bool {
        !rel.contains('/') || self.pinned.iter().any(|p| is_strictly_under(rel, p))
    }

    /// The rclone filter rules: no system files, then the files in the
    /// OneDrive root and the synced folders.
    pub fn filter_rules(&self) -> String {
        let mut out = String::from("# Generated by DriveCraft. Do not edit.\n");
        for junk in crate::snapshot::JUNK {
            out.push_str(&format!("- {junk}\n"));
        }
        out.push_str("+ /*\n");
        for p in &self.pinned {
            out.push_str(&format!("+ /{}/**\n", escape_glob(p)));
        }
        out.push_str("- **\n");
        out
    }
}

fn is_strictly_under(path: &str, ancestor: &str) -> bool {
    path.len() > ancestor.len() + 1
        && path.starts_with(ancestor)
        && path.as_bytes()[ancestor.len()] == b'/'
}

fn escape_glob(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// True when the path is a safe relative path inside the OneDrive root.
pub fn is_safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(pinned: &[&str]) -> Settings {
        Settings {
            pinned: pinned.iter().map(|s| s.to_string()).collect(),
            ..Settings::default()
        }
    }

    #[test]
    fn keep_state_of_child_of_kept_folder_is_inherited() {
        let s = settings(&["Docs"]);
        assert_eq!(s.keep_state("Docs"), KeepState::Kept);
        assert_eq!(s.keep_state("Docs/Work"), KeepState::Inherited);
        assert_eq!(s.keep_state("Docs2"), KeepState::NotKept);
    }

    #[test]
    fn set_keep_and_undo_leaves_no_pending_change() {
        let mut s = settings(&["Docs"]);
        s.set_keep("Photos", true);
        s.set_keep("Photos", false);
        s.set_keep("Docs", false);
        s.set_keep("Docs", true);
        assert!(!s.has_pending());
    }

    #[test]
    fn apply_pending_removes_nested_entries_and_returns_removed() {
        let mut s = settings(&["Docs/Work", "Music"]);
        s.set_keep("Docs", true);
        s.set_keep("Music", false);
        let removed = s.apply_pending();
        assert_eq!(removed, vec!["Music".to_string()]);
        assert_eq!(s.pinned.iter().collect::<Vec<_>>(), vec!["Docs"]);
        assert!(s.needs_resync);
    }

    #[test]
    fn filter_rules_escape_glob_characters() {
        let s = settings(&["A [1]", "B*"]);
        assert_eq!(
            s.filter_rules(),
            "# Generated by DriveCraft. Do not edit.\n\
             - .DS_Store\n- ._*\n- Thumbs.db\n- desktop.ini\n- ~$*\n- .~lock.*\n\
             + /*\n+ /A \\[1\\]/**\n+ /B\\*/**\n- **\n"
        );
    }

    #[test]
    fn check_local_dir_refuses_dangerous_folders() {
        let home = std::env::temp_dir().join("drivecraft-test-home");
        fs::create_dir_all(home.join("Library/CloudStorage/OneDrive-Personal")).unwrap();
        let link = home.join("OneDrive");
        let _ = fs::remove_file(&link);
        #[cfg(unix)]
        std::os::unix::fs::symlink(home.join("Library/CloudStorage/OneDrive-Personal"), &link)
            .unwrap();

        assert!(check_local_dir(&home.join("Synced"), &home).is_ok());
        assert!(check_local_dir(&home.join("new/deeper/folder"), &home).is_ok());
        assert!(check_local_dir(&home, &home).is_err());
        assert!(check_local_dir(Path::new("/"), &home).is_err());
        assert!(check_local_dir(Path::new("relative"), &home).is_err());
        assert!(check_local_dir(&home.join("Library/CloudStorage/X"), &home).is_err());
        #[cfg(unix)]
        assert!(
            check_local_dir(&link.join("sub"), &home).is_err(),
            "a link into CloudStorage"
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn synced_paths_are_root_files_and_files_in_synced_folders() {
        let s = settings(&["Docs"]);
        assert!(s.is_synced_path("root.txt"));
        assert!(s.is_synced_path("Docs/a.txt"));
        assert!(s.is_synced_path("Docs/Sub/b.txt"));
        assert!(!s.is_synced_path("Docs2/a.txt"));
        assert!(!s.is_synced_path("Music/song.mp3"));
    }

    #[test]
    fn safe_relative_rejects_parent_components() {
        assert!(is_safe_relative("Docs/Work"));
        assert!(!is_safe_relative("../etc"));
        assert!(!is_safe_relative("Docs/../.."));
        assert!(!is_safe_relative("/abs"));
        assert!(!is_safe_relative(""));
    }
}
