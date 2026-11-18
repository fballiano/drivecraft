# DriveCraft

An open source OneDrive client: a Rust core that drives a bundled rclone, with an egui UI and a tray icon. `README.md` describes the behavior for users. Read `HANDOFF.md` when you pick up unfinished work. It is a local file with notes about the user's machine: it is in `.gitignore`, never commit it (the repository is public).

## Decisions the user made

Keep these unless the user changes them:

- **Name:** DriveCraft. The "Craft" suffix is deliberate: it signals an open source alternative, like the ArtCraft suite.
- **Stack:** Rust with egui, one code base for macOS, Windows, and Linux.
- **Look:** the Craft "Studio" design. The palette in `crates/app/src/theme.rs` comes from PhotoCraft; Inter is the UI font, Lucide the icon set.
- **rclone is bundled.** `crates/app/build.rs` downloads a pinned release for the target platform and verifies its SHA-256. The app never uses an rclone from `PATH`.
- **One real local folder with selective sync.** The user rejected a remote mount: no `rclone mount`, no "browse online" option.
- **Folder layout:** `~/DriveCraft/<account>`, default `~/DriveCraft/OneDrive`. `~/DriveCraft` never syncs. It leaves space for more accounts (more OneDrive accounts, later other providers) and never collides with other clients.
- **No coexistence with the official client.** People replace it, so DriveCraft does not detect it at run time. It only refuses its folder and explains the change.
- **Tray icon like OneDrive.** A click opens the normal window. Closing the window hides it to the tray.
- **Latest dependency versions.** Pin an older version only when a dependency forces it, and say why (for example, `winit` 0.30 matches eframe 0.37).

## Architecture

- `crates/core` holds all sync logic. It has no UI code. The engine thread owns rclone and the settings; the UI sends `Command`s and receives `Event`s.
- `crates/app` is a thin UI. A page reads state and sends commands; every rclone call goes through the engine.
- The core talks to `rclone rcd` through the remote control API on 127.0.0.1, with a random password that goes through environment variables. A session file lets the next start stop an rclone that a crash left running.

## Safety rules

These protect the user's files on OneDrive. Every change to sync code keeps all of them:

- A **missing local folder** after a previous sync stops the sync (`start_bisync`). Recreating it empty makes bisync delete everything on OneDrive.
- Before a folder is switched off, a **pre-sync** with the old selection runs; only then is the local copy deleted (`Phase::Pre`, `apply_pending`).
- `check_local_dir` refuses `~/Library/CloudStorage` (and links into it), the folders of other OneDrive clients (`other_clients.rs`), the home folder, the disk root, `~/DriveCraft` itself, and the settings folder.
- A resync uses `resyncMode: "newer"`. rclone's default (`path1`) lets an old local copy overwrite a newer file on OneDrive.
- DriveCraft uses its own `rclone.conf` in the settings folder (`--config` on every rclone call), never the user's default rclone file.
- A resync never deletes, so only a resync may create the local folder.
- Junk files (`.DS_Store` and others in `snapshot::JUNK`) are excluded from sync.
- Before you delete or overwrite anything on the user's machine, verify it: compare with OneDrive by hash (`rclone check`), and prefer `/usr/bin/trash` to `rm`.

## Gotchas

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

## Release

- `.github/workflows/build.yml` lints and tests on three platforms, then packages six targets with `scripts/package.sh`. A `v*` tag makes a draft release.
- The macOS bundle puts rclone in `Contents/Resources` (`find_binary` looks there) and has `LSUIElement`. Update `crates/app/assets/DriveCraft.icns` with the `app_icon_set` test in `art.rs` when the app icon changes.
- Each package includes `LICENSE-MIT`, `LICENSE-APACHE`, `ATTRIBUTION.md`, and the rclone, Inter, and Lucide licenses.

## Writing

The user reads Simplified Technical English (ASD-STE100). Write UI text, README text, and messages to the user in it: short sentences, active voice, simple approved words, no contractions.
