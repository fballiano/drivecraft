//! Starts `rclone rcd` and sends requests to its remote control API.

use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const USER: &str = "drivecraft";

/// Finds the rclone binary:
/// 1. the path in the settings, if it is not empty (for tests only);
/// 2. the bundled binary next to the DriveCraft binary
///    (in the macOS app bundle: `Contents/MacOS`; `Contents/Resources` also works);
/// 3. `build_rclone`: the binary that the build downloaded (for `cargo run`).
///
/// DriveCraft does not use rclone from the PATH. It always uses the
/// bundled version, because that is the version that we test.
pub fn find_binary(setting: &str, build_rclone: Option<&Path>) -> Result<PathBuf> {
    if !setting.is_empty() {
        return Ok(PathBuf::from(setting));
    }
    let name = if cfg!(windows) {
        "rclone.exe"
    } else {
        "rclone"
    };
    let exe = std::env::current_exe().context("cannot find the DriveCraft binary")?;
    let dir = exe
        .parent()
        .context("the DriveCraft binary has no folder")?;
    let mut candidates = vec![dir.join(name), dir.join("../Resources").join(name)];
    candidates.extend(build_rclone.map(Path::to_path_buf));
    match candidates.iter().find(|p| p.is_file()) {
        Some(found) => Ok(found.clone()),
        None => bail!(
            "the bundled rclone is missing. Expected it at {}",
            candidates[0].display()
        ),
    }
}

/// A command that runs rclone.
///
/// On Windows, rclone is a console program. A GUI app that starts it gets a
/// new console window, and a click on its close button stops rclone. So the
/// command has `CREATE_NO_WINDOW`. It also gives rclone the proxy server of
/// Windows: the browser uses it, but rclone reads only `HTTPS_PROXY`.
pub fn command(binary: &Path) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(binary);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
        let has_proxy = ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
            .iter()
            .any(|v| std::env::var_os(v).is_some());
        if !has_proxy && let Some((proxy, no_proxy)) = windows_proxy() {
            c.env("HTTPS_PROXY", &proxy)
                .env("HTTP_PROXY", &proxy)
                .env("NO_PROXY", no_proxy);
        }
    }
    c
}

/// The fixed proxy server of Windows (Settings > Network > Proxy), and the
/// addresses without a proxy. A proxy script (PAC) is not supported.
#[cfg(windows)]
fn windows_proxy() -> Option<(String, String)> {
    let key = windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    if key.get_u32("ProxyEnable").ok()? == 0 {
        return None;
    }
    let server = key.get_string("ProxyServer").ok()?;
    let proxy = parse_proxy_server(&server)?;
    let bypass = key.get_string("ProxyOverride").unwrap_or_default();
    Some((proxy, no_proxy(&bypass)))
}

/// A `ProxyServer` value is "host:port", or a list such as
/// "http=host:80;https=host:443". Returns the proxy for HTTPS as a URL.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_proxy_server(value: &str) -> Option<String> {
    let value = value.trim();
    let server = if value.contains('=') {
        let entry = |scheme: &str| {
            value.split(';').find_map(|part| {
                let (k, v) = part.split_once('=')?;
                (k.trim().eq_ignore_ascii_case(scheme)).then(|| v.trim().to_string())
            })
        };
        entry("https").or_else(|| entry("http"))?
    } else {
        value.to_string()
    };
    if server.is_empty() {
        return None;
    }
    Some(if server.contains("://") {
        server
    } else {
        format!("http://{server}")
    })
}

/// `ProxyOverride` ("*.local;<local>;10.*") as `NO_PROXY`. The local
/// addresses always go without the proxy.
#[cfg_attr(not(windows), allow(dead_code))]
fn no_proxy(bypass: &str) -> String {
    let mut hosts = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    hosts.extend(
        bypass
            .split(';')
            .map(str::trim)
            .filter(|h| !h.is_empty() && *h != "<local>")
            .map(|h| {
                h.trim_start_matches("*.")
                    .trim_end_matches(".*")
                    .to_string()
            }),
    );
    hosts.join(",")
}

