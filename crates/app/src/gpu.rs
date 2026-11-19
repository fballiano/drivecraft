//! The graphics backend of the window, with a fallback after a crash.
//!
//! A GPU driver can crash DriveCraft at the start. For example, the Vulkan
//! driver of some Intel GPUs on Windows (igvk64.dll). The process cannot
//! catch such a crash, so DriveCraft writes the backend to `gpu.txt` in the
//! settings folder before the window opens, and confirms it when the window
//! works. If a start finds a backend that is not confirmed, the last run
//! crashed with it, and this start uses the next backend in `ORDER`.
//!
//! The variable `WGPU_BACKEND` overrides all of this.

use std::path::{Path, PathBuf};

use eframe::wgpu::{Backend, Backends};

/// The backends to try, the best first. macOS has only Metal.
const ORDER: &[Backend] = if cfg!(windows) {
    &[Backend::Dx12, Backend::Vulkan, Backend::Gl]
} else if cfg!(target_os = "macos") {
    &[Backend::Metal]
} else {
    &[Backend::Vulkan, Backend::Gl]
};

const FILE: &str = "gpu.txt";

/// The backend of this run, and its state in `gpu.txt`.
pub struct Probe {
    file: PathBuf,
    backend: Backend,
    /// The number of starts in a row without a confirmation.
    tries: usize,
    confirmed: bool,
}

/// What `gpu.txt` holds.
#[derive(Debug, PartialEq)]
struct State {
    backend: Backend,
    confirmed: bool,
    tries: usize,
}

/// Selects the backend for this start, or None when there is no choice:
/// one backend only, or `WGPU_BACKEND` is set.
pub fn choose(dir: &Path) -> Option<Probe> {
    if ORDER.len() < 2 || std::env::var_os("WGPU_BACKEND").is_some() {
        return None;
    }
    let file = dir.join(FILE);
    let last = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| parse(&text));
    let state = next(last.as_ref());
    let mut probe = Probe {
        file,
        backend: state.backend,
        tries: state.tries,
        confirmed: true,
    };
    probe.write(false);
    Some(probe)
}

/// The backend after the last start.
fn next(last: Option<&State>) -> State {
    match last {
        None => State {
            backend: ORDER[0],
            confirmed: false,
            tries: 1,
        },
        Some(s) if s.confirmed => State {
            backend: s.backend,
            confirmed: false,
            tries: 1,
        },
        // The last run crashed with this backend: take the next one. After
        // the last one, start again with the first one.
        Some(s) => {
            let i = ORDER.iter().position(|b| *b == s.backend).unwrap_or(0);
            State {
                backend: ORDER[(i + 1) % ORDER.len()],
                confirmed: false,
                tries: s.tries + 1,
            }
        }
    }
}

impl Probe {
    pub fn backends(&self) -> Backends {
        match self.backend {
            Backend::Dx12 => Backends::DX12,
            Backend::Vulkan => Backends::VULKAN,
            Backend::Gl => Backends::GL,
            Backend::Metal => Backends::METAL,
            _ => Backends::PRIMARY,
        }
    }

    /// The window works with this backend.
    pub fn confirm(&mut self) {
        if !self.confirmed {
            self.tries = 0;
            self.write(true);
        }
    }

    /// The window opens again. A crash now can come from the driver too, so
    /// the backend counts as confirmed only after some frames.
    pub fn unconfirm(&mut self) {
        if self.confirmed {
            self.write(false);
        }
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    /// True when each backend failed once in this series of starts. Then
    /// a start without a GPU device does not start DriveCraft again.
    pub fn tried_all(&self) -> bool {
        self.tries >= ORDER.len()
    }

    fn write(&mut self, confirmed: bool) {
        self.confirmed = confirmed;
        let text = format!(
            "backend = {}\nconfirmed = {confirmed}\ntries = {}\n",
            name(self.backend),
            self.tries
        );
        let _ = std::fs::write(&self.file, text);
    }
}

/// The name of a backend in `gpu.txt` and in the settings page.
pub fn name(backend: Backend) -> &'static str {
    match backend {
        Backend::Dx12 => "dx12",
        Backend::Vulkan => "vulkan",
        Backend::Gl => "gl",
        Backend::Metal => "metal",
        _ => "other",
    }
}

/// The name that the user sees.
pub fn display_name(backend: Backend) -> &'static str {
    match backend {
        Backend::Dx12 => "DirectX 12",
        Backend::Vulkan => "Vulkan",
        Backend::Gl => "OpenGL",
        Backend::Metal => "Metal",
        _ => "another backend",
    }
}

fn parse(text: &str) -> Option<State> {
    let value = |key: &str| {
        text.lines()
            .filter_map(|l| l.split_once('='))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().to_string())
    };
    let backend = ORDER
        .iter()
        .copied()
        .find(|b| Some(name(*b)) == value("backend").as_deref())?;
    Some(State {
        backend,
        confirmed: value("confirmed").as_deref() == Some("true"),
        tries: value("tries").and_then(|v| v.parse().ok()).unwrap_or(1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crash_moves_to_the_next_backend_and_a_confirmation_keeps_it() {
        let first = next(None);
        assert_eq!(first.backend, ORDER[0]);

        let after_ok = next(Some(&State {
            backend: ORDER[0],
            confirmed: true,
            tries: 0,
        }));
        assert_eq!(after_ok.backend, ORDER[0], "a confirmed backend stays");

        let after_crash = next(Some(&State {
            backend: ORDER[0],
            confirmed: false,
            tries: 1,
        }));
        assert_eq!(after_crash.backend, ORDER[1 % ORDER.len()]);
        assert_eq!(after_crash.tries, 2);

        let last = *ORDER.last().unwrap();
        let wrapped = next(Some(&State {
            backend: last,
            confirmed: false,
            tries: ORDER.len(),
        }));
        assert_eq!(wrapped.backend, ORDER[0], "after the last, the first again");
    }

    #[test]
    fn gpu_txt_round_trips() {
        let text = format!(
            "backend = {}\nconfirmed = false\ntries = 2\n",
            name(ORDER[0])
        );
        assert_eq!(
            parse(&text),
            Some(State {
                backend: ORDER[0],
                confirmed: false,
                tries: 2
            })
        );
        assert_eq!(parse("backend = something\n"), None, "an unknown backend");
    }
}
