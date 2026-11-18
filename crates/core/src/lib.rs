//! The DriveCraft core. It controls rclone and contains all sync logic.
//! It has no UI code, so other front ends can use it too.

pub mod engine;
pub mod other_clients;
pub mod rclone;
pub mod settings;
pub mod snapshot;

pub use engine::{Command, EngineHandle, Event, Notify, Prefs, Transfer, spawn};
pub use settings::{Account, KeepState, Settings, provider_label};
pub use snapshot::{ChangeKind, FileChange};