/// A client for the rclone remote control API.
struct Client {
    url: String,
    auth: String,
    agent: ureq::Agent,
}

impl Client {
    fn new(port: u16, pass: &str, timeout: Duration) -> Client {
        let token = base64::engine::general_purpose::STANDARD.encode(format!("{USER}:{pass}"));
        Client {
            url: format!("http://127.0.0.1:{port}"),
            auth: format!("Basic {token}"),
            agent: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(timeout))
                .build()
                .into(),
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value> {
        let mut resp = self
            .agent
            .post(&format!("{}/{method}", self.url))
            .header("Authorization", &self.auth)
            .send_json(&params)
            .with_context(|| format!("{method}: no connection to rclone"))?;
        let status = resp.status().as_u16();
        let body: Value = resp.body_mut().read_json().unwrap_or(Value::Null);
        if status != 200 {
            let msg = body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            bail!("{method}: {msg}");
        }
        Ok(body)
    }
}

/// The port and password of the running rclone. DriveCraft stores them in a
/// private file. If DriveCraft stops without a normal close, the next start
/// uses this file to stop the old rclone. Only our own rclone accepts the
/// password, so this cannot stop a different process.
#[derive(Serialize, Deserialize)]
struct Session {
    port: u16,
    pass: String,
}

/// A running `rclone rcd` process.
///
/// The process listens on 127.0.0.1 only and requires a random password.
/// The password goes through environment variables, so it is not visible
/// in the process list.
pub struct Rclone {
    child: Child,
    client: Client,
    session_file: PathBuf,
    /// True after `stop`. Then `Drop` does nothing.
    stopped: bool,
}

impl Rclone {
    /// Starts rclone with its own configuration file `config`, so that
    /// DriveCraft does not share accounts with the user's own rclone.
    /// `env` sets extra rclone options, for example
    /// `RCLONE_ONEDRIVE_AV_OVERRIDE`.
    pub fn start(
        binary: &Path,
        config: &Path,
        log_file: &Path,
        session_file: &Path,
        env: &[(&str, &str)],
    ) -> Result<Self> {
        stop_previous(session_file);

        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let pass = random_token();
        let child = command(binary)
            .arg("rcd")
            .arg("--config")
            .arg(config)
            .arg(format!("--rc-addr=127.0.0.1:{port}"))
            .arg("--log-file")
            .arg(log_file)
            .arg("--log-level=INFO")
            .env("RCLONE_RC_USER", USER)
            .env("RCLONE_RC_PASS", &pass)
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("cannot start \"{}\"", binary.display()))?;
        let mut rc = Rclone {
            child,
            client: Client::new(port, &pass, Duration::from_secs(300)),
            session_file: session_file.to_path_buf(),
            stopped: false,
        };

        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = rc.child.try_wait()? {
                bail!(
                    "rclone stopped at start ({status}). See {}",
                    log_file.display()
                );
            }
            if rc.call("rc/noop", json!({})).is_ok() {
                break;
            }
            if Instant::now() > deadline {
                bail!(
                    "rclone did not answer in 15 seconds. See {}",
                    log_file.display()
                );
            }
            sleep(Duration::from_millis(100));
        }
        write_private(session_file, &serde_json::to_vec(&Session { port, pass })?)?;
        Ok(rc)
    }

    /// Sends one request to the rclone API and returns the JSON response.
    pub fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.client.call(method, params)
    }

    /// The process ID of rclone.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The exit status, if rclone stopped by itself. Then every call fails
    /// with "no connection to rclone".
    pub fn exited(&mut self) -> Option<String> {
        if self.stopped {
            return None;
        }
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.to_string()),
            _ => None,
        }
    }

    pub fn version(&self) -> Result<String> {
        let v = self.call("core/version", json!({}))?;
        Ok(v["version"].as_str().unwrap_or("unknown").to_string())
    }
}

