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
    /// Top-level local folders that the user keeps only on this computer.
    /// They do not sync, and DriveCraft does not ask about them again.
    pub local_only: BTreeSet<String>,
    /// True when the next bisync must use --resync.
    pub needs_resync: bool,
    /// True after a sign-in, until the user confirms the folder selection.
    /// DriveCraft does not sync before that, so it downloads only the
    /// folders that the user wants.
    pub awaiting_selection: bool,
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
            local_only: BTreeSet::new(),
            needs_resync: true,
            awaiting_selection: false,
        }
    }
}

/// The parent folder of the account folders: `~/DriveCraft`. No other
/// client uses this name, so the default folders never collide with
/// them. Each account gets its own folder in it, for example
/// `~/DriveCraft/OneDrive`. The parent folder itself does not sync.
pub fn accounts_root(home: &Path) -> PathBuf {
    home.join("DriveCraft")
}

/// `~/DriveCraft/OneDrive`.
fn default_local_dir(home: &Path) -> PathBuf {
    accounts_root(home).join("OneDrive")
}

/// Refuses local folders that must not hold a synced copy.
pub fn check_local_dir(dir: &Path, home: &Path) -> Result<()> {
    check_local_dir_with(dir, home, &crate::other_clients::folders(home))
}

/// `check_local_dir` with the folders of other clients as a parameter.
fn check_local_dir_with(dir: &Path, home: &Path, other_clients: &[PathBuf]) -> Result<()> {
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
    if real == resolve(&accounts_root(home)) {
        bail!(
            "{} holds one folder for each account. Select a folder inside it, for example {}.",
            dir.display(),
            default_local_dir(home).display()
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
    for other in other_clients {
        let other_real = resolve(other);
        if real.starts_with(&other_real) || other_real.starts_with(&real) {
            bail!(
                "A different OneDrive app syncs {}. Two apps must not sync the same folder. \
                 Select a different folder, or unlink the other app first.",
                other.display()
            );
        }
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

    /// The rclone configuration file of DriveCraft. It holds the accounts.
    /// DriveCraft does not use the default file of the user's own rclone.
    pub fn rclone_config_file() -> PathBuf {
        Self::dir().join("rclone.conf")
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
            self.local_only.remove(path);
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

    /// The top-level folders in the local folder that do not sync and are
    /// not in `local_only`. The user made them on this computer, so the UI
    /// asks if they sync. Hidden folders and system files are not included.
    pub fn new_local_folders(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.local_dir) else {
            return Vec::new();
        };
        // macOS can store a name in NFD and OneDrive in NFC, so compare in NFC.
        let known: BTreeSet<String> = self
            .pinned
            .iter()
            .chain(&self.pending_pin)
            .chain(&self.pending_unpin)
            .map(|p| p.split('/').next().unwrap_or(p))
            .chain(self.local_only.iter().map(String::as_str))
            .map(nfc)
            .collect();
        let mut out: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let is_dir = e.file_type().ok()?.is_dir(); // Links are not followed.
                let new = is_dir
                    && !name.starts_with('.')
                    && !crate::snapshot::is_junk(&name)
                    && !known.contains(&nfc(&name));
                new.then_some(name)
            })
            .collect();
        out.sort_by_key(|n| n.to_lowercase());
        out
    }

    /// Removes the `local_only` entries whose local folder no longer exists.
    /// Returns true when an entry was removed.
    pub fn prune_local_only(&mut self) -> bool {
        let before = self.local_only.len();
        let dir = self.local_dir.clone();
        self.local_only.retain(|name| dir.join(name).is_dir());
        self.local_only.len() != before
    }

    /// True for a local file path (relative, "/" separators) that syncs:
    /// a file in the root, or a file in a synced folder.
    pub fn is_synced_path(&self, rel: &str) -> bool {
        if !rel.contains('/') {
            return true;
        }
        let rel = nfc(rel);
        self.pinned.iter().any(|p| is_strictly_under(&rel, &nfc(p)))
    }

    /// The rclone filter rules: no system files, then the files in the
    /// OneDrive root and the synced folders.
    pub fn filter_rules(&self) -> String {
        let mut out = String::from("# Generated by DriveCraft. Do not edit.\n");
        for junk in crate::snapshot::JUNK {
            out.push_str(&format!("- {junk}\n"));
        }
        out.push_str("+ /*\n");
        // A name can be NFC on one side and NFD on the other, so a name
        // that has two forms gets a rule for each form.
        for p in &self.pinned {
            let mut forms = vec![nfc(p), nfd(p)];
            forms.dedup();
            for form in forms {
                out.push_str(&format!("+ /{}/**\n", escape_glob(&form)));
            }
        }
        out.push_str("- **\n");
        out
    }
}

/// True when `rules` are the filter rules of the last resync. At each
/// resync, bisync stores the MD5 of the filter file in `<filter file>.md5`.
/// Later, bisync stops if the file has a different MD5, until the next
/// resync. `stored_md5` is the content of that file, if it exists.
pub fn filters_match_last_resync(rules: &str, stored_md5: Option<&str>) -> bool {
    use md5::{Digest, Md5};
    let current: String = Md5::digest(rules.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    stored_md5.is_some_and(|stored| stored.trim().eq_ignore_ascii_case(&current))
}

pub(crate) fn nfc(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfc().collect()
}

fn nfd(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfd().collect()
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
        assert!(check_local_dir(&default_local_dir(&home), &home).is_ok());
        assert!(
            check_local_dir(&home.join("DriveCraft"), &home).is_err(),
            "the parent folder of the account folders"
        );
        assert!(check_local_dir(&home.join("new/deeper/folder"), &home).is_ok());
        assert!(check_local_dir(&home, &home).is_err());
        assert!(check_local_dir(Path::new("/"), &home).is_err());
        assert!(check_local_dir(Path::new("relative"), &home).is_err());
        assert!(check_local_dir(&home.join("Library/CloudStorage/X"), &home).is_err());
        let others = [home.join("OneDrive")];
        let with = |d: &Path| check_local_dir_with(d, &home, &others);
        assert!(
            with(&home.join("OneDrive")).is_err(),
            "the folder of a different app"
        );
        assert!(
            with(&home.join("OneDrive/Sub")).is_err(),
            "inside that folder"
        );
        assert!(with(&home.join("Data")).is_ok());
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
    fn new_local_folders_are_top_level_folders_that_do_not_sync() {
        let dir = std::env::temp_dir().join("drivecraft-new-local-folders");
        let _ = fs::remove_dir_all(&dir);
        for d in [
            "Docs",
            "Photos/2026",
            "Mine",
            "Fresh",
            "Caf\u{65}\u{301}",
            ".hidden",
            "Kept here",
            "Leaving",
            "Coming",
        ] {
            fs::create_dir_all(dir.join(d)).unwrap();
        }
        fs::write(dir.join("file.txt"), "1").unwrap();
        let mut s = settings(&["Docs", "Photos/2026", "Leaving", "Caf\u{e9}"]);
        s.local_dir = dir.clone();
        s.local_only = ["Kept here".to_string(), "Gone".to_string()].into();
        s.set_keep("Leaving", false);
        s.set_keep("Coming", true);
        assert_eq!(s.new_local_folders(), vec!["Fresh", "Mine"]);

        assert!(s.prune_local_only());
        assert_eq!(s.local_only.iter().collect::<Vec<_>>(), vec!["Kept here"]);
        s.set_keep("Kept here", true);
        assert!(
            s.local_only.is_empty(),
            "a folder that syncs is not local only"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_match_in_nfc_and_nfd() {
        let s = settings(&["Caf\u{e9}"]);
        assert!(s.is_synced_path("Caf\u{65}\u{301}/menu.pdf"));
        let rules = s.filter_rules();
        assert!(rules.contains("+ /Caf\u{e9}/**\n"));
        assert!(rules.contains("+ /Caf\u{65}\u{301}/**\n"));
    }

    #[test]
    fn filters_match_the_md5_that_bisync_stores() {
        let rules = "+ /*\n- **\n";
        // The MD5 that rclone 1.76 stored for this file in a test resync.
        let stored = "229bc1c4fbb1f42fdf8fefdb146df559";
        assert!(filters_match_last_resync(rules, Some(stored)));
        assert!(filters_match_last_resync(
            rules,
            Some("229BC1C4FBB1F42FDF8FEFDB146DF559\n")
        ));
        assert!(!filters_match_last_resync(
            "+ /*\n+ /Docs/**\n- **\n",
            Some(stored)
        ));
        assert!(
            !filters_match_last_resync(rules, None),
            "no resync with a filter file yet"
        );
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
