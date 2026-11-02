//! New Crest releases, from the repository's latest GitHub release. Off by default: turning on
//! "Check for updates" in Settings is the consent for one request a day to GitHub, which sees
//! the PC's address and Crest's version (in the user agent) and nothing else.
//!
//! Crest never downloads or runs the new version itself. The notice opens the release page; the
//! user downloads `crest.exe` and opens it, and the installer offers to replace the old copy.

use std::thread;

use windows::Data::Json::JsonObject;
use windows::core::{HSTRING, Result};

use crate::system::http::{self, Request};
use crate::system::services::{Event, Notifier};

const LATEST: &str = "https://api.github.com/repos/rinci-labs/crest/releases/latest";
/// Release pages the notice may open; any other address in a reply is ignored.
const RELEASES: &str = "https://github.com/rinci-labs/crest/releases/";
const USER_AGENT: &str = concat!("Crest/", env!("CARGO_PKG_VERSION"), " (+https://github.com/rinci-labs/crest)");
/// The version of this build.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// A release newer than this build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// "0.2.0", without the tag's "v".
    pub version: String,
    /// Its page on GitHub.
    pub url: String,
}

/// Ask GitHub for the latest release on a short-lived thread. Sends `Event::Update` with the
/// release when it is newer than this build, `None` when this build is current or the check
/// failed (the failure is logged without any reply content).
pub fn check(notify: Notifier) {
    let spawned = thread::Builder::new().name("updates".into()).spawn(move || {
        let found = match latest() {
            Ok(found) => found,
            Err(e) => {
                error_log!("update check failed: {e}");
                None
            }
        };
        notify.send(Event::Update(found));
    });
    if let Err(e) = spawned {
        error_log!("failed to create update thread: {e}");
    }
}

fn latest() -> Result<Option<Release>> {
    let headers = ["Accept: application/vnd.github+json".to_string()];
    let text = http::send(&Request { method: "GET", url: LATEST, headers: &headers, body: None }, USER_AGENT)?;
    Ok(parse(&text).filter(|release| newer(&release.version, CURRENT)))
}

/// The release in a `releases/latest` reply. Drafts and pre-releases never reach that endpoint,
/// but a tag with a suffix ("0.3.0-beta") is skipped anyway.
fn parse(text: &str) -> Option<Release> {
    let root = JsonObject::Parse(&HSTRING::from(text)).ok()?;
    let tag = root.GetNamedString(&HSTRING::from("tag_name")).ok()?.to_string();
    let url = root.GetNamedString(&HSTRING::from("html_url")).ok()?.to_string();
    let version = tag.trim().trim_start_matches('v').to_string();
    let valid = version_parts(&version).is_some() && url.starts_with(RELEASES) && url.len() <= 200;
    valid.then_some(Release { version, url })
}

/// "1.2.3" as numbers; `None` for anything else.
fn version_parts(version: &str) -> Option<Vec<u32>> {
    let parts: Option<Vec<u32>> = version.split('.').map(|part| part.parse().ok()).collect();
    parts.filter(|parts| (1..=4).contains(&parts.len()))
}

/// A version as one number for the settings file ("0.2.0" → 2048); 0 for anything else.
pub fn code(version: &str) -> u32 {
    let Some(mut parts) = version_parts(version) else { return 0 };
    parts.resize(3, 0);
    (parts[0].min(1023) << 20) | (parts[1].min(1023) << 10) | parts[2].min(1023)
}

/// `remote` is a later version than `local`; missing parts count as 0 ("0.2" == "0.2.0").
pub fn newer(remote: &str, local: &str) -> bool {
    let (Some(mut remote), Some(mut local)) = (version_parts(remote), version_parts(local)) else { return false };
    let len = remote.len().max(local.len());
    remote.resize(len, 0);
    local.resize(len, 0);
    remote > local
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.99.1"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
        assert!(!newer("0.3.0-beta", "0.1.0"));
        assert!(!newer("latest", "0.1.0"));
        assert!(code("0.2.0") > code("0.1.9"));
        assert_eq!(code("0.2"), code("0.2.0"));
        assert_eq!(code("v0.2.0"), 0);
    }

    #[test]
    fn only_release_pages_are_kept() {
        let reply = r#"{"tag_name":"v0.2.0","html_url":"https://github.com/rinci-labs/crest/releases/tag/v0.2.0","body":"notes"}"#;
        assert_eq!(parse(reply), Some(Release { version: "0.2.0".into(), url: "https://github.com/rinci-labs/crest/releases/tag/v0.2.0".into() }));
        let elsewhere = r#"{"tag_name":"v0.2.0","html_url":"https://example.com/crest.exe"}"#;
        assert_eq!(parse(elsewhere), None);
        let suffix = r#"{"tag_name":"v0.2.0-rc1","html_url":"https://github.com/rinci-labs/crest/releases/tag/v0.2.0-rc1"}"#;
        assert_eq!(parse(suffix), None);
        assert_eq!(parse("not json"), None);
    }
}
