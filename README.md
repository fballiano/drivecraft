# DriveCraft

DriveCraft is an open source client for OneDrive, Dropbox, and Box, on macOS, Windows, and Linux. It syncs one or more accounts.
It uses [rclone](https://rclone.org) to connect to the cloud.
The UI uses [egui](https://github.com/emilk/egui).

Status: early prototype (phase 1).

## The app

- DriveCraft runs in the tray (the menu bar on macOS), like OneDrive.
- On macOS, DriveCraft has a Dock icon only while its window is open. Then the window is in Command-Tab and has a menu bar. When you close the window, the Dock icon goes away.
- Click the tray icon to open the window. Right-click it for a menu: status, **Sync now**, **Open DriveCraft folder**, and **Quit DriveCraft**.
- When you close the window, DriveCraft continues to run in the tray. Use **Quit DriveCraft** to stop it.
- The sidebar has three pages for all accounts: **Overview**, **Activity**, and **Settings**. Under them, **Accounts** shows each account with its status and its storage use, and **Add account**.
- The page of an account has two tabs. **Folders** selects the folders that sync. **Account** shows the storage use, the local folder, **Full resync**, and **Sign out**.
- The design follows the "Studio" look of the Craft apps: the Inter font, Lucide icons, and the Studio colors. It follows the light or dark mode of the system. You can also select a mode in **Settings**.

The tray icon works on macOS and Windows. Linux has no tray icon yet. On Linux, closing the window stops DriveCraft.

To render each page with demo data to `target/screenshots/`, run:

```
cargo test -p drivecraft screenshots -- --ignored
```

## Change from the official app

Do these steps in this order:

1. Wait until the official app shows that your files are up to date. Changes that are not uploaded are lost when you unlink the app.
2. In the settings of the official app, sign out or unlink this computer:
   - OneDrive: open **Account** and select **Unlink this Mac** (or **Unlink this PC**).
   - Dropbox: open **Account** and select **Unlink**.
   - Box Drive: select **Sign out**.

   After the unlink, a change in its folder does not change the cloud.
   If you uninstall the app without this step, macOS can keep an empty, protected folder in `~/Library/CloudStorage`.
3. Uninstall the official app.
4. Start DriveCraft and sign in. Select the folders that sync to this computer.

Do not copy or move files from the folder of the official app into the DriveCraft folder. DriveCraft downloads your files from the cloud.
A move before the unlink deletes the files in the cloud, and then DriveCraft deletes them on this computer too.
When DriveCraft has downloaded your files, you can delete the old folder.

## How it works

DriveCraft keeps one local folder for each account, with real copies of your files.
It works like the official OneDrive client before "Files On-Demand", or like selective sync in Dropbox.

- `~/DriveCraft` holds one folder for each account. The folder has the name of the provider: the first OneDrive account gets `~/DriveCraft/OneDrive`, the next one `~/DriveCraft/OneDrive 2`, a Dropbox account `~/DriveCraft/Dropbox`. You can rename an account; its folder does not change.
  No other client uses this name, so the folders of DriveCraft and of other clients never collide.
  You cannot select `~/DriveCraft` itself as the folder of an account. The folders of two accounts must not be the same, and one must not be inside the other.
- In **Folders**, each switch selects a folder that syncs to this computer.
  A folder that is off is not on this computer. It stays only online.
- The files in the root of the account always sync.
- A new account starts with all top-level folders on. DriveCraft does not sync it yet: it opens **Folders** of the account.
  Switch off the folders that you want only online, then click **Start sync**. Only then does the first download start.
- When you switch a folder off, the next sync uploads all local changes first. Then it deletes the local copy. The files stay online.
- When you make a new top-level folder in the local folder, it does not sync immediately. **Overview** shows the new folder.
  Select **Sync** to upload it. Select **Do not sync** to keep it only on this computer.
  The **Account** tab shows the folders that are only on this computer. You can sync them there later.
  Hidden folders (names that start with ".") are not shown.
- DriveCraft syncs each account at an interval (10 minutes by default), and when you click **Sync now**. Only one sync runs at a time; the accounts take turns.
- **Sign out** removes the account from DriveCraft. Its folder and the files in it stay on this computer.
- **Activity** shows each file that a sync changed: downloaded, uploaded, deleted on this computer, or deleted online.
  DriveCraft compares lists of the synced local files before and after each sync. It keeps the last list of each account in `snapshot-<id>.json` in the settings folder.
- OneDrive and Google Drive block the download of files that they mark as infected with a virus. DriveCraft reports each such file one time in **Activity**, then skips it in later syncs.
  To download these files anyway, turn on **Settings > Sync > Download blocked files**. Scan such files before you open them.
- Some providers do not report the storage use. Then the **Account** tab says so.
- DriveCraft never syncs system files: `.DS_Store`, `._*`, `Thumbs.db`, `desktop.ini`, and Office lock files (`~$*`, `.~lock.*`).

DriveCraft refuses a folder that a different cloud app syncs, because two apps that sync the same folder can delete files:

- macOS: all folders in `~/Library/CloudStorage`, and links into that location (for example `~/OneDrive`). macOS gives this location to the apps of OneDrive, Google Drive, Dropbox, and Box.
- Windows: the folder of each account in the official OneDrive app.
- Linux: the `sync_dir` folder of the `onedrive` client.
- All systems: the folders of the Dropbox app (from its `info.json`).
- macOS and Windows: `~/Box` (Box Drive) and `~/Google Drive` (the old Google app).

Google Drive for desktop on Windows uses a separate drive letter (for example `G:`). DriveCraft refuses the root of that drive, but not yet a folder inside it.

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
An earlier version of DriveCraft used the default rclone file. At the first start with the new file, DriveCraft copies its accounts from the default file. The default file does not change.

rclone uses the MIT license. Each release package includes the rclone license text (`licenses/rclone.txt`).

## Build and run

You need Rust 1.95 or later.

1. Run `cargo run -p drivecraft`.
2. Click **Sign in with Microsoft**.
3. Log in to Microsoft in the browser window that opens.
4. Select the folders for this computer, then click **Start sync**.

To run the tests, use `cargo test`.

## Download and release

GitHub Actions builds DriveCraft for each push to `main` (`.github/workflows/build.yml`):

| Platform | Package |
|---|---|
| macOS (Apple silicon, Intel) | `DriveCraft.app` in a zip file |
| Windows (x64, Arm) | a zip file with `DriveCraft.exe` and `rclone.exe` |
| Linux (x64, Arm) | a `tar.gz` file with `drivecraft` and `rclone` |

The packages are in the artifacts of each workflow run.
To make a release, push a tag such as `v0.1.0`. The workflow then publishes a release with all packages, and updates the Homebrew cask.

On macOS, you can also install DriveCraft with Homebrew:

```
brew tap fballiano/tap
brew trust --tap fballiano/tap
brew install --cask drivecraft
```

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
| Sync filter of each account | `~/Library/Application Support/drivecraft/filters-<id>.txt` |
| rclone log | `~/Library/Application Support/drivecraft/rclone.log` |
| File list of each account after its last sync | `~/Library/Application Support/drivecraft/snapshot-<id>.json` |
| rclone session (port and password, mode 600) | `~/Library/Application Support/drivecraft/rclone-session.json` |
| rclone accounts (mode 600) | `~/Library/Application Support/drivecraft/rclone.conf` |

`<id>` is the rclone remote of the account: `onedrive`, `onedrive-2`, and so on. Settings from a version with one account change to the new format at the first start. The files of that account get the new names, so no full resync is necessary.

## Google Drive

Google Drive is not available yet. rclone's shared Google sign-in stops working during 2026.
DriveCraft needs a Google sign-in of its own first. The code for Google Drive is ready (`crates/core/src/providers.rs`).

## Plan

- Phase 1 (this version): one or more accounts of OneDrive, Dropbox, and Box; one local folder for each account with selective sync; sync at an interval.
- Phase 2: tray icon, immediate sync when a local file changes, notifications, a conflict window.
- Phase 3: integration with Finder and Explorer (status icons, placeholder files).
