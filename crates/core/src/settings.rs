//! User settings and the selective sync rules.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// The app-wide settings, and the accounts.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// An rclone binary to use instead of the bundled one, for tests only.
    /// Empty means the bundled binary.
    pub rclone_path: String,
    pub sync_interval_minutes: u64,
    /// Download files that the provider blocks as infected with a virus
    /// (OneDrive `av_override`, Google Drive `acknowledge_abuse`).
    pub download_blocked_files: bool,
    /// Starts DriveCraft when the user signs in to the computer. The app
    /// registers itself only while at least one account exists. Settings
    /// from an older version get the default, true.
    pub start_at_login: bool,
    /// The accounts, in the order of the sidebar.
    pub accounts: Vec<Account>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            rclone_path: String::new(),
            sync_interval_minutes: 10,
            download_blocked_files: false,
            start_at_login: true,
            accounts: Vec::new(),
        }
    }
}

/// One cloud account with its own local folder and folder selection.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Account {
    /// The rclone remote, without the colon. It never changes, and it names
    /// the files of the account in the settings folder.
    pub id: String,
    /// The name that the user sees, for example "Personal".
    pub name: String,
    /// The rclone backend, for example "onedrive".
    pub provider: String,
    /// The local folder. It holds real copies of the synced folders and of
    /// the files in the root of the account.
    pub local_dir: PathBuf,
    /// The folders that sync to this computer, as paths from the root.
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
    /// The files that the provider blocked. While the app-wide switch
    /// `download_blocked_files` is off, the filter rules exclude them, so a
    /// sync does not stop at them again.
    pub blocked_files: BTreeSet<String>,
}

impl Default for Account {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Account {
            id: String::new(),
            name: String::new(),
            provider: "onedrive".into(),
            local_dir: accounts_root(&home).join("OneDrive"),
            pinned: BTreeSet::new(),
            pending_pin: BTreeSet::new(),
            pending_unpin: BTreeSet::new(),
            local_only: BTreeSet::new(),
            needs_resync: true,
            awaiting_selection: false,
            blocked_files: BTreeSet::new(),
        }
    }
}

/// The settings before more accounts: the one account was at the top level.
#[derive(Deserialize, Default)]
#[serde(default)]
struct OneAccountSettings {
    remote: Option<String>,
    local_dir: Option<PathBuf>,
    pinned: BTreeSet<String>,
    pending_pin: BTreeSet<String>,
    pending_unpin: BTreeSet<String>,
    local_only: BTreeSet<String>,
    needs_resync: Option<bool>,
    awaiting_selection: bool,
    blocked_files: BTreeSet<String>,
}

/// The parent folder of the account folders: `~/DriveCraft`. No other
/// client uses this name, so the default folders never collide with
/// them. Each account gets its own folder in it, for example
/// `~/DriveCraft/OneDrive`. The parent folder itself does not sync.
pub fn accounts_root(home: &Path) -> PathBuf {
    home.join("DriveCraft")
}

/// The folder for a new account: `~/DriveCraft/<name>`. If that folder is
/// in use (on disk or by an account), the name gets a number: "Work 2".
pub fn new_account_dir(home: &Path, name: &str, taken: &[PathBuf]) -> PathBuf {
    let root = accounts_root(home);
    (1..)
        .map(|n| match n {
            1 => root.join(name),
            n => root.join(format!("{name} {n}")),
        })
        .find(|dir| !taken.contains(dir) && fs::symlink_metadata(dir).is_err())
        .expect("an unused folder name exists")
}

/// Refuses local folders that must not hold a synced copy. `other_accounts`
/// are the folders of the other DriveCraft accounts.
pub fn check_local_dir(dir: &Path, home: &Path, other_accounts: &[PathBuf]) -> Result<()> {
    check_local_dir_with(
        dir,
        home,
        &crate::other_clients::folders(home),
        other_accounts,
    )
}

