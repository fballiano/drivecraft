//! The screens of DriveCraft.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use drivecraft_core::{ChangeKind, Command, KeepState, Prefs, Settings, Transfer};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, Frame, Id, Layout, Margin, Rect,
    RichText, ScrollArea, Sense, Ui, pos2, vec2,
};

use crate::theme::{RADIUS, Tokens, icon_font, medium, regular, semibold};
use crate::widgets::{self, card, card_title, list_card, page_header, primary, secondary};
use crate::{
    App, Appearance, DirNode, Entry, EntryKind, Page, Status, file_manager_name, human_bytes,
    icons, open_in_file_manager,
};

// ---------------------------------------------------------------- layout

pub fn main_layout(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    egui::Panel::left("sidebar")
        .exact_size(232.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(t.dock).inner_margin(Margin {
            left: 14,
            right: 14,
            top: 18,
            bottom: 16,
        }))
        .show(ui, |ui| sidebar(app, ui));
    egui::CentralPanel::default()
        .frame(Frame::new().fill(t.chrome).inner_margin(Margin {
            left: 32,
            right: 32,
            top: 26,
            bottom: 20,
        }))
        .show(ui, |ui| match app.page {
            Page::Overview => overview(app, ui),
            Page::Folders => folders(app, ui),
            Page::Activity => activity(app, ui),
            Page::Settings => settings(app, ui),
        });
}

fn sidebar(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    ui.horizontal(|ui| {
        let icon = app.app_icon(ui.ctx());
        ui.add(egui::Image::new(&icon).fit_to_exact_size(vec2(30.0, 30.0)));
        ui.add_space(2.0);
        ui.label(
            RichText::new("DriveCraft")
                .font(semibold(16.0))
                .color(t.text),
        );
    });
    ui.add_space(22.0);
    ui.spacing_mut().item_spacing.y = 2.0;
    for (page, icon, label) in [
        (Page::Overview, icons::HOUSE, "Overview"),
        (Page::Folders, icons::FOLDER, "Folders"),
        (Page::Activity, icons::ACTIVITY, "Activity"),
        (Page::Settings, icons::SETTINGS, "Settings"),
    ] {
        if widgets::nav_item(ui, icon, label, app.page == page).clicked() {
            app.page = page;
        }
    }

    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let account = app
            .settings
            .as_ref()
            .and_then(|s| s.remote.clone())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icons::CIRCLE_USER)
                    .font(icon_font(16.0))
                    .color(t.icon),
            );
            ui.label(RichText::new(account).font(medium(13.0)).color(t.text));
        });
        ui.add_space(6.0);
        if let Some((used, total)) = app.storage {
            let fraction = if total > 0 {
                used as f32 / total as f32
            } else {
                0.0
            };
            ui.label(
                RichText::new(format!(
                    "{} of {} used",
                    human_bytes(used),
                    human_bytes(total)
                ))
                .font(regular(12.0))
                .color(t.text_dim),
            );
            widgets::progress(ui, Some(fraction));
            ui.label(
                RichText::new("STORAGE")
                    .font(medium(10.5))
                    .color(t.text_faint),
            );
        }
    });
}

// ---------------------------------------------------------------- overview

