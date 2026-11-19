//! The engine runs on its own thread. It owns rclone and the settings.
//! The UI sends `Command`s and receives `Event`s. The UI never calls rclone.
//!
//! Each account syncs on its own, but only one sync runs at a time: the
//! accounts take turns.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::providers::{SignIn, provider_label};
use crate::rclone::{
    Rclone, command as rclone_command, config_section, default_config_file, find_binary,
    write_private,
};
use crate::settings::{
    Account, Settings, check_local_dir, filters_match_last_resync, is_safe_relative,
    new_account_dir, nfc,
};
use crate::signin::{ConfigStep, SignInStep, config_step, expired};
use crate::snapshot::{ChangeKind, FileChange, Snapshot, added, diff, mark_conflicts};

#[derive(Debug)]
pub enum Command {
    /// Signs in to a new account of `provider` (for example "onedrive").
    /// rclone opens the login page in the browser. `value` is the value of
    /// `Provider::field`, if the provider has one. For a provider with
    /// `SignIn::Password`, use `SignInWithPassword`.
    AddAccount {
        provider: String,
        value: Option<String>,
    },
    /// Signs out: removes the account and its rclone remote. With
    /// `trash_folder`, a last sync runs first; only after it succeeds does
    /// the local folder go to the Trash. Otherwise the folder stays.
    RemoveAccount {
        account: String,
        trash_folder: bool,
    },
    UpdatePrefs(Prefs),
    /// Renames an account, or gives it a different local folder.
    UpdateAccount {
        account: String,
        name: String,
        local_dir: PathBuf,
    },
    /// Lists the subfolders of a folder of an account. "" is the root.
    ListDir {
        account: String,
        path: String,
    },
    SetKeep {
        account: String,
        path: String,
        keep: bool,
    },
    /// Keeps a new top-level local folder only on this computer.
    /// See `Event::NewLocalFolders`.
    KeepLocalOnly {
        account: String,
        name: String,
    },
    /// Syncs all accounts now, one after the other.
    SyncNow,
    /// Reads the used and total storage of all accounts.
    RefreshAbout,
    /// Makes the next sync of an account a full resync. Use this after a
    /// sync error.
    ForceResync(String),
    /// The user confirmed the folder selection of a new account. Its first
    /// sync can start. See `Account::awaiting_selection`.
    ConfirmSelection(String),
    /// Signs in to an account again: after the provider ended the sign-in
    /// (`Account::needs_sign_in`), or when the user wants to. A browser
    /// provider opens the browser. A password provider needs
    /// `SignInWithPassword` instead.
    SignInAgain(String),
    /// Signs in with a password (`SignIn::Password`). `account` is None for
    /// a new account, else the account that signs in again. For an account
    /// that signs in again, an empty `user` keeps its user name.
    SignInWithPassword {
        provider: String,
        account: Option<String>,
        user: String,
        password: String,
    },
    /// The answer to `SignInStep::Question`, for example the code.
    SignInAnswer(String),
    /// Stops a sign-in that waits for an answer.
    CancelSignIn,
    Shutdown,
    #[doc(hidden)]
    BrowserSignInResult(Result<(), String>),
}

/// The app-wide settings that the user edits in Settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    pub sync_interval_minutes: u64,
    pub download_blocked_files: bool,
    pub start_at_login: bool,
}

