//! The screens of DriveCraft.
//!
//! The sidebar has the pages for all accounts (Overview, Activity, Settings),
//! then one row for each account. An account page has two tabs: Folders and
//! Account.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use drivecraft_core::{
    Account, ChangeKind, Command, KeepState, Prefs, SignIn, SignInStep, Transfer,
    available_providers, provider, provider_label,
};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, Frame, Id, Layout, Margin, Rect,
    RichText, ScrollArea, Sense, Stroke, Ui, pos2, vec2,
};

use crate::theme::{RADIUS, RADIUS_LG, Tokens, icon_font, medium, regular, semibold};
use crate::widgets::{self, card, card_title, list_card, page_header, primary, secondary};
use crate::{
    AccountTab, App, Appearance, DirNode, Entry, EntryKind, Page, PasswordForm, SignOut, Status,
    Syncing, Tree, dirs_home, file_manager_name, human_bytes, icons, open_in_file_manager,
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
        .show(ui, |ui| match app.page.clone() {
            Page::Overview => overview(app, ui),
            Page::Activity => activity(app, ui),
            Page::Settings => settings(app, ui),
            Page::Account(id) => match app.account(&id).cloned() {
                Some(account) => account_page(app, ui, &account),
                None => overview(app, ui),
            },
            Page::AddAccount => add_account(app, ui),
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
        (Page::Activity, icons::ACTIVITY, "Activity"),
        (Page::Settings, icons::SETTINGS, "Settings"),
    ] {
        if widgets::nav_item(ui, icon, label, app.page == page).clicked() {
            app.page = page;
        }
    }

    ui.add_space(22.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(
            RichText::new("ACCOUNTS")
                .font(medium(10.5))
                .color(t.text_faint),
        );
    });
    ui.add_space(4.0);
    ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let accounts: Vec<Account> = app.accounts().to_vec();
            for a in &accounts {
                let dot = status_color(t, &app.account_status(&a.id));
                let selected = app.page == Page::Account(a.id.clone());
                let storage = app.storage.get(&a.id).copied();
                if widgets::account_item(ui, &a.name, dot, storage, selected).clicked() {
                    app.page = Page::Account(a.id.clone());
                }
            }
            if widgets::nav_item(ui, icons::PLUS, "Add account", app.page == Page::AddAccount)
                .clicked()
            {
                app.page = Page::AddAccount;
            }
        });
}

/// The color of the status dot of an account.
fn status_color(t: &Tokens, status: &Status) -> Color32 {
    match status {
        Status::Starting => t.text_faint,
        Status::Syncing { .. } => t.accent,
        Status::Error(_) => t.danger,
        Status::ChooseFolders => t.warning,
        Status::SignIn => t.danger,
        Status::UpToDate => t.success,
    }
}

/// A short status text for an account.
fn status_text(app: &App, id: &str, status: &Status) -> String {
    match status {
        Status::Starting => "Starting…".into(),
        Status::Syncing { text, .. } => format!("Syncing · {text}"),
        Status::Error(e) => e.clone(),
        Status::ChooseFolders => "Choose the folders for this computer.".into(),
        Status::SignIn => "Sign in again. The sync waits.".into(),
        Status::UpToDate => match app.last_sync.get(id) {
            Some(time) => format!("Up to date · last sync at {time}"),
            None => "Waiting for the first sync.".into(),
        },
    }
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
    page_header(ui, "Overview", "Your accounts on this computer.", |ui| {
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
        status_card(app, ui, &status);
        ui.add_space(14.0);
        if app.update.is_some() {
            update_card(app, ui);
            ui.add_space(14.0);
        }

        let accounts: Vec<Account> = app.accounts().to_vec();
        for a in &accounts {
            let names = app.new_folders.get(&a.id).cloned().unwrap_or_default();
            if !names.is_empty() {
                new_folders_card(app, ui, a, &names);
                ui.add_space(14.0);
            }
        }

        accounts_card(app, ui, &accounts);
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
            let names = account_names(app);
            for entry in app.activity.iter().take(4) {
                activity_row(ui, entry, &names);
            }
        });
    });
}

/// A newer release of DriveCraft. DriveCraft does not install it itself:
/// the user downloads it from the release page.
fn update_card(app: &mut App, ui: &mut Ui) {
    let Some(update) = app.update.clone() else {
        return;
    };
    card(ui, |ui| {
        widgets::text_and_actions(
            ui,
            Align::Center,
            |ui| {
                card_title(
                    ui,
                    icons::DOWNLOAD,
                    &format!("DriveCraft {} is available", update.version),
                    Some(&format!(
                        "You have version {}. Download the new version and install it \
                         as the first time. Your accounts and settings stay.",
                        drivecraft_core::update::CURRENT
                    )),
                );
            },
            |ui, ()| {
                if secondary(ui, true, "Not now").clicked() {
                    app.update = None;
                }
                if primary(ui, true, &format!("{}  Download", icons::DOWNLOAD)).clicked() {
                    crate::open_url(&update.url);
                }
            },
        );
    });
}

