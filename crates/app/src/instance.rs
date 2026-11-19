//! Only one DriveCraft runs at a time.
//!
//! Two instances would share the rclone session file: the second one stops
//! the rclone of the first one (`stop_previous`), also during a sync. So the
//! first instance holds a lock on `instance.lock` in the settings folder
//! until it exits. A second start only asks the first instance to show its
//! window (the file `show-window`), and then exits.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Holds the lock while it exists.
pub struct Guard {
    _file: File,
}

/// The requests of later starts to show the window.
pub struct ShowRequest {
    path: PathBuf,
    last_check: Option<Instant>,
}

pub enum Start {
    /// This is the only instance.
    First(Guard, ShowRequest),
    /// Another instance runs. It got the request to show its window.
    Other,
}

/// Takes the lock in `dir`, or asks the instance that has it to show its
/// window.
pub fn start(dir: &Path) -> io::Result<Start> {
    std::fs::create_dir_all(dir)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("instance.lock"))?;
    let show_request = dir.join("show-window");
    match file.try_lock() {
        Ok(()) => {
            // A request from before this start is old.
            let _ = std::fs::remove_file(&show_request);
            Ok(Start::First(
                Guard { _file: file },
                ShowRequest {
                    path: show_request,
                    last_check: None,
                },
            ))
        }
        Err(TryLockError::WouldBlock) => {
            File::create(&show_request)?;
            Ok(Start::Other)
        }
        Err(TryLockError::Error(e)) => Err(e),
    }
}

impl ShowRequest {
    /// True once for each request of a second start to show the window.
    /// It looks at the file at most once each second.
    pub fn take(&mut self) -> bool {
        if self
            .last_check
            .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
        {
            return false;
        }
        self.last_check = Some(Instant::now());
        std::fs::remove_file(&self.path).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_start_asks_the_first_to_show_its_window() {
        let dir = std::env::temp_dir().join(format!("drivecraft-instance-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let Start::First(guard, mut request) = start(&dir).unwrap() else {
            panic!("the first start must get the lock");
        };
        assert!(!request.take(), "no request yet");

        assert!(matches!(start(&dir).unwrap(), Start::Other));
        request.last_check = None;
        assert!(request.take());
        request.last_check = None;
        assert!(!request.take(), "each request counts once");

        drop(guard);
        assert!(
            matches!(start(&dir).unwrap(), Start::First(..)),
            "the lock ends with the first instance"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