fn overview(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    let status = app.status();
    let theme_icon = match app.appearance {
        Appearance::System => icons::SUN_MOON,
        Appearance::Light => icons::SUN,
        Appearance::Dark => icons::MOON,
    };
    page_header(ui, "Overview", "Your OneDrive on this computer.", |ui| {
        if widgets::icon_button(ui, theme_icon, "Change the appearance").clicked() {
            let next = match app.appearance {
                Appearance::System => Appearance::Light,
                Appearance::Light => Appearance::Dark,
                Appearance::Dark => Appearance::System,
            };
            app.set_appearance(ui.ctx(), next);
        }
    });

    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // The status card.
        card(ui, |ui| {
            // Top-aligned: the text block starts at the top of the icon.
            ui.horizontal_top(|ui| {
                let (icon, fg, bg, title, subtitle) = match &status {
                    Status::Starting => (
                        icons::CLOUD,
                        t.icon,
                        t.field,
                        "Starting…".to_string(),
                        String::new(),
                    ),
                    Status::Syncing { text, .. } => (
                        icons::CLOUD,
                        t.accent,
                        t.accent_soft,
                        "Syncing".into(),
                        text.clone(),
                    ),
                    Status::Error(e) => (
                        icons::CLOUD_ALERT,
                        t.danger,
                        t.danger_soft,
                        "Sync problem".into(),
                        e.clone(),
                    ),
                    Status::ChooseFolders => (
                        icons::FOLDER,
                        t.accent,
                        t.accent_soft,
                        "Choose your folders".into(),
                        "The first sync starts after you choose the folders for this computer."
                            .into(),
                    ),
                    Status::UpToDate => (
                        icons::CLOUD_CHECK,
                        t.success,
                        t.success_soft,
                        "Up to date".into(),
                        match &app.last_sync {
                            Some(time) => format!("Last sync at {time}."),
                            None => "Waiting for the first sync.".into(),
                        },
                    ),
                };
                if matches!(status, Status::Syncing { .. }) {
                    let (rect, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 28.0, bg);
                    let spinner = Rect::from_center_size(rect.center(), vec2(24.0, 24.0));
                    egui::Spinner::new()
                        .size(24.0)
                        .color(fg)
                        .paint_at(ui, spinner);
                } else {
                    widgets::icon_circle(ui, icon, 56.0, fg, bg);
                }
                ui.add_space(8.0);
                widgets::text_and_actions(
                    ui,
                    Align::Center,
                    |ui| {
                        ui.add_space(6.0);
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.label(RichText::new(title).font(semibold(18.0)).color(t.text));
                        ui.add(
                            egui::Label::new(
                                RichText::new(subtitle)
                                    .font(regular(13.0))
                                    .color(t.text_dim),
                            )
                            .wrap(),
                        );
                    },
                    |ui, ()| {
                        if status == Status::ChooseFolders {
                            if primary(ui, true, &format!("{}  Choose folders", icons::FOLDER))
                                .clicked()
                            {
                                app.page = Page::Folders;
                            }
                            return;
                        }
                        let syncing = matches!(status, Status::Syncing { .. });
                        if primary(ui, !syncing, &format!("{}  Sync now", icons::REFRESH)).clicked()
                        {
                            app.engine.send(Command::SyncNow);
                        }
                    },
                );
            });
            if let Status::Syncing { fraction, .. } = &status {
                ui.add_space(10.0);
                widgets::progress(ui, *fraction);
            }
        });
        ui.add_space(14.0);

        if !app.new_folders.is_empty() {
            new_folders_card(app, ui);
            ui.add_space(14.0);
        }

        // The local OneDrive folder.
        let Some(s) = app.settings.clone() else {
            return;
        };
        card(ui, |ui| {
            let synced = s.pinned.len();
            let what = match synced {
                0 => "The files in the OneDrive root sync to this computer.".to_string(),
                1 => {
                    "1 folder and the files in the OneDrive root sync to this computer.".to_string()
                }
                n => {
                    format!("{n} folders and the files in the OneDrive root sync to this computer.")
                }
            };
            card_title(
                ui,
                icons::LAPTOP,
                "OneDrive folder",
                Some(&format!(
                    "{what} They are real files, also available without internet."
                )),
            );
            ui.add_space(4.0);
            path_label(ui, &s.local_dir);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if secondary(
                    ui,
                    true,
                    &format!("{}  Open in {}", icons::FOLDER_OPEN, file_manager_name()),
                )
                .clicked()
                {
                    let _ = std::fs::create_dir_all(&s.local_dir);
                    open_in_file_manager(&s.local_dir);
                }
                if secondary(ui, true, "Choose folders").clicked() {
                    app.page = Page::Folders;
                }
            });
        });
        ui.add_space(14.0);

        // Recent activity.
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(icons::ACTIVITY)
                        .font(icon_font(16.0))
                        .color(t.icon),
                );
                ui.label(
                    RichText::new("Recent activity")
                        .font(semibold(14.0))
                        .color(t.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::link(ui, "View all").clicked() {
                        app.page = Page::Activity;
                    }
                });
            });
            ui.add_space(4.0);
            if app.activity.is_empty() {
                ui.label(
                    RichText::new("Nothing yet.")
                        .font(regular(12.5))
                        .color(t.text_faint),
                );
            }
            for entry in app.activity.iter().take(4) {
                activity_row(ui, entry);
            }
        });
    });
}

