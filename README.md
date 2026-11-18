# DriveCraft

DriveCraft is an open source OneDrive client for macOS, Windows, and Linux.
It uses [rclone](https://rclone.org) to connect to OneDrive.
The UI uses [egui](https://github.com/emilk/egui).

Status: early prototype (phase 1).

## The app

- DriveCraft runs in the tray (the menu bar on macOS), like OneDrive. It has no Dock icon on macOS.
- Click the tray icon to open the window. Right-click it for a menu: status, **Sync now**, **Open OneDrive folder**, and **Quit DriveCraft**.
- When you close the window, DriveCraft continues to run in the tray. Use **Quit DriveCraft** to stop it.
- The window has four pages: **Overview**, **Folders**, **Activity**, and **Settings**.
- The design follows the "Studio" look of the Craft apps: the Inter font, Lucide icons, and the Studio colors. It follows the light or dark mode of the system. You can also select a mode in **Settings**.

The tray icon works on macOS and Windows. Linux has no tray icon yet. On Linux, closing the window stops DriveCraft.

To render each page with demo data to `target/screenshots/`, run:

```
cargo test -p drivecraft screenshots -- --ignored
```

## How it works

DriveCraft keeps one local folder with real copies of your OneDrive files.
It works like the official client before "Files On-Demand", or like selective sync in Dropbox.

- The default folder is `~/OneDrive`. If that name is already used, the default is `~/OneDrive (DriveCraft)`.
- In **Folders**, each switch selects a folder that syncs to this computer.
  A folder that is off is not on this computer. It stays only on OneDrive.
- The files in the OneDrive root always sync.
- A new account starts with all top-level folders on. Switch off the folders that you do not want.
- When you switch a folder off, the next sync uploads all local changes first. Then it deletes the local copy. The files stay on OneDrive.
- DriveCraft syncs at an interval (10 minutes by default), and when you click **Sync now**.
- **Activity** shows each file that a sync changed: downloaded, uploaded, deleted on this computer, or deleted from OneDrive.
  DriveCraft compares lists of the synced local files before and after each sync. It keeps the last list in `snapshot.json` in the settings folder.
- DriveCraft never syncs system files: `.DS_Store`, `._*`, `Thumbs.db`, `desktop.ini`, and Office lock files (`~$*`, `.~lock.*`).

DriveCraft refuses a folder in `~/Library/CloudStorage`, because macOS gives that location to other cloud apps.
The official OneDrive app makes `~/OneDrive` as a link into that location, so DriveCraft also refuses that link.

## Structure

```
crates/core   The core. It controls rclone and contains all sync logic. It has no UI code.
crates/app    The egui UI. It only sends commands to the core and shows events.
scripts/      Packaging scripts.
```

The core starts `rclone rcd` and uses the rclone remote control API:

- The API listens on 127.0.0.1 only.
- The API needs a random password. The password goes through environment variables.
- The sync uses `sync/bisync` with a generated filter file.

## Bundled rclone

DriveCraft includes rclone. You do not install rclone yourself.
DriveCraft never uses an rclone from the `PATH`. It always uses the bundled version, because that is the version that we test.

At build time, `crates/app/build.rs` does these steps:

1. It reads the target platform from Cargo, for example `osx-arm64` or `windows-amd64`.
   A cross build gets the rclone for the target, not for the build computer.
2. It downloads the rclone release in `RCLONE_VERSION` from downloads.rclone.org.
3. It verifies the SHA-256 checksum against the `SHA256SUMS` file of the release.
4. It gives the path of the binary to the compiler as `DRIVECRAFT_BUILD_RCLONE`.

At run time, DriveCraft looks for rclone in this order:

1. Next to the DriveCraft binary. On macOS, also in `Contents/Resources` of the app bundle.
   A release package puts rclone here.
2. The binary that the build downloaded. `cargo run` uses this binary.

For an offline build, set `DRIVECRAFT_RCLONE_BIN` to a local rclone binary.

To update rclone, change `RCLONE_VERSION` in `crates/app/build.rs`.

rclone uses the MIT license. Include the rclone license text in each release package.

## Build and run

You need Rust 1.95 or later.

1. Run `cargo run -p drivecraft`.
2. Enter a name for the account, then click **Connect account**.
3. Log in to Microsoft in the browser window that opens.

To run the tests, use `cargo test`.

## Files

| File | Location on macOS |
|---|---|
| Settings | `~/Library/Application Support/drivecraft/settings.toml` |
| Sync filter | `~/Library/Application Support/drivecraft/filters.txt` |
| rclone log | `~/Library/Application Support/drivecraft/rclone.log` |
| File list after the last sync | `~/Library/Application Support/drivecraft/snapshot.json` |
| rclone session (port and password, mode 600) | `~/Library/Application Support/drivecraft/rclone-session.json` |
| rclone accounts | The default rclone configuration file |

## Plan

- Phase 1 (this version): account setup, one local folder with selective sync, sync at an interval.
- Phase 2: tray icon, immediate sync when a local file changes, notifications, a conflict window.
- Phase 3: integration with Finder and Explorer (status icons, placeholder files).
