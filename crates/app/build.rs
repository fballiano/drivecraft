//! Bundles rclone with DriveCraft.
//!
//! This script downloads a fixed rclone release for the build target
//! (not for the build computer), and verifies its SHA-256 checksum.
//! It gives the path of the binary to the compiler as
//! `DRIVECRAFT_BUILD_RCLONE`. `cargo run` uses that binary.
//! A release package copies that binary next to the DriveCraft binary.
//!
//! For an offline build, set `DRIVECRAFT_RCLONE_BIN` to a local rclone
//! binary. The script then copies that file and downloads nothing.

use std::env;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The bundled rclone release. Update this to the latest release.
const RCLONE_VERSION: &str = "v1.76.0";
const BASE_URL: &str = "https://downloads.rclone.org";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DRIVECRAFT_RCLONE_BIN");
    println!("cargo:rustc-env=DRIVECRAFT_RCLONE_VERSION={RCLONE_VERSION}");

    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    if os == "windows" {
        windows_resources();
    }
    let exe_name = if os == "windows" {
        "rclone.exe"
    } else {
        "rclone"
    };
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let source = match env::var_os("DRIVECRAFT_RCLONE_BIN") {
        Some(path) => PathBuf::from(path),
        None => {
            let cached = out_dir.join(RCLONE_VERSION).join(exe_name);
            if !cached.is_file() {
                download(&platform(&os, &arch), exe_name, &cached);
            }
            cached
        }
    };

    assert!(
        source.is_file(),
        "rclone is missing at {}",
        source.display()
    );
    println!(
        "cargo:rustc-env=DRIVECRAFT_BUILD_RCLONE={}",
        source.display()
    );
}

fn platform(os: &str, arch: &str) -> String {
    let os = match os {
        "macos" => "osx",
        "linux" | "windows" | "freebsd" | "openbsd" | "netbsd" => os,
        _ => panic!("rclone has no build for {os}. Set DRIVECRAFT_RCLONE_BIN."),
    };
    let arch = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        _ => panic!("rclone has no build for {arch}. Set DRIVECRAFT_RCLONE_BIN."),
    };
    format!("{os}-{arch}")
}

fn download(platform: &str, exe_name: &str, dest: &Path) {
    let zip_name = format!("rclone-{RCLONE_VERSION}-{platform}.zip");
    let zip_url = format!("{BASE_URL}/{RCLONE_VERSION}/{zip_name}");
    let sums_url = format!("{BASE_URL}/{RCLONE_VERSION}/SHA256SUMS");

    let sums = String::from_utf8(get(&sums_url)).expect("SHA256SUMS is not text");
    let expected = sums
        .lines()
        .filter_map(|l| l.split_once("  "))
        .find(|(_, name)| *name == zip_name)
        .map(|(hash, _)| hash.to_string())
        .unwrap_or_else(|| panic!("{zip_name} is not in {sums_url}"));

    let zip = get(&zip_url);
    let actual: String = Sha256::digest(&zip)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(actual, expected, "the checksum of {zip_name} is wrong");

    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).expect("invalid rclone zip");
    let mut binary = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let matches = entry
            .name()
            .is_ok_and(|n| n.rsplit('/').next() == Some(exe_name));
        if matches {
            entry.read_to_end(&mut binary).unwrap();
            break;
        }
    }
    assert!(!binary.is_empty(), "{exe_name} is not in {zip_name}");

    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let tmp = dest.with_extension("part");
    fs::write(&tmp, &binary).unwrap();
    make_executable(&tmp);
    fs::rename(&tmp, dest).unwrap();
}

fn get(url: &str) -> Vec<u8> {
    ureq::get(url)
        .call()
        .unwrap_or_else(|e| {
            panic!("cannot download {url}: {e}. Set DRIVECRAFT_RCLONE_BIN for an offline build.")
        })
        .body_mut()
        .with_config()
        .limit(300 * 1024 * 1024)
        .read_to_vec()
        .unwrap_or_else(|e| panic!("cannot read {url}: {e}"))
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

/// Gives DriveCraft.exe its icon, and the name and the version that
/// Explorer and the crash reports of Windows show. Without them, Windows
/// shows a default icon and the version 0.0.0.0.
fn windows_resources() {
    println!("cargo:rerun-if-changed=assets/DriveCraft.ico");
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/DriveCraft.ico")
        .set("ProductName", "DriveCraft")
        .set("FileDescription", "DriveCraft")
        .set("CompanyName", "Fabrizio Balliano")
        .set("LegalCopyright", "MIT or Apache-2.0");
    if let Err(e) = res.compile() {
        panic!("cannot embed the icon and the version of DriveCraft.exe: {e}");
    }
}