/// The status of the app: the running sync, or the first problem.
fn status_card(app: &mut App, ui: &mut Ui, status: &Status) {
    let t = Tokens::of(ui);
    // The account that the status is about, if it is about one account.
    let about = match status {
        Status::Syncing { .. } => app.syncing.as_ref().map(|s| s.account.clone()),
        Status::Error(_) => app
            .accounts()
            .iter()
            .find(|a| app.errors.contains_key(&a.id))
            .map(|a| a.id.clone()),
        Status::ChooseFolders => app
            .accounts()
            .iter()
            .find(|a| a.awaiting_selection)
            .map(|a| a.id.clone()),
        Status::SignIn => app
            .accounts()
            .iter()
            .find(|a| a.needs_sign_in)
            .map(|a| a.id.clone()),
        _ => None,
    };
    let about_account = about.as_deref().and_then(|id| app.account(id)).cloned();
    let about_name = about.as_deref().map(|id| app.account_name(id));
    card(ui, |ui| {
        // Top-aligned: the text block starts at the top of the icon.
        ui.horizontal_top(|ui| {
            let (icon, fg, bg, title, subtitle) = match status {
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
                    format!("Syncing {}", about_name.clone().unwrap_or_default()),
                    text.clone(),
                ),
                Status::Error(e) => (
                    icons::CLOUD_ALERT,
                    t.danger,
                    t.danger_soft,
                    format!("Sync problem in {}", about_name.clone().unwrap_or_default()),
                    e.clone(),
                ),
                Status::ChooseFolders => (
                    icons::FOLDER,
                    t.accent,
                    t.accent_soft,
                    format!(
                        "Choose the folders of {}",
                        about_name.clone().unwrap_or_default()
                    ),
                    "Its first sync starts after you choose the folders for this computer.".into(),
                ),
                Status::SignIn => (
                    icons::CLOUD_ALERT,
                    t.danger,
                    t.danger_soft,
                    format!(
                        "Sign in to {} again",
                        about_name.clone().unwrap_or_default()
                    ),
                    format!(
                        "{} ended the sign-in. The sync of this account waits until you \
                         sign in again.",
                        about_account
                            .as_ref()
                            .map_or("The provider", |a| provider_label(&a.provider))
                    ),
                ),
                Status::UpToDate => (
                    icons::CLOUD_CHECK,
                    t.success,
                    t.success_soft,
                    "Up to date".into(),
                    match app.accounts().len() {
                        1 => "Your account is in sync.".into(),
                        n => format!("All {n} accounts are in sync."),
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
                    if *status == Status::SignIn {
                        if primary(ui, app.sign_in.is_idle(), "Sign in again").clicked()
                            && let Some(a) = &about_account
                        {
                            start_sign_in_again(app, a);
                        }
                        return;
                    }
                    if *status == Status::ChooseFolders {
                        if primary(ui, true, &format!("{}  Choose folders", icons::FOLDER))
                            .clicked()
                            && let Some(id) = &about
                        {
                            app.page = Page::Account(id.clone());
                            app.account_tab = AccountTab::Folders;
                        }
                        return;
                    }
                    let syncing = matches!(status, Status::Syncing { .. });
                    if primary(ui, !syncing, &format!("{}  Sync now", icons::REFRESH)).clicked() {
                        app.engine.send(Command::SyncNow);
                    }
                },
            );
        });
        if let Status::Syncing { fraction, .. } = status {
            ui.add_space(10.0);
            widgets::progress(ui, *fraction);
        }
    });
}

/// One row for each account: its status, its folder, and shortcuts.
fn accounts_card(app: &mut App, ui: &mut Ui, accounts: &[Account]) {
    let t = Tokens::of(ui);
    card(ui, |ui| {
        card_title(ui, icons::LAPTOP, "Accounts on this computer", None);
        ui.add_space(4.0);
        let n = accounts.len();
        for (i, a) in accounts.iter().enumerate() {
            let status = app.account_status(&a.id);
            let line = status_text(app, &a.id, &status);
            widgets::text_and_actions(
                ui,
                Align::Center,
                |ui| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(52.0);
                        let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        ui.painter()
                            .circle_filled(rect.center(), 4.0, status_color(t, &status));
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(
                                RichText::new(format!(
                                    "{} · {}",
                                    a.name,
                                    provider_label(&a.provider)
                                ))
                                .font(medium(13.5))
                                .color(t.text),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&line).font(regular(12.0)).color(t.text_dim),
                                )
                                .truncate(),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(short_path(&a.local_dir))
                                        .font(egui::FontId::monospace(11.0))
                                        .color(t.text_faint),
                                )
                                .truncate(),
                            );
                        });
                    });
                },
                |ui, ()| {
                    if secondary(ui, true, "Folders").clicked() {
                        app.page = Page::Account(a.id.clone());
                        app.account_tab = AccountTab::Folders;
                    }
                    if secondary(ui, true, &format!("{}  Open", icons::FOLDER_OPEN))
                        .on_hover_text(format!("Open in {}", file_manager_name()))
                        .clicked()
                    {
                        let _ = std::fs::create_dir_all(&a.local_dir);
                        open_in_file_manager(&a.local_dir);
                    }
                },
            );
            if i + 1 < n {
                widgets::separator(ui);
            }
        }
    });
}

