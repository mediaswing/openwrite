//! Is there a newer version than this one?
//!
//! Asked once, in the background, when the editor starts. If the answer is yes
//! the window offers the download; if it is no, or the question could not be
//! asked, nothing is said and nothing is in the way. A writer opening a draft
//! should not have to dismiss anything to start typing.
//!
//! # It talks to GitHub
//!
//! This is the one thing the program does over the network without being asked,
//! so it should be easy to find and easy to refuse. It fetches one small JSON
//! document from `api.github.com`, sends no identifying information beyond a
//! `User-Agent` of `openwrite`, and setting [`DISABLE_ENV`] to anything at all
//! stops it happening. Every check is recorded in the debug log.
//!
//! # Why `curl`
//!
//! GitHub's API is HTTPS, and [`crate::ai::http`] deliberately speaks only
//! plain HTTP — its whole point is that nothing leaves the machine, so a TLS
//! stack there would be the wrong tool wearing the wrong name. Bringing in a
//! TLS library for one request a session would be a hundred crates for a
//! version number.
//!
//! So this shells out to `curl` — see [`crate::curl`] for where it is found.
//! If it is missing the check fails, which is a thing this module already has
//! to handle gracefully.

use crate::json;
use crate::log;
use std::process::{Command, Stdio};
use std::time::Duration;

/// The repository releases are published from.
pub const REPO: &str = "mediaswing/openwrite";

/// The only place a release page is allowed to be.
///
/// The download button hands [`crate::browser::open`] whatever `html_url` the
/// reply carried, and that is a string off the network being turned into
/// "launch whatever the system opens this with". It is HTTPS from GitHub, so it
/// should always be a GitHub page — which is exactly why it costs nothing to
/// insist on it, and to fall back to the canonical releases page if it is not.
const RELEASES_PREFIX: &str = "https://github.com/";

/// Set this to anything to stop the editor asking.
pub const DISABLE_ENV: &str = "OPENWRITE_NO_UPDATE_CHECK";

/// How long to give the whole request. A check nobody asked for does not get to
/// hold anything up, and a slow answer is the same as no answer.
const TIMEOUT: Duration = Duration::from_secs(8);

/// A version, as three numbers. Anything after them — `-rc1`, build metadata —
/// is not compared, because this only ever asks "is there something newer".
pub type Version = (u64, u64, u64);

/// A release newer than this build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The tag as GitHub spells it, for showing: `v1.2.0`.
    pub tag: String,
    /// The release page, which is where the downloads are.
    pub url: String,
}

