# DriveCraft

An open source client for OneDrive, Dropbox, and Box: a Rust core that drives a bundled rclone, with an egui UI and a tray icon. `README.md` describes the behavior for users. Read `HANDOFF.md` when you pick up unfinished work. It is a local file with notes about the user's machine: it is in `.gitignore`, never commit it (the repository is public).

## Decisions the user made

Keep these unless the user changes them:

- **Name:** DriveCraft. The "Craft" suffix is deliberate: it signals an open source alternative, like the ArtCraft suite.
- **Stack:** Rust with egui, one code base for macOS, Windows, and Linux.
- **Look:** the Craft "Studio" design. The palette in `crates/app/src/theme.rs` comes from PhotoCraft; Inter is the UI font, Lucide the icon set.
- **rclone is bundled.** `crates/app/build.rs` downloads a pinned release for the target platform and verifies its SHA-256. The app never uses an rclone from `PATH`.
- **One real local folder with selective sync.** The user rejected a remote mount: no `rclone mount`, no "browse online" option.
- **Folder layout:** `~/DriveCraft/<account>`, default `~/DriveCraft/OneDrive`. `~/DriveCraft` never syncs. It leaves space for more accounts (more OneDrive accounts, later other providers) and never collides with other clients.
- **No coexistence with the official clients.** People replace them, so DriveCraft does not detect them at run time. It only refuses their folders and explains the change.
- **Providers:** only rclone backends with a sign-in in the browser, listed in `providers.rs` (OneDrive, Dropbox, Box; Google Drive is ready but hidden with `available: false`, because rclone's shared Google client ID stops working in 2026 and DriveCraft has no client ID of its own yet). The Add account page and the welcome page show one button for each. Add a provider only after a test with a real account. Providers that need a password or keys (Proton Drive, Nextcloud, S3) come later, with a form.
- **Tray icon like OneDrive.** A click opens the normal window. Closing the window hides it to the tray.
- **Dock icon on macOS only while the window is open** (`set_dock_icon` in `main.rs`, `NSApplication.setActivationPolicy`).
- **The first sync waits for the folder selection.** After a sign-in, `Account::awaiting_selection` blocks every sync of that account until `Command::ConfirmSelection` (**Start sync** on its Folders tab).
- **More accounts.** `Settings` holds the app-wide values and `accounts: Vec<Account>`. The account `id` is the rclone remote and never changes; `name` is for the UI. The sidebar lists the accounts; an account page has the tabs Folders and Account. Overview, Activity, and Settings cover all accounts. Only one sync runs at a time.
- **Latest dependency versions.** Pin an older version only when a dependency forces it, and say why (for example, `objc2-app-kit` 0.3 matches eframe 0.37).

## Architecture

- `crates/core` holds all sync logic. It has no UI code. The engine thread owns rclone and the settings; the UI sends `Command`s and receives `Event`s.
- `crates/app` is a thin UI. A page reads state and sends commands; every rclone call goes through the engine.
- Each command and event about one account carries its id. Per-account files are `filters-<id>.txt` (with `.md5`) and `snapshot-<id>.json`. `Settings::parse` turns the old one-account format into one account, and `rename_one_account_files` renames its files, so no resync is forced. Keep that migration.
- The core talks to `rclone rcd` through the remote control API on 127.0.0.1, with a random password that goes through environment variables. A session file lets the next start stop an rclone that a crash left running.

## Safety rules

These protect the user's files on OneDrive. Every change to sync code keeps all of them:

- A **missing local folder** after a previous sync stops the sync (`start_bisync`). Recreating it empty makes bisync delete everything on OneDrive.
- Before a folder is switched off, a **pre-sync** with the old selection runs; only then is the local copy deleted (`Phase::Pre`, `apply_pending`).
- `check_local_dir` refuses `~/Library/CloudStorage` (and links into it), the folders of other cloud clients (`other_clients.rs`), the folders of the other accounts (same, inside, or around), the home folder, the disk root, `~/DriveCraft` itself, and the settings folder.
- A resync uses `resyncMode: "newer"`. rclone's default (`path1`) lets an old local copy overwrite a newer file on OneDrive.
- DriveCraft uses its own `rclone.conf` in the settings folder (`--config` on every rclone call), never the user's default rclone file.
- A resync never deletes, so only a resync may create the local folder.
- Junk files (`.DS_Store` and others in `snapshot::JUNK`) are excluded from sync.
- **Sign out with "Move the folder to the Trash"** runs a last sync first (`Engine::removing`); only a successful sync lets `remove_account` trash the folder, after `check_local_dir` again. Never trash the folder of an account that never synced unless it is empty. Use the `trash` crate with `NsFileManager` on macOS (the Finder method asks for an Automation permission).
- Before you delete or overwrite anything on the user's machine, verify it: compare with OneDrive by hash (`rclone check`), and prefer `/usr/bin/trash` to `rm`.

## Gotchas

- **Google Docs** have no size and no hash. A local export is not safe in a two-way sync, so Google Drive remotes get `skip_gdocs = true` at `config create` (`providers.rs`).

- **Blocked files stop a bisync.** A provider can refuse a download (OneDrive `malwareDetected`, Google `cannotDownloadAbusiveFile`), and each later bisync stops at the same file. `report_transfers` records such files in `Settings::blocked_files`, and `filter_rules` excludes them; a sync that found new ones starts again. The switch `download_blocked_files` sets `RCLONE_ONEDRIVE_AV_OVERRIDE` and `RCLONE_DRIVE_ACKNOWLEDGE_ABUSE` (`rclone_env`), so a change restarts rclone between syncs.
- **Bisync checks the filter file.** At each resync it stores the MD5 in `filters-<id>.txt.md5`; later it aborts ("must run --resync") if the rules differ. `start_bisync` compares the rules first and makes that sync a resync. Keep that check after the missing-folder check.
- **rclone records no side for deletes.** Per-file activity comes from comparing local file lists (`snapshot.rs`): last sync against before this sync gives uploads; before against after gives downloads.
- **Unicode forms:** macOS names can be NFD and OneDrive names NFC. `rclone check --files-from` reports such files as missing; confirm with `rclone hashsum quickxor` on both sides.
- **Cargo build layout:** `OUT_DIR` sits deeper than `target/<profile>/build/<pkg>-<hash>/out`, so the build script passes the rclone path as `DRIVECRAFT_BUILD_RCLONE` and copies nothing next to the binary.
- **egui 0.37:** `App` has `logic` (also runs while the window is hidden, so tray events go there) and `ui`; panels are `egui::Panel::left/bottom`.
- **Icons in text:** Inter has glyphs at the Lucide code points. Build mixed icon-and-text strings with `widgets::mixed`, never with plain `RichText`.
- **jiff 1.0** renamed `Zoned` to `ZonedDateTime`.

## Verify UI changes

Render every page in light and dark mode with demo data, then look at the PNG files:

```
cargo test -p drivecraft screenshots -- --ignored   # writes target/screenshots/*.png
```

The egui_kittest accessibility check fails on unnamed controls. Name them (`labelled_by`, `widget_info`); keep the check on.

## Lint

`.githooks/pre-commit` runs `cargo fmt --check` and `cargo clippy -D warnings`, the same checks as the CI lint job. It runs only when a commit changes `.rs` or Cargo files. Enable it once in each clone with `git config core.hooksPath .githooks`. Fix a failure; do not commit with `--no-verify`.

## Release

- `.github/workflows/build.yml` lints and tests on three platforms, then packages six targets with `scripts/package.sh`. A `v*` tag publishes a release and writes `Casks/drivecraft.rb` in `fballiano/homebrew-tap` (secret `TAP_DEPLOY_KEY`, a write deploy key of the tap). Check a cask change with `brew style`.
- The macOS bundle puts rclone in `Contents/MacOS`, next to the main executable (Apple does not allow code in `Resources`), and has `LSUIElement`. For a tag or a manual run, the workflow signs it with the Developer ID of team H95XQBELRP and notarizes it (`scripts/notarize.sh`). The secrets are the same as in Shellyard; `~/Projects/shellyard/Scripts/setup-signing.sh fballiano/drivecraft` sets them. Update `crates/app/assets/DriveCraft.icns` with the `app_icon_set` test in `art.rs` when the app icon changes.
- Each package includes `LICENSE-MIT`, `LICENSE-APACHE`, `ATTRIBUTION.md`, and the rclone, Inter, and Lucide licenses.

## Writing

The user reads Simplified Technical English (ASD-STE100). Write UI text, README text, and messages to the user in it: short sentences, active voice, simple approved words, no contractions.