impl Prefs {
    pub fn from_settings(s: &Settings) -> Prefs {
        Prefs {
            sync_interval_minutes: s.sync_interval_minutes,
            download_blocked_files: s.download_blocked_files,
            start_at_login: s.start_at_login,
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
    /// The step of the sign-in. `SignInStep::Idle` when it ends.
    SignIn(SignInStep),
    Dir {
        account: String,
        path: String,
        result: Result<Vec<String>, String>,
    },
    SyncProgress {
        account: String,
        text: String,
        fraction: Option<f32>,
        /// The files that are transferring now.
        current: Vec<Transfer>,
    },
    /// The storage of an account, in bytes. None when the provider does
    /// not report it.
    About {
        account: String,
        used: Option<u64>,
        total: Option<u64>,
    },
    /// Files that a sync changed, sorted by path.
    FilesChanged {
        account: String,
        changes: Vec<FileChange>,
    },
    /// The top-level folders that the user made on this computer and that
    /// do not sync yet. The UI asks if each one syncs (`SetKeep`) or stays
    /// only on this computer (`KeepLocalOnly`).
    NewLocalFolders {
        account: String,
        names: Vec<String>,
    },
    SyncFinished {
        account: String,
        ok: bool,
        message: String,
    },
    /// A message. `account` is None for a message about the app.
    Log {
        account: Option<String>,
        text: String,
    },
    Error {
        account: Option<String>,
        text: String,
    },
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

/// The state of an account between syncs.
struct Runtime {
    /// The local files after the last successful sync.
    last_files: Option<Snapshot>,
    last_sync: Option<Instant>,
    /// False after a failed sync. Then pending changes wait for the interval.
    last_ok: bool,
    /// The new local folders that the UI shows. See `check_local_folders`.
    new_folders: Option<Vec<String>>,
}

impl Runtime {
    fn new(account: &str) -> Runtime {
        Runtime {
            last_files: Snapshot::load(&Settings::snapshot_file(account)),
            last_sync: None,
            last_ok: true,
            new_folders: None,
        }
    }
}

/// The sync that runs now. It has one or two rclone jobs: an optional
/// `Phase::Pre`, then `Phase::Main`.
struct RunningSync {
    account: String,
    job: i64,
    phase: Phase,
    /// The local files when the running job started.
    files_before: Option<Snapshot>,
    /// True when this sync already reported the local changes (uploads).
    uploads_reported: bool,
    /// The number of files that this sync changed.
    changed_count: usize,
    /// Live reports (`report_transfers`): the transfers seen, the paths
    /// reported (NFC), and the files with an error reported.
    live_seen: HashSet<String>,
    live_paths: HashSet<String>,
    live_errors: HashSet<String>,
    /// The files that this sync found blocked for the first time.
    newly_blocked: usize,
    /// The original paths (NFC) of the conflict copies of this sync.
    conflicts: HashSet<String>,
}

impl RunningSync {
    fn new(account: &str) -> RunningSync {
        RunningSync {
            account: account.to_string(),
            job: 0,
            phase: Phase::Main { resync: false },
            files_before: None,
            uploads_reported: false,
            changed_count: 0,
            live_seen: HashSet::new(),
            live_paths: HashSet::new(),
            live_errors: HashSet::new(),
            newly_blocked: 0,
            conflicts: HashSet::new(),
        }
    }
}

struct Engine {
    rc: Rclone,
    binary: PathBuf,
    settings: Settings,
    emit: Emitter,
    self_tx: Sender<Command>,
    sync: Option<RunningSync>,
    runtimes: HashMap<String, Runtime>,
    /// True when rclone must restart with new options (`rclone_env`),
    /// as soon as no sync runs.
    restart_rclone: bool,
    last_folder_check: Instant,
    /// The account that signs out after its last sync, and moves its folder
    /// to the Trash. See `Command::RemoveAccount`.
    removing: Option<String>,
    /// The sign-in that runs. Only one runs at a time.
    signing_in: Option<SignInRun>,
    /// When rclone stopped by itself, in the last `RCLONE_EXIT_WINDOW`.
    rclone_exits: Vec<Instant>,
}

/// rclone can stop by itself, for example when security software ends the
/// process. DriveCraft starts it again, but not more than
/// `MAX_RCLONE_EXITS` times in `RCLONE_EXIT_WINDOW`.
const MAX_RCLONE_EXITS: usize = 3;
const RCLONE_EXIT_WINDOW: Duration = Duration::from_secs(10 * 60);

/// A sign-in that runs.
struct SignInRun {
    /// The rclone remote. For a new account, it exists only after the
    /// sign-in.
    id: String,
    provider: String,
    /// True for an account that signs in again.
    again: bool,
    /// The rclone option of the password (`SignIn::Password`), or None for
    /// a sign-in in the browser.
    password_key: Option<&'static str>,
    /// The config state that waits for the answer of the user.
    state: Option<String>,
}

impl SignInRun {
    fn account(&self) -> Option<String> {
        self.again.then(|| self.id.clone())
    }
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
        let config = Settings::rclone_config_file();
        let copied = copy_old_accounts(&binary, &settings, &config);
        let rc = Rclone::start(
            &binary,
            &config,
            &Settings::log_file(),
            &Settings::session_file(),
            &rclone_env(&settings),
        )?;
        let version = rc.version()?;
        emit.send(Event::Started {
            rclone_version: version,
            rclone_path: binary.clone(),
        });
        if !copied.is_empty() {
            emit.send(Event::Log {
                account: None,
                text: format!(
                    "DriveCraft now has its own rclone configuration file. \
                     It copied these accounts into it: {}.",
                    copied.join(", ")
                ),
            });
        }
        let runtimes = settings
            .accounts
            .iter()
            .map(|a| (a.id.clone(), Runtime::new(&a.id)))
            .collect();
        Ok(Engine {
            rc,
            binary,
            settings,
            emit,
            self_tx,
            sync: None,
            runtimes,
            restart_rclone: false,
            last_folder_check: Instant::now(),
            removing: None,
            signing_in: None,
            rclone_exits: Vec::new(),
        })
    }

    fn run(&mut self, rx: Receiver<Command>) {
        self.emit.send(Event::Settings(self.settings.clone()));
        if let Some(remotes) = self.list_remotes() {
            // An account whose remote is not in the rclone file of DriveCraft
            // cannot sync. The user signs in again; the folder stays.
            let missing: Vec<Account> = self
                .settings
                .accounts
                .iter()
                .filter(|a| !remotes.contains(&a.id))
                .cloned()
                .collect();
            for a in missing {
                self.error(
                    None,
                    format!(
                        "The account \"{}\" is not set up in DriveCraft. Sign in again. \
                         Its folder {} stays on this computer.",
                        a.name,
                        a.local_dir.display()
                    ),
                );
                self.settings.accounts.retain(|x| x.id != a.id);
                self.runtimes.remove(&a.id);
            }
            self.save();
        }
        self.refresh_about_all();
        self.check_all_local_folders();
        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.tick();
        }
        if let Some(sync) = &self.sync {
            let _ = self.rc.call("job/stop", json!({ "jobid": sync.job }));
        }
        // Dropping `self.rc` stops rclone.
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::AddAccount { provider, value } => self.connect_account(provider, value),
            Command::SignInAgain(id) => self.sign_in_again(&id),
            Command::SignInWithPassword {
                provider,
                account,
                user,
                password,
            } => self.sign_in_with_password(provider, account, user, password),
            Command::SignInAnswer(answer) => self.answer_sign_in(answer),
            Command::CancelSignIn => self.cancel_sign_in(),
            Command::BrowserSignInResult(Ok(())) => self.sign_in_complete(),
            Command::BrowserSignInResult(Err(e)) => self.sign_in_failed(e),
            Command::RemoveAccount {
                account,
                trash_folder,
            } => self.report(|s| s.sign_out(&account, trash_folder)),
            Command::UpdatePrefs(p) => self.update_prefs(p),
            Command::UpdateAccount {
                account,
                name,
                local_dir,
            } => self.report(|s| s.update_account(&account, name, local_dir)),
            Command::ListDir { account, path } => {
                let result = self.list_dir(&account, &path).map_err(|e| format!("{e:#}"));
                if let Err(e) = &result {
                    self.check_sign_in(&account, e);
                }
                self.emit.send(Event::Dir {
                    account,
                    path,
                    result,
                });
            }
            Command::SetKeep {
                account,
                path,
                keep,
            } => {
                if let Some(a) = self.settings.account_mut(&account) {
                    a.set_keep(&path, keep);
                    self.save();
                    self.check_local_folders(&account);
                }
            }
            Command::KeepLocalOnly { account, name } => {
                if is_safe_relative(&name)
                    && !name.contains('/')
                    && let Some(a) = self.settings.account_mut(&account)
                {
                    a.local_only.insert(name);
                    self.save();
                    self.check_local_folders(&account);
                }
            }
            Command::SyncNow => {
                for r in self.runtimes.values_mut() {
                    r.last_sync = None;
                }
            }
            Command::RefreshAbout => self.refresh_about_all(),
            Command::ForceResync(id) => {
                if let Some(a) = self.settings.account_mut(&id) {
                    a.needs_resync = true;
                    self.save();
                    self.mark_due(&id);
                }
            }
            Command::ConfirmSelection(id) => {
                if let Some(a) = self.settings.account_mut(&id) {
                    a.awaiting_selection = false;
                    self.save();
                    self.mark_due(&id);
                }
            }
            Command::Shutdown => {}
        }
    }

    // ----- accounts -----

