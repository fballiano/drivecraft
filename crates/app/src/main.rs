//! The DriveCraft desktop UI. This is a thin egui layer on top of
//! `drivecraft-core`. It holds no sync logic. It only sends commands.

// A release build on Windows opens no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod art;
mod autostart;
mod gpu;
mod icons;
mod instance;
mod pages;
mod theme;
mod tray;
mod widgets;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use drivecraft_core::{
    Account, ChangeKind, Command, EngineHandle, Event, FileChange, Settings, SignInStep, Transfer,
};
use eframe::egui::{self, ViewportCommand};

use crate::tray::{Tray, TrayAction, TrayState};

fn main() -> eframe::Result {
    let dir = Settings::dir();
    let (guard, show_request) = match instance::start(&dir) {
        Ok(instance::Start::First(guard, request)) => (Some(guard), Some(request)),
        // The running DriveCraft shows its window.
        Ok(instance::Start::Other) => return Ok(()),
        Err(e) => {
            eprintln!("Cannot check for a running DriveCraft: {e}");
            (None, None)
        }
    };
    let gpu = gpu::choose(&dir);
    let backends = gpu.as_ref().map(gpu::Probe::backends);
    let start_again = gpu.as_ref().is_some_and(|p| !p.tried_all());
    // After a start at login, DriveCraft waits in the tray. Linux has no tray
    // icon yet, so there the window opens.
    let hidden =
        std::env::args().any(|a| a == autostart::ARG) && cfg!(any(target_os = "macos", windows));

    let icon = art::render(art::APP_ICON, 256);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DriveCraft")
            // On Linux, the desktop file and the icon have this name.
            .with_app_id("io.github.fballiano.drivecraft")
            .with_inner_size([980.0, 660.0])
            .with_min_inner_size([780.0, 540.0])
            .with_visible(!hidden)
            .with_icon(Arc::new(egui::IconData {
                rgba: icon.pixels,
                width: icon.size,
                height: icon.size,
            })),
        wgpu_options: wgpu_options(backends),
        ..Default::default()
    };
    let result = eframe::run_native(
        "DriveCraft",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, show_request, gpu, hidden)))),
    );
    // Without a GPU device, eframe returns an error. gpu.txt has no
    // confirmation, so a new start takes the next backend. It needs the lock.
    if let Err(e) = &result
        && start_again
    {
        eprintln!(
            "The window cannot start ({e}). DriveCraft starts again with a different GPU backend."
        );
        drop(guard);
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe)
                .args(std::env::args_os().skip(1))
                .spawn();
        }
    }
    result
}

/// The graphics setup of the window.
/// - `backends`: the choice of `gpu::choose`. None keeps the default of
///   egui-wgpu, which also reads `WGPU_BACKEND`.
/// - All systems: the GPU with the lower power use. An app in the tray must
///   not wake up the discrete GPU of a laptop. `WGPU_POWER_PREF` overrides it.
fn wgpu_options(backends: Option<eframe::wgpu::Backends>) -> eframe::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuSetup, WgpuSetupCreateNew};
    use eframe::wgpu::PowerPreference;

    let mut setup = WgpuSetupCreateNew::without_display_handle();
    if let Some(backends) = backends {
        setup.instance_descriptor.backends = backends;
    }
    if PowerPreference::from_env().is_none() {
        setup.power_preference = PowerPreference::LowPower;
    }
    eframe::WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(setup),
        ..Default::default()
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Page {
    Overview,
    Activity,
    Settings,
    /// The page of one account, by its id.
    Account(String),
    AddAccount,
}

/// The tabs of an account page.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AccountTab {
    Folders,
    Account,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    fn key(self) -> &'static str {
        match self {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }

    fn from_key(key: &str) -> Self {
        match key {
            "light" => Appearance::Light,
            "dark" => Appearance::Dark,
            _ => Appearance::System,
        }
    }

    fn preference(self) -> egui::ThemePreference {
        match self {
            Appearance::System => egui::ThemePreference::System,
            Appearance::Light => egui::ThemePreference::Light,
            Appearance::Dark => egui::ThemePreference::Dark,
        }
    }
}

enum DirNode {
    Loading,
    Loaded(Vec<String>),
    Failed(String),
}

/// The folder tree of one account, as far as the user opened it.
#[derive(Default)]
struct Tree {
    nodes: HashMap<String, DirNode>,
    expanded: HashSet<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Success,
    Info,
    Error,
    File(ChangeKind),
}

struct Entry {
    kind: EntryKind,
    /// The id of the account, or None for a message about the app.
    account: Option<String>,
    /// The message, or the file name for a file entry.
    text: String,
    /// The folder of a file entry.
    folder: Option<String>,
    time: String,
}

/// The maximum number of file entries that one report adds to the activity.
const MAX_FILES_PER_SYNC: usize = 200;