/// Asks for each new local folder of an account if it syncs or stays only
/// on this computer.
fn new_folders_card(app: &mut App, ui: &mut Ui, account: &Account, names: &[String]) {
    let t = Tokens::of(ui);
    let mut commands = Vec::new();
    card(ui, |ui| {
        let title = match names.len() {
            1 => format!("New folder in {}", account.name),
            n => format!("{n} new folders in {}", account.name),
        };
        card_title(
            ui,
            icons::FOLDER_PLUS,
            &title,
            Some(&format!(
                "You made these folders in {}. They do not sync yet. \
                 Select \"Sync\" to upload a folder to {}.",
                short_path(&account.local_dir),
                provider_label(&account.provider)
            )),
        );
        ui.add_space(6.0);
        let n = names.len();
        for (i, name) in names.iter().enumerate() {
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
                        .on_hover_text("Upload this folder and keep it in sync")
                        .clicked()
                    {
                        commands.push(Command::SetKeep {
                            account: account.id.clone(),
                            path: name.clone(),
                            keep: true,
                        });
                    }
                    if secondary(ui, true, "Do not sync")
                        .on_hover_text("Keep this folder only on this computer")
                        .clicked()
                    {
                        commands.push(Command::KeepLocalOnly {
                            account: account.id.clone(),
                            name: name.clone(),
                        });
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

/// A path for the UI, with "~" for the home folder.
fn short_path(path: &Path) -> String {
    match dirs_home().and_then(|h| {
        path.strip_prefix(h)
            .ok()
            .map(|p| PathBuf::from("~").join(p))
    }) {
        Some(p) => p.display().to_string(),
        None => path.display().to_string(),
    }
}

fn path_label(ui: &mut Ui, path: &Path) {
    let t = Tokens::of(ui);
    Frame::new()
        .fill(t.field)
        .corner_radius(CornerRadius::same(RADIUS - 2))
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(short_path(path))
                        .font(egui::FontId::monospace(11.5))
                        .color(t.text_dim),
                )
                .truncate(),
            );
        });
}

// ---------------------------------------------------------------- activity

/// The account names by id, for the activity rows.
fn account_names(app: &App) -> HashMap<String, String> {
    app.accounts()
        .iter()
        .map(|a| (a.id.clone(), a.name.clone()))
        .collect()
}

fn activity_row(ui: &mut Ui, entry: &Entry, names: &HashMap<String, String>) {
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
            "Deleted on this computer (deleted online)",
        ),
        EntryKind::File(ChangeKind::Conflict) => (
            icons::COPY,
            t.warning,
            "Conflict copy (changed on both sides)",
        ),
        EntryKind::File(ChangeKind::DeletedOnline) => (
            icons::TRASH,
            t.text_dim,
            "Deleted online (deleted on this computer)",
        ),
    };
    let account = entry
        .account
        .as_ref()
        .map(|id| names.get(id).cloned().unwrap_or_else(|| id.clone()));
    // A file has a second line; a message has one when it is about an account.
    let two_lines = entry.folder.is_some() || account.is_some();
    ui.horizontal(|ui| {
        ui.set_min_height(if two_lines { 40.0 } else { 28.0 });
        ui.label(RichText::new(icon).font(icon_font(15.0)).color(color));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(&entry.time)
                    .font(regular(12.0))
                    .color(t.text_faint),
            );
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let second = match (&entry.folder, &account) {
                    (Some(folder), account) => {
                        let place = if folder.is_empty() {
                            "the root".to_string()
                        } else {
                            folder.clone()
                        };
                        match account {
                            Some(a) => Some(format!("{action} · {a} · in {place}")),
                            None => Some(format!("{action} · in {place}")),
                        }
                    }
                    (None, Some(a)) => Some(a.clone()),
                    (None, None) => None,
                };
                let font = if entry.folder.is_some() {
                    medium(13.0)
                } else {
                    regular(13.0)
                };
                let Some(second) = second else {
                    // A message: one line, centered on the icon.
                    ui.add(
                        egui::Label::new(RichText::new(&entry.text).font(font).color(t.text))
                            .truncate(),
                    );
                    return;
                };
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(
                        egui::Label::new(RichText::new(&entry.text).font(font).color(t.text))
                            .truncate(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(second)
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

fn activity(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    page_header(ui, "Activity", "What DriveCraft did recently.", |ui| {
        if widgets::icon_button(ui, icons::EXTERNAL_LINK, "Open the rclone log").clicked() {
            open_in_file_manager(&drivecraft_core::Settings::log_file());
        }
    });
    let names = account_names(app);
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        if let Some(s) = &app.syncing {
            let name = names.get(&s.account).cloned().unwrap_or_default();
            syncing_now_card(ui, &name, s);
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
                activity_row(ui, entry, &names);
                if i + 1 < n {
                    widgets::separator(ui);
                }
            }
        });
    });
}

/// The sync that runs now: its progress, and the files that transfer now.
/// Finished files go to the list below at once.
fn syncing_now_card(ui: &mut Ui, account: &str, s: &Syncing) {
    let t = Tokens::of(ui);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(15.0).color(t.accent));
            ui.label(
                RichText::new(format!("Syncing {account} now"))
                    .font(semibold(14.0))
                    .color(t.text),
            );
        });
        ui.add_space(2.0);
        ui.label(RichText::new(&s.text).font(regular(12.5)).color(t.text_dim));
        ui.add_space(8.0);
        widgets::progress(ui, s.fraction);
        if !s.current.is_empty() {
            ui.add_space(6.0);
        }
        for tr in s.current.iter().take(8) {
            transfer_row(ui, tr);
        }
    });
}