    /// The remotes in the rclone file of DriveCraft, or None if rclone fails.
    fn list_remotes(&mut self) -> Option<Vec<String>> {
        match self.rc.call("config/listremotes", json!({})) {
            Ok(v) => {
                let remotes: Vec<String> = v["remotes"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|r| r.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                Some(remotes)
            }
            Err(e) => {
                self.error(None, format!("{e:#}"));
                None
            }
        }
    }

    fn connect_account(&mut self, provider: String, value: Option<String>) {
        let Some(p) = crate::providers::provider(&provider).filter(|p| p.available) else {
            self.error(None, format!("DriveCraft does not support \"{provider}\"."));
            return;
        };
        if !matches!(p.sign_in, SignIn::Browser) {
            self.error(
                None,
                format!("Cannot connect the account: {} needs a password.", p.label),
            );
            return;
        }
        if self.sign_in_runs() {
            return;
        }
        let mut options: Vec<(String, String)> = p
            .options
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        if let Some(field) = &p.field {
            // The value goes to the rclone command line, so only a value
            // that passes the check of the field.
            let Some(v) = value.as_deref().and_then(|v| field.accept(v)) else {
                self.error(
                    None,
                    format!(
                        "Cannot connect the account: the {} is not valid.",
                        field.label
                    ),
                );
                return;
            };
            options.push((field.key.to_string(), v));
        }
        let id = self.unused_remote(&provider);
        self.browser_sign_in(
            SignInRun {
                id,
                provider,
                again: false,
                password_key: None,
                state: None,
            },
            Some(options),
        );
    }

    /// A remote name that no remote and no account has. It never shows in
    /// the UI: "onedrive", "onedrive-2", …
    fn unused_remote(&mut self, provider: &str) -> String {
        let remotes = self.list_remotes().unwrap_or_default();
        (1..)
            .map(|n| match n {
                1 => provider.to_string(),
                n => format!("{provider}-{n}"),
            })
            .find(|id| !remotes.contains(id) && self.settings.account(id).is_none())
            .expect("an unused remote name exists")
    }

    /// True, with an error message, while a different sign-in runs.
    fn sign_in_runs(&mut self) -> bool {
        if self.signing_in.is_some() {
            self.error(None, "Finish the sign-in that runs now first.".into());
        }
        self.signing_in.is_some()
    }

    /// Starts rclone for a sign-in in the browser: `config create` with
    /// `options` for a new account, else `config reconnect`. The sign-in
    /// can take minutes, so it runs on its own thread.
    fn browser_sign_in(&mut self, run: SignInRun, options: Option<Vec<(String, String)>>) {
        self.emit.send(Event::SignIn(SignInStep::Browser {
            account: run.account(),
        }));
        self.log(
            run.account().as_deref(),
            format!(
                "Sign in to {} in the browser window that opens.",
                provider_label(&run.provider)
            ),
        );
        let mut args: Vec<String> = match options {
            Some(options) => ["config", "create", &run.id, &run.provider]
                .map(String::from)
                .into_iter()
                .chain(options.into_iter().flat_map(|(k, v)| [k, v]))
                .collect(),
            // Without a terminal, rclone takes the default answers, as for
            // `config create`. The other values of the remote stay.
            None => vec![
                "--auto-confirm".into(),
                "config".into(),
                "reconnect".into(),
                format!("{}:", run.id),
            ],
        };
        // The log file shows where a sign-in stops, for example at a proxy
        // server.
        let mut all: Vec<String> = vec![
            "--config".into(),
            Settings::rclone_config_file().display().to_string(),
            "--log-file".into(),
            Settings::log_file().display().to_string(),
            "--log-level=DEBUG".into(),
        ];
        all.append(&mut args);
        let binary = self.binary.clone();
        let tx = self.self_tx.clone();
        self.signing_in = Some(run);
        thread::spawn(move || {
            let result = rclone_command(&binary)
                .args(&all)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())
                .and_then(|child| wait_for_sign_in(child, SIGN_IN_TIMEOUT));
            let _ = tx.send(Command::BrowserSignInResult(result));
        });
    }

    fn sign_in_again(&mut self, id: &str) {
        let Some(account) = self.settings.account(id).cloned() else {
            return;
        };
        let Some(p) = crate::providers::provider(&account.provider) else {
            return;
        };
        if !matches!(p.sign_in, SignIn::Browser) {
            self.error(
                Some(id),
                format!("Type your password to sign in to {} again.", p.label),
            );
            return;
        }
        if self.sign_in_runs() {
            return;
        }
        self.browser_sign_in(
            SignInRun {
                id: id.to_string(),
                provider: account.provider.clone(),
                again: true,
                password_key: None,
                state: None,
            },
            None,
        );
    }

    /// Signs in with a password through the rclone config API. The answer
    /// of rclone can ask for a code (`answer_sign_in`).
    fn sign_in_with_password(
        &mut self,
        provider: String,
        account: Option<String>,
        user: String,
        password: String,
    ) {
        let Some(p) =
            crate::providers::provider(&provider).filter(|p| p.available || account.is_some())
        else {
            self.error(None, format!("DriveCraft does not support \"{provider}\"."));
            return;
        };
        let SignIn::Password {
            user_key,
            password_key,
            user_label,
            ..
        } = p.sign_in
        else {
            self.error(None, format!("{} does not use a password.", p.label));
            return;
        };
        if account
            .as_deref()
            .is_some_and(|id| self.settings.account(id).is_none())
        {
            return;
        }
        let user = user.trim();
        if (account.is_none() && user.is_empty()) || password.is_empty() {
            self.error(
                account.as_deref(),
                format!("Type your {user_label} and your password."),
            );
            return;
        }
        if self.sign_in_runs() {
            return;
        }
        let mut parameters = serde_json::Map::new();
        for (k, v) in p.options {
            parameters.insert(k.to_string(), json!(v));
        }
        if !user.is_empty() {
            parameters.insert(user_key.into(), json!(user));
        }
        parameters.insert(password_key.into(), json!(password));
        let again = account.is_some();
        let id = match account {
            Some(id) => id,
            None => self.unused_remote(&provider),
        };
        let run = SignInRun {
            id: id.clone(),
            provider: provider.clone(),
            again,
            password_key: Some(password_key),
            state: None,
        };
        self.emit.send(Event::SignIn(SignInStep::Working {
            account: run.account(),
        }));
        self.signing_in = Some(run);
        // `obscure`: rclone stores the password only in its obscured form.
        let opt = json!({ "nonInteractive": true, "obscure": true });
        let out = if again {
            self.rc.call(
                "config/update",
                json!({ "name": id, "parameters": parameters, "opt": opt }),
            )
        } else {
            self.rc.call(
                "config/create",
                json!({ "name": id, "type": provider, "parameters": parameters, "opt": opt }),
            )
        };
        self.config_answer(out);
    }

    /// Sends the answer of the user to the question of rclone.
    fn answer_sign_in(&mut self, answer: String) {
        let Some(run) = self.signing_in.as_mut() else {
            return;
        };
        let Some(state) = run.state.take() else {
            return;
        };
        let account = run.account();
        self.emit
            .send(Event::SignIn(SignInStep::Working { account }));
        let out = self.config_continue(&state, answer.trim());
        self.config_answer(out);
    }

    fn config_continue(&mut self, state: &str, result: &str) -> Result<Value> {
        let id = self
            .signing_in
            .as_ref()
            .map(|r| r.id.clone())
            .unwrap_or_default();
        self.rc.call(
            "config/update",
            json!({
                "name": id,
                "parameters": {},
                "opt": { "nonInteractive": true, "continue": true, "state": state, "result": result },
            }),
        )
    }

    /// Follows the answers of rclone until it asks the user, or until the
    /// configuration is complete.
    fn config_answer(&mut self, mut out: Result<Value>) {
        // A limit, so that an unexpected loop of rclone cannot hang.
        for _ in 0..10 {
            let step = match &out {
                Ok(v) => config_step(v),
                Err(e) => return self.sign_in_failed(format!("{e:#}")),
            };
            match step {
                ConfigStep::Done => return self.sign_in_complete(),
                ConfigStep::Next { state, result } => out = self.config_continue(&state, &result),
                ConfigStep::Ask {
                    state,
                    name,
                    help,
                    error,
                } => {
                    let Some(run) = self.signing_in.as_mut() else {
                        return;
                    };
                    run.state = Some(state);
                    let account = run.account();
                    self.emit.send(Event::SignIn(SignInStep::Question {
                        account,
                        name,
                        help,
                        error,
                    }));
                    return;
                }
            }
        }
        self.sign_in_failed("rclone did not finish the sign-in.".into());
    }

    fn cancel_sign_in(&mut self) {
        // A sign-in in the browser stops only with its time limit.
        if self.signing_in.as_ref().is_some_and(|r| r.state.is_some()) {
            self.sign_in_failed("You stopped the sign-in.".into());
        }
    }