/// What a status card and the tray icon show.
#[derive(Clone, PartialEq)]
enum Status {
    Starting,
    Syncing {
        text: String,
        fraction: Option<f32>,
    },
    Error(String),
    /// After a sign-in: the first sync waits for the folder selection.
    ChooseFolders,
    /// The provider ended the sign-in. The sync waits for a new one.
    SignIn,
    UpToDate,
}

/// The form of a sign-in with a password (`SignIn::Password`).
struct PasswordForm {
    provider: String,
    /// None for a new account, else the account that signs in again.
    account: Option<String>,
    user: String,
    password: String,
    /// The optional code (`SignIn::Password::code`).
    code: String,
}

/// The state of the sign-out dialog.
struct SignOut {
    account: String,
    /// True: move the folder to the Trash after a last sync.
    trash: bool,
}

/// The sync that runs now.
struct Syncing {
    account: String,
    text: String,
    fraction: Option<f32>,
    /// The files that transfer now.
    current: Vec<Transfer>,
}

struct App {
    engine: EngineHandle,
    tray: Option<Tray>,
    tray_rx: Receiver<TrayAction>,
    quitting: bool,
    /// True while the window is on screen. On macOS, DriveCraft has a Dock
    /// icon only then. `dock_icon` is the state that macOS has now.
    window_visible: bool,
    /// True while the window has the focus, and the moment it lost it.
    /// A click on the tray icon can take the focus first, so the toggle
    /// looks a moment back.
    window_focused: bool,
    focus_lost: Option<std::time::Instant>,
    dock_icon: Option<bool>,
    app_icon: Option<egui::TextureHandle>,

    page: Page,
    account_tab: AccountTab,
    appearance: Appearance,

    fatal: Option<String>,
    rclone: Option<(String, PathBuf)>,
    settings: Option<Settings>,
    /// The step of the sign-in that runs.
    sign_in: SignInStep,
    /// The password form, while it is open.
    password_form: Option<PasswordForm>,
    /// The answer to `SignInStep::Question`, while the user types it.
    sign_in_answer: String,
    /// A newer release, until the user selects "Not now".
    update: Option<drivecraft_core::update::Update>,
    /// Per account, by id.
    trees: HashMap<String, Tree>,
    storage: HashMap<String, (u64, u64)>,
    /// The accounts whose provider does not report the storage use.
    no_storage: HashSet<String>,
    /// The top-level local folders that do not sync yet. Overview asks
    /// if each one syncs.
    new_folders: HashMap<String, Vec<String>>,
    errors: HashMap<String, String>,
    last_sync: HashMap<String, String>,

    syncing: Option<Syncing>,
    activity: VecDeque<Entry>,

    // The forms.
    form_interval: u64,
    /// The account name in the Account tab, while the user edits it.
    form_name: Option<(String, String)>,
    /// The account that waits for a second click on "Sign out".
    /// The sign-out dialog, while it is open.
    sign_out: Option<SignOut>,
    /// A provider that needs a value before the sign-in, and the value.
    add_field: Option<(String, String)>,

    /// The requests of later starts to show the window. None in tests.
    show_request: Option<instance::ShowRequest>,
    /// The GPU backend of this run, until the window works with it.
    gpu: Option<gpu::Probe>,
    /// The frames since the window opened last.
    gpu_frames: u32,
    started: std::time::Instant,
    /// The GPU backend for the settings page, for example "DirectX 12".
    gpu_name: Option<&'static str>,
    /// True in the real app: it registers the start at login. Tests do not.
    manage_autostart: bool,
    /// The state of the start at login that this run registered last.
    autostart_applied: Option<bool>,
}

impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        show_request: Option<instance::ShowRequest>,
        gpu: Option<gpu::Probe>,
        hidden: bool,
    ) -> Self {
        theme::install(&cc.egui_ctx);
        let appearance = cc
            .storage
            .and_then(|s| s.get_string("appearance"))
            .map(|k| Appearance::from_key(&k))
            .unwrap_or(Appearance::System);
        cc.egui_ctx.set_theme(appearance.preference());

        let ctx = cc.egui_ctx.clone();
        let build_rclone = PathBuf::from(env!("DRIVECRAFT_BUILD_RCLONE"));
        let engine =
            drivecraft_core::spawn(Arc::new(move || ctx.request_repaint()), Some(build_rclone));

        let (tray_tx, tray_rx) = channel();
        let tray = match tray::create(cc.egui_ctx.clone(), tray_tx) {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("No tray icon: {e}");
                None
            }
        };
        // Without a tray icon, a hidden window cannot open again.
        let hidden = hidden && tray.is_some();
        if !hidden {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::Visible(true));
        }
        let mut app = App::from_parts(engine, tray, tray_rx, appearance);
        app.window_visible = !hidden;
        app.show_request = show_request;
        app.gpu = gpu;
        app.gpu_name = cc
            .wgpu_render_state
            .as_ref()
            .map(|s| gpu::display_name(s.adapter.get_info().backend));
        app.manage_autostart = true;
        app
    }

    fn from_parts(
        engine: EngineHandle,
        tray: Option<Tray>,
        tray_rx: Receiver<TrayAction>,
        appearance: Appearance,
    ) -> Self {
        App {
            engine,
            tray,
            tray_rx,
            quitting: false,
            window_visible: true,
            window_focused: false,
            focus_lost: None,
            dock_icon: None,
            app_icon: None,
            page: Page::Overview,
            account_tab: AccountTab::Folders,
            appearance,
            fatal: None,
            rclone: None,
            settings: None,
            sign_in: SignInStep::Idle,
            password_form: None,
            sign_in_answer: String::new(),
            update: None,
            trees: HashMap::new(),
            storage: HashMap::new(),
            no_storage: HashSet::new(),
            new_folders: HashMap::new(),
            errors: HashMap::new(),
            last_sync: HashMap::new(),
            syncing: None,
            activity: VecDeque::new(),
            form_interval: 10,
            form_name: None,
            sign_out: None,
            add_field: None,
            show_request: None,
            gpu: None,
            gpu_frames: 0,
            started: std::time::Instant::now(),
            gpu_name: None,
            manage_autostart: false,
            autostart_applied: None,
        }
    }

    fn app_icon(&mut self, ctx: &egui::Context) -> egui::TextureHandle {
        self.app_icon
            .get_or_insert_with(|| {
                let icon = art::render(art::APP_ICON, 192);
                let image =
                    egui::ColorImage::from_rgba_unmultiplied([icon.size as usize; 2], &icon.pixels);
                ctx.load_texture("app-icon", image, egui::TextureOptions::LINEAR)
            })
            .clone()
    }

    fn accounts(&self) -> &[Account] {
        self.settings
            .as_ref()
            .map(|s| s.accounts.as_slice())
            .unwrap_or_default()
    }

    fn account(&self, id: &str) -> Option<&Account> {
        self.accounts().iter().find(|a| a.id == id)
    }

    /// The name of an account for the UI. A removed account shows its id.
    fn account_name(&self, id: &str) -> String {
        self.account(id)
            .map_or_else(|| id.to_string(), |a| a.name.clone())
    }

    fn add_activity(&mut self, kind: EntryKind, account: Option<String>, text: String) {
        self.push_entry(kind, account, text, None);
    }

    fn push_entry(
        &mut self,
        kind: EntryKind,
        account: Option<String>,
        text: String,
        folder: Option<String>,
    ) {
        let time = now_hhmm();
        // The same entry again only updates the time, so periodic syncs
        // do not fill the list.
        if let Some(last) = self.activity.front_mut()
            && last.kind == kind
            && last.account == account
            && last.text == text
            && last.folder == folder
        {
            last.time = time;
            return;
        }
        self.activity.push_front(Entry {
            kind,
            account,
            text,
            folder,
            time,
        });
        self.activity.truncate(1000);
    }

    /// Adds one entry for each changed file. The newest entry is on top,
    /// so the files go in reverse order to show them sorted by path.
    fn add_files(&mut self, account: &str, changes: Vec<FileChange>) {
        let extra = changes.len().saturating_sub(MAX_FILES_PER_SYNC);
        if extra > 0 {
            let text = format!("{extra} more files changed.");
            self.add_activity(EntryKind::Info, Some(account.to_string()), text);
        }
        for change in changes.into_iter().take(MAX_FILES_PER_SYNC).rev() {
            let (folder, name) = match change.path.rsplit_once('/') {
                Some((folder, name)) => (folder.to_string(), name.to_string()),
                None => (String::new(), change.path),
            };
            self.push_entry(
                EntryKind::File(change.kind),
                Some(account.to_string()),
                name,
                Some(folder),
            );
        }
    }

    fn handle_events(&mut self) {
        while let Ok(ev) = self.engine.rx.try_recv() {
            match ev {
                Event::Started {
                    rclone_version,
                    rclone_path,
                } => self.rclone = Some((rclone_version, rclone_path)),
                Event::Fatal(e) => self.fatal = Some(e),
                Event::Settings(s) => self.apply_settings(s),
                Event::SignIn(step) => {
                    if !matches!(step, SignInStep::Question { .. }) {
                        self.sign_in_answer.clear();
                    }
                    self.sign_in = step;
                }
                Event::Dir {
                    account,
                    path,
                    result,
                } => {
                    let node = match result {
                        Ok(children) => DirNode::Loaded(children),
                        Err(e) => DirNode::Failed(e),
                    };
                    self.trees
                        .entry(account)
                        .or_default()
                        .nodes
                        .insert(path, node);
                }
                Event::About {
                    account,
                    used,
                    total,
                } => match used.zip(total) {
                    Some(u) => {
                        self.no_storage.remove(&account);
                        self.storage.insert(account, u);
                    }
                    None => {
                        self.storage.remove(&account);
                        self.no_storage.insert(account);
                    }
                },
                Event::SyncProgress {
                    account,
                    text,
                    fraction,
                    current,
                } => {
                    self.syncing = Some(Syncing {
                        account,
                        text,
                        fraction,
                        current,
                    });
                }
                Event::SyncFinished {
                    account,
                    ok,
                    message,
                } => {
                    if self.syncing.as_ref().is_some_and(|s| s.account == account) {
                        self.syncing = None;
                    }
                    if ok {
                        self.errors.remove(&account);
                        self.last_sync.insert(account.clone(), now_hhmm());
                        self.add_activity(EntryKind::Success, Some(account), message);
                    } else {
                        self.errors.insert(account.clone(), message.clone());
                        self.add_activity(EntryKind::Error, Some(account), message);
                    }
                }
                Event::FilesChanged { account, changes } => self.add_files(&account, changes),
                Event::NewLocalFolders { account, names } => {
                    self.new_folders.insert(account, names);
                }
                Event::Log { account, text } => self.add_activity(EntryKind::Info, account, text),
                Event::UpdateAvailable(update) => self.update = Some(update),
                Event::Error { account, text } => {
                    self.add_activity(EntryKind::Error, account, text);
                }
            }
        }
    }

    /// New settings from the engine. A new account opens its page, so the
    /// user chooses its folders first.
    fn apply_settings(&mut self, s: Settings) {
        let first = self.settings.is_none();
        let old: HashSet<String> = self.accounts().iter().map(|a| a.id.clone()).collect();
        let added = s
            .accounts
            .iter()
            .find(|a| !old.contains(&a.id) && (!first || a.awaiting_selection));
        if let Some(a) = added {
            self.page = Page::Account(a.id.clone());
            self.account_tab = AccountTab::Folders;
        }
        if first {
            self.form_interval = s.sync_interval_minutes;
        }
        // Forget the state of removed accounts.
        let ids: HashSet<String> = s.accounts.iter().map(|a| a.id.clone()).collect();
        self.trees.retain(|k, _| ids.contains(k));
        self.storage.retain(|k, _| ids.contains(k));
        self.no_storage.retain(|k| ids.contains(k));
        self.new_folders.retain(|k, _| ids.contains(k));
        self.errors.retain(|k, _| ids.contains(k));
        if let Page::Account(id) = &self.page
            && !ids.contains(id)
        {
            self.page = Page::Overview;
        }
        self.settings = Some(s);
        self.apply_autostart();
    }

    /// Registers or removes the start at login. DriveCraft registers only
    /// while at least one account exists: without an account, a start at
    /// login has nothing to sync.
    fn apply_autostart(&mut self) {
        let Some(s) = &self.settings else {
            return;
        };
        if !self.manage_autostart || !autostart::supported() {
            return;
        }
        let wanted = s.start_at_login && !s.accounts.is_empty();
        if self.autostart_applied == Some(wanted) {
            return;
        }
        self.autostart_applied = Some(wanted);
        if let Err(e) = autostart::set(wanted) {
            let text = format!("Cannot change the start at login: {e}");
            self.add_activity(EntryKind::Error, None, text);
        }
    }

    /// The status of one account.
    fn account_status(&self, id: &str) -> Status {
        if let Some(s) = self.syncing.as_ref().filter(|s| s.account == id) {
            return Status::Syncing {
                text: s.text.clone(),
                fraction: s.fraction,
            };
        }
        if self.account(id).is_some_and(|a| a.needs_sign_in) {
            return Status::SignIn;
        }
        if let Some(e) = self.errors.get(id) {
            return Status::Error(e.clone());
        }
        if self.account(id).is_some_and(|a| a.awaiting_selection) {
            return Status::ChooseFolders;
        }
        Status::UpToDate
    }

    /// The status of the app: a sync that runs, else the first problem.
    fn status(&self) -> Status {
        if self.settings.is_none() {
            return Status::Starting;
        }
        let all: Vec<Status> = self
            .accounts()
            .iter()
            .map(|a| self.account_status(&a.id))
            .collect();
        all.iter()
            .find(|s| matches!(s, Status::Syncing { .. }))
            .or_else(|| all.iter().find(|s| matches!(s, Status::SignIn)))
            .or_else(|| all.iter().find(|s| matches!(s, Status::Error(_))))
            .or_else(|| all.iter().find(|s| matches!(s, Status::ChooseFolders)))
            .cloned()
            .unwrap_or(Status::UpToDate)
    }

    /// The folder that "Open DriveCraft folder" opens: `~/DriveCraft`, or the
    /// folder of the only account if it is somewhere else.
    fn drive_folder(&self) -> Option<PathBuf> {
        let root = dirs_home().map(|h| drivecraft_core::settings::accounts_root(&h))?;
        match self.accounts() {
            [] => None,
            [only] if !only.local_dir.starts_with(&root) => Some(only.local_dir.clone()),
            _ => Some(root),
        }
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        // A driver can crash when the window draws, so the backend counts
        // as confirmed again only after some frames.
        if let Some(gpu) = &mut self.gpu {
            gpu.unconfirm();
        }
        self.gpu_frames = 0;
        self.window_visible = true;
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    /// Hides the window. DriveCraft continues in the tray.
    fn hide_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        self.window_visible = false;
    }

    /// True when the window is on screen and in front.
    fn window_in_front(&self, ctx: &egui::Context) -> bool {
        let minimized = ctx.input(|i| i.viewport().minimized).unwrap_or(false);
        self.window_visible
            && !minimized
            && (self.window_focused
                || self
                    .focus_lost
                    .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(1)))
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        // A change of focus repaints, so this sees each change at once.
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
        if self.window_focused && !focused {
            self.focus_lost = Some(std::time::Instant::now());
        }
        self.window_focused = focused;
        while let Ok(action) = self.tray_rx.try_recv() {
            match action {
                TrayAction::Toggle if self.window_in_front(ctx) => self.hide_window(ctx),
                TrayAction::Toggle | TrayAction::Open => self.show_window(ctx),
                TrayAction::SyncNow => self.engine.send(Command::SyncNow),
                TrayAction::OpenDrive => {
                    if let Some(dir) = self.drive_folder() {
                        let _ = std::fs::create_dir_all(&dir);
                        open_in_file_manager(&dir);
                    }
                }
                TrayAction::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
        }

        // Closing the window hides it. DriveCraft continues in the tray.
        if ctx.input(|i| i.viewport().close_requested()) && self.tray.is_some() && !self.quitting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_window(ctx);
        }
        if self.dock_icon != Some(self.window_visible) {
            set_dock_icon(self.window_visible);
            self.dock_icon = Some(self.window_visible);
        }

        let (state, text) = match self.status() {
            Status::Starting => (TrayState::Idle, "Starting…".to_string()),
            Status::Syncing { .. } => (TrayState::Syncing, "Syncing…".to_string()),
            Status::Error(_) => (TrayState::Error, "Sync problem".to_string()),
            Status::ChooseFolders => (TrayState::Idle, "Choose your folders".to_string()),
            Status::SignIn => (TrayState::Error, "Sign in again".to_string()),
            Status::UpToDate => (TrayState::UpToDate, "Up to date".to_string()),
        };
        let state = if self.fatal.is_some() {
            TrayState::Error
        } else {
            state
        };
        if let Some(tray) = &mut self.tray {
            tray.set_state(state, &text);
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        if let Some(err) = self.fatal.clone() {
            pages::fatal(self, ui, &err);
        } else if self
            .settings
            .as_ref()
            .is_some_and(|s| s.accounts.is_empty())
        {
            pages::welcome(self, ui);
        } else {
            pages::main_layout(self, ui);
        }
    }

    fn set_appearance(&mut self, ctx: &egui::Context, a: Appearance) {
        self.appearance = a;
        ctx.set_theme(a.preference());
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events();
        self.handle_tray(ctx);
        // A second start of DriveCraft asks this one to show its window.
        if let Some(request) = &mut self.show_request {
            if request.take() {
                self.show_window(ctx);
            }
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        // A hidden window draws nothing. The GPU device works if the app
        // runs for some seconds.
        if !self.window_visible
            && self.started.elapsed() > std::time::Duration::from_secs(5)
            && let Some(gpu) = &mut self.gpu
        {
            gpu.confirm();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
        // The third frame on screen: the first two were drawn without a
        // crash of the driver.
        if self.window_visible
            && let Some(gpu) = &mut self.gpu
        {
            self.gpu_frames += 1;
            if self.gpu_frames >= 3 && !gpu.is_confirmed() {
                gpu.confirm();
            }
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("appearance", self.appearance.key().to_string());
    }

    fn on_exit(&mut self) {
        // A normal exit: the backend did not crash.
        if let Some(gpu) = &mut self.gpu {
            gpu.confirm();
        }
        self.engine.shutdown();
    }
}

/// On macOS, shows the Dock icon while the window is open, so the window
/// is in Command-Tab and has a menu bar. With the window closed,
/// DriveCraft is only in the menu bar, like OneDrive.
#[cfg(target_os = "macos")]
fn set_dock_icon(visible: bool) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    // `logic` runs on the main thread, so this is always Some.
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let policy = if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    NSApplication::sharedApplication(mtm).setActivationPolicy(policy);
}

#[cfg(not(target_os = "macos"))]
fn set_dock_icon(_visible: bool) {}

fn now_hhmm() -> String {
    jiff::ZonedDateTime::now().strftime("%H:%M").to_string()
}

pub fn file_manager_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Finder"
    } else if cfg!(windows) {
        "Explorer"
    } else {
        "Files"
    }
}