/// Asks for each new local folder if it syncs or stays only on this computer.
fn new_folders_card(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    let mut commands = Vec::new();
    card(ui, |ui| {
        let title = match app.new_folders.len() {
            1 => "New folder on this computer".to_string(),
            n => format!("{n} new folders on this computer"),
        };
        card_title(
            ui,
            icons::FOLDER_PLUS,
            &title,
            Some(
                "You made these folders in your OneDrive folder. They do not sync yet. \
                 Select \"Sync\" to upload a folder to OneDrive.",
            ),
        );
        ui.add_space(6.0);
        let n = app.new_folders.len();
        for (i, name) in app.new_folders.iter().enumerate() {
            widgets::text_and_actions(
                ui,
                Align::Center,
                |ui| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(40.0);
                        ui.label(
                            RichText::new(icons::FOLDER)
                                .font(icon_font(16.0))
                                .color(t.icon),
                        );
                        ui.add(
                            egui::Label::new(RichText::new(name).font(medium(13.5)).color(t.text))
                                .truncate(),
                        );
                    });
                },
                |ui, ()| {
                    if primary(ui, true, &format!("{}  Sync", icons::UPLOAD))
                        .on_hover_text("Upload this folder to OneDrive and keep it in sync")
                        .clicked()
                    {
                        commands.push(Command::SetKeep {
                            path: name.clone(),
                            keep: true,
                        });
                    }
                    if secondary(ui, true, "Do not sync")
                        .on_hover_text("Keep this folder only on this computer")
                        .clicked()
                    {
                        commands.push(Command::KeepLocalOnly(name.clone()));
                    }
                },
            );
            if i + 1 < n {
                widgets::separator(ui);
            }
        }
    });
    for c in commands {
        app.engine.send(c);
    }
}

fn path_label(ui: &mut Ui, path: &std::path::Path) {
    let t = Tokens::of(ui);
    let home = dirs_home();
    let shown = match home.and_then(|h| {
        path.strip_prefix(h)
            .ok()
            .map(|p| PathBuf::from("~").join(p))
    }) {
        Some(p) => p.display().to_string(),
        None => path.display().to_string(),
    };
    Frame::new()
        .fill(t.field)
        .corner_radius(CornerRadius::same(RADIUS - 2))
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(shown)
                        .font(egui::FontId::monospace(11.5))
                        .color(t.text_dim),
                )
                .truncate(),
            );
        });
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

fn activity_row(ui: &mut Ui, entry: &Entry) {
    let t = Tokens::of(ui);
    let (icon, color, action) = match entry.kind {
        EntryKind::Success => (icons::CIRCLE_CHECK, t.success, ""),
        EntryKind::Info => (icons::INFO, t.text_faint, ""),
        EntryKind::Error => (icons::TRIANGLE_ALERT, t.danger, ""),
        EntryKind::File(ChangeKind::Downloaded) => (icons::DOWNLOAD, t.accent, "Downloaded"),
        EntryKind::File(ChangeKind::Uploaded) => (icons::UPLOAD, t.accent, "Uploaded"),
        EntryKind::File(ChangeKind::DeletedHere) => (
            icons::TRASH,
            t.text_dim,
            "Deleted on this computer (deleted on OneDrive)",
        ),
        EntryKind::File(ChangeKind::DeletedOnline) => (
            icons::TRASH,
            t.text_dim,
            "Deleted from OneDrive (deleted on this computer)",
        ),
    };
    ui.horizontal(|ui| {
        ui.set_min_height(if entry.folder.is_some() { 40.0 } else { 28.0 });
        ui.label(RichText::new(icon).font(icon_font(15.0)).color(color));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(&entry.time)
                    .font(regular(12.0))
                    .color(t.text_faint),
            );
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let Some(folder) = &entry.folder else {
                    // A message: one line, centered on the icon.
                    ui.add(
                        egui::Label::new(
                            RichText::new(&entry.text).font(regular(13.0)).color(t.text),
                        )
                        .truncate(),
                    );
                    return;
                };
                // A file: the name, and the action and folder under it.
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(
                        egui::Label::new(
                            RichText::new(&entry.text).font(medium(13.0)).color(t.text),
                        )
                        .truncate(),
                    );
                    let place = if folder.is_empty() {
                        "OneDrive"
                    } else {
                        folder.as_str()
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{action} · in {place}"))
                                .font(regular(11.5))
                                .color(t.text_faint),
                        )
                        .truncate(),
                    );
                });
            });
        });
    });
}