    fn sign_in_failed(&mut self, e: String) {
        let Some(run) = self.signing_in.take() else {
            return;
        };
        self.emit.send(Event::SignIn(SignInStep::Idle));
        if run.again {
            self.forget_password(&run);
            self.error(Some(&run.id), format!("Cannot sign in again: {e}"));
        } else {
            // No remote without an account: the next sign-in starts clean.
            let _ = self.rc.call("config/delete", json!({ "name": run.id }));
            self.error(None, format!("Cannot connect the account: {e}"));
        }
    }

    /// The provider accepted the sign-in.
    fn sign_in_complete(&mut self) {
        let Some(run) = self.signing_in.take() else {
            return;
        };
        self.emit.send(Event::SignIn(SignInStep::Idle));
        if !run.again {
            if let Err(e) = self.add_account(run.id.clone(), run.provider.clone()) {
                let _ = self.rc.call("config/delete", json!({ "name": run.id }));
                self.error(None, format!("Cannot connect the account: {e:#}"));
                return;
            }
            self.forget_password(&run);
            return;
        }
        // A cached remote can still have the old session.
        let _ = self.rc.call("fscache/clear", json!({}));
        if let Err(e) = self.list_dir(&run.id, "") {
            self.forget_password(&run);
            self.error(Some(&run.id), format!("Cannot sign in again: {e:#}"));
            return;
        }
        self.forget_password(&run);
        let Some(a) = self.settings.account_mut(&run.id) else {
            return;
        };
        a.needs_sign_in = false;
        let name = a.name.clone();
        self.save();
        self.log(
            Some(&run.id),
            format!("You signed in to \"{name}\" again. The sync continues."),
        );
        self.mark_due(&run.id);
        self.refresh_about(&run.id);
    }

    /// Removes the password from the rclone file. rclone continues with
    /// the session of the provider (see `SignIn::Password`).
    fn forget_password(&mut self, run: &SignInRun) {
        let Some(key) = run.password_key else {
            return;
        };
        if let Err(e) = self
            .rc
            .call("config/unset", json!({ "name": run.id, "keys": [key] }))
        {
            self.error(
                None,
                format!("Cannot remove the password from the rclone file: {e:#}"),
            );
        }
    }

    /// Marks an account that needs a new sign-in, when `error` says that
    /// the provider ended the sign-in. Its syncs stop until then.
    fn check_sign_in(&mut self, id: &str, error: &str) {
        if !expired(error) {
            return;
        }
        let Some(a) = self.settings.account_mut(id) else {
            return;
        };
        if a.needs_sign_in {
            return;
        }
        a.needs_sign_in = true;
        let (name, provider) = (a.name.clone(), a.provider.clone());
        self.save();
        self.error(
            Some(id),
            format!(
                "{} ended the sign-in of \"{name}\". Sign in again to continue the sync.",
                provider_label(&provider)
            ),
        );
    }

    /// Adds an account for the rclone remote `id`. Like the official
    /// client, a new account starts with all top-level folders on, but it
    /// does not sync before the user confirms the selection.
    fn add_account(&mut self, id: String, provider: String) -> Result<()> {
        if self.settings.account(&id).is_some() {
            return Ok(());
        }
        let label = provider_label(&provider).to_string();
        let name = (1..)
            .map(|n| match n {
                1 => label.clone(),
                n => format!("{label} {n}"),
            })
            .find(|name| !self.settings.accounts.iter().any(|a| &a.name == name))
            .expect("an unused account name exists");
        let taken = self.settings.other_account_dirs(&id);
        let local_dir = new_account_dir(&home(), &name, &taken);
        check_local_dir(&local_dir, &home(), &taken)?;
        let mut account = Account {
            id: id.clone(),
            name: name.clone(),
            provider,
            local_dir,
            needs_resync: true,
            awaiting_selection: true,
            ..Account::default()
        };
        // An earlier account with the same remote can have left a file list.
        let _ = fs::remove_file(Settings::snapshot_file(&id));
        self.settings.accounts.push(account.clone());
        self.runtimes.insert(id.clone(), Runtime::new(&id));
        match self.list_dir(&id, "") {
            Ok(top) => account.pending_pin.extend(top),
            Err(e) => self.error(
                Some(&id),
                format!("Cannot list the folders of \"{name}\": {e:#}"),
            ),
        }
        if let Some(a) = self.settings.account_mut(&id) {
            *a = account;
        }
        self.log(Some(&id), format!("The account \"{name}\" is connected."));
        self.save();
        self.refresh_about(&id);
        self.check_local_folders(&id);
        Ok(())
    }

    /// Signs out of an account at once, or after a last sync when the
    /// folder goes to the Trash.
    fn sign_out(&mut self, id: &str, trash_folder: bool) -> Result<()> {
        let Some(account) = self.settings.account(id).cloned() else {
            return Ok(());
        };
        if !trash_folder {
            return self.remove_account(id, false);
        }
        if account.needs_sign_in && !account.awaiting_selection {
            bail!(
                "Sign in to \"{}\" again first: the last sync before the sign-out needs it. \
                 Or sign out with \"Keep the folder\".",
                account.name
            );
        }
        if account.awaiting_selection {
            // Never synced: files in the folder are not online.
            let empty = fs::read_dir(&account.local_dir).map_or(true, |mut d| d.next().is_none());
            if !empty {
                bail!(
                    "\"{}\" never synced, so the files in {} are not online. \
                     DriveCraft keeps the folder. Sign out with \"Keep the folder\".",
                    account.name,
                    account.local_dir.display()
                );
            }
            return self.remove_account(id, true);
        }
        self.removing = Some(id.to_string());
        self.log(
            Some(id),
            format!(
                "DriveCraft syncs \"{}\" one last time. Then it signs out and moves the \
                 folder to the Trash.",
                account.name
            ),
        );
        self.mark_due(id);
        Ok(())
    }

    /// Removes an account. Its folder stays, or goes to the Trash.
    fn remove_account(&mut self, id: &str, trash_folder: bool) -> Result<()> {
        if self.sync.as_ref().is_some_and(|s| s.account == id) {
            bail!("Wait until the sync of this account is complete, then sign out.");
        }
        let Some(account) = self.settings.account(id).cloned() else {
            return Ok(());
        };
        let trashed = trash_folder && account.local_dir.exists();
        if trashed {
            // The same checks as before each sync: never the home folder,
            // the disk root, or a folder of a different app or account.
            check_local_dir(
                &account.local_dir,
                &home(),
                &self.settings.other_account_dirs(id),
            )?;
            move_to_trash(&account.local_dir).with_context(|| {
                format!(
                    "Cannot move {} to the Trash. DriveCraft did not sign out",
                    account.local_dir.display()
                )
            })?;
        }
        self.settings.accounts.retain(|a| a.id != id);
        self.runtimes.remove(id);
        let filters = Settings::filters_file(id);
        let mut md5 = filters.clone().into_os_string();
        md5.push(".md5");
        for file in [filters, PathBuf::from(md5), Settings::snapshot_file(id)] {
            let _ = fs::remove_file(file);
        }
        if let Err(e) = self.rc.call("config/delete", json!({ "name": id })) {
            self.error(None, format!("Cannot remove the rclone remote: {e:#}"));
        }
        self.save();
        let folder = if trashed {
            format!(
                "Its folder {} is in the Trash.",
                account.local_dir.display()
            )
        } else {
            format!(
                "Its folder {} stays on this computer.",
                account.local_dir.display()
            )
        };
        self.log(
            None,
            format!("You signed out of \"{}\". {folder}", account.name),
        );
        Ok(())
    }