fn transfer_row(ui: &mut Ui, tr: &Transfer) {
    let t = Tokens::of(ui);
    let (icon, action) = match tr.kind {
        ChangeKind::Uploaded => (icons::UPLOAD, "Uploading"),
        _ => (icons::DOWNLOAD, "Downloading"),
    };
    let (folder, name) = match tr.path.rsplit_once('/') {
        Some((folder, name)) => (folder, name),
        None => ("the root", tr.path.as_str()),
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
                        egui::Label::new(RichText::new(name).font(medium(13.0)).color(t.text))
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

// ---------------------------------------------------------------- settings

/// The settings of the app. The settings of an account are on its page.
fn settings(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    page_header(
        ui,
        "Settings",
        "Sync and appearance, for all accounts.",
        |_| {},
    );
    let Some(s) = app.settings.clone() else {
        return;
    };
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        card(ui, |ui| {
            card_title(ui, icons::REFRESH, "Sync", None);
            ui.add_space(6.0);
            setting_row(
                ui,
                "Sync every",
                "How often DriveCraft checks each account for changes.",
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
            widgets::separator(ui);
            let blocked: usize = s.accounts.iter().map(|a| a.blocked_files.len()).sum();
            let mut description = String::from(
                "Cloud providers can block files that they mark as infected with a virus. \
                 Turn this on to download them anyway. Scan such files before you open them.",
            );
            match blocked {
                0 => {}
                1 => description.push_str(" 1 file is blocked now."),
                n => description.push_str(&format!(" {n} files are blocked now.")),
            }
            setting_row(ui, "Download blocked files", &description, |ui, _| {
                let (rect, _) = ui.allocate_exact_size(vec2(34.0, 20.0), Sense::hover());
                let on = s.download_blocked_files;
                if widgets::toggle_at(
                    ui,
                    rect,
                    Id::unique("download-blocked-files"),
                    on,
                    true,
                    "Download blocked files",
                )
                .clicked()
                {
                    let mut prefs = Prefs::from_settings(&s);
                    prefs.download_blocked_files = !on;
                    app.engine.send(Command::UpdatePrefs(prefs));
                }
            });
            // The screenshot tests show the switch too.
            if crate::autostart::supported() || cfg!(test) {
                widgets::separator(ui);
                setting_row(
                    ui,
                    "Start at login",
                    "DriveCraft starts when you sign in to this computer, so your files stay in sync.",
                    |ui, _| {
                        let (rect, _) = ui.allocate_exact_size(vec2(34.0, 20.0), Sense::hover());
                        let on = s.start_at_login;
                        if widgets::toggle_at(
                            ui,
                            rect,
                            Id::unique("start-at-login"),
                            on,
                            true,
                            "Start at login",
                        )
                        .clicked()
                        {
                            let mut prefs = Prefs::from_settings(&s);
                            prefs.start_at_login = !on;
                            app.engine.send(Command::UpdatePrefs(prefs));
                        }
                    },
                );
            }
            widgets::separator(ui);
            setting_row(
                ui,
                "Check for updates",
                "Once a day, DriveCraft asks GitHub for a new version. It sends no data about \
                 you. Homebrew, Scoop, Flatpak, and the AUR update DriveCraft themselves.",
                |ui, _| {
                    let (rect, _) = ui.allocate_exact_size(vec2(34.0, 20.0), Sense::hover());
                    let on = s.check_updates;
                    if widgets::toggle_at(
                        ui,
                        rect,
                        Id::unique("check-updates"),
                        on,
                        true,
                        "Check for updates",
                    )
                    .clicked()
                    {
                        let mut prefs = Prefs::from_settings(&s);
                        prefs.check_updates = !on;
                        app.engine.send(Command::UpdatePrefs(prefs));
                    }
                },
            );
        });
        ui.add_space(14.0);

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
        ui.add_space(10.0);
        if let Some((version, _)) = &app.rclone {
            ui.label(
                RichText::new(format!(
                    "DriveCraft {} · bundled rclone {version}{}",
                    env!("CARGO_PKG_VERSION"),
                    app.gpu_name.map(|g| format!(" · {g}")).unwrap_or_default()
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

// ---------------------------------------------------------------- account page

fn account_page(app: &mut App, ui: &mut Ui, account: &Account) {
    let subtitle = format!(
        "{} · {}",
        provider_label(&account.provider),
        short_path(&account.local_dir)
    );
    let tab = app.account_tab;
    page_header(ui, &account.name, &subtitle, |ui| {
        if tab == AccountTab::Folders
            && widgets::icon_button(ui, icons::LIST_RESTART, "Reload the folder list").clicked()
        {
            app.trees.remove(&account.id);
        }
    });
    let items = [(icons::FOLDER, "Folders"), (icons::CIRCLE_USER, "Account")];
    let selected = match tab {
        AccountTab::Folders => 0,
        AccountTab::Account => 1,
    };
    if let Some(i) = widgets::segmented(ui, &items, selected) {
        app.account_tab = [AccountTab::Folders, AccountTab::Account][i];
        app.sign_out = None;
    }
    ui.add_space(14.0);
    sign_in_again_card(app, ui, account);
    match app.account_tab {
        AccountTab::Folders => folders(app, ui, account),
        AccountTab::Account => account_settings(app, ui, account),
    }
}

enum FolderAction {
    Expand(String),
    Collapse(String),
    Load(String),
    Keep(String, bool),
}

fn folders(app: &mut App, ui: &mut Ui, account: &Account) {
    let t = Tokens::of(ui);
    if account.awaiting_selection {
        first_sync_card(app, ui, account);
        ui.add_space(14.0);
    }
    let mut actions = Vec::new();
    let tree = app.trees.entry(account.id.clone()).or_default();
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        list_card(ui, |ui| {
            folder_children(ui, t, 0, "", tree, account, &mut actions);
        });
    });
    for a in actions {
        let tree = app.trees.entry(account.id.clone()).or_default();
        match a {
            FolderAction::Expand(p) => {
                tree.expanded.insert(p);
            }
            FolderAction::Collapse(p) => {
                tree.expanded.remove(&p);
            }
            FolderAction::Load(p) => {
                tree.nodes.insert(p.clone(), DirNode::Loading);
                app.engine.send(Command::ListDir {
                    account: account.id.clone(),
                    path: p,
                });
            }
            FolderAction::Keep(path, keep) => app.engine.send(Command::SetKeep {
                account: account.id.clone(),
                path,
                keep,
            }),
        }
    }
}

/// After a sign-in: the user switches off the folders that stay only
/// online, then starts the first sync. Nothing downloads before that.
fn first_sync_card(app: &mut App, ui: &mut Ui, account: &Account) {
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
                        "Turn off the folders that you want only online. \
                         Nothing downloads until you start the first sync.",
                    ),
                );
            },
            |ui, ()| {
                if primary(ui, true, &format!("{}  Start sync", icons::REFRESH)).clicked() {
                    app.engine
                        .send(Command::ConfirmSelection(account.id.clone()));
                    app.page = Page::Overview;
                }
            },
        );
    });
}