// ---------------------------------------------------------------- folders

enum FolderAction {
    Expand(String),
    Collapse(String),
    Load(String),
    Keep(String, bool),
}

fn folders(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    page_header(
        ui,
        "Folders",
        "Turn on the folders that sync to this computer. Folders that are off stay only on OneDrive.",
        |ui| {
            if widgets::icon_button(ui, icons::LIST_RESTART, "Reload the folder list").clicked() {
                app.tree.clear();
            }
        },
    );
    let Some(s) = app.settings.clone() else {
        return;
    };
    if s.awaiting_selection {
        first_sync_card(app, ui);
        ui.add_space(14.0);
    }
    let mut actions = Vec::new();
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        list_card(ui, |ui| {
            folder_children(ui, t, 0, "", &app.tree, &app.expanded, &s, &mut actions);
        });
    });
    for a in actions {
        match a {
            FolderAction::Expand(p) => {
                app.expanded.insert(p);
            }
            FolderAction::Collapse(p) => {
                app.expanded.remove(&p);
            }
            FolderAction::Load(p) => {
                app.tree.insert(p.clone(), DirNode::Loading);
                app.engine.send(Command::ListDir(p));
            }
            FolderAction::Keep(path, keep) => app.engine.send(Command::SetKeep { path, keep }),
        }
    }
}

/// After a sign-in: the user switches off the folders that stay only on
/// OneDrive, then starts the first sync. Nothing downloads before that.
fn first_sync_card(app: &mut App, ui: &mut Ui) {
    card(ui, |ui| {
        widgets::text_and_actions(
            ui,
            Align::Center,
            |ui| {
                card_title(
                    ui,
                    icons::FOLDER_CHECK,
                    "Choose the folders for this computer",
                    Some(
                        "Turn off the folders that you want only on OneDrive. \
                         Nothing downloads until you start the first sync.",
                    ),
                );
            },
            |ui, ()| {
                if primary(ui, true, &format!("{}  Start sync", icons::REFRESH)).clicked() {
                    app.engine.send(Command::ConfirmSelection);
                    app.page = Page::Overview;
                }
            },
        );
    });
}