/// Opens a web page in the default browser.
pub fn open_url(url: &str) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(url).spawn();
}

pub fn open_in_file_manager(path: &Path) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(path).spawn();
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Renders each page with demo data to `target/screenshots/*.png`.
/// Run it with `cargo test -p drivecraft screenshots -- --ignored`.
#[cfg(test)]
mod preview {
    use super::*;
    use egui_kittest::Harness;

    /// Two accounts: "Personal" syncs, "Work" waits for its folder selection.
    fn demo_app(page: Page) -> App {
        let (engine, _events) = EngineHandle::detached();
        let (_tx, rx) = channel();
        let mut app = App::from_parts(engine, None, rx, Appearance::System);
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/home/demo".into()));
        let personal = Account {
            id: "onedrive".into(),
            name: "Personal".into(),
            provider: "onedrive".into(),
            local_dir: home.join("DriveCraft/Personal"),
            pinned: ["Documents".to_string(), "Photos/2026".to_string()].into(),
            pending_pin: ["Projects".to_string()].into(),
            local_only: ["Scratch".to_string()].into(),
            blocked_files: ["Mail/backup 1999.tar.gz".to_string()].into(),
            needs_resync: false,
            ..Account::default()
        };
        let work = Account {
            id: "onedrive-2".into(),
            name: "Work".into(),
            provider: "onedrive".into(),
            local_dir: home.join("DriveCraft/Work"),
            pending_pin: ["Clients".to_string(), "Templates".to_string()].into(),
            awaiting_selection: true,
            ..Account::default()
        };
        let dropbox = Account {
            id: "dropbox".into(),
            name: "Dropbox".into(),
            provider: "dropbox".into(),
            local_dir: home.join("DriveCraft/Dropbox"),
            pinned: ["Camera Uploads".to_string()].into(),
            needs_resync: false,
            ..Account::default()
        };
        app.settings = Some(Settings {
            accounts: vec![personal, work, dropbox],
            ..Settings::default()
        });
        app.form_interval = 10;
        app.page = page;
        app.storage.insert(
            "onedrive".into(),
            (16 * 1024u64.pow(3), 33 * 1024u64.pow(3)),
        );
        app.storage
            .insert("onedrive-2".into(), (2 * 1024u64.pow(3), 1024u64.pow(4)));
        app.storage.insert(
            "dropbox".into(),
            (1536 * 1024u64.pow(2), 2 * 1024u64.pow(3)),
        );
        app.last_sync.insert("dropbox".into(), "15:38".into());
        app.rclone = Some(("v1.76.0".into(), PathBuf::from("rclone")));
        app.last_sync.insert("onedrive".into(), "15:42".into());
        app.new_folders.insert(
            "onedrive".into(),
            vec!["Band photos".into(), "Recipes".into()],
        );
        let dirs = |v: &[&str]| DirNode::Loaded(v.iter().map(|s| s.to_string()).collect());
        let tree = app.trees.entry("onedrive".into()).or_default();
        tree.nodes.insert(
            "".into(),
            dirs(&[
                "Desktop",
                "Documents",
                "Music",
                "Photos",
                "Projects",
                "Shared with me",
            ]),
        );
        tree.nodes
            .insert("Photos".into(), dirs(&["2024", "2025", "2026"]));
        tree.nodes
            .insert("Documents".into(), dirs(&["Invoices", "Taxes"]));
        for p in [
            "Photos/2024",
            "Photos/2025",
            "Photos/2026",
            "Documents/Invoices",
            "Documents/Taxes",
        ] {
            tree.nodes.insert(p.into(), dirs(&[]));
        }
        tree.expanded = ["Photos".to_string(), "Documents".to_string()].into();
        let work_tree = app.trees.entry("onedrive-2".into()).or_default();
        work_tree
            .nodes
            .insert("".into(), dirs(&["Archive", "Clients", "Templates"]));
        for p in ["Archive", "Clients", "Templates"] {
            work_tree.nodes.insert(p.into(), dirs(&[]));
        }
        let personal_id = Some("onedrive".to_string());
        for (kind, account, text) in [
            (EntryKind::Info, None, "DriveCraft started."),
            (
                EntryKind::Info,
                Some("onedrive-2".to_string()),
                "The account \"Work\" is connected.",
            ),
            (
                EntryKind::Info,
                personal_id.clone(),
                "\"Music\" is now only online.",
            ),
            (
                EntryKind::Error,
                personal_id.clone(),
                "The sync failed: a file in \"Photos/2026\" changed during the sync.",
            ),
            (
                EntryKind::Success,
                personal_id.clone(),
                "The sync is complete. Nothing changed.",
            ),
        ] {
            app.add_activity(kind, account, text.into());
        }
        let change = |kind, path: &str| FileChange {
            kind,
            path: path.into(),
        };
        app.add_files(
            "onedrive",
            vec![
                change(ChangeKind::Downloaded, "Documents/Taxes/2025 return.pdf"),
                change(ChangeKind::Uploaded, "Documents/notes.md"),
                change(ChangeKind::DeletedHere, "Photos/2026/IMG_2041.HEIC"),
                change(ChangeKind::DeletedOnline, "Projects/old draft.docx"),
                change(ChangeKind::Downloaded, "budget.xlsx"),
                change(ChangeKind::Conflict, "Documents/plan.conflict1.docx"),
            ],
        );
        app.add_activity(
            EntryKind::Success,
            personal_id,
            "The sync is complete. 5 files changed.".into(),
        );
        app
    }

