//! The engine runs on its own thread. It owns rclone and the settings.
//! The UI sends `Command`s and receives `Event`s. The UI never calls rclone.

use std::fs;
use std::path::PathBuf;
use std::process::{Command as Process, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::rclone::{Rclone, find_binary};
use crate::settings::{Settings, check_local_dir, is_safe_relative};
use crate::snapshot::{ChangeKind, FileChange, Snapshot, diff};

#[derive(Debug)]
pub enum Command {
    RefreshRemotes,
    /// Runs `rclone config create <name> onedrive`. rclone opens the
    /// Microsoft login page in the browser.
    ConnectAccount {
        name: String,
    },
    SelectRemote(String),
    UpdatePrefs(Prefs),
    /// Lists the subfolders of a OneDrive folder. "" is the root.
    ListDir(String),
    SetKeep {
        path: String,
        keep: bool,
    },
    SyncNow,
    /// Reads the used and total storage of the OneDrive account.
    RefreshAbout,
    /// Makes the next sync a full resync. Use this after a sync error.
    ForceResync,
    Shutdown,
    #[doc(hidden)]
    AccountResult(Result<String, String>),
}

/// The settings that the user edits in the settings form.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    pub local_dir: PathBuf,
    pub sync_interval_minutes: u64,
}

impl Prefs {
    pub fn from_settings(s: &Settings) -> Prefs {
        Prefs {
            local_dir: s.local_dir.clone(),
            sync_interval_minutes: s.sync_interval_minutes,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Started {
        rclone_version: String,
        rclone_path: PathBuf,
    },
    /// rclone cannot start. The engine stops.
    Fatal(String),
    Settings(Settings),
    Remotes(Vec<String>),
    AccountConnecting,
    Dir {
        path: String,
        result: Result<Vec<String>, String>,
    },
    SyncProgress {
        text: String,
        fraction: Option<f32>,
    },
    /// The storage of the OneDrive account, in bytes.
    About {
        used: Option<u64>,
        total: Option<u64>,
    },
    /// The files that one sync changed, sorted by path.
    FilesChanged(Vec<FileChange>),
    SyncFinished {
        ok: bool,
        message: String,
    },
    Log(String),
    Error(String),
}

pub type Notify = Arc<dyn Fn() + Send + Sync>;

pub struct EngineHandle {
    pub tx: Sender<Command>,
    pub rx: Receiver<Event>,
    thread: Option<thread::JoinHandle<()>>,
}

impl EngineHandle {
    /// A handle with no engine behind it, for UI previews and tests.
    /// Commands go nowhere. `events` sends events to the UI.
    pub fn detached() -> (EngineHandle, Sender<Event>) {
        let (tx, _rx) = channel();
        let (events, rx) = channel();
        (
            EngineHandle {
                tx,
                rx,
                thread: None,
            },
            events,
        )
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    /// Stops the engine. This stops a running sync and rclone.
    pub fn shutdown(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Starts the engine thread. `notify` is called after each event,
/// so that the UI can repaint. `build_rclone` is the rclone binary that
/// the build downloaded. See `rclone::find_binary`.
pub fn spawn(notify: Notify, build_rclone: Option<PathBuf>) -> EngineHandle {
    let (cmd_tx, cmd_rx) = channel();
    let (ev_tx, ev_rx) = channel();
    let self_tx = cmd_tx.clone();
    let thread = thread::Builder::new()
        .name("drivecraft-engine".into())
        .spawn(move || {
            let emit = Emitter { tx: ev_tx, notify };
            match Engine::start(emit.clone(), self_tx, build_rclone) {
                Ok(mut engine) => engine.run(cmd_rx),
                Err(e) => emit.send(Event::Fatal(format!("{e:#}"))),
            }
        })
        .expect("cannot start the engine thread");
    EngineHandle {
        tx: cmd_tx,
        rx: ev_rx,
        thread: Some(thread),
    }
}

#[derive(Clone)]
struct Emitter {
    tx: Sender<Event>,
    notify: Notify,
}

impl Emitter {
    fn send(&self, ev: Event) {
        let _ = self.tx.send(ev);
        (self.notify)();
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// A normal sync before pending switch changes are applied, so that no
    /// local change is lost when a folder no longer syncs.
    Pre,
    Main {
        resync: bool,
    },
}

struct Job {
    id: i64,
    phase: Phase,
}

struct Engine {
    rc: Rclone,
    binary: PathBuf,
    settings: Settings,
    emit: Emitter,
    self_tx: Sender<Command>,
    job: Option<Job>,
    /// The local files after the last successful sync.
    last_files: Option<Snapshot>,
    /// The local files when the running job started.
    files_before: Option<Snapshot>,
    /// True when this sync already reported the local changes (uploads).
    uploads_reported: bool,
    /// The number of files that this sync changed.
    changed_count: usize,
    last_sync: Option<Instant>,
    /// False after a failed sync. Then pending changes wait for the interval.
    last_ok: bool,
}

impl Engine {
    fn start(
        emit: Emitter,
        self_tx: Sender<Command>,
        build_rclone: Option<PathBuf>,
    ) -> Result<Engine> {
        let settings = Settings::load()?;
        fs::create_dir_all(Settings::dir())?;
        let binary = find_binary(&settings.rclone_path, build_rclone.as_deref())?;
        let rc = Rclone::start(&binary, &Settings::log_file(), &Settings::session_file())?;
        let version = rc.version()?;
        let last_files = Snapshot::load(&Settings::snapshot_file());
        emit.send(Event::Started {
            rclone_version: version,
            rclone_path: binary.clone(),
        });
        Ok(Engine {
            rc,
            binary,
            settings,
            emit,
            self_tx,
            job: None,
            last_files,
            files_before: None,
            uploads_reported: false,
            changed_count: 0,
            last_sync: None,
            last_ok: true,
        })
    }

    fn run(&mut self, rx: Receiver<Command>) {
        self.emit.send(Event::Settings(self.settings.clone()));
        self.refresh_remotes();
        if self.settings.remote.is_some() {
            self.refresh_about();
        }
        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.tick();
        }
        if let Some(job) = &self.job {
            let _ = self.rc.call("job/stop", json!({ "jobid": job.id }));
        }
        // Dropping `self.rc` stops rclone.
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::RefreshRemotes => self.refresh_remotes(),
            Command::ConnectAccount { name } => self.connect_account(name),
            Command::AccountResult(Ok(name)) => {
                self.log(format!("The account \"{name}\" is connected."));
                self.refresh_remotes();
                self.handle(Command::SelectRemote(name));
            }
            Command::AccountResult(Err(e)) => {
                self.error(format!("Cannot connect the account: {e}"))
            }
            Command::SelectRemote(name) => self.report(|s| s.select_remote(name)),
            Command::UpdatePrefs(p) => self.report(|s| s.update_prefs(p)),
            Command::ListDir(path) => {
                let result = self.list_dir(&path).map_err(|e| format!("{e:#}"));
                self.emit.send(Event::Dir { path, result });
            }
            Command::SetKeep { path, keep } => {
                self.settings.set_keep(&path, keep);
                self.save();
                self.start_sync();
            }
            Command::SyncNow => self.start_sync(),
            Command::RefreshAbout => self.refresh_about(),
            Command::ForceResync => {
                self.settings.needs_resync = true;
                self.save();
                self.start_sync();
            }
            Command::Shutdown => {}
        }
    }

    // ----- account -----

    fn refresh_remotes(&mut self) {
        match self.rc.call("config/listremotes", json!({})) {
            Ok(v) => {
                let remotes = v["remotes"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|r| r.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                self.emit.send(Event::Remotes(remotes));
            }
            Err(e) => self.error(format!("{e:#}")),
        }
    }

    fn connect_account(&mut self, name: String) {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || "-_ ".contains(c))
        {
            self.error(
                "Use only letters, digits, spaces, \"-\" and \"_\" in the account name.".into(),
            );
            return;
        }
        self.emit.send(Event::AccountConnecting);
        self.log("Log in to Microsoft in the browser window that opens.".into());
        let binary = self.binary.clone();
        let tx = self.self_tx.clone();
        // The login can take minutes, so it runs on its own thread.
        thread::spawn(move || {
            let result = Process::new(&binary)
                .args(["config", "create", &name, "onedrive"])
                .stdin(Stdio::null())
                .output()
                .map_err(|e| e.to_string())
                .and_then(|out| {
                    if out.status.success() {
                        Ok(name)
                    } else {
                        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
                    }
                });
            let _ = tx.send(Command::AccountResult(result));
        });
    }

    fn select_remote(&mut self, name: String) -> Result<()> {
        if self.settings.remote.as_deref() == Some(name.as_str()) {
            return Ok(());
        }
        if self.job.is_some() {
            bail!("Wait until the sync is complete, then change the account.");
        }
        // The folder selection belongs to the old account. A new account
        // starts with all top-level folders on, like the official client.
        self.settings.remote = Some(name);
        self.settings.pinned.clear();
        self.settings.pending_pin.clear();
        self.settings.pending_unpin.clear();
        self.settings.needs_resync = true;
        self.forget_files();
        match self.list_dir("") {
            Ok(top) => self.settings.pending_pin.extend(top),
            Err(e) => self.error(format!("Cannot list the OneDrive folders: {e:#}")),
        }
        self.save();
        self.refresh_about();
        Ok(())
    }

    fn update_prefs(&mut self, p: Prefs) -> Result<()> {
        if self.settings.local_dir != p.local_dir {
            if self.job.is_some() {
                bail!("Wait until the sync is complete, then change the folder.");
            }
            check_local_dir(&p.local_dir, &home())?;
            // The old folder stays as it is. The next sync fills the new folder.
            self.settings.local_dir = p.local_dir;
            self.settings.needs_resync = true;
            self.forget_files();
            self.log(format!(
                "The OneDrive folder is now {}.",
                self.settings.local_dir.display()
            ));
        }
        self.settings.sync_interval_minutes = p.sync_interval_minutes.max(1);
        self.save();
        Ok(())
    }

    fn refresh_about(&mut self) {
        let Ok(fs_name) = self.remote_fs() else {
            return;
        };
        match self.rc.call("operations/about", json!({ "fs": fs_name })) {
            Ok(v) => self.emit.send(Event::About {
                used: v["used"].as_u64(),
                total: v["total"].as_u64(),
            }),
            Err(e) => self.log(format!("Cannot read the storage use: {e:#}")),
        }
    }

    fn remote_fs(&self) -> Result<String> {
        match &self.settings.remote {
            Some(r) => Ok(format!("{r}:")),
            None => bail!("Connect a OneDrive account first."),
        }
    }

    // ----- folders -----

    fn list_dir(&self, path: &str) -> Result<Vec<String>> {
        let v = self.rc.call(
            "operations/list",
            json!({ "fs": self.remote_fs()?, "remote": path, "opt": { "dirsOnly": true } }),
        )?;
        let mut names: Vec<String> = v["list"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| e["Name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        names.sort_by_key(|n| n.to_lowercase());
        Ok(names)
    }

    // ----- sync -----

    fn tick(&mut self) {
        if self.job.is_some() {
            self.poll_job();
            return;
        }
        let interval = Duration::from_secs(self.settings.sync_interval_minutes.max(1) * 60);
        let due = self.last_sync.is_none_or(|t| t.elapsed() >= interval)
            || (self.settings.has_pending() && self.last_ok);
        if due && self.settings.remote.is_some() {
            self.start_sync();
        }
    }

    fn start_sync(&mut self) {
        if self.job.is_some() {
            return; // The running job sees the new settings when it ends.
        }
        let s = &self.settings;
        // Before a switch change, sync once with the old selection, so that no
        // local change is lost. Without a previous sync (needs_resync) this is
        // only needed when a folder gets removed from this computer.
        let phase = if s.has_pending() && (!s.needs_resync || !s.pending_unpin.is_empty()) {
            Phase::Pre
        } else {
            if s.has_pending() {
                self.apply_pending();
            }
            Phase::Main {
                resync: self.settings.needs_resync,
            }
        };
        self.uploads_reported = false;
        self.changed_count = 0;
        if let Err(e) = self.start_bisync(phase) {
            self.last_sync = Some(Instant::now());
            self.finish(false, format!("{e:#}"));
        }
    }

    fn start_bisync(&mut self, phase: Phase) -> Result<()> {
        let resync = match phase {
            Phase::Pre => self.settings.needs_resync,
            Phase::Main { resync } => resync,
        };
        let s = &self.settings;
        check_local_dir(&s.local_dir, &home())?;
        if !s.local_dir.is_dir() {
            // After a previous sync, a missing folder was moved, renamed, or
            // deleted. A new empty folder would make bisync delete all files
            // on OneDrive. A resync never deletes, so it can create the folder.
            if !resync {
                bail!(
                    "The OneDrive folder {} is missing. If you moved or renamed it, \
                     select it again in Settings. DriveCraft does not sync until then.",
                    s.local_dir.display()
                );
            }
            fs::create_dir_all(&s.local_dir)
                .with_context(|| format!("cannot create {}", s.local_dir.display()))?;
        }
        fs::write(Settings::filters_file(), s.filter_rules())?;
        self.files_before = Some(self.take_files());
        let s = &self.settings;
        let v = self.rc.call(
            "sync/bisync",
            json!({
                "path1": s.local_dir,
                "path2": self.remote_fs()?,
                "filtersFile": Settings::filters_file(),
                "resync": resync,
                "_async": true,
            }),
        )?;
        let id = v["jobid"]
            .as_i64()
            .context("rclone did not return a job id")?;
        self.job = Some(Job { id, phase });
        let text = if resync {
            "Preparing a full sync…"
        } else {
            "Checking for changes…"
        };
        self.emit.send(Event::SyncProgress {
            text: text.into(),
            fraction: None,
        });
        Ok(())
    }

    fn poll_job(&mut self) {
        let Some(job) = &self.job else { return };
        let (id, phase) = (job.id, job.phase);
        let status = match self.rc.call("job/status", json!({ "jobid": id })) {
            Ok(v) => v,
            Err(e) => {
                self.job = None;
                self.finish(false, format!("{e:#}"));
                return;
            }
        };
        if !status["finished"].as_bool().unwrap_or(false) {
            if let Ok(st) = self
                .rc
                .call("core/stats", json!({ "group": format!("job/{id}") }))
            {
                self.emit.send(progress(&st));
            }
            return;
        }
        self.job = None;
        self.last_sync = Some(Instant::now());
        if !status["success"].as_bool().unwrap_or(false) {
            self.files_before = None;
            let err = status["error"].as_str().unwrap_or("unknown error");
            self.finish(false, format!("The sync failed: {err}"));
            return;
        }
        self.report_changes();
        match phase {
            Phase::Pre => {
                self.settings.needs_resync = false;
                self.apply_pending();
                let resync = self.settings.needs_resync;
                if let Err(e) = self.start_bisync(Phase::Main { resync }) {
                    self.finish(false, format!("{e:#}"));
                }
            }
            Phase::Main { resync } => {
                if resync {
                    self.settings.needs_resync = false;
                    self.save();
                }
                let message = match self.changed_count {
                    0 => "The sync is complete. Nothing changed.".to_string(),
                    1 => "The sync is complete. 1 file changed.".to_string(),
                    n => format!("The sync is complete. {n} files changed."),
                };
                self.finish(true, message);
            }
        }
    }

    /// Applies the switch changes. The local copies of folders that no longer
    /// sync are deleted. A successful sync ran before this, so the files are
    /// also on OneDrive.
    fn apply_pending(&mut self) {
        for path in self.settings.apply_pending() {
            if !is_safe_relative(&path) {
                continue;
            }
            let local = self.settings.local_dir.join(&path);
            if local.exists() {
                match fs::remove_dir_all(&local) {
                    Ok(()) => self.log(format!("\"{path}\" is now only on OneDrive.")),
                    Err(e) => self.error(format!("Cannot remove {}: {e}", local.display())),
                }
            }
        }
        self.save();
    }

    // ----- file changes -----

    fn take_files(&self) -> Snapshot {
        let s = &self.settings;
        Snapshot::take(
            s.remote.as_deref().unwrap_or_default(),
            &s.local_dir,
            |rel| s.is_synced_path(rel),
        )
    }

    /// Reports the files that the finished job changed, and keeps the new
    /// list for the next sync.
    fn report_changes(&mut self) {
        let Some(before) = self.files_before.take() else {
            return;
        };
        let after = self.take_files();
        let mut changes = Vec::new();
        // The local changes since the last sync. Only the first job of a
        // sync reports them; a later job uses a new folder selection.
        if !self.uploads_reported {
            self.uploads_reported = true;
            if let Some(last) = &self.last_files
                && last.matches(&before.remote, &before.local_dir)
            {
                changes.extend(diff(
                    last,
                    &before,
                    ChangeKind::Uploaded,
                    ChangeKind::DeletedOnline,
                ));
            }
        }
        changes.extend(diff(
            &before,
            &after,
            ChangeKind::Downloaded,
            ChangeKind::DeletedHere,
        ));
        if let Err(e) = after.save(&Settings::snapshot_file()) {
            self.error(format!("Cannot save the file list: {e:#}"));
        }
        self.last_files = Some(after);
        if !changes.is_empty() {
            self.changed_count += changes.len();
            self.emit.send(Event::FilesChanged(changes));
        }
    }

    /// The account or the folder changed, so the old file list is not valid.
    fn forget_files(&mut self) {
        self.last_files = None;
        let _ = fs::remove_file(Settings::snapshot_file());
    }

    // ----- helpers -----

    fn finish(&mut self, ok: bool, message: String) {
        self.last_ok = ok;
        self.emit.send(Event::SyncFinished { ok, message });
        self.emit.send(Event::Settings(self.settings.clone()));
        if ok {
            self.refresh_about();
        }
    }

    fn save(&mut self) {
        if let Err(e) = self.settings.save() {
            self.error(format!("Cannot save the settings: {e:#}"));
        }
        self.emit.send(Event::Settings(self.settings.clone()));
    }

    fn report(&mut self, f: impl FnOnce(&mut Self) -> Result<()>) {
        if let Err(e) = f(self) {
            self.error(format!("{e:#}"));
        }
    }

    fn log(&self, msg: String) {
        self.emit.send(Event::Log(msg));
    }

    fn error(&self, msg: String) {
        self.emit.send(Event::Error(msg));
    }
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

fn progress(st: &Value) -> Event {
    let n = |k: &str| st[k].as_u64().unwrap_or(0);
    let (done, total) = (n("bytes"), n("totalBytes"));
    let files = n("totalTransfers");
    let text = if files == 0 {
        "Checking for changes…".to_string()
    } else {
        format!(
            "{} of {} files · {:.1} of {:.1} MB",
            n("transfers"),
            files,
            done as f64 / 1e6,
            total as f64 / 1e6
        )
    };
    let fraction = (total > 0).then(|| (done as f64 / total as f64).clamp(0.0, 1.0) as f32);
    Event::SyncProgress { text, fraction }
}
