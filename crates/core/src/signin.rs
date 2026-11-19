//! The sign-in rules that do not need rclone: when an error means that the
//! provider ended the sign-in, and what an answer of the rclone config API
//! asks for.

use serde_json::Value;

/// The step of a sign-in that the UI shows. `account` is None for a new
/// account, else the account that signs in again.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SignInStep {
    /// No sign-in runs.
    #[default]
    Idle,
    /// rclone waits for the sign-in on the website, in the browser.
    Browser { account: Option<String> },
    /// DriveCraft sends the password or the code to the provider.
    Working { account: Option<String> },
    /// The provider asks for a value, for example the code that it sent.
    Question {
        account: Option<String>,
        /// The rclone option, for example "config_2fa".
        name: String,
        /// The question of rclone.
        help: String,
        /// The problem with the last answer, if any.
        error: String,
    },
}

impl SignInStep {
    /// The account of the sign-in that runs, or None.
    pub fn account(&self) -> Option<&str> {
        match self {
            SignInStep::Idle => None,
            SignInStep::Browser { account }
            | SignInStep::Working { account }
            | SignInStep::Question { account, .. } => account.as_deref(),
        }
    }

    pub fn is_idle(&self) -> bool {
        *self == SignInStep::Idle
    }
}

/// True when an rclone error means that the provider ended the sign-in.
/// Then a new sign-in is necessary, not a resync.
///
/// - Most OAuth providers: rclone cannot refresh the token, and its error
///   says `try refreshing with "rclone config reconnect <remote>:"`, often
///   with `invalid_grant`.
/// - Dropbox: `expired_access_token`, `invalid_access_token`.
/// - pCloud: `Invalid 'access_token' provided`. Yandex: `UnauthorizedError`.
/// - iCloud: the trust token ended ("trust token expired, please reauth"),
///   or Apple refused the sign-in without the password ("sign in failed").
pub fn expired(error: &str) -> bool {
    let e = error.to_lowercase();
    [
        "config reconnect",
        "invalid_grant",
        "expired_access_token",
        "invalid_access_token",
        "invalid 'access_token'",
        "unauthorizederror",
        "trust token",
        "please reauth",
        "sign in failed:",
    ]
    .iter()
    .any(|p| e.contains(p))
}

/// What an answer of `config/create` or `config/update` (with
/// `nonInteractive`) asks for next.
#[derive(Debug, PartialEq)]
pub enum ConfigStep {
    /// The configuration is complete.
    Done,
    /// rclone continues without a question: send `result` for `state`.
    Next { state: String, result: String },
    /// rclone asks the user. The answer goes with `state`.
    Ask {
        state: String,
        name: String,
        help: String,
        error: String,
    },
}

/// Reads an `fs.ConfigOut` answer: `{"State", "Option", "Result", "Error"}`.
pub fn config_step(out: &Value) -> ConfigStep {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let state = text(&out["State"]);
    if state.is_empty() {
        return ConfigStep::Done;
    }
    let option = &out["Option"];
    if option.is_object() {
        ConfigStep::Ask {
            state,
            name: text(&option["Name"]),
            help: text(&option["Help"]),
            error: text(&out["Error"]),
        }
    } else {
        ConfigStep::Next {
            state,
            result: text(&out["Result"]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ended_sign_ins_are_detected() {
        for e in [
            "couldn't fetch token: invalid_grant: maybe token expired? - try refreshing with \"rclone config reconnect onedrive:\"",
            "path/to/x: expired_access_token/",
            "pcloud error: Invalid 'access_token' provided. (2094)",
            "[401 - UnauthorizedError]",
            "trust token expired, please reauth",
            "missing icloud trust token: try refreshing it with \"rclone config reconnect iclouddrive:\"",
            "sign in failed: incorrect username or password",
        ] {
            assert!(expired(e), "{e}");
        }
        for e in [
            "bisync aborted: must run --resync",
            "dial tcp: lookup graph.microsoft.com: no such host",
            "429 Too Many Requests",
            "file is blocked: malwareDetected",
        ] {
            assert!(!expired(e), "{e}");
        }
    }

    #[test]
    fn config_answers_become_steps() {
        assert_eq!(config_step(&json!({})), ConfigStep::Done);
        assert_eq!(
            config_step(&json!({"State": "", "Option": null})),
            ConfigStep::Done
        );
        assert_eq!(
            config_step(&json!({
                "State": "2fa_do",
                "Option": {"Name": "config_2fa", "Help": "Two-factor authentication"},
                "Error": ""
            })),
            ConfigStep::Ask {
                state: "2fa_do".into(),
                name: "config_2fa".into(),
                help: "Two-factor authentication".into(),
                error: String::new(),
            }
        );
        assert_eq!(
            config_step(&json!({"State": "next", "Option": null, "Result": "x"})),
            ConfigStep::Next {
                state: "next".into(),
                result: "x".into()
            }
        );
    }
}
