//! The cloud providers that DriveCraft can add as accounts.
//!
//! Each provider is an rclone backend with a sign-in in the browser
//! (OAuth), so `rclone config create <id> <backend> <options>` is the whole
//! setup. Some need one value from the user first (`Provider::field`).
//! Each provider must have modification times or hashes, because bisync
//! needs them (see https://rclone.org/overview/).

pub struct Provider {
    /// The rclone backend, for example "drive".
    pub id: &'static str,
    /// The name of the service, for example "Google Drive".
    pub label: &'static str,
    /// rclone options for `config create`, as key and value.
    pub options: &'static [(&'static str, &'static str)],
    /// False hides the sign-in button. Existing accounts continue to work.
    pub available: bool,
    /// A value that the user types before the sign-in.
    pub field: Option<Field>,
}

/// One value for `config create`, for example the drive ID of kDrive.
pub struct Field {
    /// The rclone option.
    pub key: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    /// What the value may contain.
    pub check: fn(&str) -> bool,
}

impl Field {
    /// The value without spaces at the ends, if it is valid.
    pub fn accept(&self, value: &str) -> Option<String> {
        let value = value.trim();
        (!value.is_empty() && (self.check)(value)).then(|| value.to_string())
    }
}

fn digits(value: &str) -> bool {
    value.chars().all(|c| c.is_ascii_digit())
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "onedrive",
        label: "OneDrive",
        options: &[],
        available: true,
        field: None,
    },
    Provider {
        id: "drive",
        label: "Google Drive",
        // Full access to "My Drive". Google Docs, Sheets, and Slides are not
        // real files: a local export of them is not safe in a two-way sync
        // (deleting the export deletes the document). They stay only online.
        options: &[("scope", "drive"), ("skip_gdocs", "true")],
        // rclone's shared Google client ID stops working during 2026, and
        // rclone 1.76 refuses it by default. Google Drive needs a client ID
        // of its own first (a Google Cloud project, and Google's review of
        // the "drive" scope).
        available: false,
        field: None,
    },
    Provider {
        id: "dropbox",
        label: "Dropbox",
        options: &[],
        available: true,
        field: None,
    },
    Provider {
        id: "box",
        label: "Box",
        options: &[],
        available: true,
        field: None,
    },
    Provider {
        id: "pcloud",
        label: "pCloud",
        options: &[],
        available: true,
        field: None,
    },
    Provider {
        id: "kdrive",
        label: "kDrive",
        options: &[],
        available: true,
        // rclone does not find the drive itself.
        field: Some(Field {
            key: "drive_id",
            label: "kDrive ID",
            hint: "The number in the address of the kDrive web app, for example 0654321.",
            check: digits,
        }),
    },
    Provider {
        id: "hidrive",
        label: "HiDrive",
        options: &[],
        available: true,
        field: None,
    },
    Provider {
        id: "yandex",
        label: "Yandex Disk",
        options: &[],
        available: true,
        field: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_is_complete_and_unique() {
        let mut ids: Vec<_> = PROVIDERS.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PROVIDERS.len());
        assert_eq!(provider_label("kdrive"), "kDrive");
        assert_eq!(provider_label("unknown"), "unknown");
    }

    #[test]
    fn the_kdrive_id_must_be_a_number() {
        let field = provider("kdrive").and_then(|p| p.field.as_ref()).unwrap();
        assert_eq!(field.accept(" 0654321 ").as_deref(), Some("0654321"));
        assert_eq!(field.accept(""), None);
        assert_eq!(field.accept("12 34"), None);
        assert_eq!(field.accept("--config=/etc/x"), None);
    }
}
