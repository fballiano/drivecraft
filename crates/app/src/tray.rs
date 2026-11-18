//! The tray icon (the menu bar icon on macOS).
//!
//! - A left click opens the DriveCraft window.
//! - A right click shows a menu: status, Sync now, Open OneDrive folder, Quit.
//!
//! The tray works on macOS and Windows. On Linux, tray-icon needs a GTK
//! event loop on its own thread; that is not done yet, so Linux has no tray.

use std::sync::mpsc::Sender;

use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Open,
    SyncNow,
    OpenDrive,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    UpToDate,
    Syncing,
    Error,
    Idle,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod imp {
    use super::*;
    use crate::art;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    pub struct Tray {
        icon: TrayIcon,
        status: MenuItem,
        shown: Option<(TrayState, String)>,
    }

    fn icon_for(state: TrayState) -> Icon {
        let svg = match state {
            TrayState::UpToDate => art::TRAY_DONE,
            TrayState::Syncing => art::TRAY_SYNCING,
            TrayState::Error => art::TRAY_ERROR,
            TrayState::Idle => art::TRAY_IDLE,
        };
        // macOS draws template icons in the menu bar color, so black is correct.
        // Windows shows the icon as it is, so use the accent color.
        let (color, size) = if cfg!(target_os = "macos") {
            ("#000000", 44)
        } else {
            ("#8B7CF6", 32)
        };
        let rgba = art::render(&art::lucide(svg, color), size);
        Icon::from_rgba(rgba.pixels, rgba.size, rgba.size).expect("valid tray icon")
    }

    pub fn create(ctx: egui::Context, tx: Sender<TrayAction>) -> Result<Tray, String> {
        let open = MenuItem::with_id("open", "Open DriveCraft", true, None);
        let status = MenuItem::with_id("status", "Starting…", false, None);
        let sync = MenuItem::with_id("sync", "Sync now", true, None);
        let drive = MenuItem::with_id("drive", "Open OneDrive folder", true, None);
        let quit = MenuItem::with_id("quit", "Quit DriveCraft", true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &open,
            &PredefinedMenuItem::separator(),
            &status,
            &sync,
            &drive,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;

        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("DriveCraft");
        // macOS draws a template icon in the menu bar color (light or dark).
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(icon_for(TrayState::Idle));
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(icon_for(TrayState::Idle));
        let icon = builder.build().map_err(|e| e.to_string())?;

        let (tx1, ctx1) = (tx.clone(), ctx.clone());
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = tx1.send(TrayAction::Open);
                ctx1.request_repaint();
            }
        }));
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = match event.id.0.as_str() {
                "open" => TrayAction::Open,
                "sync" => TrayAction::SyncNow,
                "drive" => TrayAction::OpenDrive,
                "quit" => TrayAction::Quit,
                _ => return,
            };
            let _ = tx.send(action);
            ctx.request_repaint();
        }));

        Ok(Tray {
            icon,
            status,
            shown: None,
        })
    }

    impl Tray {
        pub fn set_state(&mut self, state: TrayState, text: &str) {
            if self
                .shown
                .as_ref()
                .is_some_and(|(s, t)| *s == state && t == text)
            {
                return;
            }
            if self.shown.as_ref().is_none_or(|(s, _)| *s != state) {
                #[cfg(target_os = "macos")]
                let _ = self.icon.set_icon_templated(Some(icon_for(state)));
                #[cfg(not(target_os = "macos"))]
                let _ = self.icon.set_icon(Some(icon_for(state)));
            }
            self.status.set_text(text);
            let _ = self.icon.set_tooltip(Some(format!("DriveCraft – {text}")));
            self.shown = Some((state, text.to_string()));
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    use super::*;

    pub struct Tray;

    pub fn create(_ctx: egui::Context, _tx: Sender<TrayAction>) -> Result<Tray, String> {
        Err("the tray icon is not available on this platform yet".into())
    }

    impl Tray {
        pub fn set_state(&mut self, _state: TrayState, _text: &str) {}
    }
}

pub use imp::{Tray, create};
