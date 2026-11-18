//! The DriveCraft desktop UI. This is a thin egui layer on top of
//! `drivecraft-core`. It holds no sync logic. It only sends commands.

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

use drivecraft_core::{ChangeKind, Command, EngineHandle, Event, FileChange, Settings};
use eframe::egui::{self, ViewportCommand};

use crate::tray::{Tray, TrayAction, TrayState};

fn main() -> eframe::Result {
    let icon = art::render(art::APP_ICON, 256);
    let mut options = eframe::NativeOptions {
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
    // Like OneDrive, DriveCraft lives in the menu bar and has no Dock icon.
    #[cfg(target_os = "macos")]
    {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            builder.with_activation_policy(ActivationPolicy::Accessory);
        }));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = &mut options;
    eframe::run_native(
        "DriveCraft",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Folders,
    Activity,
    Settings,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Success,
    Info,
    Error,
    File(ChangeKind),
}

struct Entry {
    kind: EntryKind,
    /// The message, or the file name for a file entry.
    text: String,
    /// The folder of a file entry.
    folder: Option<String>,
    time: String,
}

/// The maximum number of file entries that one sync adds to the activity.
const MAX_FILES_PER_SYNC: usize = 200;

/// What the status card and the tray icon show.
#[derive(Clone, PartialEq)]
enum Status {
    Starting,
    Syncing { text: String, fraction: Option<f32> },
    Error(String),
    UpToDate,
}

struct App {
    engine: EngineHandle,
    tray: Option<Tray>,
    tray_rx: Receiver<TrayAction>,
    quitting: bool,
    app_icon: Option<egui::TextureHandle>,

    page: Page,
    appearance: Appearance,

    fatal: Option<String>,
    rclone: Option<(String, PathBuf)>,
    settings: Option<Settings>,
    remotes: Vec<String>,
    connecting: bool,
    tree: HashMap<String, DirNode>,
    expanded: HashSet<String>,
    storage: Option<(u64, u64)>,

    syncing: Option<(String, Option<f32>)>,
    last_error: Option<String>,
    last_sync: Option<String>,
    activity: VecDeque<Entry>,

    // The settings form.
    form_interval: u64,
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
            app_icon: None,
            page: Page::Overview,
            appearance,
            fatal: None,
            rclone: None,
            settings: None,
            remotes: Vec::new(),
            connecting: false,
            tree: HashMap::new(),
            expanded: HashSet::new(),
            storage: None,
            syncing: None,
            last_error: None,
            last_sync: None,
            activity: VecDeque::new(),
            form_interval: 10,
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

    fn add_activity(&mut self, kind: EntryKind, text: String) {
        self.push_entry(kind, text, None);
    }

    fn push_entry(&mut self, kind: EntryKind, text: String, folder: Option<String>) {
        let time = now_hhmm();
        // The same entry again only updates the time, so periodic syncs
        // do not fill the list.
        if let Some(last) = self.activity.front_mut()
            && last.kind == kind
            && last.text == text
            && last.folder == folder
        {
            last.time = time;
            return;
        }
        self.activity.push_front(Entry {
            kind,
            text,
            folder,
            time,
        });
        self.activity.truncate(1000);
    }

