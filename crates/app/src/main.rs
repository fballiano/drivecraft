//! The DriveCraft desktop UI. This is a thin egui layer on top of
//! `drivecraft-core`. It holds no sync logic. It only sends commands.

// A release build on Windows opens no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod art;
mod icons;
mod pages;
mod theme;
mod tray;
mod widgets;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use drivecraft_core::{
    Account, ChangeKind, Command, EngineHandle, Event, FileChange, Settings, Transfer,
};
use eframe::egui::{self, ViewportCommand};

use crate::tray::{Tray, TrayAction, TrayState};

fn main() -> eframe::Result {
    let icon = art::render(art::APP_ICON, 256);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DriveCraft")
            .with_inner_size([980.0, 660.0])
            .with_min_inner_size([780.0, 540.0])
            .with_icon(Arc::new(egui::IconData {
                rgba: icon.pixels,
                width: icon.size,
                height: icon.size,
            })),
        ..Default::default()
    };
    eframe::run_native(
        "DriveCraft",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
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
    UpToDate,
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
    dock_icon: Option<bool>,
    app_icon: Option<egui::TextureHandle>,

    page: Page,
    account_tab: AccountTab,
    appearance: Appearance,

    fatal: Option<String>,
    rclone: Option<(String, PathBuf)>,
    settings: Option<Settings>,
    connecting: bool,
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
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
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
        App::from_parts(engine, tray, tray_rx, appearance)
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
            dock_icon: None,
            app_icon: None,
            page: Page::Overview,
            account_tab: AccountTab::Folders,
            appearance,
            fatal: None,
            rclone: None,
            settings: None,
            connecting: false,
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
                Event::AccountConnecting => self.connecting = true,
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
                Event::Error { account, text } => {
                    if text.starts_with("Cannot connect the account") {
                        self.connecting = false;
                    }
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
            self.connecting = false;
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
    }

    /// The status of one account.
    fn account_status(&self, id: &str) -> Status {
        if let Some(s) = self.syncing.as_ref().filter(|s| s.account == id) {
            return Status::Syncing {
                text: s.text.clone(),
                fraction: s.fraction,
            };
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
        self.window_visible = true;
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        while let Ok(action) = self.tray_rx.try_recv() {
            match action {
                TrayAction::Open => self.show_window(ctx),
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
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            self.window_visible = false;
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
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("appearance", self.appearance.key().to_string());
    }

    fn on_exit(&mut self) {
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

    fn shoot_sized(name: &str, mut app: App, dark: bool, width: f32) {
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
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/screenshots");
        std::fs::create_dir_all(&dir).unwrap();
        let mode = if dark { "dark" } else { "light" };
        image.save(dir.join(format!("{name}-{mode}.png"))).unwrap();
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
        }
    }
}
