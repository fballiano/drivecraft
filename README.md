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

## Change from the official OneDrive app

Do these steps in this order:

1. Wait until the official app shows that your files are up to date. Changes that are not uploaded are lost when you unlink the app.
2. In the settings of the official app, open **Account** and select **Unlink this Mac** (or **Unlink this PC**).
   After the unlink, a change in its folder does not change OneDrive.
   If you uninstall the app without this step, macOS can keep an empty, protected folder in `~/Library/CloudStorage`.
3. Uninstall the official app.
4. Start DriveCraft and sign in. Select the folders that sync to this computer.

Do not copy or move files from the folder of the official app into the DriveCraft folder. DriveCraft downloads your files from OneDrive.
A move before the unlink deletes the files on OneDrive, and then DriveCraft deletes them on this computer too.
When DriveCraft has downloaded your files, you can delete the old folder.

## How it works

DriveCraft keeps one local folder with real copies of your OneDrive files.
It works like the official client before "Files On-Demand", or like selective sync in Dropbox.

- The default folder is `~/DriveCraft/OneDrive`. `~/DriveCraft` holds one folder for each account.
  No other client uses this name, so the folders of DriveCraft and of other clients never collide.
  You cannot select `~/DriveCraft` itself as the folder of an account.
- In **Folders**, each switch selects a folder that syncs to this computer.
  A folder that is off is not on this computer. It stays only on OneDrive.
- The files in the OneDrive root always sync.
- A new account starts with all top-level folders on. Switch off the folders that you do not want.
- When you switch a folder off, the next sync uploads all local changes first. Then it deletes the local copy. The files stay on OneDrive.
- When you make a new top-level folder in the local folder, it does not sync immediately. **Overview** shows the new folder.
  Select **Sync** to upload it to OneDrive. Select **Do not sync** to keep it only on this computer.
  **Settings** shows the folders that are only on this computer. You can sync them there later.
  Hidden folders (names that start with ".") are not shown.
- DriveCraft syncs at an interval (10 minutes by default), and when you click **Sync now**.
- **Activity** shows each file that a sync changed: downloaded, uploaded, deleted on this computer, or deleted from OneDrive.
  DriveCraft compares lists of the synced local files before and after each sync. It keeps the last list in `snapshot.json` in the settings folder.
- DriveCraft never syncs system files: `.DS_Store`, `._*`, `Thumbs.db`, `desktop.ini`, and Office lock files (`~$*`, `.~lock.*`).

DriveCraft refuses a folder that a different OneDrive app syncs, because two apps that sync the same folder can delete files:

- macOS: all folders in `~/Library/CloudStorage`, and links into that location (for example `~/OneDrive`). macOS gives this location to cloud apps.
- Windows: the folder of each account in the official OneDrive app.
- Linux: the `sync_dir` folder of the `onedrive` client.

DriveCraft does the check when you select a folder, and again before each sync.

When a sync finds a file with different content on the two sides, and there is no earlier sync to compare with, DriveCraft keeps the newer version. This occurs at the first sync, after a change of folders, and after **Full resync**.

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

1. Next to the DriveCraft binary. In the macOS app bundle, this is `Contents/MacOS`.
   A release package puts rclone here.
2. The binary that the build downloaded. `cargo run` uses this binary.

For an offline build, set `DRIVECRAFT_RCLONE_BIN` to a local rclone binary.

To update rclone, change `RCLONE_VERSION` in `crates/app/build.rs`.

DriveCraft uses its own rclone configuration file, `rclone.conf` in the settings folder. It does not see or change the accounts of your own rclone.
An earlier version of DriveCraft used the default rclone file. At the first start with the new file, DriveCraft copies the selected account from the default file. The default file does not change.

rclone uses the MIT license. Each release package includes the rclone license text (`licenses/rclone.txt`).

## Build and run

You need Rust 1.95 or later.

1. Run `cargo run -p drivecraft`.
2. Enter a name for the account, then click **Connect account**.
3. Log in to Microsoft in the browser window that opens.

To run the tests, use `cargo test`.

## Download and release

GitHub Actions builds DriveCraft for each push to `main` (`.github/workflows/build.yml`):

| Platform | Package |
|---|---|
| macOS (Apple silicon, Intel) | `DriveCraft.app` in a zip file |
| Windows (x64, Arm) | a zip file with `DriveCraft.exe` and `rclone.exe` |
| Linux (x64, Arm) | a `tar.gz` file with `drivecraft` and `rclone` |

The packages are in the artifacts of each workflow run.
To make a release, push a tag such as `v0.1.0`. The workflow then makes a draft release with all packages. Check the draft, then publish it.

The macOS app of a release has a Developer ID signature, and Apple notarizes it.
The packages of a normal push have only an ad-hoc signature. On the first start, macOS refuses to open such an app. Open it from **System Settings > Privacy & Security** with **Open Anyway**.
To get a signed package without a release, start the workflow by hand: **Actions > Build > Run workflow**.

To make a package on your computer, run `scripts/package.sh <target> <name> <version>`, for example:

```
scripts/package.sh aarch64-apple-darwin macos-arm64 0.1.0
```

## License

DriveCraft is available under the MIT license or the Apache License 2.0, at your choice (`LICENSE-MIT`, `LICENSE-APACHE`).
`ATTRIBUTION.md` lists the parts that come from other projects, with their licenses.

## Files

| File | Location on macOS |
|---|---|
| Settings | `~/Library/Application Support/drivecraft/settings.toml` |
| Sync filter | `~/Library/Application Support/drivecraft/filters.txt` |
| rclone log | `~/Library/Application Support/drivecraft/rclone.log` |
| File list after the last sync | `~/Library/Application Support/drivecraft/snapshot.json` |
| rclone session (port and password, mode 600) | `~/Library/Application Support/drivecraft/rclone-session.json` |
| rclone accounts (mode 600) | `~/Library/Application Support/drivecraft/rclone.conf` |

## Plan

- Phase 1 (this version): account setup, one local folder with selective sync, sync at an interval.
- Phase 2: tray icon, immediate sync when a local file changes, notifications, a conflict window.
- Phase 3: integration with Finder and Explorer (status icons, placeholder files).