#[allow(clippy::too_many_arguments)]
fn folder_children(
    ui: &mut Ui,
    t: &Tokens,
    depth: usize,
    path: &str,
    tree: &HashMap<String, DirNode>,
    expanded: &HashSet<String>,
    s: &Settings,
    actions: &mut Vec<FolderAction>,
) {
    let indent = 12.0 + depth as f32 * 24.0;
    match tree.get(path) {
        None | Some(DirNode::Loading) => {
            if tree.get(path).is_none() {
                actions.push(FolderAction::Load(path.to_string()));
            }
            ui.horizontal(|ui| {
                ui.set_min_height(38.0);
                ui.add_space(indent + 24.0);
                ui.add(egui::Spinner::new().size(14.0).color(t.text_faint));
                ui.label(
                    RichText::new("Loading…")
                        .font(regular(12.5))
                        .color(t.text_faint),
                );
            });
        }
        Some(DirNode::Failed(e)) => {
            ui.horizontal(|ui| {
                ui.set_min_height(38.0);
                ui.add_space(indent + 24.0);
                ui.label(
                    RichText::new(icons::TRIANGLE_ALERT)
                        .font(icon_font(14.0))
                        .color(t.danger),
                );
                ui.add(
                    egui::Label::new(RichText::new(e).font(regular(12.5)).color(t.danger))
                        .truncate(),
                );
            });
        }
        Some(DirNode::Loaded(children)) if children.is_empty() => {
            if depth == 0 {
                ui.horizontal(|ui| {
                    ui.set_min_height(38.0);
                    ui.add_space(indent);
                    ui.label(
                        RichText::new("This OneDrive has no folders.")
                            .font(regular(13.0))
                            .color(t.text_faint),
                    );
                });
            }
        }
        Some(DirNode::Loaded(children)) => {
            for name in children {
                let child = if path.is_empty() {
                    name.clone()
                } else {
                    format!("{path}/{name}")
                };
                folder_row(
                    ui,
                    t,
                    depth,
                    &child,
                    name,
                    tree,
                    s,
                    expanded.contains(&child),
                    actions,
                );
                if expanded.contains(&child) {
                    folder_children(ui, t, depth + 1, &child, tree, expanded, s, actions);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn folder_row(
    ui: &mut Ui,
    t: &Tokens,
    depth: usize,
    path: &str,
    name: &str,
    tree: &HashMap<String, DirNode>,
    s: &Settings,
    open: bool,
    actions: &mut Vec<FolderAction>,
) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let id = Id::unique(("folder", path));
    let leaf = matches!(tree.get(path), Some(DirNode::Loaded(c)) if c.is_empty());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(RADIUS), t.hover);
    }
    let cy = rect.center().y;
    let x0 = rect.left() + 12.0 + depth as f32 * 24.0;
    let p = ui.painter();

    if !leaf {
        let chevron = if open {
            icons::CHEVRON_DOWN
        } else {
            icons::CHEVRON_RIGHT
        };
        p.text(
            pos2(x0 + 8.0, cy),
            Align2::CENTER_CENTER,
            chevron,
            icon_font(14.0),
            t.text_faint,
        );
    }
    let keep = s.keep_state(path);
    let folder_icon = if keep.is_on() {
        icons::FOLDER_CHECK
    } else {
        icons::FOLDER
    };
    let folder_color = if keep.is_on() { t.accent } else { t.icon };
    p.text(
        pos2(x0 + 34.0, cy),
        Align2::CENTER_CENTER,
        folder_icon,
        icon_font(17.0),
        folder_color,
    );

    // The switch and the status pill on the right.
    let toggle_rect = Rect::from_min_size(
        pos2(rect.right() - 14.0 - 34.0, cy - 10.0),
        vec2(34.0, 20.0),
    );
    let (label, fg, bg) = match keep {
        KeepState::Kept => ("Synced", t.success, t.success_soft),
        KeepState::Inherited => ("Included", t.text_dim, t.field),
        KeepState::Pending { keep: true } => ("Downloads at next sync", t.warning, t.warning_soft),
        KeepState::Pending { keep: false } => ("Removed at next sync", t.warning, t.warning_soft),
        KeepState::NotKept => ("Only on OneDrive", t.text_faint, Color32::TRANSPARENT),
    };
    let pill_w = widgets::paint_pill(ui, toggle_rect.left() - 12.0, cy, label, fg, bg);

    let name_rect = Rect::from_min_max(
        pos2(x0 + 50.0, rect.top()),
        pos2(toggle_rect.left() - 24.0 - pill_w, rect.bottom()),
    );
    ui.painter().with_clip_rect(name_rect).text(
        pos2(name_rect.left(), cy),
        Align2::LEFT_CENTER,
        name,
        medium(13.5),
        t.text,
    );

    let enabled = keep != KeepState::Inherited;
    let toggle = widgets::toggle_at(
        ui,
        toggle_rect,
        id.with("keep"),
        keep.is_on(),
        enabled,
        &format!("Sync {name} to this computer"),
    );
    let toggle = if enabled {
        toggle.on_hover_text("Sync to this computer")
    } else {
        toggle.on_hover_text("A parent folder syncs, so this folder syncs too")
    };
    if toggle.clicked() {
        actions.push(FolderAction::Keep(path.to_string(), !keep.is_on()));
    } else if resp.clicked() && !leaf {
        actions.push(if open {
            FolderAction::Collapse(path.into())
        } else {
            FolderAction::Expand(path.into())
        });
    }
    if !leaf {
        resp.on_hover_cursor(CursorIcon::PointingHand);
    }
}

// ---------------------------------------------------------------- activity

fn activity(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    page_header(ui, "Activity", "What DriveCraft did recently.", |ui| {
        if widgets::icon_button(ui, icons::EXTERNAL_LINK, "Open the rclone log").clicked() {
            open_in_file_manager(&Settings::log_file());
        }
    });
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        if let Some((text, fraction)) = &app.syncing {
            syncing_now_card(ui, text, *fraction, &app.current);
            ui.add_space(14.0);
        }
        card(ui, |ui| {
            if app.activity.is_empty() {
                ui.label(
                    RichText::new("Nothing yet.")
                        .font(regular(13.0))
                        .color(t.text_faint),
                );
            }
            let n = app.activity.len();
            for (i, entry) in app.activity.iter().enumerate() {
                activity_row(ui, entry);
                if i + 1 < n {
                    widgets::separator(ui);
                }
            }
        });
    });
}

