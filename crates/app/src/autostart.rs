//! Starts DriveCraft when the user signs in to the computer.
//!
//! - macOS: a LaunchAgent in `~/Library/LaunchAgents`. It works from macOS 11,
//!   and it can give the app an argument (SMAppService cannot).
//! - Windows: the `Run` key of the current user.
//! - Linux: a desktop file in `~/.config/autostart`.
//! - Flatpak: the Background portal. The sandbox cannot write a desktop file
//!   that starts the Flatpak.
//!
//! The login start gives the argument `--autostart`. Then DriveCraft starts
//! in the tray, without its window.

/// The argument of a start at login.
pub const ARG: &str = "--autostart";

/// The name of the login item: the app ID on macOS and Linux.
#[cfg(any(target_os = "macos", target_os = "linux"))]
const APP_ID: &str = "io.github.fballiano.drivecraft";

/// True when this copy of DriveCraft can register itself. On macOS, only an
/// app bundle can (not `cargo run`).
pub fn supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::bundle().is_some()
    }
    #[cfg(not(target_os = "macos"))]
    {
        cfg!(any(windows, target_os = "linux"))
    }
}

/// Registers (`enabled`) or removes the start at login.
pub fn set(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::set(enabled)
    }
    #[cfg(windows)]
    {
        windows::set(enabled)
    }
    #[cfg(target_os = "linux")]
    {
        linux::set(enabled)
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = enabled;
        Ok(())
    }
}

/// The DriveCraft binary that the login start runs.
#[cfg(any(windows, target_os = "linux"))]
fn exe() -> Result<std::path::PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("cannot find the DriveCraft binary: {e}"))
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;

    use super::{APP_ID, ARG};

    /// `DriveCraft.app`, from the binary in `DriveCraft.app/Contents/MacOS`.
    pub fn bundle() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        exe.ancestors()
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .map(PathBuf::from)
    }

    fn plist_path() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library/LaunchAgents")
                .join(format!("{APP_ID}.plist")),
        )
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let path = plist_path().ok_or("cannot find the home folder")?;
        if !enabled {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    Err(format!("cannot remove {}: {e}", path.display()))
                }
                _ => Ok(()),
            };
        }
        let bundle = bundle().ok_or("DriveCraft does not run from an app bundle")?;
        let text = plist(&bundle.to_string_lossy());
        // macOS tells the user about each new or changed login item, so
        // write the file only when it changes.
        if std::fs::read_to_string(&path).is_ok_and(|old| old == text) {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    /// `open` starts the bundle through Launch Services, like a click in the
    /// Finder, and gives it the argument. AssociatedBundleIdentifiers shows
    /// the name and the icon of DriveCraft in System Settings, Login Items.
    pub fn plist(bundle: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{APP_ID}</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/bin/open</string>
        <string>-a</string>
        <string>{bundle}</string>
        <string>--args</string>
        <string>{ARG}</string>
    </array>
    <key>RunAtLoad</key><true/>
    <key>LimitLoadToSessionType</key><string>Aqua</string>
    <key>AssociatedBundleIdentifiers</key>
    <array><string>{APP_ID}</string></array>
</dict>
</plist>
"#,
            bundle = xml_escape(bundle)
        )
    }

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn plist_escapes_the_bundle_path() {
            let text = super::plist("/Applications/A & B/DriveCraft.app");
            assert!(text.contains("<string>/Applications/A &amp; B/DriveCraft.app</string>"));
            assert!(text.contains("<string>--autostart</string>"));
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::{ARG, exe};

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "DriveCraft";

    pub fn set(enabled: bool) -> Result<(), String> {
        let key = windows_registry::CURRENT_USER
            .create(RUN)
            .map_err(|e| format!("cannot open the Run key: {e}"))?;
        if !enabled {
            // The value can be missing already.
            let _ = key.remove_value(NAME);
            return Ok(());
        }
        let command = format!("\"{}\" {ARG}", exe()?.display());
        if key.get_string(NAME).is_ok_and(|old| old == command) {
            return Ok(());
        }
        key.set_string(NAME, &command)
            .map_err(|e| format!("cannot write the Run key: {e}"))
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::PathBuf;

    use super::{APP_ID, ARG, exe};

    pub fn set(enabled: bool) -> Result<(), String> {
        if drivecraft_core::flatpak::is_sandboxed() {
            return portal::set(enabled);
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("cannot find the home folder")?;
        let dir = drivecraft_core::flatpak::host_config_dir(&home)
            .unwrap_or_else(|| home.join(".config"))
            .join("autostart");
        let path = dir.join(format!("{APP_ID}.desktop"));
        if !enabled {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    Err(format!("cannot remove {}: {e}", path.display()))
                }
                _ => Ok(()),
            };
        }
        let text = desktop_file(&exe()?.to_string_lossy());
        if std::fs::read_to_string(&path).is_ok_and(|old| old == text) {
            return Ok(());
        }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    pub fn desktop_file(exe: &str) -> String {
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=DriveCraft\n\
             Comment=Sync your cloud files with a folder\n\
             Exec={} {ARG}\n\
             Icon={APP_ID}\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n",
            exec_quote(exe)
        )
    }

    /// Quotes a path for the Exec key of a desktop file: double quotes, a
    /// backslash before `"`, `` ` ``, `$`, and `\`, then the escapes of a
    /// desktop file string (`\\`) and of field codes (`%%`).
    fn exec_quote(path: &str) -> String {
        let mut quoted = String::from("\"");
        for c in path.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        quoted.replace('\\', "\\\\").replace('%', "%%")
    }

    /// The Background portal registers the login start of a Flatpak. The
    /// desktop can ask the user first, so the connection stays open: a
    /// request ends when its caller disconnects.
    mod portal {
        use std::collections::HashMap;
        use std::sync::OnceLock;

        use zbus::blocking::{Connection, Proxy};
        use zbus::zvariant::{OwnedObjectPath, Value};

        use super::ARG;

        static CONNECTION: OnceLock<Connection> = OnceLock::new();

        pub fn set(enabled: bool) -> Result<(), String> {
            let connection = match CONNECTION.get() {
                Some(c) => c.clone(),
                None => {
                    let c = Connection::session().map_err(|e| e.to_string())?;
                    CONNECTION.get_or_init(|| c).clone()
                }
            };
            let proxy = Proxy::new(
                &connection,
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Background",
            )
            .map_err(|e| e.to_string())?;
            let mut options: HashMap<&str, Value> = HashMap::new();
            options.insert(
                "reason",
                Value::from("DriveCraft syncs your files after you sign in."),
            );
            options.insert("autostart", Value::from(enabled));
            options.insert("commandline", Value::from(vec!["drivecraft", ARG]));
            let _request: OwnedObjectPath = proxy
                .call("RequestBackground", &("", options))
                .map_err(|e| format!("the Background portal failed: {e}"))?;
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn desktop_file_quotes_the_path() {
            let text = desktop_file("/home/demo/Apps/Drive Craft/drivecraft");
            assert!(text.contains("Exec=\"/home/demo/Apps/Drive Craft/drivecraft\" --autostart\n"));
            assert_eq!(exec_quote("/a/100%/$x"), "\"/a/100%%/\\\\$x\"");
        }
    }
}