    fn update_prefs(&mut self, p: Prefs) {
        self.settings.sync_interval_minutes = p.sync_interval_minutes.max(1);
        self.settings.start_at_login = p.start_at_login;
        if self.settings.download_blocked_files != p.download_blocked_files {
            // The filter rules change too, so the next syncs are resyncs.
            // Turned on, they download the blocked files.
            self.settings.download_blocked_files = p.download_blocked_files;
            self.restart_rclone = true;
        }
        self.save();
    }

    fn update_account(&mut self, id: &str, name: String, local_dir: PathBuf) -> Result<()> {
        let others = self.settings.other_account_dirs(id);
        let syncing = self.sync.as_ref().is_some_and(|s| s.account == id);
        let Some(account) = self.settings.account(id).cloned() else {
            return Ok(());
        };
        let name = name.trim().to_string();
        if !name.is_empty()
            && name != account.name
            && let Some(a) = self.settings.account_mut(id)
        {
            a.name = name;
        }
        if account.local_dir != local_dir {
            if syncing {
                bail!("Wait until the sync is complete, then change the folder.");
            }
            check_local_dir(&local_dir, &home(), &others)?;
            // The old folder stays as it is. The next sync fills the new folder.
            if let Some(a) = self.settings.account_mut(id) {
                a.local_dir = local_dir.clone();
                a.local_only.clear();
                a.needs_resync = true;
            }
            self.forget_files(id);
            self.check_local_folders(id);
            self.log(
                Some(id),
                format!("The folder of this account is now {}.", local_dir.display()),
            );
        }
        self.save();
        Ok(())
    }

    /// Finds an rclone that stopped by itself, and starts it again (after
    /// the running sync fails), or stops the engine when it stops too often.
    fn check_rclone(&mut self) {
        // After too many stops, the engine reported a fatal error once.
        if self.restart_rclone || self.rclone_exits.len() > MAX_RCLONE_EXITS {
            return;
        }
        let Some(status) = self.rc.exited() else {
            return;
        };
        let log = Settings::log_file();
        self.rclone_exits
            .retain(|t| t.elapsed() < RCLONE_EXIT_WINDOW);
        self.rclone_exits.push(Instant::now());
        if self.rclone_exits.len() > MAX_RCLONE_EXITS {
            self.emit.send(Event::Fatal(format!(
                "rclone stopped {} times in 10 minutes ({status}). Security software can \
                 stop it. The details are in {}.",
                self.rclone_exits.len(),
                log.display()
            )));
            return;
        }
        self.error(
            None,
            format!(
                "rclone stopped ({status}). DriveCraft starts it again. The details are in {}.",
                log.display()
            ),
        );
        self.restart_rclone = true;
    }

    /// Starts rclone again with the options of the settings. Only call it
    /// while no sync runs.
    fn restart_rclone(&mut self) {
        self.restart_rclone = false;
        self.rc.stop();
        match Rclone::start(
            &self.binary,
            &Settings::rclone_config_file(),
            &Settings::log_file(),
            &Settings::session_file(),
            &rclone_env(&self.settings),
        ) {
            Ok(rc) => {
                self.rc = rc;
                for r in self.runtimes.values_mut() {
                    r.last_sync = None;
                }
            }
            Err(e) => self.emit.send(Event::Fatal(format!("{e:#}"))),
        }
    }

    fn refresh_about_all(&mut self) {
        let ids: Vec<String> = self
            .settings
            .accounts
            .iter()
            .map(|a| a.id.clone())
            .collect();
        for id in ids {
            self.refresh_about(&id);
        }
    }

    fn refresh_about(&mut self, id: &str) {
        let Some(fs_name) = self.settings.account(id).map(Account::remote_fs) else {
            return;
        };
        match self.rc.call("operations/about", json!({ "fs": fs_name })) {
            Ok(v) => self.emit.send(Event::About {
                account: id.to_string(),
                used: v["used"].as_u64(),
                total: v["total"].as_u64(),
            }),
            Err(e) => {
                // Some providers do not report it. The UI says so.
                self.emit.send(Event::About {
                    account: id.to_string(),
                    used: None,
                    total: None,
                });
                let e = format!("{e:#}");
                self.check_sign_in(id, &e);
                self.log(Some(id), format!("Cannot read the storage use: {e}"));
            }
        }
    }

    // ----- folders -----

