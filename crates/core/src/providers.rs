//! The cloud providers that DriveCraft can add as accounts.
//!
//! Most providers are rclone backends with a sign-in in the browser
//! (OAuth), so `rclone config create <id> <backend> <options>` is the whole
//! setup. Some need one value from the user first (`Provider::field`).
//! Some sign in with a password and a code in DriveCraft (`SignIn::Password`).
//! bisync needs modification times that rclone can set, or hashes (see
//! https://rclone.org/overview/). A provider without them (Mega) has
//! `computer_wins`: this computer is the master of the account.

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
    pub sign_in: SignIn,
    /// True when rclone cannot set modification times on the provider
    /// (`Precision` is `ModTimeNotSupported`). Then bisync cannot find the
    /// newer version, so this computer is the master: a resync keeps the
    /// local version of each different file, a conflict keeps both versions,
    /// and an online change that keeps the size is not found.
    pub computer_wins: bool,
}

/// How the user signs in to a provider.
pub enum SignIn {
    /// On the website of the provider, in the browser (OAuth).
    Browser,
    /// With a user name and a password in DriveCraft, and then a code that
    /// the provider sends. The password goes to rclone through its local
    /// API, never on a command line. After the sign-in, DriveCraft removes
    /// it from the rclone file: rclone continues with the session of the
    /// provider. When the provider ends that session, the user signs in
    /// again (`Account::needs_sign_in`).
    Password {
        /// The rclone options for the user name and the password.
        user_key: &'static str,
        password_key: &'static str,
        /// The name of the user name, for example "Apple ID".
        user_label: &'static str,
        /// What the user must know before the sign-in.
        hint: &'static str,
        /// An optional code that goes with the password (the rclone option
        /// and its label), for example the 2FA code of Mega.
        code: Option<(&'static str, &'static str)>,
        /// The rclone options of the session. A new sign-in removes them
        /// first, and starts rclone again after it, because rclone keeps
        /// the session in memory (Mega).
        session_keys: &'static [&'static str],
    },
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
        sign_in: SignIn::Browser,
        computer_wins: false,
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
        sign_in: SignIn::Browser,
        computer_wins: false,
    },
    Provider {
        id: "dropbox",
        label: "Dropbox",
        options: &[],
        available: true,
        field: None,
        sign_in: SignIn::Browser,
        computer_wins: false,
    },
    Provider {
        id: "box",
        label: "Box",
        options: &[],
        available: true,
        field: None,
        sign_in: SignIn::Browser,
        computer_wins: false,
    },
    Provider {
        id: "pcloud",
        label: "pCloud",
        options: &[],
        available: true,
        field: None,
        sign_in: SignIn::Browser,
        computer_wins: false,
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
        sign_in: SignIn::Browser,
        computer_wins: false,
    },
    Provider {
        id: "hidrive",
        label: "HiDrive",
        options: &[],
        available: true,
        field: None,
        sign_in: SignIn::Browser,
        computer_wins: false,
    },
    Provider {
        id: "yandex",
        label: "Yandex Disk",
        options: &[],
        available: true,
        field: None,
        sign_in: SignIn::Browser,
        computer_wins: false,
    },
    Provider {
        id: "iclouddrive",
        label: "iCloud Drive",
        options: &[("service", "drive")],
        // "Access iCloud Data on the Web" must be on, and the trust token of
        // Apple ends after 30 days.
        available: true,
        field: None,
        sign_in: SignIn::Password {
            user_key: "apple_id",
            password_key: "password",
            user_label: "Apple ID",
            hint: "Use your normal Apple Account password, not an app-specific password. \
                   On your iPhone, turn on Settings > Apple Account > iCloud > \
                   Access iCloud Data on the Web.",
            code: None,
            session_keys: &[],
        },
        computer_wins: false,
    },
    Provider {
        id: "mega",
        label: "MEGA",
        options: &[],
        available: true,
        field: None,
        sign_in: SignIn::Password {
            user_key: "user",
            password_key: "pass",
            user_label: "Email",
            hint: "Use the email and the password of your MEGA account.",
            code: Some(("2fa", "Two-factor code (only if your account uses it)")),
            session_keys: &["session_id", "master_key"],
        },
        // rclone cannot set modification times on MEGA.
        computer_wins: true,
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
    fn only_mega_lets_this_computer_win() {
        // rclone cannot set modification times only on MEGA. A different
        // provider with `computer_wins` would lose the "newer" rule.
        let winners: Vec<_> = PROVIDERS
            .iter()
            .filter(|p| p.computer_wins)
            .map(|p| p.id)
            .collect();
        assert_eq!(winners, ["mega"]);
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