/// `check_local_dir` with the folders of other clients as a parameter.
fn check_local_dir_with(
    dir: &Path,
    home: &Path,
    other_clients: &[PathBuf],
    other_accounts: &[PathBuf],
) -> Result<()> {
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
            accounts_root(home).join("OneDrive").display()
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
                "A different cloud app syncs {}. Two apps must not sync the same folder. \
                 Select a different folder, or sign out of the other app first.",
                other.display()
            );
        }
    }
    for other in other_accounts {
        let other_real = resolve(other);
        if real.starts_with(&other_real) || other_real.starts_with(&real) {
            bail!(
                "A different account syncs {}. Each account needs a folder of its own, \
                 and one folder must not be inside the other.",
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

    /// The filter rules of one account.
    pub fn filters_file(account: &str) -> PathBuf {
        Self::dir().join(format!("filters-{account}.txt"))
    }

    pub fn session_file() -> PathBuf {
        Self::dir().join("rclone-session.json")
    }

    /// The list of the synced local files of one account after its last sync.
    pub fn snapshot_file(account: &str) -> PathBuf {
        Self::dir().join(format!("snapshot-{account}.json"))
    }

    /// The rclone configuration file of DriveCraft. It holds the accounts.
    /// DriveCraft does not use the default file of the user's own rclone.
    pub fn rclone_config_file() -> PathBuf {
        Self::dir().join("rclone.conf")
    }

    pub fn log_file() -> PathBuf {
        Self::dir().join("rclone.log")
    }

    /// Loads the settings. Settings from before more accounts become one
    /// account, and the files of that account get their new names.
    pub fn load() -> Result<Settings> {
        let path = Self::file();
        if !path.exists() {
            return Ok(Settings::default());
        }
        let text = fs::read_to_string(&path)?;
        let (settings, migrated) =
            Self::parse(&text).with_context(|| format!("cannot read {}", path.display()))?;
        if let Some(id) = migrated {
            rename_one_account_files(&Self::dir(), &id);
            settings.save()?;
        }
        Ok(settings)
    }

    /// Parses settings.toml. Returns the id of the account that it made from
    /// settings with one account at the top level, if any.
    fn parse(text: &str) -> Result<(Settings, Option<String>)> {
        let mut settings: Settings = toml::from_str(text)?;
        if !settings.accounts.is_empty() {
            return Ok((settings, None));
        }
        let old: OneAccountSettings = toml::from_str(text)?;
        let Some(id) = old.remote else {
            return Ok((settings, None));
        };
        let mut account = Account {
            id: id.clone(),
            name: "OneDrive".into(),
            provider: "onedrive".into(),
            pinned: old.pinned,
            pending_pin: old.pending_pin,
            pending_unpin: old.pending_unpin,
            local_only: old.local_only,
            needs_resync: old.needs_resync.unwrap_or(true),
            awaiting_selection: old.awaiting_selection,
            blocked_files: old.blocked_files,
            ..Account::default()
        };
        if let Some(dir) = old.local_dir {
            account.local_dir = dir;
        }
        settings.accounts.push(account);
        Ok((settings, Some(id)))
    }

    pub fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn account_mut(&mut self, id: &str) -> Option<&mut Account> {
        self.accounts.iter_mut().find(|a| a.id == id)
    }

    /// The local folders of all accounts except `id`.
    pub fn other_account_dirs(&self, id: &str) -> Vec<PathBuf> {
        self.accounts
            .iter()
            .filter(|a| a.id != id)
            .map(|a| a.local_dir.clone())
            .collect()
    }

    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(Self::dir())?;
        let tmp = Self::file().with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        fs::rename(&tmp, Self::file())?;
        Ok(())
    }
}

impl Account {
    /// The rclone path of the account root, for example "onedrive:".
    pub fn remote_fs(&self) -> String {
        format!("{}:", self.id)
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

    /// The rclone filter rules: no system files, no blocked files (unless
    /// `download_blocked_files`), then the files in the root and the
    /// synced folders.
    pub fn filter_rules(&self, download_blocked_files: bool) -> String {
        let mut out = String::from("# Generated by DriveCraft. Do not edit.\n");
        for junk in crate::snapshot::JUNK {
            out.push_str(&format!("- {junk}\n"));
        }
        if !download_blocked_files {
            for path in &self.blocked_files {
                let mut forms = vec![nfc(path), nfd(path)];
                forms.dedup();
                for form in forms {
                    out.push_str(&format!("- /{}\n", escape_glob(&form)));
                }
            }
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

/// Gives the files of a settings folder from before more accounts the names
/// of account `id`. The MD5 of the filter rules moves with them, so the next
/// sync needs no resync. A file that has its new name already stays.
fn rename_one_account_files(dir: &Path, id: &str) {
    for (old, new) in [
        ("filters.txt".to_string(), format!("filters-{id}.txt")),
        (
            "filters.txt.md5".to_string(),
            format!("filters-{id}.txt.md5"),
        ),
        ("snapshot.json".to_string(), format!("snapshot-{id}.json")),
    ] {
        let (old, new) = (dir.join(old), dir.join(new));
        if old.exists() && !new.exists() {
            let _ = fs::rename(old, new);
        }
    }
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

    fn settings(pinned: &[&str]) -> Account {
        Account {
            id: "onedrive".into(),
            pinned: pinned.iter().map(|s| s.to_string()).collect(),
            ..Account::default()
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
            s.filter_rules(false),
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

        let check = |d: &Path| check_local_dir(d, &home, &[]);
        assert!(check(&home.join("Synced")).is_ok());
        assert!(check(&home.join("DriveCraft/OneDrive")).is_ok());
        assert!(
            check(&home.join("DriveCraft")).is_err(),
            "the parent folder of the account folders"
        );
        assert!(check(&home.join("new/deeper/folder")).is_ok());
        assert!(check(&home).is_err());
        assert!(check(Path::new("/")).is_err());
        assert!(check(Path::new("relative")).is_err());
        assert!(check(&home.join("Library/CloudStorage/X")).is_err());
        let accounts = [home.join("DriveCraft/Work")];
        let with_account = |d: &Path| check_local_dir(d, &home, &accounts);
        assert!(
            with_account(&home.join("DriveCraft/Work")).is_err(),
            "same folder"
        );
        assert!(
            with_account(&home.join("DriveCraft/Work/Sub")).is_err(),
            "inside"
        );
        assert!(with_account(&home.join("DriveCraft/Personal")).is_ok());
        let others = [home.join("OneDrive")];
        let with = |d: &Path| check_local_dir_with(d, &home, &others, &[]);
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
            check(&link.join("sub")).is_err(),
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
        let rules = s.filter_rules(false);
        assert!(rules.contains("+ /Caf\u{e9}/**\n"));
        assert!(rules.contains("+ /Caf\u{65}\u{301}/**\n"));
    }

    #[test]
    fn blocked_files_are_excluded_until_the_switch_is_on() {
        let mut s = settings(&[]);
        s.blocked_files = ["Mail/backup [1].tar.gz".to_string()].into();
        let rules = s.filter_rules(false);
        let exclude = "- /Mail/backup \\[1\\].tar.gz\n";
        assert!(rules.contains(exclude), "{rules}");
        assert!(
            rules.find(exclude) < rules.find("+ /*"),
            "the exclusion comes first"
        );
        assert!(!s.filter_rules(true).contains("backup"));
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
    fn start_at_login_is_on_unless_the_file_turns_it_off() {
        let (old, _) = Settings::parse("sync_interval_minutes = 10\n").unwrap();
        assert!(old.start_at_login, "settings of an older version");
        let (off, _) = Settings::parse("start_at_login = false\n").unwrap();
        assert!(!off.start_at_login);
    }

    #[test]
    fn settings_with_one_account_become_an_account_list() {
        let old = "rclone_path = \"\"\nremote = \"onedrive\"\n\
                   local_dir = \"/Users/demo/DriveCraft/OneDrive\"\n\
                   sync_interval_minutes = 15\npinned = [\"Docs\"]\n\
                   needs_resync = false\ndownload_blocked_files = true\n\
                   blocked_files = [\"Mail/x.tar.gz\"]\n";
        let (s, migrated) = Settings::parse(old).unwrap();
        assert_eq!(migrated.as_deref(), Some("onedrive"));
        assert_eq!(s.sync_interval_minutes, 15);
        assert!(s.download_blocked_files);
        assert_eq!(s.accounts.len(), 1);
        let a = &s.accounts[0];
        assert_eq!((a.id.as_str(), a.name.as_str()), ("onedrive", "OneDrive"));
        assert_eq!(
            a.local_dir,
            PathBuf::from("/Users/demo/DriveCraft/OneDrive")
        );
        assert_eq!(a.pinned.iter().collect::<Vec<_>>(), vec!["Docs"]);
        assert!(!a.needs_resync, "no new full sync");
        assert!(a.blocked_files.contains("Mail/x.tar.gz"));

        // The new format loads as it is.
        let text = toml::to_string_pretty(&s).unwrap();
        let (again, migrated) = Settings::parse(&text).unwrap();
        assert_eq!((again, migrated), (s, None));
        // No account at all: nothing to migrate.
        assert_eq!(Settings::parse("").unwrap(), (Settings::default(), None));
    }

    #[test]
    fn one_account_files_get_account_names() {
        let dir = std::env::temp_dir().join("drivecraft-migrate-files");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for f in ["filters.txt", "filters.txt.md5", "snapshot.json"] {
            fs::write(dir.join(f), f).unwrap();
        }
        rename_one_account_files(&dir, "onedrive");
        for (f, content) in [
            ("filters-onedrive.txt", "filters.txt"),
            ("filters-onedrive.txt.md5", "filters.txt.md5"),
            ("snapshot-onedrive.json", "snapshot.json"),
        ] {
            assert_eq!(fs::read_to_string(dir.join(f)).unwrap(), content);
        }
        assert!(!dir.join("filters.txt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_account_dirs_do_not_collide() {
        let home = std::env::temp_dir().join("drivecraft-new-account-dir");
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(home.join("DriveCraft/Work")).unwrap();
        let taken = [home.join("DriveCraft/Work 2")];
        assert_eq!(
            new_account_dir(&home, "Work", &taken),
            home.join("DriveCraft/Work 3")
        );
        assert_eq!(
            new_account_dir(&home, "Personal", &taken),
            home.join("DriveCraft/Personal")
        );
        let _ = fs::remove_dir_all(&home);
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
