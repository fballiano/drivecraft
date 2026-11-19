# DriveCraft

An open source client for OneDrive, Dropbox, Box, pCloud, kDrive, HiDrive, Yandex Disk, iCloud Drive, and MEGA: a Rust core that drives a bundled rclone, with an egui UI and a tray icon. `README.md` describes the behavior for users. Read `HANDOFF.md` when you pick up unfinished work. It is a local file with notes about the user's machine: it is in `.gitignore`, never commit it (the repository is public).

## Decisions the user made

Keep these unless the user changes them:

- **Name:** DriveCraft. The "Craft" suffix is deliberate: it signals an open source alternative, like the ArtCraft suite.
- **Stack:** Rust with egui, one code base for macOS, Windows, and Linux.
- **Look:** the Craft "Studio" design. The palette in `crates/app/src/theme.rs` comes from PhotoCraft; Inter is the UI font, Lucide the icon set.
- **rclone is bundled.** `crates/app/build.rs` downloads a pinned release for the target platform and verifies its SHA-256. The app never uses an rclone from `PATH`.
- **One real local folder with selective sync.** The user rejected a remote mount: no `rclone mount`, no "browse online" option.
- **Folder layout:** `~/DriveCraft/<account>`, default `~/DriveCraft/OneDrive`. `~/DriveCraft` never syncs. It leaves space for more accounts (more OneDrive accounts, later other providers) and never collides with other clients.
- **No coexistence with the official clients.** People replace them, so DriveCraft does not detect them at run time. It only refuses their folders and explains the change.
- **Providers:** rclone backends listed in `providers.rs`: with a sign-in in the browser (OneDrive, Dropbox, Box, pCloud, kDrive, HiDrive, Yandex Disk) or with a password form (`SignIn::Password`: iCloud Drive, MEGA, tested with real accounts); Google Drive is ready but hidden with `available: false`, because rclone's shared Google client ID stops working in 2026 and DriveCraft has no client ID of its own yet. The Add account page and the welcome page show one button for each. A provider that needs a value before the sign-in has a `Field` (kDrive: `drive_id`, digits only); the engine checks the value again, because it goes to the rclone command line. A new provider needs modification times that rclone can set, or hashes, because bisync needs them: when `Precision()` is `ModTimeNotSupported`, bisync ignores `resyncMode: newer` and `conflictResolve: newer` and lets path1 (local) win. The user decided that MEGA is the exception: `Provider::computer_wins` makes this computer the master (`resyncMode: path1`, `conflictResolve: none`), and the UI and the README say so. Never set `computer_wins` for a provider that has usable modification times. iCloud Drive and MEGA (`SignIn::Password`) passed a test with real accounts and are available. iCloud lists macOS packages as files but gives an archive of another size: `Account::skipped_files` excludes them. Add a provider only after a test with a real account. Other providers with a password or keys (Proton Drive, Nextcloud, S3) can use the same password form later.
- **Start at login, on by default** (`Settings::start_at_login`, `autostart.rs`). DriveCraft registers only while at least one account exists. The login start gives `--autostart`: on macOS and Windows the window stays hidden (tray only); Linux has no tray, so the window opens. macOS uses a LaunchAgent with `open -a`, not SMAppService, because it works from macOS 11 and can give the argument. The uninstall of each channel removes the login item.
- **One instance only** (`instance.rs`). A second instance would stop the rclone of the first one (`stop_previous`). The first instance locks `instance.lock` in the settings folder; a second start creates `show-window` and exits, and the first one shows its window.
- **Tray icon like OneDrive.** A click opens the normal window; a click while the window is in front hides it (`TrayAction::Toggle`). Closing the window hides it to the tray.
- **Dock icon on macOS only while the window is open** (`set_dock_icon` in `main.rs`, `NSApplication.setActivationPolicy`).
- **The first sync waits for the folder selection.** After a sign-in, `Account::awaiting_selection` blocks every sync of that account until `Command::ConfirmSelection` (**Start sync** on its Folders tab).
- **More accounts.** `Settings` holds the app-wide values and `accounts: Vec<Account>`. The account `id` is the rclone remote and never changes; `name` is for the UI. The sidebar lists the accounts; an account page has the tabs Folders and Account. Overview, Activity, and Settings cover all accounts. Only one sync runs at a time.
- **Sign in again** (`signin.rs`, `Account::needs_sign_in`). `signin::expired` finds the errors of an ended sign-in (after a sync, a folder list, or the storage read); the account then stops syncing until a new sign-in. The Account tab always has **Sign in again**. Browser providers use `rclone config reconnect --auto-confirm`. Password providers send the password through the rc API (`config/create` or `config/update` with `nonInteractive` and `obscure`), never on a command line, answer the questions (2FA code) with `continue`, and remove the password with `config/unset` after the sign-in: the user wants no stored password, and signs in again when the provider ends the session.
- **Check for updates** (`update.rs`, `Settings::check_updates`, on by default: the user agreed, because DriveCraft has no server and gets no data from it). Only installs that no package manager updates check (`update::wanted`: not Homebrew, Scoop, Flatpak, or the AUR, and not a debug build), once a day, with the GitHub API. Overview shows a card with **Download** (the release page). DriveCraft never downloads or installs an update itself.
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
- **Sign out with "Move the folder to the Trash"** runs a last sync first (`Engine::removing`); only a successful sync lets `remove_account` trash the folder, after `check_local_dir` again. Never trash the folder of an account that never synced unless it is empty. Use the `trash` crate with `NsFileManager` on macOS (the Finder method asks for an Automation permission). In a Flatpak, use `flatpak::trash` (`gio trash`, through the Trash portal): the crate would move the folder into the hidden Trash of the sandbox.
- Before you delete or overwrite anything on the user's machine, verify it: compare with OneDrive by hash (`rclone check`), and prefer `/usr/bin/trash` to `rm`.