/// The Account tab: storage, the local folder, sync, and sign out.
fn account_settings(app: &mut App, ui: &mut Ui, account: &Account) {
    let id = account.id.clone();
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        card(ui, |ui| {
            card_title(ui, icons::CIRCLE_USER, "Account", None);
            ui.add_space(6.0);
            setting_row(
                ui,
                "Name",
                "The name in DriveCraft. It does not change the folder.",
                |ui, label| {
                    let editing = app.form_name.as_ref().is_some_and(|(i, _)| *i == id);
                    if !editing {
                        app.form_name = Some((id.clone(), account.name.clone()));
                    }
                    let Some((_, text)) = app.form_name.as_mut() else {
                        return;
                    };
                    let changed = text.trim() != account.name && !text.trim().is_empty();
                    if primary(ui, changed, "Rename").clicked() {
                        app.engine.send(Command::UpdateAccount {
                            account: id.clone(),
                            name: text.clone(),
                            local_dir: account.local_dir.clone(),
                        });
                    }
                    ui.add_sized(
                        vec2(180.0, 32.0),
                        egui::TextEdit::singleline(text).align(Align2::LEFT_CENTER),
                    )
                    .labelled_by(label);
                },
            );
            widgets::separator(ui);
            let used = match app.storage.get(&id) {
                Some((used, total)) => format!(
                    "{} of {} used on {}.",
                    human_bytes(*used),
                    human_bytes(*total),
                    provider_label(&account.provider)
                ),
                None if app.no_storage.contains(&id) => format!(
                    "{} does not report the storage use.",
                    provider_label(&account.provider)
                ),
                None => "Reading the storage use…".into(),
            };
            ui.add_space(6.0);
            widgets::setting_label(ui, "Storage", &used);
            ui.add_space(6.0);
            let fraction = app
                .storage
                .get(&id)
                .map(|(u, t)| if *t > 0 { *u as f32 / *t as f32 } else { 0.0 });
            if let Some(f) = fraction {
                widgets::progress(ui, Some(f));
                ui.add_space(6.0);
            }
        });
        ui.add_space(14.0);

        card(ui, |ui| {
            card_title(ui, icons::FOLDER, "Folder on this computer", None);
            ui.add_space(6.0);
            widgets::text_and_actions(
                ui,
                Align::Center,
                |ui| {
                    widgets::setting_label(
                        ui,
                        "Location",
                        "If you change it, the next sync fills the new folder. \
                         The old folder stays as it is.",
                    );
                    path_label(ui, &account.local_dir);
                },
                |ui, ()| {
                    let busy = app.syncing.as_ref().is_some_and(|s| s.account == id);
                    if secondary(ui, !busy, "Change…").clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_directory(&account.local_dir)
                            .pick_folder()
                    {
                        app.engine.send(Command::UpdateAccount {
                            account: id.clone(),
                            name: account.name.clone(),
                            local_dir: dir,
                        });
                    }
                    if secondary(ui, true, &format!("{}  Open", icons::FOLDER_OPEN))
                        .on_hover_text(format!("Open in {}", file_manager_name()))
                        .clicked()
                    {
                        let _ = std::fs::create_dir_all(&account.local_dir);
                        open_in_file_manager(&account.local_dir);
                    }
                },
            );
            for name in &account.local_only {
                widgets::separator(ui);
                setting_row(
                    ui,
                    name,
                    "Only on this computer. This folder does not sync.",
                    |ui, _| {
                        if secondary(ui, true, &format!("{}  Sync", icons::UPLOAD))
                            .on_hover_text("Upload this folder and keep it in sync")
                            .clicked()
                        {
                            app.engine.send(Command::SetKeep {
                                account: id.clone(),
                                path: name.clone(),
                                keep: true,
                            });
                        }
                    },
                );
            }
        });
        ui.add_space(14.0);

        card(ui, |ui| {
            card_title(ui, icons::REFRESH, "Sync", None);
            ui.add_space(6.0);
            let busy = app.syncing.as_ref().is_some_and(|s| s.account == id);
            setting_row(
                ui,
                "Full resync",
                "Compares all synced files again. Use this after a sync problem.",
                |ui, _| {
                    if secondary(ui, !busy, "Run full resync").clicked() {
                        app.engine.send(Command::ForceResync(id.clone()));
                    }
                },
            );
            widgets::separator(ui);
            setting_row(
                ui,
                "Sign in again",
                &format!(
                    "Use this when DriveCraft cannot connect to {}, for example after \
                     you changed your password.",
                    provider_label(&account.provider)
                ),
                |ui, _| {
                    let idle = app.sign_in.is_idle() && app.password_form.is_none();
                    if secondary(ui, idle, "Sign in again").clicked() {
                        start_sign_in_again(app, account);
                    }
                },
            );
            widgets::separator(ui);
            setting_row(
                ui,
                "Sign out",
                "DriveCraft stops syncing this account. You select what happens to the folder.",
                |ui, _| {
                    if secondary(ui, true, &format!("{}  Sign out…", icons::LOG_OUT)).clicked() {
                        app.sign_out = Some(SignOut {
                            account: id.clone(),
                            trash: false,
                        });
                    }
                },
            );
        });
    });
    sign_out_dialog(app, ui, account);
}