/// The sync that runs now: its progress, and the files that transfer now.
/// Finished files go to the list below at once.
fn syncing_now_card(ui: &mut Ui, text: &str, fraction: Option<f32>, current: &[Transfer]) {
    let t = Tokens::of(ui);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(15.0).color(t.accent));
            ui.label(
                RichText::new("Syncing now")
                    .font(semibold(14.0))
                    .color(t.text),
            );
        });
        ui.add_space(2.0);
        ui.label(RichText::new(text).font(regular(12.5)).color(t.text_dim));
        ui.add_space(8.0);
        widgets::progress(ui, fraction);
        if !current.is_empty() {
            ui.add_space(6.0);
        }
        for tr in current.iter().take(8) {
            let (icon, action) = match tr.kind {
                ChangeKind::Uploaded => (icons::UPLOAD, "Uploading"),
                _ => (icons::DOWNLOAD, "Downloading"),
            };
            let (folder, name) = match tr.path.rsplit_once('/') {
                Some((folder, name)) => (folder, name),
                None => ("OneDrive", tr.path.as_str()),
            };
            widgets::text_and_actions(
                ui,
                Align::Center,
                |ui| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(40.0);
                        ui.label(RichText::new(icon).font(icon_font(15.0)).color(t.accent));
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.add(
                                egui::Label::new(
                                    RichText::new(name).font(medium(13.0)).color(t.text),
                                )
                                .truncate(),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("{action} · in {folder}"))
                                        .font(regular(11.5))
                                        .color(t.text_faint),
                                )
                                .truncate(),
                            );
                        });
                    });
                },
                |ui, ()| {
                    if let Some(f) = tr.fraction {
                        ui.label(
                            RichText::new(format!("{:.0}%", f * 100.0))
                                .font(regular(12.0))
                                .color(t.text_faint),
                        );
                    }
                },
            );
        }
    });
}

// ---------------------------------------------------------------- settings

