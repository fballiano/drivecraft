//! The storage use of an iCloud account.
//!
//! The iCloud backend of rclone has no "about", so `operations/about` fails.
//! DriveCraft then asks the endpoint that the iCloud website uses, with the
//! session that rclone saved in the rclone file (the `cookies` option). The
//! request only reads. It does not change the session, and it needs no
//! password.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

const STORAGE_URL: &str = "https://setup.icloud.com/setup/ws/1/storageUsageInfo";
const WEBSITE: &str = "https://www.icloud.com";

/// The used and the total storage of the account, in bytes. `cookies` is
/// the `cookies` option of the rclone remote: "name=value; name=value".
pub fn storage(cookies: &str) -> Result<(u64, u64)> {
    let url = match dsid(cookies) {
        Some(dsid) => format!("{STORAGE_URL}?dsid={dsid}"),
        None => STORAGE_URL.to_string(),
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut resp = agent
        .get(&url)
        .header("Cookie", cookies)
        .header("Origin", WEBSITE)
        .header("Referer", &format!("{WEBSITE}/"))
        .call()
        .context("no connection to iCloud")?;
    let status = resp.status().as_u16();
    if status != 200 {
        bail!("iCloud answered with HTTP {status}");
    }
    let body: Value = resp.body_mut().read_json()?;
    parse(&body).context("iCloud did not report the storage use")
}

/// The account ID of Apple, from the cookie `X-APPLE-WEBAUTH-USER`, for
/// example `"v=1:s=0:d=12345678"`.
fn dsid(cookies: &str) -> Option<&str> {
    cookies.split(';').find_map(|c| {
        let (name, value) = c.trim().split_once('=')?;
        if name != "X-APPLE-WEBAUTH-USER" {
            return None;
        }
        value
            .trim_matches('"')
            .split(':')
            .find_map(|part| part.strip_prefix("d="))
            .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
    })
}

fn parse(body: &Value) -> Option<(u64, u64)> {
    let info = &body["storageUsageInfo"];
    let used = info["usedStorageInBytes"].as_u64()?;
    let total = info["totalStorageInBytes"].as_u64()?;
    (total > 0).then_some((used, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_account_id_comes_from_the_cookie() {
        let cookies = "X-APPLE-WEBAUTH-TOKEN=\"v=2:t=abc\"; X-APPLE-WEBAUTH-USER=\"v=1:s=0:d=12345678\"; X-APPLE-DS-WEB-SESSION-TOKEN=xyz";
        assert_eq!(dsid(cookies), Some("12345678"));
        assert_eq!(dsid("X-APPLE-WEBAUTH-USER=\"v=1:d=1&x\""), None);
        assert_eq!(dsid("other=1"), None);
    }

    #[test]
    fn the_answer_gives_used_and_total() {
        let body = json!({"storageUsageInfo": {
            "usedStorageInBytes": 1_000_u64,
            "totalStorageInBytes": 50_000_u64,
            "compStorageInBytes": 0
        }});
        assert_eq!(parse(&body), Some((1_000, 50_000)));
        assert_eq!(parse(&json!({})), None);
    }
}