impl Rclone {
    /// Stops rclone and removes the session file. Before a restart, call
    /// this first: the new rclone writes a new session file.
    pub fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        let _ = self.call("mount/unmountall", json!({}));
        let _ = self.call("core/quit", json!({}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                break;
            }
            sleep(Duration::from_millis(100));
        }
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = fs::remove_file(&self.session_file);
    }
}

impl Drop for Rclone {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Stops an rclone that a previous DriveCraft run did not stop.
fn stop_previous(session_file: &Path) {
    let Ok(data) = fs::read(session_file) else {
        return;
    };
    if let Ok(old) = serde_json::from_slice::<Session>(&data) {
        let client = Client::new(old.port, &old.pass, Duration::from_secs(10));
        if client.call("rc/noop", json!({})).is_ok() {
            let _ = client.call("mount/unmountall", json!({}));
            let _ = client.call("core/quit", json!({}));
        }
    }
    let _ = fs::remove_file(session_file);
}

/// The default rclone configuration file of the user's own rclone.
/// `RCLONE_CONFIG` and the other rclone rules apply, because rclone itself
/// finds the file.
pub fn default_config_file(binary: &Path) -> Option<PathBuf> {
    let out = command(binary)
        .args(["config", "file"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    // The last line is the path, after a line such as
    // "Configuration file is stored at:".
    let text = String::from_utf8_lossy(&out.stdout);
    let path = text.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    Some(PathBuf::from(path))
}

/// The `[name]` section of an rclone configuration file, with its header.
/// None if the file is encrypted or has no such section.
pub fn config_section(text: &str, name: &str) -> Option<String> {
    if text.starts_with("# Encrypted rclone configuration File") {
        return None;
    }
    let header = format!("[{name}]");
    let mut lines = text.lines().skip_while(|l| l.trim() != header);
    let mut out = format!("{}\n", lines.next()?.trim());
    for line in lines.take_while(|l| !l.trim_start().starts_with('[')) {
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

/// Writes a file that only the current user can read.
pub fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(data)?;
    Ok(())
}

fn random_token() -> String {
    // RandomState gets its keys from the OS random source.
    (0..4)
        .map(|_| format!("{:016x}", RandomState::new().build_hasher().finish()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_proxy_values_become_urls() {
        assert_eq!(
            parse_proxy_server("proxy.corp:8080").as_deref(),
            Some("http://proxy.corp:8080")
        );
        assert_eq!(
            parse_proxy_server("http=p1:80;https=p2:443;ftp=p3:21").as_deref(),
            Some("http://p2:443"),
            "the HTTPS entry first"
        );
        assert_eq!(
            parse_proxy_server("http=p1:80").as_deref(),
            Some("http://p1:80")
        );
        assert_eq!(parse_proxy_server("ftp=p3:21"), None);
        assert_eq!(parse_proxy_server(" "), None);
        assert_eq!(
            no_proxy("*.corp.example;<local>;10.*"),
            "localhost,127.0.0.1,corp.example,10"
        );
    }

    #[test]
    fn config_section_returns_only_the_named_section() {
        let text = "[other]\ntype = drive\n\n[onedrive]\ntype = onedrive\ntoken = {\"a\":1}\ndrive_id = abc\n\n[last]\ntype = s3\n";
        assert_eq!(
            config_section(text, "onedrive").unwrap(),
            "[onedrive]\ntype = onedrive\ntoken = {\"a\":1}\ndrive_id = abc\n\n"
        );
        assert_eq!(config_section(text, "last").unwrap(), "[last]\ntype = s3\n");
        assert!(config_section(text, "missing").is_none());
        assert!(
            config_section(
                "# Encrypted rclone configuration File\n\nRCLONE_ENCRYPT_V0:\nxyz",
                "onedrive"
            )
            .is_none()
        );
    }
}