fn settings(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    page_header(
        ui,
        "Settings",
        "Folder, sync, account, and appearance.",
        |_| {},
    );
    let Some(s) = app.settings.clone() else {
        return;
    };
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // The local folder.
        card(ui, |ui| {
            card_title(ui, icons::FOLDER, "OneDrive folder", None);
            ui.add_space(6.0);
            widgets::text_and_actions(
                ui,
                Align::Center,
                |ui| {
                    widgets::setting_label(
                        ui,
                        "Location",
                        "If you change it, the next sync fills the new folder. The old folder stays as it is.",
                    );
                    path_label(ui, &s.local_dir);
                },
                |ui, ()| {
                    let busy = app.syncing.is_some();
                    if secondary(ui, !busy, "Change…").clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_directory(&s.local_dir)
                            .pick_folder()
                    {
                        let mut prefs = Prefs::from_settings(&s);
                        prefs.local_dir = dir;
                        app.engine.send(Command::UpdatePrefs(prefs));
                    }
                },
            );
            for name in &s.local_only {
                widgets::separator(ui);
                setting_row(
                    ui,
                    name,
                    "Only on this computer. This folder does not sync.",
                    |ui, _| {
                        if secondary(ui, true, &format!("{}  Sync", icons::UPLOAD))
                            .on_hover_text("Upload this folder to OneDrive and keep it in sync")
                            .clicked()
                        {
                            app.engine.send(Command::SetKeep {
                                path: name.clone(),
                                keep: true,
                            });
                        }
                    },
                );
            }
        });
        ui.add_space(14.0);

        // Sync.
        card(ui, |ui| {
            card_title(ui, icons::REFRESH, "Sync", None);
            ui.add_space(6.0);
            setting_row(
                ui,
                "Sync every",
                "How often DriveCraft checks for changes.",
                |ui, label| {
                    ui.add(
                        egui::DragValue::new(&mut app.form_interval)
                            .range(1..=1440)
                            .suffix(" min"),
                    )
                    .labelled_by(label);
                },
            );
            if app.form_interval != s.sync_interval_minutes {
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary(ui, true, "Apply").clicked() {
                        let mut prefs = Prefs::from_settings(&s);
                        prefs.sync_interval_minutes = app.form_interval;
                        app.engine.send(Command::UpdatePrefs(prefs));
                    }
                    if secondary(ui, true, "Cancel").clicked() {
                        app.form_interval = s.sync_interval_minutes;
                    }
                });
            }
        });
        ui.add_space(14.0);

        // Appearance.
        card(ui, |ui| {
            card_title(ui, icons::SUN_MOON, "Appearance", None);
            ui.add_space(6.0);
            setting_row(
                ui,
                "Theme",
                "Follow the system, or select light or dark.",
                |ui, _| {
                    let items = [
                        (icons::SUN_MOON, "System"),
                        (icons::SUN, "Light"),
                        (icons::MOON, "Dark"),
                    ];
                    let selected = match app.appearance {
                        Appearance::System => 0,
                        Appearance::Light => 1,
                        Appearance::Dark => 2,
                    };
                    if let Some(i) = widgets::segmented(ui, &items, selected) {
                        let a = [Appearance::System, Appearance::Light, Appearance::Dark][i];
                        app.set_appearance(ui.ctx(), a);
                    }
                },
            );
        });
        ui.add_space(14.0);

        // Account and advanced.
        card(ui, |ui| {
            card_title(ui, icons::CIRCLE_USER, "Account", None);
            ui.add_space(6.0);
            setting_row(
                ui,
                "OneDrive account",
                "The account that DriveCraft syncs.",
                |ui, label| {
                    let current = s.remote.clone().unwrap_or_default();
                    egui::ComboBox::from_id_salt("remote")
                        .selected_text(&current)
                        .show_ui(ui, |ui| {
                            for r in &app.remotes {
                                if ui.selectable_label(*r == current, r).clicked() {
                                    app.engine.send(Command::SelectRemote(r.clone()));
                                }
                            }
                        })
                        .response
                        .labelled_by(label);
                },
            );
            widgets::separator(ui);
            setting_row(
                ui,
                "Add an account",
                "Sign in to a different Microsoft account.",
                |ui, _| {
                    if app.connecting {
                        ui.add(egui::Spinner::new().size(16.0).color(t.accent));
                        ui.label(
                            RichText::new("Finish the sign-in in your browser…").color(t.text_dim),
                        );
                    } else if secondary(ui, true, &format!("{}  Sign in", icons::LOG_IN)).clicked()
                    {
                        let name = unique_account_name(&app.remotes);
                        app.engine.send(Command::ConnectAccount { name });
                    }
                },
            );
            widgets::separator(ui);
            setting_row(
                ui,
                "Full resync",
                "Compares all synced files again. Use this after a sync problem.",
                |ui, _| {
                    if secondary(ui, app.syncing.is_none(), "Run full resync").clicked() {
                        app.engine.send(Command::ForceResync);
                    }
                },
            );
        });
        ui.add_space(10.0);
        if let Some((version, _)) = &app.rclone {
            ui.label(
                RichText::new(format!(
                    "DriveCraft {} · bundled rclone {version}",
                    env!("CARGO_PKG_VERSION")
                ))
                .font(regular(11.5))
                .color(t.text_faint),
            );
        }
    });
}

/// A settings row: the title and description on the left, controls on the right.
/// `controls` gets the id of the title label, to name its control for screen readers.
fn setting_row(ui: &mut Ui, title: &str, description: &str, controls: impl FnOnce(&mut Ui, Id)) {
    widgets::text_and_actions(
        ui,
        Align::Center,
        |ui| {
            ui.set_min_height(44.0);
            widgets::setting_label(ui, title, description)
        },
        controls,
    );
}

fn unique_account_name(existing: &[String]) -> String {
    let mut name = "onedrive".to_string();
    let mut n = 2;
    while existing.contains(&name) {
        name = format!("onedrive-{n}");
        n += 1;
    }
    name
}

// ---------------------------------------------------------------- welcome and errors