/// Asks what happens to the folder of an account that signs out.
fn sign_out_dialog(app: &mut App, ui: &mut Ui, account: &Account) {
    let Some(trash) = app
        .sign_out
        .as_ref()
        .filter(|s| s.account == account.id)
        .map(|s| s.trash)
    else {
        return;
    };
    let t = Tokens::of(ui);
    let provider = provider_label(&account.provider);
    let busy = app
        .syncing
        .as_ref()
        .is_some_and(|s| s.account == account.id);
    // Folders that are not online: they go to the Trash with the rest.
    let local: Vec<String> = account
        .local_only
        .iter()
        .cloned()
        .chain(
            app.new_folders
                .get(&account.id)
                .cloned()
                .unwrap_or_default(),
        )
        .collect();
    let mut choice = trash;
    let mut cancel = false;
    let mut confirm = false;
    let frame = Frame::new()
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.card_border))
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .inner_margin(Margin::same(24));
    let modal = egui::Modal::new(Id::unique("sign-out"))
        .frame(frame)
        .show(ui.ctx(), |ui| {
            ui.set_width(460.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(
                RichText::new(format!("Sign out of {}?", account.name))
                    .font(semibold(18.0))
                    .color(t.text),
            );
            ui.label(
                RichText::new(format!(
                    "DriveCraft stops syncing this account. Your files stay in {provider}."
                ))
                .font(regular(13.0))
                .color(t.text_dim),
            );
            ui.add_space(6.0);
            if widgets::choice(
                ui,
                !choice,
                "Keep the folder on this computer",
                &format!(
                    "{} and its files stay as they are. They do not sync any more.",
                    short_path(&account.local_dir)
                ),
            )
            .clicked()
            {
                choice = false;
            }
            if widgets::choice(
                ui,
                choice,
                "Move the folder to the Trash",
                "DriveCraft syncs one last time, then moves the folder to the Trash. \
                 If the sync fails, the account and the folder stay.",
            )
            .clicked()
            {
                choice = true;
            }
            if choice && !local.is_empty() {
                ui.add_space(2.0);
                Frame::new()
                    .fill(t.warning_soft)
                    .corner_radius(CornerRadius::same(RADIUS))
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(format!(
                                "These folders are only on this computer. They go to the \
                                 Trash too: {}.",
                                local.join(", ")
                            ))
                            .font(regular(12.5))
                            .color(t.text),
                        );
                    });
            }
            if busy && !choice {
                ui.label(
                    RichText::new("This account syncs now. Wait until the sync is complete.")
                        .font(regular(12.5))
                        .color(t.text_dim),
                );
            }
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = format!("{}  Sign out", icons::LOG_OUT);
                if widgets::danger(ui, !(busy && !choice), &label).clicked() {
                    confirm = true;
                }
                if secondary(ui, true, "Cancel").clicked() {
                    cancel = true;
                }
            });
        });
    if confirm {
        app.engine.send(Command::RemoveAccount {
            account: account.id.clone(),
            trash_folder: choice,
        });
        app.sign_out = None;
    } else if cancel || modal.should_close() {
        app.sign_out = None;
    } else if let Some(s) = &mut app.sign_out {
        s.trash = choice;
    }
}

fn folder_children(
    ui: &mut Ui,
    t: &Tokens,
    depth: usize,
    path: &str,
    tree: &Tree,
    s: &Account,
    actions: &mut Vec<FolderAction>,
) {
    let expanded = &tree.expanded;
    let indent = 12.0 + depth as f32 * 24.0;
    match tree.nodes.get(path) {
        None | Some(DirNode::Loading) => {
            if !tree.nodes.contains_key(path) {
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
                        RichText::new("This account has no folders.")
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
                    &tree.nodes,
                    s,
                    expanded.contains(&child),
                    actions,
                );
                if expanded.contains(&child) {
                    folder_children(ui, t, depth + 1, &child, tree, s, actions);
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
    s: &Account,
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
        KeepState::NotKept => ("Only online", t.text_faint, Color32::TRANSPARENT),
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

pub fn welcome(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    egui::CentralPanel::default()
        .frame(Frame::new().fill(t.chrome))
        .show(ui, |ui| {
            centered(ui, 480.0, |ui| {
                let icon = app.app_icon(ui.ctx());
                ui.add(egui::Image::new(&icon).fit_to_exact_size(vec2(88.0, 88.0)));
                ui.add_space(18.0);
                ui.label(
                    RichText::new("Welcome to DriveCraft")
                        .font(semibold(26.0))
                        .color(t.text),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "An open source client for OneDrive, Dropbox, Box, and more.\n\
                         Sign in to your first account to start.",
                    )
                    .font(regular(14.0))
                    .color(t.text_dim),
                );
                ui.add_space(22.0);
                ui.with_layout(Layout::top_down(Align::Min), |ui| sign_in(app, ui));
                ui.add_space(28.0);
                official_app_steps(ui);
                ui.add_space(18.0);
                if let Some((version, _)) = &app.rclone {
                    ui.label(
                        RichText::new(format!("Uses rclone {version}"))
                            .font(regular(11.5))
                            .color(t.text_faint),
                    );
                }
            });
        });
}

/// The page to add one more account.
fn add_account(app: &mut App, ui: &mut Ui) {
    page_header(
        ui,
        "Add account",
        "Each account syncs to a folder of its own in ~/DriveCraft.",
        |_| {},
    );
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        card(ui, |ui| {
            card_title(ui, icons::CLOUD, "Select your cloud", None);
            ui.add_space(12.0);
            sign_in(app, ui);
        });
        ui.add_space(14.0);
        official_app_steps(ui);
    });
}

/// One sign-in button for each provider, in one row if they fit, or the
/// progress of a sign-in.
fn sign_in(app: &mut App, ui: &mut Ui) {
    if !app.sign_in.is_idle() && app.sign_in.account().is_none() {
        sign_in_step(app, ui);
        return;
    }
    let gap = 10.0;
    let mut providers: Vec<_> = available_providers().collect();
    providers.sort_by_key(|p| p.label.to_lowercase());
    // As many in a row as fit, in rows of the same length (4 + 3, not 6 + 1).
    let fit = ((ui.available_width() + gap) / (150.0 + gap)) as usize;
    let n = providers.len().max(1);
    let rows = n.div_ceil(fit.clamp(1, n));
    let columns = n.div_ceil(rows);
    let width = ((ui.available_width() - gap * (columns - 1) as f32) / columns as f32).floor();
    for row in providers.chunks(columns) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for p in row {
                if !widgets::provider_button(ui, width, p.label).clicked() {
                    continue;
                }
                app.add_field = None;
                app.password_form = None;
                if matches!(p.sign_in, SignIn::Password { .. }) {
                    app.password_form = Some(PasswordForm {
                        provider: p.id.to_string(),
                        account: None,
                        user: String::new(),
                        password: String::new(),
                        code: String::new(),
                    });
                } else if p.field.is_some() {
                    app.add_field = Some((p.id.to_string(), String::new()));
                } else {
                    app.engine.send(Command::AddAccount {
                        provider: p.id.into(),
                        value: None,
                    });
                }
            }
        });
        ui.add_space(gap - ui.spacing().item_spacing.y);
    }
    provider_field(app, ui);
    if app
        .password_form
        .as_ref()
        .is_some_and(|f| f.account.is_none())
    {
        password_form(app, ui);
    }
}

