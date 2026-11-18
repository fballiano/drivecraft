//! The cloud providers that DriveCraft can add as accounts.
//!
//! Each provider is an rclone backend with a sign-in in the browser
//! (OAuth), so `rclone config create <id> <backend> <options>` is the whole
//! setup. Add a provider here only after a test with a real account.

pub struct Provider {
    /// The rclone backend, for example "drive".
    pub id: &'static str,
    /// The name of the service, for example "Google Drive".
    pub label: &'static str,
    /// The company that runs the sign-in, for "Sign in with …".
    pub company: &'static str,
    /// rclone options for `config create`, as key and value.
    pub options: &'static [(&'static str, &'static str)],
    /// False hides the sign-in button. Existing accounts continue to work.
    pub available: bool,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "onedrive",
        label: "OneDrive",
        company: "Microsoft",
        options: &[],
        available: true,
    },
    Provider {
        id: "drive",
        label: "Google Drive",
        company: "Google",
        // Full access to "My Drive". Google Docs, Sheets, and Slides are not
        // real files: a local export of them is not safe in a two-way sync
        // (deleting the export deletes the document). They stay only online.
        options: &[("scope", "drive"), ("skip_gdocs", "true")],
        // rclone's shared Google client ID stops working during 2026, and
        // rclone 1.76 refuses it by default. Google Drive needs a client ID
        // of its own first (a Google Cloud project, and Google's review of
        // the "drive" scope).
        available: false,
    },
    Provider {
        id: "dropbox",
        label: "Dropbox",
        company: "Dropbox",
        options: &[],
        available: true,
    },
    Provider {
        id: "box",
        label: "Box",
        company: "Box",
        options: &[],
        available: true,
    },
];

/// The providers that the user can add now.
pub fn available_providers() -> impl Iterator<Item = &'static Provider> {
    PROVIDERS.iter().filter(|p| p.available)
}

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// The name that the user sees for a provider.
pub fn provider_label(id: &str) -> &str {
    provider(id).map_or(id, |p| p.label)
}