pub fn welcome(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    egui::CentralPanel::default().frame(Frame::new().fill(t.chrome)).show(ui, |ui| {
        centered(ui, 400.0, |ui| {
            let icon = app.app_icon(ui.ctx());
            ui.add(egui::Image::new(&icon).fit_to_exact_size(vec2(88.0, 88.0)));
            ui.add_space(18.0);
            ui.label(RichText::new("Welcome to DriveCraft").font(semibold(26.0)).color(t.text));
            ui.add_space(4.0);
            ui.label(
                RichText::new("An open source OneDrive client.\nSign in with your Microsoft account to start.")
                    .font(regular(14.0))
                    .color(t.text_dim),
            );
            ui.add_space(22.0);
            if app.connecting {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(18.0).color(t.accent));
                    ui.label(RichText::new("Finish the sign-in in your browser…").font(medium(13.5)).color(t.text_dim));
                });
            } else {
                let button = egui::Button::new(widgets::mixed(
                    &format!("{}  Sign in with Microsoft", icons::LOG_IN),
                    medium(14.0),
                    t.primary_text,
                ))
                .fill(t.primary_bg)
                .corner_radius(CornerRadius::same(RADIUS))
                .min_size(vec2(260.0, 40.0));
                if ui.add(button).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    let name = unique_account_name(&app.remotes);
                    app.engine.send(Command::ConnectAccount { name });
                }
            }
            if !app.remotes.is_empty() && !app.connecting {
                ui.add_space(18.0);
                ui.label(RichText::new("Or use an account that is already set up:").font(regular(12.5)).color(t.text_faint));
                ui.add_space(4.0);
                for r in app.remotes.clone() {
                    if secondary(ui, true, &format!("{}  {r}", icons::CIRCLE_USER)).clicked() {
                        app.engine.send(Command::SelectRemote(r));
                    }
                }
            }
            ui.add_space(28.0);
            official_app_steps(ui);
            ui.add_space(18.0);
            if let Some((version, _)) = &app.rclone {
                ui.label(RichText::new(format!("Uses rclone {version}")).font(regular(11.5)).color(t.text_faint));
            }
        });
    });
}

/// The safe order of steps for a user who stops using the official
/// OneDrive app. The README has the same steps.
fn official_app_steps(ui: &mut Ui) {
    let t = Tokens::of(ui);
    let unlink = if cfg!(windows) {
        "Unlink this PC"
    } else {
        "Unlink this Mac"
    };
    card(ui, |ui| {
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(
                RichText::new("Do you use the official OneDrive app?")
                    .font(semibold(13.5))
                    .color(t.text),
            );
            for (n, step) in [
                "Wait until the app shows that your files are up to date.".to_string(),
                format!("In its settings, open Account and select \"{unlink}\"."),
                "Uninstall the app.".to_string(),
            ]
            .iter()
            .enumerate()
            {
                ui.label(
                    RichText::new(format!("{}. {step}", n + 1))
                        .font(regular(12.5))
                        .color(t.text_dim),
                );
            }
            ui.label(
                RichText::new(
                    "Do not copy or move files from its folder. \
                     DriveCraft downloads your files from OneDrive.",
                )
                .font(medium(12.5))
                .color(t.text),
            );
        });
    });
}

pub fn fatal(_app: &mut App, ui: &mut Ui, err: &str) {
    let t = Tokens::of(ui);
    egui::CentralPanel::default()
        .frame(Frame::new().fill(t.chrome))
        .show(ui, |ui| {
            centered(ui, 460.0, |ui| {
                widgets::icon_circle(ui, icons::TRIANGLE_ALERT, 64.0, t.danger, t.danger_soft);
                ui.add_space(14.0);
                ui.label(
                    RichText::new("DriveCraft cannot start")
                        .font(semibold(22.0))
                        .color(t.text),
                );
                ui.add_space(6.0);
                ui.label(RichText::new(err).font(regular(13.0)).color(t.text_dim));
                ui.add_space(6.0);
                ui.label(
                    RichText::new("The installation is not complete. Install DriveCraft again.")
                        .font(regular(13.0))
                        .color(t.text_dim),
                );
            });
        });
}

/// Puts a column of fixed width in the center of the available space.
fn centered(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    let avail = ui.available_rect_before_wrap();
    // Measure the content height in a first pass, then center it.
    let id = ui.scope_id().with("centered-height");
    let height: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(400.0);
    let top = avail.top() + ((avail.height() - height) / 2.0).max(24.0);
    let rect = Rect::from_min_size(
        pos2(avail.center().x - width / 2.0, top),
        vec2(width, avail.height()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Center)),
    );
    child.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
    add(&mut child);
    let used = child.min_rect().height();
    if (used - height).abs() > 1.0 {
        ui.ctx().data_mut(|d| d.insert_temp(id, used));
        ui.ctx().request_repaint();
    }
}