/// Starts a new sign-in to an account. A browser provider starts at once;
/// a password provider opens the password form. The account page shows
/// the progress.
fn start_sign_in_again(app: &mut App, account: &Account) {
    app.page = Page::Account(account.id.clone());
    app.add_field = None;
    app.password_form = None;
    match provider(&account.provider).map(|p| &p.sign_in) {
        Some(SignIn::Password { .. }) => {
            app.password_form = Some(PasswordForm {
                provider: account.provider.clone(),
                account: Some(account.id.clone()),
                user: String::new(),
                password: String::new(),
                code: String::new(),
            });
        }
        _ => app.engine.send(Command::SignInAgain(account.id.clone())),
    }
}

/// On the account page: the request to sign in again, and its progress.
fn sign_in_again_card(app: &mut App, ui: &mut Ui, account: &Account) {
    let id = account.id.as_str();
    let running = !app.sign_in.is_idle() && app.sign_in.account() == Some(id);
    let form = app
        .password_form
        .as_ref()
        .is_some_and(|f| f.account.as_deref() == Some(id));
    if !account.needs_sign_in && !running && !form {
        return;
    }
    let label = provider_label(&account.provider);
    let subtitle = if account.needs_sign_in {
        format!(
            "{label} ended the sign-in. DriveCraft does not sync this account until you \
             sign in again."
        )
    } else {
        format!("Sign in to {label} again.")
    };
    card(ui, |ui| {
        widgets::text_and_actions(
            ui,
            Align::Center,
            |ui| card_title(ui, icons::CLOUD_ALERT, "Sign in again", Some(&subtitle)),
            |ui, ()| {
                if !running
                    && !form
                    && primary(ui, app.sign_in.is_idle(), "Sign in again").clicked()
                {
                    start_sign_in_again(app, account);
                }
            },
        );
        if running {
            ui.add_space(12.0);
            sign_in_step(app, ui);
        } else if form {
            ui.add_space(8.0);
            password_form(app, ui);
        }
    });
    ui.add_space(14.0);
}

/// The step of the sign-in that runs: a spinner, or the question of the
/// provider.
fn sign_in_step(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    let text = match app.sign_in.clone() {
        SignInStep::Idle => return,
        SignInStep::Browser { .. } => "Finish the sign-in in your browser…",
        SignInStep::Working { .. } => "DriveCraft signs in…",
        SignInStep::Question {
            name, help, error, ..
        } => return sign_in_question(app, ui, &name, &help, &error),
    };
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(18.0).color(t.accent));
        ui.label(RichText::new(text).font(medium(13.5)).color(t.text_dim));
    });
}

/// A field with a frame, for the forms of a sign-in.
fn form_frame(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::of(ui);
    ui.add_space(4.0);
    Frame::new()
        .fill(t.field)
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            add(ui);
        });
}

/// A text field with a label above it.
fn labelled_field(ui: &mut Ui, label: &str, text: &mut String, password: bool) -> egui::Response {
    let t = Tokens::of(ui);
    let id = ui
        .label(RichText::new(label).font(medium(13.0)).color(t.text))
        .id;
    let width = ui.available_width().min(320.0);
    ui.add_sized(
        vec2(width, 32.0),
        egui::TextEdit::singleline(text)
            .password(password)
            .align(Align2::LEFT_CENTER),
    )
    .labelled_by(id)
}

/// A short note with an icon in a sign-in form.
fn form_note(ui: &mut Ui, icon: &str, title: &str, text: &str) {
    let t = Tokens::of(ui);
    ui.horizontal_top(|ui| {
        ui.label(RichText::new(icon).font(icon_font(16.0)).color(t.accent));
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).font(medium(13.0)).color(t.text));
            ui.add(
                egui::Label::new(RichText::new(text).font(regular(12.5)).color(t.text_dim)).wrap(),
            );
        });
    });
}