    fn list_dir(&self, id: &str, path: &str) -> Result<Vec<String>> {
        let Some(account) = self.settings.account(id) else {
            bail!("This account does not exist any more.");
        };
        let v = self.rc.call(
            "operations/list",
            json!({ "fs": account.remote_fs(), "remote": path, "opt": { "dirsOnly": true } }),
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

    fn mark_due(&mut self, id: &str) {
        if let Some(r) = self.runtimes.get_mut(id) {
            r.last_sync = None;
        }
    }

    fn tick(&mut self) {
        self.check_rclone();
        if self.sync.is_some() {
            self.poll_job();
            return;
        }
        if self.restart_rclone {
            self.restart_rclone();
            return;
        }
        if self.last_folder_check.elapsed() >= Duration::from_secs(5) {
            self.check_all_local_folders();
        }
        // An account that signs out syncs first, before the others.
        if let Some(id) = self.removing.clone() {
            // start_sync does nothing for these, so do not wait for ever.
            if self
                .settings
                .account(&id)
                .is_none_or(|a| a.awaiting_selection || a.needs_sign_in)
            {
                self.removing = None;
            } else {
                self.start_sync(&id);
                return;
            }
        }
        let interval = Duration::from_secs(self.settings.sync_interval_minutes.max(1) * 60);
        // One sync at a time: the first account that is due starts.
        let due = self.settings.accounts.iter().find(|a| {
            let Some(r) = self.runtimes.get(&a.id) else {
                return false;
            };
            !a.awaiting_selection
                && !a.needs_sign_in
                && (r.last_sync.is_none_or(|t| t.elapsed() >= interval)
                    || (a.has_pending() && r.last_ok))
        });
        if let Some(id) = due.map(|a| a.id.clone()) {
            self.start_sync(&id);
        }
    }

    fn start_sync(&mut self, id: &str) {
        if self.sync.is_some() {
            return; // The running sync sees the new settings when it ends.
        }
        let Some(a) = self.settings.account(id) else {
            return;
        };
        if a.awaiting_selection {
            return; // The first sync waits for `Command::ConfirmSelection`.
        }
        if a.needs_sign_in {
            return; // It waits for `Command::SignInAgain`.
        }
        // Before a switch change, sync once with the old selection, so that no
        // local change is lost. Without a previous sync (needs_resync) this is
        // only needed when a folder gets removed from this computer.
        let phase = if a.has_pending() && (!a.needs_resync || !a.pending_unpin.is_empty()) {
            Phase::Pre
        } else {
            if a.has_pending() {
                self.apply_pending(id);
            }
            Phase::Main {
                resync: self.settings.account(id).is_some_and(|a| a.needs_resync),
            }
        };
        self.sync = Some(RunningSync::new(id));
        if let Err(e) = self.start_bisync(phase) {
            self.sync = None;
            if let Some(r) = self.runtimes.get_mut(id) {
                r.last_sync = Some(Instant::now());
            }
            self.finish(id, false, format!("{e:#}"));
        }
    }

    fn start_bisync(&mut self, phase: Phase) -> Result<()> {
        let id = self.sync.as_ref().context("no sync runs")?.account.clone();
        let a = self
            .settings
            .account(&id)
            .cloned()
            .context("the account does not exist any more")?;
        let resync = match phase {
            Phase::Pre => a.needs_resync,
            Phase::Main { resync } => resync,
        };
        check_local_dir(
            &a.local_dir,
            &home(),
            &self.settings.other_account_dirs(&id),
        )?;
        if !a.local_dir.is_dir() {
            // After a previous sync, a missing folder was moved, renamed, or
            // deleted. A new empty folder would make bisync delete all files
            // online. A resync never deletes, so it can create the folder.
            if !resync {
                bail!(
                    "The folder {} is missing. If you moved or renamed it, select it again \
                     in the settings of this account. DriveCraft does not sync it until then.",
                    a.local_dir.display()
                );
            }
            fs::create_dir_all(&a.local_dir)
                .with_context(|| format!("cannot create {}", a.local_dir.display()))?;
        }
        let rules = a.filter_rules(self.settings.download_blocked_files);
        let filters = Settings::filters_file(&id);
        fs::write(&filters, &rules)?;
        // Bisync stops when the filter rules differ from the rules of the
        // last resync, for example after a DriveCraft update. Then this sync
        // must be a resync. A resync never deletes. This check comes after
        // the check of the missing folder: only the user may make a resync
        // that creates the folder.
        let mut md5_file = filters.clone().into_os_string();
        md5_file.push(".md5");
        let stored = fs::read_to_string(&md5_file).ok();
        let resync = if !resync && !filters_match_last_resync(&rules, stored.as_deref()) {
            self.log(
                Some(&id),
                "The sync rules changed, so this sync compares all files again. \
                 It deletes nothing."
                    .into(),
            );
            true
        } else {
            resync
        };
        let files_before = take_files(&a);
        let mut params = json!({
            "path1": a.local_dir,
            "path2": a.remote_fs(),
            "filtersFile": filters,
            "resync": resync,
            "_async": true,
        });
        if resync {
            // Without a mode, a resync prefers path1: an old local copy
            // overwrites a newer file online. Keep the newer version.
            params["resyncMode"] = json!("newer");
        } else {
            // A file that changed on both sides: the newer version keeps the
            // name, the other one becomes "name.conflict1.ext" on both
            // sides. Nothing is lost, and the copy keeps its extension.
            params["conflictResolve"] = json!("newer");
            params["conflictLoser"] = json!("num");
            params["_config"] = json!({ "SuffixKeepExtension": true });
        }
        let v = self.rc.call("sync/bisync", params)?;
        let job = v["jobid"]
            .as_i64()
            .context("rclone did not return a job id")?;
        if let Some(sync) = self.sync.as_mut() {
            sync.job = job;
            sync.phase = phase;
            sync.files_before = Some(files_before);
        }
        let text = if resync {
            "Preparing a full sync…"
        } else {
            "Checking for changes…"
        };
        self.emit.send(Event::SyncProgress {
            account: id,
            text: text.into(),
            fraction: None,
            current: Vec::new(),
        });
        Ok(())
    }

    fn poll_job(&mut self) {
        let Some(sync) = &self.sync else { return };
        let (id, job, phase) = (sync.account.clone(), sync.job, sync.phase);
        let status = match self.rc.call("job/status", json!({ "jobid": job })) {
            Ok(v) => v,
            Err(e) => {
                self.sync = None;
                self.finish(&id, false, format!("{e:#}"));
                return;
            }
        };
        let finished = status["finished"].as_bool().unwrap_or(false);
        self.report_transfers();
        let remote = self
            .settings
            .account(&id)
            .map(Account::remote_fs)
            .unwrap_or_default();
        if !finished {
            if let Ok(st) = self
                .rc
                .call("core/stats", json!({ "group": format!("job/{job}") }))
            {
                let (text, fraction, current) = progress(&st, &remote);
                self.emit.send(Event::SyncProgress {
                    account: id,
                    text,
                    fraction,
                    current,
                });
            }
            return;
        }
        if let Some(r) = self.runtimes.get_mut(&id) {
            r.last_sync = Some(Instant::now());
        }
        if !status["success"].as_bool().unwrap_or(false) {
            let newly_blocked = self.sync.take().map_or(0, |s| s.newly_blocked);
            if newly_blocked > 0 {
                // The filter rules exclude the new blocked files now. Their
                // number is finite, so this cannot repeat for ever.
                self.log(
                    Some(&id),
                    "The sync stopped at blocked files. DriveCraft starts it again \
                     and skips these files."
                        .into(),
                );
                self.start_sync(&id);
                return;
            }
            let err = status["error"].as_str().unwrap_or("unknown error");
            self.finish(&id, false, format!("The sync failed: {err}"));
            return;
        }
        self.report_changes();
        match phase {
            Phase::Pre => {
                if let Some(a) = self.settings.account_mut(&id) {
                    a.needs_resync = false;
                }
                self.apply_pending(&id);
                self.check_local_folders(&id);
                let resync = self.settings.account(&id).is_some_and(|a| a.needs_resync);
                if let Err(e) = self.start_bisync(Phase::Main { resync }) {
                    self.sync = None;
                    self.finish(&id, false, format!("{e:#}"));
                }
            }
            Phase::Main { resync } => {
                if resync {
                    if let Some(a) = self.settings.account_mut(&id) {
                        a.needs_resync = false;
                    }
                    self.save();
                }
                let changed = self.sync.take().map_or(0, |s| s.changed_count);
                let message = match changed {
                    0 => "The sync is complete. Nothing changed.".to_string(),
                    1 => "The sync is complete. 1 file changed.".to_string(),
                    n => format!("The sync is complete. {n} files changed."),
                };
                self.finish(&id, true, message);
            }
        }
    }

    /// Applies the switch changes of an account. The local copies of
    /// folders that no longer sync are deleted. A successful sync ran
    /// before this, so the files are also online.
    fn apply_pending(&mut self, id: &str) {
        let Some(a) = self.settings.account_mut(id) else {
            return;
        };
        let removed = a.apply_pending();
        let dir = a.local_dir.clone();
        for path in removed {
            if !is_safe_relative(&path) {
                continue;
            }
            let local = dir.join(&path);
            if local.exists() {
                match fs::remove_dir_all(&local) {
                    Ok(()) => self.log(Some(id), format!("\"{path}\" is now only online.")),
                    Err(e) => {
                        self.error(Some(id), format!("Cannot remove {}: {e}", local.display()))
                    }
                }
            }
        }
        self.save();
    }

    // ----- file changes -----

    /// Reports the files that the running job copied since the last call,
    /// and the files that it could not copy. rclone names the source and the
    /// destination of each copy, so the direction is known. Deletes come
    /// only from the file lists, in `report_changes`.
    fn report_transfers(&mut self) {
        let Some(sync) = self.sync.as_mut() else {
            return;
        };
        let id = sync.account.clone();
        let Ok(v) = self.rc.call(
            "core/transferred",
            json!({ "group": format!("job/{}", sync.job) }),
        ) else {
            return;
        };
        let remote = format!("{id}:");
        let live = live_transfers(&v, &remote, &mut sync.live_seen);
        let mut messages = Vec::new();
        let mut errors = Vec::new();
        let mut settings_changed = false;
        let download_blocked = self.settings.download_blocked_files;
        let Some(account) = self.settings.accounts.iter_mut().find(|a| a.id == id) else {
            return;
        };
        let provider = provider_label(&account.provider).to_string();
        for (name, err) in live.errors {
            if is_blocked_error(&err) {
                // Reported once. Later syncs skip the file (filter rules).
                if account.blocked_files.insert(name.clone()) {
                    sync.newly_blocked += 1;
                    settings_changed = true;
                    messages.push(format!(
                        "{provider} blocks \"{name}\": it marks the file as infected with a \
                         virus. DriveCraft skips this file. To download blocked files, turn on \
                         \"Download blocked files\" in Settings."
                    ));
                }
            } else if sync.live_errors.insert(name.clone()) {
                errors.push(format!("Cannot sync \"{name}\": {err}"));
            }
        }
        // A blocked file that downloads now (the switch is on) is not
        // blocked any more.
        if download_blocked {
            for c in &live.changes {
                settings_changed |= account.blocked_files.remove(&c.path);
            }
        }
        let mut changes: Vec<FileChange> = live
            .changes
            .into_iter()
            .filter(|c| sync.live_paths.insert(nfc(&c.path)))
            .collect();
        mark_conflicts(&mut changes, &mut sync.conflicts);
        sync.changed_count += changes.len();
        for m in messages {
            self.log(Some(&id), m);
        }
        for e in errors {
            self.error(Some(&id), e);
        }
        if settings_changed {
            self.save();
        }
        if !changes.is_empty() {
            changes.sort_by(|a, b| a.path.cmp(&b.path));
            self.emit.send(Event::FilesChanged {
                account: id,
                changes,
            });
        }
    }

    /// Reports the files that the finished job changed, and keeps the new
    /// list for the next sync.
    fn report_changes(&mut self) {
        let Some(sync) = self.sync.as_mut() else {
            return;
        };
        let id = sync.account.clone();
        let Some(before) = sync.files_before.take() else {
            return;
        };
        let Some(account) = self.settings.account(&id) else {
            return;
        };
        let after = take_files(account);
        let Some(runtime) = self.runtimes.get_mut(&id) else {
            return;
        };
        let mut changes = Vec::new();
        // The local changes since the last sync. Only the first job of a
        // sync reports all of them. A later job has a new folder selection:
        // it reports only the local files that the new selection added,
        // because they upload. The files of a folder that no longer syncs
        // are not "deleted online".
        if let Some(last) = &runtime.last_files
            && last.matches(&before.remote, &before.local_dir)
        {
            if sync.uploads_reported {
                changes.extend(added(last, &before, ChangeKind::Uploaded));
            } else {
                changes.extend(diff(
                    last,
                    &before,
                    ChangeKind::Uploaded,
                    ChangeKind::DeletedOnline,
                ));
            }
        }
        sync.uploads_reported = true;
        changes.extend(diff(
            &before,
            &after,
            ChangeKind::Downloaded,
            ChangeKind::DeletedHere,
        ));
        // The copies that `report_transfers` reported already.
        changes.retain(|c| {
            matches!(c.kind, ChangeKind::DeletedHere | ChangeKind::DeletedOnline)
                || !sync.live_paths.contains(&nfc(&c.path))
        });
        mark_conflicts(&mut changes, &mut sync.conflicts);
        sync.changed_count += changes.len();
        let saved = after.save(&Settings::snapshot_file(&id));
        runtime.last_files = Some(after);
        if let Err(e) = saved {
            self.error(Some(&id), format!("Cannot save the file list: {e:#}"));
        }
        if !changes.is_empty() {
            self.emit.send(Event::FilesChanged {
                account: id,
                changes,
            });
        }
    }

    /// The account or its folder changed, so the old file list is not valid.
    fn forget_files(&mut self, id: &str) {
        if let Some(r) = self.runtimes.get_mut(id) {
            r.last_files = None;
        }
        let _ = fs::remove_file(Settings::snapshot_file(id));
    }

    // ----- helpers -----

    fn finish(&mut self, id: &str, ok: bool, message: String) {
        if let Some(r) = self.runtimes.get_mut(id) {
            r.last_ok = ok;
        }
        if !ok {
            self.check_sign_in(id, &message);
        }
        self.emit.send(Event::SyncFinished {
            account: id.to_string(),
            ok,
            message,
        });
        self.emit.send(Event::Settings(self.settings.clone()));
        if self.removing.as_deref() == Some(id) {
            self.removing = None;
            let name = self
                .settings
                .account(id)
                .map(|a| a.name.clone())
                .unwrap_or_default();
            if ok {
                self.report(|s| s.remove_account(id, true));
                return;
            }
            self.error(
                Some(id),
                format!(
                    "DriveCraft did not sign out of \"{name}\", because the last sync \
                     failed. The account and its folder stay."
                ),
            );
        }
        if ok {
            self.refresh_about(id);
        }
        self.check_local_folders(id);
    }

    fn check_all_local_folders(&mut self) {
        self.last_folder_check = Instant::now();
        let ids: Vec<String> = self
            .settings
            .accounts
            .iter()
            .map(|a| a.id.clone())
            .collect();
        for id in ids {
            self.check_local_folders(&id);
        }
    }

    /// Finds the top-level local folders of an account that do not sync,
    /// and sends them to the UI when the list changes. Each new folder gets
    /// one log entry.
    fn check_local_folders(&mut self, id: &str) {
        let Some(a) = self.settings.account_mut(id) else {
            return;
        };
        let pruned = a.prune_local_only();
        let found = a.new_local_folders();
        if pruned {
            self.save();
        }
        let Some(r) = self.runtimes.get_mut(id) else {
            return;
        };
        if r.new_folders.as_ref() == Some(&found) {
            return;
        }
        let shown = r.new_folders.replace(found.clone()).unwrap_or_default();
        for name in found.iter().filter(|n| !shown.contains(n)) {
            self.log(
                Some(id),
                format!(
                    "\"{name}\" is a new folder on this computer. It does not sync until \
                     you select it in Overview."
                ),
            );
        }
        self.emit.send(Event::NewLocalFolders {
            account: id.to_string(),
            names: found,
        });
    }

    fn save(&mut self) {
        if let Err(e) = self.settings.save() {
            self.error(None, format!("Cannot save the settings: {e:#}"));
        }
        self.emit.send(Event::Settings(self.settings.clone()));
    }

    fn report(&mut self, f: impl FnOnce(&mut Self) -> Result<()>) {
        if let Err(e) = f(self) {
            self.error(None, format!("{e:#}"));
        }
    }

    fn log(&self, account: Option<&str>, text: String) {
        self.emit.send(Event::Log {
            account: account.map(String::from),
            text,
        });
    }

    fn error(&self, account: Option<&str>, text: String) {
        self.emit.send(Event::Error {
            account: account.map(String::from),
            text,
        });
    }
}

/// Moves a file or folder to the Trash (the Recycle Bin on Windows).
fn move_to_trash(path: &Path) -> Result<()> {
    if cfg!(target_os = "linux") && crate::flatpak::is_sandboxed() {
        return crate::flatpak::trash(path);
    }
    #[allow(unused_mut)]
    let mut ctx = trash::TrashContext::default();
    // The default asks Finder through AppleScript, and macOS then asks the
    // user for a permission. NSFileManager needs no permission.
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        ctx.set_delete_method(DeleteMethod::NsFileManager);
    }
    ctx.delete(path)?;
    Ok(())
}

/// The synced local files of an account.
fn take_files(a: &Account) -> Snapshot {
    Snapshot::take(&a.id, &a.local_dir, |rel| a.is_synced_path(rel))
}

/// Before DriveCraft had its own rclone configuration file, the accounts
/// were in the default rclone file. On the first start with the new file,
/// this copies the accounts into it. The default file does not change.
/// Returns the names of the copied accounts.
fn copy_old_accounts(binary: &Path, settings: &Settings, config: &Path) -> Vec<String> {
    if config.exists() || settings.accounts.is_empty() {
        return Vec::new();
    }
    let Some(text) = default_config_file(binary).and_then(|f| fs::read_to_string(f).ok()) else {
        return Vec::new();
    };
    let mut out = String::new();
    let mut names = Vec::new();
    for a in &settings.accounts {
        if let Some(section) = config_section(&text, &a.id) {
            out.push_str(&section);
            names.push(a.name.clone());
        }
    }
    if names.is_empty() || write_private(config, out.as_bytes()).is_err() {
        return Vec::new();
    }
    names
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// A file that is transferring now.
#[derive(Debug, Clone, PartialEq)]
pub struct Transfer {
    /// `Downloaded` for a download, `Uploaded` for an upload.
    pub kind: ChangeKind,
    pub path: String,
    /// Done, from 0 to 1, when rclone knows it.
    pub fraction: Option<f32>,
}

/// The copies and the failed copies in an rclone `core/transferred` answer.
#[derive(Debug, Default, PartialEq)]
struct LiveTransfers {
    changes: Vec<FileChange>,
    /// The path and the rclone error of each failed copy.
    errors: Vec<(String, String)>,
}

/// Reads the transfers that are not in `seen`, and adds them to it. A copy
/// from `remote_fs` (for example "onedrive:") is a download.
fn live_transfers(v: &Value, remote_fs: &str, seen: &mut HashSet<String>) -> LiveTransfers {
    let mut out = LiveTransfers::default();
    for t in v["transferred"].as_array().into_iter().flatten() {
        // `checked` entries compare files; they copy nothing.
        if t["checked"].as_bool().unwrap_or(false) {
            continue;
        }
        let Some(name) = t["name"].as_str() else {
            continue;
        };
        let key = format!("{}|{}|{}", t["group"], name, t["started_at"]);
        if !seen.insert(key) {
            continue;
        }
        let error = t["error"].as_str().unwrap_or_default();
        if !error.is_empty() {
            out.errors.push((name.to_string(), error.to_string()));
            continue;
        }
        let kind = if t["srcFs"].as_str() == Some(remote_fs) {
            ChangeKind::Downloaded
        } else {
            ChangeKind::Uploaded
        };
        out.changes.push(FileChange {
            kind,
            path: name.to_string(),
        });
    }
    out
}

/// True when the provider refuses a download because it marks the file as
/// infected (OneDrive `malwareDetected`, Google Drive
/// `cannotDownloadAbusiveFile`).
fn is_blocked_error(error: &str) -> bool {
    error.contains("malwareDetected") || error.contains("cannotDownloadAbusiveFile")
}

/// A sign-in that takes longer stops. Without a limit, a hanging rclone
/// (for example behind a proxy) shows "Finish the sign-in" forever.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Waits for `rclone config create`. Its output goes to the log file, so
/// the error text points there.
fn wait_for_sign_in(mut child: std::process::Child, limit: Duration) -> Result<(), String> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => {
                return Err(format!(
                    "rclone could not finish the sign-in. The details are in {}.",
                    Settings::log_file().display()
                ));
            }
            Ok(None) if start.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "The sign-in did not finish in {} minutes. A proxy server or a firewall can block rclone. The details are in {}.",
                    limit.as_secs() / 60,
                    Settings::log_file().display()
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(500)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// The rclone options that come from the settings, as environment
/// variables. One switch covers each provider that can block files.
fn rclone_env(settings: &Settings) -> Vec<(&'static str, &'static str)> {
    if settings.download_blocked_files {
        vec![
            ("RCLONE_ONEDRIVE_AV_OVERRIDE", "true"),
            ("RCLONE_DRIVE_ACKNOWLEDGE_ABUSE", "true"),
        ]
    } else {
        Vec::new()
    }
}

/// The text, the done fraction, and the current transfers of a sync, from
/// an rclone `core/stats` answer.
fn progress(st: &Value, remote_fs: &str) -> (String, Option<f32>, Vec<Transfer>) {
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
    let current = st["transferring"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            Some(Transfer {
                kind: if t["srcFs"].as_str() == Some(remote_fs) {
                    ChangeKind::Downloaded
                } else {
                    ChangeKind::Uploaded
                },
                path: t["name"].as_str()?.to_string(),
                fraction: t["percentage"]
                    .as_f64()
                    .map(|p| (p / 100.0).clamp(0.0, 1.0) as f32),
            })
        })
        .collect();
    (text, fraction, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real rclone 1.76 answer during a bisync.
    #[test]
    fn live_transfers_reads_direction_errors_and_skips_seen_entries() {
        let v = json!({ "transferred": [
            { "name": "Docs/a.pdf", "error": "", "checked": false, "group": "job/6",
              "started_at": "2026-11-18T23:08:42Z", "srcFs": "onedrive:",
              "dstFs": "/Users/demo/DriveCraft/OneDrive", "what": "transferring" },
            { "name": "notes.md", "error": "", "checked": false, "group": "job/6",
              "started_at": "2026-11-18T23:08:43Z", "srcFs": "/Users/demo/DriveCraft/OneDrive",
              "dstFs": "onedrive:", "what": "transferring" },
            { "name": "bad.exe", "checked": false, "group": "job/6",
              "started_at": "2026-11-18T23:08:44Z", "srcFs": "onedrive:",
              "error": "failed to open source object: {\"code\":\"malwareDetected\"}" },
            { "name": "same.txt", "error": "", "checked": true, "group": "job/6",
              "started_at": "2026-11-18T23:08:45Z" }
        ]});
        let mut seen = HashSet::new();
        let live = live_transfers(&v, "onedrive:", &mut seen);
        assert_eq!(
            live.changes,
            vec![
                FileChange {
                    kind: ChangeKind::Downloaded,
                    path: "Docs/a.pdf".into()
                },
                FileChange {
                    kind: ChangeKind::Uploaded,
                    path: "notes.md".into()
                },
            ]
        );
        assert_eq!(live.errors.len(), 1);
        assert!(is_blocked_error(&live.errors[0].1));
        assert!(!is_blocked_error("HTTP error 500"));
        // The next poll returns the same entries: nothing is new.
        assert_eq!(
            live_transfers(&v, "onedrive:", &mut seen),
            LiveTransfers::default()
        );
    }
}
