//! The tray icon (the menu bar icon on macOS).
//!
//! - A left click opens the DriveCraft window, or hides it when it is in front.
//! - A right click shows a menu: status, Sync now, Open DriveCraft folder, Quit.
//!
//! macOS and Windows use the `tray-icon` crate. Linux uses `ksni`: a
//! StatusNotifierItem on D-Bus, without GTK. KDE Plasma, Cinnamon, XFCE,
//! MATE, Budgie, LXQt, and Ubuntu show it. Plain GNOME shows it only with
//! the AppIndicator extension. Without a tray, `create` returns an error,
//! and DriveCraft works without a tray.

use std::sync::mpsc::Sender;

use eframe::egui;

// On other systems, nothing sends these actions, because there is no tray.
#[cfg_attr(
    not(any(target_os = "macos", target_os = "windows", target_os = "linux")),
    allow(dead_code)
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// The left click: show the window, or hide it when it is in front.
    Toggle,
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

/// True when DriveCraft can hide its window and show it again from the
/// tray. On Linux, winit cannot hide a window on Wayland, so there the
/// window stays open, and closing it stops DriveCraft.
pub fn hides_window() -> bool {
    #[cfg(target_os = "linux")]
    {
        // winit selects Wayland when one of these variables has a value.
        let set = |name| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        !set("WAYLAND_DISPLAY") && !set("WAYLAND_SOCKET")
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// The Lucide icon for a state.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn svg_for(state: TrayState) -> &'static str {
    use crate::art;
    match state {
        TrayState::UpToDate => art::TRAY_DONE,
        TrayState::Syncing => art::TRAY_SYNCING,
        TrayState::Error => art::TRAY_ERROR,
        TrayState::Idle => art::TRAY_IDLE,
    }
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
        let svg = svg_for(state);
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
        let drive = MenuItem::with_id("drive", "Open DriveCraft folder", true, None);
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
                let _ = tx1.send(TrayAction::Toggle);
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

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use crate::art;
    use ksni::blocking::{Handle, TrayMethods};
    use ksni::menu::StandardItem;
    use ksni::{Icon, MenuItem, ToolTip};

    pub struct Tray {
        handle: Handle<Item>,
        shown: Option<(TrayState, String)>,
    }

    /// The data of the tray item. ksni reads it on its own thread.
    struct Item {
        tx: Sender<TrayAction>,
        ctx: egui::Context,
        icon: Vec<Icon>,
        status: String,
    }

    impl Item {
        fn send(&self, action: TrayAction) {
            let _ = self.tx.send(action);
            self.ctx.request_repaint();
        }
    }

    /// The icon in two sizes. The panel selects the size. Linux panels can
    /// be light or dark, so use the accent color, as on Windows.
    fn icon_for(state: TrayState) -> Vec<Icon> {
        let svg = art::lucide(svg_for(state), "#8B7CF6");
        [32, 64]
            .into_iter()
            .map(|size| {
                let rgba = art::render(&svg, size);
                let mut data = rgba.pixels;
                // RGBA to ARGB32 in network byte order.
                for pixel in data.as_chunks_mut::<4>().0 {
                    pixel.rotate_right(1);
                }
                Icon {
                    width: rgba.size as i32,
                    height: rgba.size as i32,
                    data,
                }
            })
            .collect()
    }

    impl ksni::Tray for Item {
        fn id(&self) -> String {
            "io.github.fballiano.drivecraft".into()
        }

        fn title(&self) -> String {
            "DriveCraft".into()
        }

        /// The left click: show the window, or hide it when it is in front.
        fn activate(&mut self, _x: i32, _y: i32) {
            self.send(TrayAction::Toggle);
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            self.icon.clone()
        }

        fn tool_tip(&self) -> ToolTip {
            ToolTip {
                title: "DriveCraft".into(),
                description: self.status.clone(),
                ..Default::default()
            }
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            let item = |label: &str, action: TrayAction| -> MenuItem<Self> {
                StandardItem {
                    label: label.into(),
                    activate: Box::new(move |item: &mut Self| item.send(action)),
                    ..Default::default()
                }
                .into()
            };
            vec![
                item("Open DriveCraft", TrayAction::Open),
                MenuItem::Separator,
                StandardItem {
                    label: self.status.clone(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
                item("Sync now", TrayAction::SyncNow),
                item("Open DriveCraft folder", TrayAction::OpenDrive),
                MenuItem::Separator,
                item("Quit DriveCraft", TrayAction::Quit),
            ]
        }
    }

    pub fn create(ctx: egui::Context, tx: Sender<TrayAction>) -> Result<Tray, String> {
        let item = Item {
            tx,
            ctx,
            icon: icon_for(TrayState::Idle),
            status: "Starting…".into(),
        };
        // A Flatpak cannot own the D-Bus name `org.kde.StatusNotifierItem-*`.
        // Without a StatusNotifierWatcher, spawn returns an error.
        let handle = item
            .disable_dbus_name(drivecraft_core::flatpak::is_sandboxed())
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(Tray {
            handle,
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
            let icon = self
                .shown
                .as_ref()
                .is_none_or(|(s, _)| *s != state)
                .then(|| icon_for(state));
            let status = text.to_string();
            // ksni compares the properties after the update and tells the panel.
            self.handle.update(move |item| {
                if let Some(icon) = icon {
                    item.icon = icon;
                }
                item.status = status;
            });
            self.shown = Some((state, text.to_string()));
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
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