impl Release {
    /// `1.2.0`, without the tag's leading `v`.
    pub fn number(&self) -> String {
        let (major, minor, patch) = self.version;
        format!("{major}.{minor}.{patch}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The check was turned off.
    Disabled,
    /// `curl` is not on this machine, or would not run.
    NoCurl(String),
    /// The request failed or timed out.
    Request(String),
    /// The answer was not what GitHub sends.
    Reply(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Disabled => write!(f, "the update check is turned off"),
            Error::NoCurl(what) => write!(f, "could not run curl: {what}"),
            Error::Request(what) => write!(f, "could not ask GitHub: {what}"),
            Error::Reply(what) => write!(f, "GitHub said something unexpected: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// The version this build is.
pub fn current() -> Version {
    version(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0))
}

/// Has the writer turned the check off?
///
/// Set to anything, including nothing: `OPENWRITE_NO_UPDATE_CHECK=` is how an
/// environment file or a CI matrix usually ends up expressing it, and somebody
/// who has put the name in an environment at all has said what they want.
pub fn disabled() -> bool {
    std::env::var_os(DISABLE_ENV).is_some()
}

/// Read `v1.2.3`, `1.2`, `1.2.3-rc1` into three numbers.
pub fn version(text: &str) -> Option<Version> {
    let text = text.trim();
    let text = text.strip_prefix('v').or_else(|| text.strip_prefix('V')).unwrap_or(text);
    // A pre-release or build suffix is not part of the comparison.
    let text = text.split(['-', '+']).next()?;

    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    // A missing minor or patch is a zero, so `v2` compares as 2.0.0.
    let minor = parts.next().map(str::parse).transpose().ok()?.unwrap_or(0);
    let patch = parts.next().map(str::parse).transpose().ok()?.unwrap_or(0);
    Some((major, minor, patch))
}

/// Ask GitHub for the latest release.
///
/// `Ok(None)` means there is nothing newer, which is the usual answer and not
/// worth telling anybody about.
pub fn check() -> Result<Option<Release>, Error> {
    if disabled() {
        return Err(Error::Disabled);
    }
    let timer = log::Timer::start();
    let body = fetch(&format!("https://api.github.com/repos/{REPO}/releases/latest"))?;
    let release = latest(&body)?;

    let current = current();
    let newer = release.version > current;
    log::info(
        "update",
        format!(
            "{} is the latest, this is {}.{}.{}, {} in {} ms",
            release.tag,
            current.0,
            current.1,
            current.2,
            if newer { "newer available" } else { "up to date" },
            timer.ms()
        ),
    );
    Ok(newer.then_some(release))
}

/// Pull the tag and the page out of a GitHub release document.
fn latest(body: &str) -> Result<Release, Error> {
    let value = json::parse(body).map_err(|err| Error::Reply(err.to_string()))?;
    if let Some(message) = value.string("message") {
        // GitHub reports rate limiting and a missing repository this way.
        return Err(Error::Request(message.to_string()));
    }
    let tag = value
        .string("tag_name")
        .ok_or_else(|| Error::Reply("no tag in it".to_string()))?;
    let version = version(tag)
        .ok_or_else(|| Error::Reply(format!("{tag:?} is not a version")))?;
    let fallback = || format!("{RELEASES_PREFIX}{REPO}/releases/latest");
    let url = value
        .string("html_url")
        .filter(|url| url.starts_with(RELEASES_PREFIX))
        .map(str::to_string)
        .unwrap_or_else(fallback);

    Ok(Release { version, tag: tag.to_string(), url })
}

fn fetch(url: &str) -> Result<String, Error> {
    let output = Command::new(crate::curl::path())
        .args([
            "--silent",
            "--show-error",
            "--fail",
            // Redirects are followed, but only to another HTTPS address and
            // only a few times: `--location` on its own will happily follow a
            // reply down to plain http, or into a `file://`.
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-redirs",
            "5",
            "--max-time",
            &TIMEOUT.as_secs().to_string(),
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            // GitHub refuses a request with no user agent. This is the whole of
            // what the check says about whoever is running it.
            "User-Agent: openwrite",
            url,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|err| Error::NoCurl(err.to_string()))?;

    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let why = if why.is_empty() { format!("curl exited {}", output.status) } else { why };
        return Err(Error::Request(why));
    }
    let body = String::from_utf8_lossy(&output.stdout).into_owned();
    if body.trim().is_empty() {
        return Err(Error::Reply("an empty reply".to_string()));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_page_somewhere_other_than_github_is_not_opened() {
        // `html_url` is a string off the network that ends up at the system's
        // "open this" handler, so anywhere but GitHub falls back to the
        // canonical page rather than being followed.
        let body = r#"{"tag_name":"v9.0.0","html_url":"https://evil.example/pwn"}"#;
        let release = latest(body).unwrap();
        assert!(release.url.starts_with(RELEASES_PREFIX));
        assert!(release.url.contains(REPO));

        // A real GitHub page is kept as it is.
        let body = r#"{"tag_name":"v9.0.0","html_url":"https://github.com/mediaswing/openwrite/releases/tag/v9.0.0"}"#;
        assert!(latest(body).unwrap().url.ends_with("/v9.0.0"));
    }

    #[test]
    fn a_version_is_read_however_it_is_spelled() {
        assert_eq!(version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(version(" V1.2.3 "), Some((1, 2, 3)));
        // A missing part is a zero.
        assert_eq!(version("v2"), Some((2, 0, 0)));
        assert_eq!(version("v2.1"), Some((2, 1, 0)));
        // A suffix is not part of the comparison.
        assert_eq!(version("v1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(version("1.2.3+build9"), Some((1, 2, 3)));
    }

    #[test]
    fn something_that_is_not_a_version_is_not_read_as_one() {
        for text in ["", "latest", "v", "v.1.2", "vx.y.z", "1.2.x"] {
            assert_eq!(version(text), None, "{text:?}");
        }
    }

    #[test]
    fn versions_compare_the_way_versions_should() {
        assert!(version("v1.10.0") > version("v1.9.0"), "10 is after 9");
        assert!(version("v2.0.0") > version("v1.99.99"));
        assert!(version("v1.1.1") > version("v1.1.0"));
        assert_eq!(version("v1.1.0"), version("1.1.0"));
    }

    #[test]
    fn a_release_document_gives_up_its_tag_and_its_page() {
        let body = r#"{
            "tag_name": "v1.2.0",
            "html_url": "https://github.com/mediaswing/openwrite/releases/tag/v1.2.0",
            "name": "Screenplay Creation Tool v1.2.0"
        }"#;
        let release = latest(body).unwrap();
        assert_eq!(release.version, (1, 2, 0));
        assert_eq!(release.tag, "v1.2.0");
        assert!(release.url.ends_with("/v1.2.0"));
        assert_eq!(release.number(), "1.2.0");
    }

    #[test]
    fn a_release_with_no_page_still_points_somewhere_useful() {
        let release = latest(r#"{"tag_name":"v9.0.0"}"#).unwrap();
        assert!(release.url.contains(REPO));
    }

    #[test]
    fn githubs_own_complaints_are_reported_as_such() {
        let body = r#"{"message":"API rate limit exceeded","documentation_url":"https://..."}"#;
        assert!(matches!(latest(body), Err(Error::Request(_))));
    }

    #[test]
    fn a_reply_that_is_not_a_release_is_an_error_rather_than_a_version() {
        assert!(latest("not json").is_err());
        assert!(latest("{}").is_err());
        assert!(latest(r#"{"tag_name":"nightly"}"#).is_err());
    }

    #[test]
    fn this_build_knows_its_own_version() {
        // Whatever Cargo.toml says, it parses — a build that cannot read its
        // own version would compare against 0.0.0 and offer every release.
        assert_eq!(current(), version(env!("CARGO_PKG_VERSION")).unwrap());
        assert!(current() > (0, 0, 0));
    }
}