    /// The demo without the "Work" account that waits for its folders.
    fn demo_synced(page: Page) -> App {
        let mut app = demo_app(page);
        if let Some(s) = &mut app.settings {
            s.accounts.retain(|a| !a.awaiting_selection);
        }
        app
    }

    fn shoot(name: &str, app: App, dark: bool) {
        shoot_sized(name, app, dark, 1080.0);
    }

    fn shoot_sized(name: &str, app: App, dark: bool, width: f32) {
        let mode = if dark { "dark" } else { "light" };
        render(app, dark, width, "screenshots", &format!("{name}-{mode}"));
    }

    /// Renders `app` to `target/<dir>/<name>.png`.
    fn render(mut app: App, dark: bool, width: f32, dir: &str, name: &str) {
        app.appearance = if dark {
            Appearance::Dark
        } else {
            Appearance::Light
        };
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width, 720.0))
            .with_pixels_per_point(2.0)
            .build_ui_state(
                move |ui, (app, ready): &mut (App, bool)| {
                    // Fonts become active in the next pass, so the first pass only installs them.
                    if !*ready {
                        theme::install(ui.ctx());
                        ui.ctx().set_theme(if dark {
                            egui::Theme::Dark
                        } else {
                            egui::Theme::Light
                        });
                        *ready = true;
                        ui.ctx().request_repaint();
                        return;
                    }
                    app.draw(ui);
                },
                (app, false),
            );
        harness.run_steps(5);
        let image = harness.render().expect("render");
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        image.save(dir.join(format!("{name}.png"))).unwrap();
    }

    /// The frames of `docs/demo.gif`: one more account, from the sign-in to the end of its first sync.
    /// Run it with `cargo test -p drivecraft demo_frames -- --ignored`, then `scripts/demo-gif.sh`.
    #[test]
    #[ignore = "writes PNG files; run on demand"]
    fn demo_frames() {
        let work = || Page::Account("onedrive-2".into());
        let set_work = |app: &mut App, f: &dyn Fn(&mut Account)| {
            if let Some(s) = &mut app.settings {
                s.accounts
                    .iter_mut()
                    .filter(|a| a.id == "onedrive-2")
                    .for_each(f);
            }
        };
        let syncing = |fraction: f32, done: u32, current: Vec<Transfer>| Syncing {
            account: "onedrive-2".into(),
            text: format!("{done} of 418 files · {:.1} of 862.0 MB", 862.0 * fraction),
            fraction: Some(fraction),
            current,
        };
        let mut frames = Vec::new();

        frames.push(demo_app(Page::AddAccount));
        frames.push(demo_app(work()));
        let mut archive = demo_app(work());
        set_work(&mut archive, &|a| {
            a.pending_pin.insert("Archive".into());
        });
        frames.push(archive);
        let mut overview = demo_app(Page::Overview);
        set_work(&mut overview, &|a| {
            a.awaiting_selection = false;
            a.pending_pin.insert("Archive".into());
        });
        overview.syncing = Some(syncing(0.18, 74, Vec::new()));
        frames.push(overview);
        let mut activity = demo_app(Page::Activity);
        set_work(&mut activity, &|a| a.awaiting_selection = false);
        activity.syncing = Some(syncing(
            0.62,
            259,
            vec![
                Transfer {
                    kind: ChangeKind::Downloaded,
                    path: "Clients/Acme/contract.pdf".into(),
                    fraction: Some(0.48),
                },
                Transfer {
                    kind: ChangeKind::Downloaded,
                    path: "Templates/invoice.xlsx".into(),
                    fraction: Some(0.81),
                },
            ],
        ));
        frames.push(activity);
        let mut done = demo_app(Page::Overview);
        set_work(&mut done, &|a| {
            a.awaiting_selection = false;
            a.pinned = ["Archive", "Clients", "Templates"].map(String::from).into();
            a.pending_pin.clear();
        });
        done.last_sync.insert("onedrive-2".into(), "15:47".into());
        frames.push(done);

        for (i, mut app) in frames.into_iter().enumerate() {
            app.new_folders.clear();
            render(app, false, 1080.0, "demo-frames", &format!("{i:02}"));
        }
    }

    #[test]
    #[ignore = "writes PNG files; run on demand"]
    fn screenshots() {
        let personal = || Page::Account("onedrive".into());
        for dark in [false, true] {
            shoot("overview", demo_synced(Page::Overview), dark);
            shoot("overview-first-sync", demo_app(Page::Overview), dark);
            let mut syncing = demo_synced(Page::Overview);
            syncing.syncing = Some(Syncing {
                account: "onedrive".into(),
                text: "128 of 342 files · 1210.4 of 3402.0 MB".into(),
                fraction: Some(0.37),
                current: Vec::new(),
            });
            shoot("overview-syncing", syncing, dark);
            shoot("account-folders", demo_app(personal()), dark);
            let mut account = demo_app(personal());
            account.account_tab = AccountTab::Account;
            shoot("account-settings", account, dark);
            let mut confirm = demo_app(personal());
            confirm.account_tab = AccountTab::Account;
            confirm.sign_out = Some(SignOut {
                account: "onedrive".into(),
                trash: true,
            });
            shoot("account-sign-out", confirm, dark);
            shoot(
                "account-first-sync",
                demo_app(Page::Account("onedrive-2".into())),
                dark,
            );
            shoot("add-account", demo_app(Page::AddAccount), dark);
            let mut kdrive = demo_app(Page::AddAccount);
            kdrive.add_field = Some(("kdrive".into(), "0654321".into()));
            shoot("add-account-kdrive", kdrive, dark);
            shoot_sized(
                "add-account-narrow",
                demo_app(Page::AddAccount),
                dark,
                780.0,
            );
            shoot("activity", demo_app(Page::Activity), dark);
            let mut busy = demo_app(Page::Activity);
            busy.syncing = Some(Syncing {
                account: "onedrive".into(),
                text: "96 of 2294 files · 376.0 of 1752.0 MB".into(),
                fraction: Some(0.21),
                current: vec![
                    Transfer {
                        kind: ChangeKind::Downloaded,
                        path: "Personali/2019/contract.zip".into(),
                        fraction: Some(0.64),
                    },
                    Transfer {
                        kind: ChangeKind::Uploaded,
                        path: "notes.md".into(),
                        fraction: None,
                    },
                ],
            });
            shoot("activity-syncing", busy, dark);
            shoot("settings", demo_app(Page::Settings), dark);
            let mut welcome = demo_app(Page::Overview);
            if let Some(s) = &mut welcome.settings {
                s.accounts.clear();
            }
            shoot("welcome", welcome, dark);
            // The narrowest window: text must wrap, never run under buttons.
            shoot_sized("overview-narrow", demo_app(Page::Overview), dark, 780.0);
            shoot_sized(
                "account-first-sync-narrow",
                demo_app(Page::Account("onedrive-2".into())),
                dark,
                780.0,
            );
            let mut narrow = demo_app(personal());
            narrow.account_tab = AccountTab::Account;
            shoot_sized("account-settings-narrow", narrow, dark, 780.0);
            // A provider ended the sign-in.
            let expired = |page: Page| {
                let mut app = demo_synced(page);
                if let Some(s) = &mut app.settings {
                    s.accounts[0].needs_sign_in = true;
                }
                app
            };
            shoot("overview-sign-in", expired(Page::Overview), dark);
            let mut update = demo_synced(Page::Overview);
            update.update = Some(drivecraft_core::update::Update {
                version: "1.0.0".into(),
                url: "https://github.com/fballiano/drivecraft/releases/latest".into(),
            });
            shoot("overview-update", update, dark);
            shoot("account-sign-in", expired(personal()), dark);
            let mut browser = expired(personal());
            browser.sign_in = SignInStep::Browser {
                account: Some("onedrive".into()),
            };
            shoot("account-sign-in-browser", browser, dark);
            // iCloud: the password form, then the code.
            let mut icloud = demo_app(Page::AddAccount);
            icloud.password_form = Some(PasswordForm {
                provider: "iclouddrive".into(),
                account: None,
                user: "someone@example.com".into(),
                password: "password".into(),
                code: String::new(),
            });
            shoot("add-account-icloud", icloud, dark);
            let mut code = demo_app(Page::AddAccount);
            code.sign_in = SignInStep::Question {
                account: None,
                name: "config_2fa".into(),
                help: String::new(),
                error: "The code is not valid.".into(),
            };
            code.sign_in_answer = "1234".into();
            shoot("add-account-icloud-code", code, dark);
            let mut narrow_form = demo_app(Page::AddAccount);
            narrow_form.password_form = Some(PasswordForm {
                provider: "iclouddrive".into(),
                account: None,
                user: String::new(),
                password: String::new(),
                code: String::new(),
            });
            shoot_sized("add-account-icloud-narrow", narrow_form, dark, 780.0);
            let mut mega = demo_app(Page::AddAccount);
            mega.password_form = Some(PasswordForm {
                provider: "mega".into(),
                account: None,
                user: "someone@example.com".into(),
                password: "password".into(),
                code: String::new(),
            });
            shoot("add-account-mega", mega, dark);
        }
    }
}