## Gotchas

- **Conflicts.** A normal bisync sends `conflictResolve: newer`, `conflictLoser: num`, and `_config.SuffixKeepExtension`: the newer version keeps the name, the other becomes `name.conflict1.ext` on both sides (tested with the bundled rclone). `snapshot::mark_conflicts` shows such copies as `ChangeKind::Conflict` and drops the false delete of the original when both versions were renamed.

- **Google Docs** have no size and no hash. A local export is not safe in a two-way sync, so Google Drive remotes get `skip_gdocs = true` at `config create` (`providers.rs`).

- **Blocked files stop a bisync.** A provider can refuse a download (OneDrive `malwareDetected`, Google `cannotDownloadAbusiveFile`), and each later bisync stops at the same file. `report_transfers` records such files in `Settings::blocked_files`, and `filter_rules` excludes them; a sync that found new ones starts again. The switch `download_blocked_files` sets `RCLONE_ONEDRIVE_AV_OVERRIDE` and `RCLONE_DRIVE_ACKNOWLEDGE_ABUSE` (`rclone_env`), so a change restarts rclone between syncs.
- **Bisync checks the filter file.** At each resync it stores the MD5 in `filters-<id>.txt.md5`; later it aborts ("must run --resync") if the rules differ. `start_bisync` compares the rules first and makes that sync a resync. Keep that check after the missing-folder check.
- **rclone records no side for deletes.** Per-file activity comes from comparing local file lists (`snapshot.rs`): last sync against before this sync gives uploads; before against after gives downloads.
- **Unicode forms:** macOS names can be NFD and OneDrive names NFC. `rclone check --files-from` reports such files as missing; confirm with `rclone hashsum quickxor` on both sides.
- **Cargo build layout:** `OUT_DIR` sits deeper than `target/<profile>/build/<pkg>-<hash>/out`, so the build script passes the rclone path as `DRIVECRAFT_BUILD_RCLONE` and copies nothing next to the binary.
- **egui 0.37:** `App` has `logic` (also runs while the window is hidden, so tray events go there) and `ui`; panels are `egui::Panel::left/bottom`.
- **Icons in text:** Inter has glyphs at the Lucide code points. Build mixed icon-and-text strings with `widgets::mixed`, never with plain `RichText`.
- **jiff 1.0** renamed `Zoned` to `ZonedDateTime`.
- **Flatpak:** the sandbox points `XDG_CONFIG_HOME` and `XDG_DATA_HOME` into `~/.var/app/io.github.fballiano.drivecraft`. The settings can stay there. To read the configuration of other cloud clients, use `flatpak::host_config_dir`, not `dirs::config_dir`.
- **GPU backend** (`gpu.rs`). A driver crash cannot be caught, so `gpu.txt` in the settings folder records the backend before the window opens; three frames on screen, 5 seconds with a hidden window, or a normal exit confirm it. An unconfirmed backend at the next start means a crash: take the next one (Windows: DirectX 12, Vulkan, OpenGL; Linux: Vulkan, OpenGL; macOS: Metal only). Without a GPU device, `main` starts DriveCraft again. The power preference is low power. `WGPU_BACKEND` and `WGPU_POWER_PREF` override both. Reason: the Vulkan driver of an Intel GPU (igvk64.dll 30.0.101.1404) crashed DriveCraft at the start.
- **App ID:** `io.github.fballiano.drivecraft` everywhere: the macOS bundle, the Wayland app ID (`with_app_id`), the desktop file, the AppStream metadata, the icons, and the Flatpak.

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
- `packaging/` holds the files of the other channels. The Linux `tar.gz` includes `share/` (desktop file, AppStream metadata, icons from `packaging/linux`).
  - **Windows installer:** `packaging/windows/drivecraft.iss` (Inno Setup, per user, no administrator rights). The `windows-installer` job makes `*-setup.exe` from the zip files. winget uses it.
  - **Scoop:** the release job writes `bucket/drivecraft.json` in `fballiano/scoop-bucket` from `packaging/scoop/drivecraft.json.in` (secret `SCOOP_DEPLOY_KEY`).
  - **AUR:** `drivecraft-bin`, from `packaging/aur/PKGBUILD.in` and `scripts/aur.sh`. The `aur-check` job builds it with each Linux package; the release job pushes it (secret `AUR_SSH_PRIVATE_KEY`).
  - **winget:** `FabrizioBalliano.DriveCraft`. `scripts/winget.sh <version>` writes the manifests. The first version is in pull request microsoft/winget-pkgs#484091: **after each release, until Microsoft merges it, run `scripts/winget-pr.sh <version>`**, which moves the pull request to the new version. After the merge, the release workflow sends new versions with winget-releaser (secret `WINGET_TOKEN`). `.github/workflows/winget-test.yml` (manual) validates, installs, starts, and uninstalls a release on a Windows runner.
  - **Flathub:** `packaging/flatpak/io.github.fballiano.drivecraft.yml`. The `flatpak` job makes a test build. For a release, `scripts/flatpak.sh flathub <tag>` writes the manifest and `cargo-sources.json` for the Flathub repository. Not on Flathub yet. **Never open, update, or comment on a Flathub pull request:** Flathub forbids AI tools and agents in submissions and in review interactions. The user submits; see the TODO in `HANDOFF.md`.
- At each release, add a `<release>` element with `<li>` items to `packaging/linux/io.github.fballiano.drivecraft.metainfo.xml`. Flathub requires it, and the release job makes the notes of the GitHub release from it (`scripts/release-notes.sh`). Without it, the release job fails.

## Writing

The user reads Simplified Technical English (ASD-STE100). Write UI text, README text, and messages to the user in it: short sentences, active voice, simple approved words, no contractions.
