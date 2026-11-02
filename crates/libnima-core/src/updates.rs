//! Telling the user a newer libNIMA is out.
//!
//! At most once a day the newest release on GitHub is asked for, in the background while the install runs, and kept
//! in the user's cache folder. When it is newer than the one running, the last line of the run says so, with where to
//! get it.
//!
//! It must never be in the way: no answer within a second or two, no internet, a proxy, a cache folder that cannot be
//! written, all mean saying nothing. It is off with --no-update-check, with LIBNIMA_NO_UPDATE_CHECK set, and on a build
//! server, where CI is set.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::manifest::version_tuple;

const LATEST_URL: &str = "https://api.github.com/repos/nimaltd/libNIMA/releases/latest";

/// How long an answer is good for, and how long to wait for a new one.
const DAY_SECONDS: f64 = 24.0 * 60.0 * 60.0;
const TIMEOUT: Duration = Duration::from_secs(2);

/// Turns the check off.
const OFF_VARIABLES: [&str; 2] = ["LIBNIMA_NO_UPDATE_CHECK", "CI"];

/// Where the last answer is kept: the user's cache folder.
fn cache_file() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    };

    Some(base?.join("libnima").join("latest.json"))
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |time| time.as_secs_f64())
}

/// The version kept from a check less than a day old, or None.
fn cached(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let data: Value = serde_json::from_str(&text).ok()?;

    if now() - data.get("checked")?.as_f64()? > DAY_SECONDS {
        return None;
    }

    data.get("latest")?.as_str().map(str::to_string)
}

/// A tag as a version: 1.2.0 from v1.2.0.
fn version_of_tag(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// The newest version on GitHub, or None.
fn ask() -> Option<String> {
    let agent = crate::download::agent(TIMEOUT);
    let mut response = agent
        .get(LATEST_URL)
        .header("Accept", "application/vnd.github+json")
        .call()
        .ok()?;

    if response.status().as_u16() != 200 {
        return None;
    }

    let data: Value = serde_json::from_slice(&response.body_mut().read_to_vec().ok()?).ok()?;

    data.get("tag_name")?
        .as_str()
        .map(|tag| version_of_tag(tag).to_string())
}

/// The newest version, from the cache or GitHub, or None. Never fails.
pub fn latest() -> Option<String> {
    latest_from(cache_file().as_deref(), ask)
}

/// The newest version, from the cache at `path` or by asking.
fn latest_from(path: Option<&Path>, ask: impl FnOnce() -> Option<String>) -> Option<String> {
    if let Some(found) = path.and_then(cached) {
        return Some(found);
    }

    let found = ask()?;

    if let Some(path) = path {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let _ = fs::write(
            path,
            format!("{}\n", json!({"checked": now(), "latest": found})),
        );
    }

    Some(found)
}

/// Whether a version is newer than the one running.
pub fn is_newer(found: &str, version: &str) -> bool {
    match (version_tuple(version), version_tuple(found)) {
        (Some(have), Some(newer)) => newer > have,
        _ => false,
    }
}

/// A check started in the background, asked for its answer at the end of the run.
pub struct Check {
    answer: Option<mpsc::Receiver<Option<String>>>,
}

impl Check {
    pub fn start(enabled: bool) -> Check {
        if !enabled || switched_off(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
        {
            return Check { answer: None };
        }

        let (send, answer) = mpsc::channel();

        thread::spawn(move || {
            let _ = send.send(latest());
        });

        Check {
            answer: Some(answer),
        }
    }

    /// The line to print when a newer version is out, or None.
    pub fn message(&self) -> Option<String> {
        let found = self.answer.as_ref()?.recv_timeout(TIMEOUT).ok()??;

        said_of(&found, crate::VERSION)
    }
}

fn switched_off(set: impl Fn(&str) -> bool) -> bool {
    OFF_VARIABLES.iter().any(|name| set(name))
}

/// What to say of the newest version when this one is `version`, or nothing.
fn said_of(found: &str, version: &str) -> Option<String> {
    is_newer(found, version).then(|| {
        format!(
            "libNIMA {found} is out, and this is {version}. Get it from: {}",
            crate::RELEASES_PAGE
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_version_is_told_from_an_older_one() {
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
        assert!(!is_newer("1.0.0-beta", "1.0.0"));
    }

    #[test]
    fn a_tag_is_read_as_a_version() {
        assert_eq!(version_of_tag("v1.2.0"), "1.2.0");
        assert_eq!(version_of_tag("1.2.0"), "1.2.0");
    }

    #[test]
    fn a_check_turned_off_says_nothing() {
        assert_eq!(Check::start(false).message(), None);
    }

    #[test]
    fn a_newer_version_is_said_with_where_to_get_it() {
        assert_eq!(
            said_of("2.1.0", "2.0.0").as_deref(),
            Some(
                "libNIMA 2.1.0 is out, and this is 2.0.0. Get it from: https://github.com/nimaltd/libNIMA/releases/latest"
            )
        );
    }

    #[test]
    fn nothing_newer_says_nothing() {
        for found in ["2.0.0", "1.9.9", "2.1.0rc1", "garbage"] {
            assert_eq!(said_of(found, "2.0.0"), None, "{found}");
        }
    }

    #[test]
    fn no_internet_says_nothing() {
        let root = tempfile::tempdir().unwrap();

        assert_eq!(
            latest_from(Some(&root.path().join("x/latest.json")), || None),
            None
        );
        assert_eq!(latest_from(None, || None), None);
    }

    #[test]
    fn github_is_asked_at_most_once_a_day() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("libnima/latest.json");
        let asked = std::cell::Cell::new(0);
        let ask = || {
            asked.set(asked.get() + 1);

            Some("2.1.0".to_string())
        };

        assert_eq!(latest_from(Some(&path), ask).as_deref(), Some("2.1.0"));
        assert_eq!(latest_from(Some(&path), ask).as_deref(), Some("2.1.0"));
        assert_eq!(asked.get(), 1);
    }

    #[test]
    fn an_answer_older_than_a_day_is_asked_again() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("latest.json");
        let old = json!({"checked": now() - 2.0 * DAY_SECONDS, "latest": "2.0.5"});

        fs::write(&path, old.to_string()).unwrap();

        assert_eq!(
            latest_from(Some(&path), || Some("2.2.0".to_string())).as_deref(),
            Some("2.2.0")
        );
    }

    #[test]
    fn a_cache_that_cannot_be_read_is_no_answer() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("latest.json");

        fs::write(&path, "{ not json").unwrap();

        assert_eq!(
            latest_from(Some(&path), || Some("2.2.0".to_string())).as_deref(),
            Some("2.2.0")
        );
    }

    #[test]
    fn it_is_off_on_a_build_server_or_when_asked() {
        assert!(switched_off(|name| name == "CI"));
        assert!(switched_off(|name| name == "LIBNIMA_NO_UPDATE_CHECK"));
        assert!(!switched_off(|_| false));
    }
}
