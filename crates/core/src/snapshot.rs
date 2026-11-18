//! Lists of the synced local files, to show which files a sync changed.
//!
//! rclone records copies with their direction, but not on which side a
//! delete happened. So DriveCraft compares lists of the local files:
//!
//! - your changes: the list after the last sync against the list before
//!   this sync (uploaded, or deleted from OneDrive);
//! - changes from OneDrive: the lists before and after this sync
//!   (downloaded, or deleted on this computer).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Size, modification seconds, and nanoseconds of one file.
type Stamp = (u64, i64, u32);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub remote: String,
    pub local_dir: PathBuf,
    /// Paths relative to `local_dir`, with "/" as the separator.
    pub files: HashMap<String, Stamp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    /// From OneDrive to this computer: new or changed.
    Downloaded,
    /// From this computer to OneDrive: new or changed.
    Uploaded,
    /// Deleted on this computer, because it was deleted on OneDrive.
    DeletedHere,
    /// Deleted from OneDrive, because it was deleted on this computer.
    DeletedOnline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub kind: ChangeKind,
    pub path: String,
}

/// System files that DriveCraft never syncs. The filter rules exclude them too.
pub const JUNK: &[&str] = &[
    ".DS_Store",
    "._*",
    "Thumbs.db",
    "desktop.ini",
    "~$*",
    ".~lock.*",
];

pub fn is_junk(name: &str) -> bool {
    JUNK.iter().any(|pattern| match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == *pattern,
    })
}

impl Snapshot {
    /// Lists the files under `local_dir` for which `include` is true.
    /// Symbolic links and system files are not listed (rclone skips them too).
    pub fn take(remote: &str, local_dir: &Path, include: impl Fn(&str) -> bool) -> Snapshot {
        let mut files = HashMap::new();
        walk(local_dir, "", &include, &mut files);
        Snapshot {
            remote: remote.to_string(),
            local_dir: local_dir.to_path_buf(),
            files,
        }
    }

    pub fn load(path: &Path) -> Option<Snapshot> {
        serde_json::from_slice(&fs::read(path).ok()?).ok()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(self)?)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// True when this list belongs to the same account and folder.
    pub fn matches(&self, remote: &str, local_dir: &Path) -> bool {
        self.remote == remote && self.local_dir == local_dir
    }
}

fn walk(
    dir: &Path,
    prefix: &str,
    include: &impl Fn(&str) -> bool,
    out: &mut HashMap<String, Stamp>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_junk(&name) {
            continue;
        }
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() {
            walk(&entry.path(), &rel, include, out);
        } else if meta.is_file() && include(&rel) {
            let (secs, nanos) = match meta.modified().map(|t| t.duration_since(UNIX_EPOCH)) {
                Ok(Ok(d)) => (d.as_secs() as i64, d.subsec_nanos()),
                Ok(Err(e)) => (
                    -(e.duration().as_secs() as i64),
                    e.duration().subsec_nanos(),
                ),
                Err(_) => (0, 0),
            };
            out.insert(rel, (meta.len(), secs, nanos));
        }
    }
}

/// The files that are new or changed in `new`, and the files that are not
/// in `new` any more. The result is sorted by path.
pub fn diff(
    old: &Snapshot,
    new: &Snapshot,
    changed: ChangeKind,
    removed: ChangeKind,
) -> Vec<FileChange> {
    let mut out: Vec<FileChange> = new
        .files
        .iter()
        .filter(|(path, stamp)| old.files.get(*path) != Some(stamp))
        .map(|(path, _)| FileChange {
            kind: changed,
            path: path.clone(),
        })
        .chain(
            old.files
                .keys()
                .filter(|path| !new.files.contains_key(*path))
                .map(|path| FileChange {
                    kind: removed,
                    path: path.clone(),
                }),
        )
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Only the files that are in `new` and not in `old`, sorted by path.
pub fn added(old: &Snapshot, new: &Snapshot, kind: ChangeKind) -> Vec<FileChange> {
    let mut out: Vec<FileChange> = new
        .files
        .keys()
        .filter(|path| !old.files.contains_key(*path))
        .map(|path| FileChange {
            kind,
            path: path.clone(),
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(files: &[(&str, u64)]) -> Snapshot {
        Snapshot {
            files: files
                .iter()
                .map(|(p, s)| (p.to_string(), (*s, 0, 0)))
                .collect(),
            ..Snapshot::default()
        }
    }

    #[test]
    fn diff_finds_new_changed_and_removed_files() {
        let old = snap(&[("a.txt", 1), ("b.txt", 2), ("c.txt", 3)]);
        let new = snap(&[("a.txt", 1), ("b.txt", 20), ("d.txt", 4)]);
        let changes = diff(&old, &new, ChangeKind::Downloaded, ChangeKind::DeletedHere);
        assert_eq!(
            changes,
            vec![
                FileChange {
                    kind: ChangeKind::Downloaded,
                    path: "b.txt".into()
                },
                FileChange {
                    kind: ChangeKind::DeletedHere,
                    path: "c.txt".into()
                },
                FileChange {
                    kind: ChangeKind::Downloaded,
                    path: "d.txt".into()
                },
            ]
        );
    }

    #[test]
    fn added_ignores_changed_and_removed_files() {
        let old = snap(&[("a.txt", 1), ("b.txt", 2)]);
        let new = snap(&[("a.txt", 10), ("New/c.txt", 3)]);
        assert_eq!(
            added(&old, &new, ChangeKind::Uploaded),
            vec![FileChange {
                kind: ChangeKind::Uploaded,
                path: "New/c.txt".into()
            }]
        );
    }

    #[test]
    fn junk_names_are_recognized() {
        for name in [
            ".DS_Store",
            "._photo.jpg",
            "Thumbs.db",
            "desktop.ini",
            "~$report.docx",
            ".~lock.a.odt#",
        ] {
            assert!(is_junk(name), "{name}");
        }
        for name in ["photo.jpg", "DS_Store", "report.docx"] {
            assert!(!is_junk(name), "{name}");
        }
    }

    #[test]
    fn take_lists_included_files_and_skips_junk() {
        let dir = std::env::temp_dir().join("drivecraft-snapshot-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Docs/Sub")).unwrap();
        fs::create_dir_all(dir.join("Other")).unwrap();
        fs::write(dir.join("root.txt"), "1").unwrap();
        fs::write(dir.join("Docs/Sub/a.txt"), "22").unwrap();
        fs::write(dir.join("Docs/.DS_Store"), "x").unwrap();
        fs::write(dir.join("Other/b.txt"), "3").unwrap();
        let s = Snapshot::take("r", &dir, |p| !p.contains('/') || p.starts_with("Docs/"));
        let mut paths: Vec<_> = s.files.keys().cloned().collect();
        paths.sort();
        assert_eq!(paths, vec!["Docs/Sub/a.txt", "root.txt"]);
        assert_eq!(s.files["Docs/Sub/a.txt"].0, 2);
        let _ = fs::remove_dir_all(&dir);
    }
}