/// The user name and the password of a provider with `SignIn::Password`.
fn password_form(app: &mut App, ui: &mut Ui) {
    let Some(form) = app.password_form.as_ref() else {
        return;
    };
    let Some(SignIn::Password {
        user_label,
        hint,
        code: code_option,
        ..
    }) = provider(&form.provider).map(|p| &p.sign_in)
    else {
        return;
    };
    let label = provider_label(&form.provider).to_string();
    let computer_wins = provider(&form.provider).is_some_and(|p| p.computer_wins);
    let again = form.account.is_some();
    let t = Tokens::of(ui);
    let mut send = false;
    let mut cancel = false;
    form_frame(ui, |ui| {
        let Some(form) = app.password_form.as_mut() else {
            return;
        };
        ui.label(
            RichText::new(format!("Sign in to {label}"))
                .font(semibold(15.0))
                .color(t.text),
        );
        ui.add(egui::Label::new(RichText::new(*hint).font(regular(13.0)).color(t.text_dim)).wrap());
        if computer_wins {
            form_note(
                ui,
                icons::LAPTOP,
                "This computer is the master",
                &format!(
                    "{label} keeps no modification times. When a file is different on \
                     this computer and online, the copy on this computer wins."
                ),
            );
        }
        form_note(
            ui,
            icons::LOCK,
            "DriveCraft does not save your password",
            "rclone uses it on this computer for the sign-in only. Then DriveCraft \
             deletes it. DriveCraft has no server, so nothing goes to DriveCraft.",
        );
        ui.add_space(2.0);
        let mut enter = false;
        if !again {
            let user = labelled_field(ui, user_label, &mut form.user, false);
            enter |= user.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        }
        let password = labelled_field(ui, "Password", &mut form.password, true);
        enter |= password.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some((_, code_label)) = code_option {
            let code = labelled_field(ui, code_label, &mut form.code, false);
            enter |= code.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        }
        let ready = (again || !form.user.trim().is_empty()) && !form.password.is_empty();
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            send = primary(ui, ready, "Sign in").clicked() || (enter && ready);
            cancel = secondary(ui, true, "Cancel").clicked();
        });
    });
    if send && let Some(form) = app.password_form.take() {
        app.engine.send(Command::SignInWithPassword {
            provider: form.provider,
            account: form.account,
            user: form.user,
            password: form.password,
            code: form.code,
        });
    } else if cancel {
        app.password_form = None;
    }
}

/// A question of the provider during a sign-in, for example the code that
/// Apple shows on the devices of the user.
fn sign_in_question(app: &mut App, ui: &mut Ui, name: &str, help: &str, error: &str) {
    let t = Tokens::of(ui);
    let (title, text) = if name == "config_2fa" {
        (
            "Type the code",
            "Apple shows a code on your iPhone, iPad, or Mac. Type the code here. \
             If you have no device, type sms to get the code in a text message."
                .to_string(),
        )
    } else {
        ("Answer the question", help.trim().to_string())
    };
    let mut send = false;
    let mut cancel = false;
    form_frame(ui, |ui| {
        widgets::setting_label(ui, title, &text);
        if !error.is_empty() {
            ui.label(RichText::new(error).font(regular(12.5)).color(t.danger));
        }
        let field = labelled_field(ui, "Answer", &mut app.sign_in_answer, false);
        let ready = !app.sign_in_answer.trim().is_empty();
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            send = primary(ui, ready, "Continue").clicked() || (enter && ready);
            cancel = secondary(ui, true, "Cancel").clicked();
        });
    });
    if send {
        let answer = std::mem::take(&mut app.sign_in_answer);
        app.engine.send(Command::SignInAnswer(answer));
    } else if cancel {
        app.sign_in_answer.clear();
        app.engine.send(Command::CancelSignIn);
    }
}

/// The value that a provider needs before the sign-in, for example the
/// drive ID of kDrive.
fn provider_field(app: &mut App, ui: &mut Ui) {
    let t = Tokens::of(ui);
    let Some((id, _)) = &app.add_field else {
        return;
    };
    let Some(field) = provider(id).and_then(|p| p.field.as_ref()) else {
        return;
    };
    let label = provider_label(id).to_string();
    let mut send = None;
    let mut cancel = false;
    ui.add_space(4.0);
    Frame::new()
        .fill(t.field)
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let Some((_, value)) = app.add_field.as_mut() else {
                return;
            };
            let valid = field.accept(value);
            widgets::text_and_actions(
                ui,
                Align::Center,
                |ui| widgets::setting_label(ui, field.label, field.hint),
                |ui, title| {
                    if primary(ui, valid.is_some(), &format!("Sign in to {label}")).clicked() {
                        send = valid.clone();
                    }
                    if secondary(ui, true, "Cancel").clicked() {
                        cancel = true;
                    }
                    let edit = ui
                        .add_sized(
                            vec2(140.0, 32.0),
                            egui::TextEdit::singleline(value).align(Align2::LEFT_CENTER),
                        )
                        .labelled_by(title);
                    if edit.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && valid.is_some()
                    {
                        send = valid;
                    }
                },
            );
        });
    if let Some(value) = send
        && let Some((provider, _)) = app.add_field.take()
    {
        app.engine.send(Command::AddAccount {
            provider,
            value: Some(value),
        });
    } else if cancel {
        app.add_field = None;
    }
}

/// The safe order of steps for a user who stops using the official app of
/// a provider. The README has the same steps.
fn official_app_steps(ui: &mut Ui) {
    let t = Tokens::of(ui);
    card(ui, |ui| {
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(
                RichText::new("Do you use the official app of your cloud?")
                    .font(semibold(13.5))
                    .color(t.text),
            );
            for (n, step) in [
                "Wait until the app shows that your files are up to date.",
                "In its settings, sign out, or unlink this computer.",
                "Uninstall the app.",
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
                     DriveCraft downloads your files from the cloud.",
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