    /// Adds one entry for each changed file. The newest entry is on top,
    /// so the files go in reverse order to show them sorted by path.
    fn add_files(&mut self, changes: Vec<FileChange>) {
        let extra = changes.len().saturating_sub(MAX_FILES_PER_SYNC);
        if extra > 0 {
            let text = format!("{extra} more files changed.");
            self.add_activity(EntryKind::Info, text);
        }
        for change in changes.into_iter().take(MAX_FILES_PER_SYNC).rev() {
            let (folder, name) = match change.path.rsplit_once('/') {
                Some((folder, name)) => (folder.to_string(), name.to_string()),
                None => (String::new(), change.path),
            };
            self.push_entry(EntryKind::File(change.kind), name, Some(folder));
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
                Event::Settings(s) => {
                    let first = self.settings.is_none();
                    let remote_changed =
                        self.settings.as_ref().map(|o| &o.remote) != Some(&s.remote);
                    if remote_changed {
                        self.tree.clear();
                        self.expanded.clear();
                        self.storage = None;
                    }
                    if first || remote_changed {
                        self.form_interval = s.sync_interval_minutes;
                    }
                    if s.remote.is_some() {
                        self.connecting = false;
                    }
                    self.settings = Some(s);
                }
                Event::Remotes(r) => self.remotes = r,
                Event::AccountConnecting => self.connecting = true,
                Event::Dir { path, result } => {
                    let node = match result {
                        Ok(children) => DirNode::Loaded(children),
                        Err(e) => DirNode::Failed(e),
                    };
                    self.tree.insert(path, node);
                }
                Event::About { used, total } => self.storage = used.zip(total),
                Event::SyncProgress { text, fraction } => self.syncing = Some((text, fraction)),
                Event::SyncFinished { ok, message } => {
                    self.syncing = None;
                    if ok {
                        self.last_error = None;
                        self.last_sync = Some(now_hhmm());
                        self.add_activity(EntryKind::Success, message);
                    } else {
                        self.last_error = Some(message.clone());
                        self.add_activity(EntryKind::Error, message);
                    }
                }
                Event::FilesChanged(changes) => self.add_files(changes),
                Event::Log(m) => self.add_activity(EntryKind::Info, m),
                Event::Error(e) => {
                    if e.starts_with("Cannot connect the account") {
                        self.connecting = false;
                    }
                    self.add_activity(EntryKind::Error, e);
                }
            }
        }
    }

    fn status(&self) -> Status {
        if self.settings.is_none() {
            return Status::Starting;
        }
        if let Some((text, fraction)) = &self.syncing {
            return Status::Syncing {
                text: text.clone(),
                fraction: *fraction,
            };
        }
        if let Some(e) = &self.last_error {
            return Status::Error(e.clone());
        }
        Status::UpToDate
    }

    fn show_window(&self, ctx: &egui::Context) {
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
                    if let Some(s) = &self.settings {
                        let _ = std::fs::create_dir_all(&s.local_dir);
                        open_in_file_manager(&s.local_dir);
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
        }

        let (state, text) = match self.status() {
            Status::Starting => (TrayState::Idle, "Starting…".to_string()),
            Status::Syncing { .. } => (TrayState::Syncing, "Syncing…".to_string()),
            Status::Error(_) => (TrayState::Error, "Sync problem".to_string()),
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
        } else if self.settings.as_ref().is_some_and(|s| s.remote.is_none()) {
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

    fn demo_app(page: Page) -> App {
        let (engine, _events) = EngineHandle::detached();
        let (_tx, rx) = channel();
        let mut app = App::from_parts(engine, None, rx, Appearance::System);
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/home/demo".into()));
        let mut s = Settings {
            remote: Some("onedrive".into()),
            local_dir: home.join("OneDrive"),
            ..Settings::default()
        };
        s.pinned = ["Documents".to_string(), "Photos/2026".to_string()].into();
        s.pending_pin = ["Projects".to_string()].into();
        s.needs_resync = false;
        app.settings = Some(s);
        app.form_interval = 10;
        app.page = page;
        app.storage = Some((412 * 1024u64.pow(3), 1024u64.pow(4)));
        app.remotes = vec!["onedrive".into()];
        app.rclone = Some(("v1.76.0".into(), PathBuf::from("rclone")));
        app.last_sync = Some("15:42".into());
        let dirs = |v: &[&str]| DirNode::Loaded(v.iter().map(|s| s.to_string()).collect());
        app.tree.insert(
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
        app.tree
            .insert("Photos".into(), dirs(&["2024", "2025", "2026"]));
        app.tree
            .insert("Documents".into(), dirs(&["Invoices", "Taxes"]));
        for p in [
            "Photos/2024",
            "Photos/2025",
            "Photos/2026",
            "Documents/Invoices",
            "Documents/Taxes",
        ] {
            app.tree.insert(p.into(), dirs(&[]));
        }
        app.expanded = ["Photos".to_string(), "Documents".to_string()].into();
        for (kind, text) in [
            (EntryKind::Info, "The account \"onedrive\" is connected."),
            (EntryKind::Info, "\"Music\" is now only on OneDrive."),
            (
                EntryKind::Error,
                "The sync failed: a file in \"Photos/2026\" changed during the sync.",
            ),
            (EntryKind::Success, "The sync is complete. Nothing changed."),
        ] {
            app.add_activity(kind, text.into());
        }
        let change = |kind, path: &str| FileChange {
            kind,
            path: path.into(),
        };
        app.add_files(vec![
            change(ChangeKind::Downloaded, "Documents/Taxes/2025 return.pdf"),
            change(ChangeKind::Uploaded, "Documents/notes.md"),
            change(ChangeKind::DeletedHere, "Photos/2026/IMG_2041.HEIC"),
            change(ChangeKind::DeletedOnline, "Projects/old draft.docx"),
            change(ChangeKind::Downloaded, "budget.xlsx"),
        ]);
        app.add_activity(
            EntryKind::Success,
            "The sync is complete. 5 files changed.".into(),
        );
        app
    }

    fn shoot(name: &str, mut app: App, dark: bool) {
        app.appearance = if dark {
            Appearance::Dark
        } else {
            Appearance::Light
        };
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1080.0, 720.0))
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
        for dark in [false, true] {
            shoot("overview", demo_app(Page::Overview), dark);
            let mut syncing = demo_app(Page::Overview);
            syncing.syncing = Some(("128 of 342 files · 1210.4 of 3402.0 MB".into(), Some(0.37)));
            shoot("overview-syncing", syncing, dark);
            shoot("folders", demo_app(Page::Folders), dark);
            shoot("activity", demo_app(Page::Activity), dark);
            shoot("settings", demo_app(Page::Settings), dark);
            let mut welcome = demo_app(Page::Overview);
            if let Some(s) = &mut welcome.settings {
                s.remote = None;
            }
            welcome.remotes.clear();
            shoot("welcome", welcome, dark);
        }
    }
}
