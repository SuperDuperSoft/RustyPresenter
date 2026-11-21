//! Is there a newer RustyPresenter? (1.4.6)
//!
//! Once a day, at start, the app asks GitHub which release of RustyPresenter is the newest and, when it is newer
//! than this one, says so and offers its download page. That is all it does:
//! - It asks one address, `<RELEASES>/latest`, and reads only where that points (`…/releases/tag/v1.5.0`). No page
//!   is read, nothing about the church or the computer is sent (GitHub sees what any web visit shows: the
//!   computer's address and "RustyPresenter/<version>").
//! - Nothing is downloaded or installed. Download opens the release's page in the person's own web browser; the
//!   page's address is built here from the version, never taken from the answer.
//! - No release yet, the project not public yet, no internet: silence. Only "Check now" (Settings → Help and
//!   about) says why it could not check.
//! - Settings → Help and about has the switch (`Settings::check_updates`).
//!
//! What it found is kept beside the data (`update-check.json`), so a start without internet still knows of a newer
//! version found earlier, and a version already announced is not announced at every start.
use crate::youtube::fetch::FetchError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The project's releases on GitHub.
pub const RELEASES: &str = "https://github.com/SuperDuperSoft/RustyPresenter/releases";
/// Asked at most this often by itself.
const EVERY_HOURS: i64 = 24;
const FILE: &str = "update-check.json";

/// What the last look found.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct State {
    /// When GitHub last answered.
    pub checked: Option<DateTime<Utc>>,
    /// The newest release's tag then (`v1.5.0`); `None` with no release out.
    pub latest: Option<String>,
    /// The newest version the person was told about at start (they are told once of each).
    pub told: Option<String>,
}

impl State {
    /// The newer version there is, without the tag's `v`: `None` when this one is the newest known.
    pub fn newer_than(&self, current: &str) -> Option<String> {
        let tag = self.latest.as_deref()?;
        newer(tag, current).then(|| shown(tag))
    }

    /// Time to ask again (never asked, a day ago, or a clock that went backwards).
    pub fn due(&self, now: DateTime<Utc>) -> bool {
        self.checked.is_none_or(|at| now < at || (now - at).num_hours() >= EVERY_HOURS)
    }

    /// The newer version to announce at start: one not announced yet.
    pub fn to_tell(&self, current: &str) -> Option<String> {
        let v = self.newer_than(current)?;
        (self.told.as_deref() != Some(v.as_str())).then_some(v)
    }
}

pub fn load(root: &Path) -> State {
    std::fs::read(root.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(root: &Path, state: &State) {
    let Ok(text) = serde_json::to_vec_pretty(state) else { return };
    if let Err(e) = std::fs::write(root.join(FILE), text) { eprintln!("update check not saved: {e}") }
}

/// A release tag's numbers: `v1.5.0` → `[1, 5, 0]`. Two to four numbers of at most five digits, with or without
/// the `v`; anything else (a test build's `v1.5.0-beta`, a made-up tag) is no version.
pub fn numbers(tag: &str) -> Option<Vec<u32>> {
    let t = tag.strip_prefix('v').unwrap_or(tag);
    let parts: Vec<&str> = t.split('.').collect();
    if !(2..=4).contains(&parts.len()) { return None }
    parts.iter().map(|p| (!p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit())).then(|| p.parse().ok()).flatten()).collect()
}

/// The tag as a version to show: no `v`.
pub fn shown(tag: &str) -> String { tag.strip_prefix('v').unwrap_or(tag).to_string() }

/// `tag` is a later version than `current` (`1.5` and `1.5.0` are the same).
pub fn newer(tag: &str, current: &str) -> bool {
    let (Some(mut a), Some(mut b)) = (numbers(tag), numbers(current)) else { return false };
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

/// The tag in the address GitHub's "latest" points at: `…/releases/tag/v1.5.0`. Only a tag that is a version.
pub fn tag_in(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let (_, tag) = path.trim_end_matches('/').rsplit_once("/releases/tag/")?;
    numbers(tag).map(|_| tag.to_string())
}

/// The page of a release, for the web browser.
pub fn page(base: &str, tag: &str) -> String { format!("{base}/tag/{tag}") }

/// The newest release's tag. `Ok(None)`: there is none to be seen (no release yet, or the project is not public).
pub async fn latest(base: &str) -> Result<Option<String>, FetchError> {
    let url = format!("{base}/latest");
    let resp = crate::youtube::fetch::first_hop_client()?.get(&url).send().await?;
    let status = resp.status();
    if status.as_u16() == 404 { return Ok(None) }
    if !status.is_redirection() {
        // GitHub answers with the releases page itself when there is no release.
        return if status.is_success() { Ok(None) } else { Err(FetchError::Http(status.as_u16(), url)) };
    }
    let to = resp.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).and_then(|to| resp.url().join(to).ok());
    Ok(to.and_then(|to| tag_in(to.as_str())))
}

/// Asks GitHub and keeps the answer. An error leaves what was known as it was.
pub async fn check(root: &Path, base: &str, now: DateTime<Utc>) -> Result<State, FetchError> {
    let found = latest(base).await?;
    let mut state = load(root);
    state.checked = Some(now);
    state.latest = found;
    save(root, &state);
    Ok(state)
}

/// The version was announced at start: not again.
pub fn mark_told(root: &Path, version: &str) {
    let mut state = load(root);
    state.told = Some(version.to_string());
    save(root, &state);
}

#[cfg(test)]
pub(crate) mod testing {
    thread_local! {
        /// Where this thread's tests look for releases; unset, they look nowhere (as without internet).
        pub(crate) static BASE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
        /// The pages Download opened on this thread.
        pub(crate) static OPENED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }
}

/// Where the releases are looked for: GitHub; in tests, the stand-in server (or nowhere).
pub fn base() -> Option<String> {
    #[cfg(test)]
    { testing::BASE.with(|b| b.borrow().clone()) }
    #[cfg(not(test))]
    { Some(RELEASES.to_string()) }
}

/// Opens the release's page in the computer's web browser. Tests open nothing and note it instead.
pub fn open_page(tag: &str) -> Result<(), String> {
    let url = page(RELEASES, tag);
    #[cfg(test)]
    { testing::OPENED.with(|o| o.borrow_mut().push(url)); Ok(()) }
    #[cfg(not(test))]
    { crate::usage::open::open_file(std::path::Path::new(&url)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::youtube::testing::FakeReleases;

    #[test]
    fn versions_are_compared_by_their_numbers() {
        assert!(newer("v1.5.0", "1.4.6"));
        assert!(newer("v1.10.0", "1.9.9"), "by number, not by letter");
        assert!(newer("1.4.7", "1.4.6"));
        assert!(newer("v2.0", "1.99.99"));
        assert!(!newer("v1.4.6", "1.4.6"));
        assert!(!newer("v1.5", "1.5.0"), "the same version");
        assert!(!newer("v1.4.5", "1.4.6"), "GitHub is behind: nothing to offer");
        for odd in ["v1.5.0-beta", "latest", "", "v", "1", "1..2", "v1.2.3.4.5", "1.x", "v999999.0", "1.5.0/../x"] {
            assert_eq!(numbers(odd), None, "{odd}");
            assert!(!newer(odd, "1.0.0"), "{odd}");
        }
        assert_eq!(numbers("v1.4.6"), Some(vec![1, 4, 6]));
        assert_eq!(shown("v1.5.0"), "1.5.0");
    }

    #[test]
    fn the_tag_is_read_from_where_latest_points() {
        assert_eq!(tag_in("https://github.com/SuperDuperSoft/RustyPresenter/releases/tag/v1.5.0").as_deref(), Some("v1.5.0"));
        assert_eq!(tag_in("https://github.com/x/y/releases/tag/1.5.0/?a=b").as_deref(), Some("1.5.0"));
        assert_eq!(tag_in("https://github.com/SuperDuperSoft/RustyPresenter/releases"), None, "no release yet");
        assert_eq!(tag_in("https://github.com/login?return_to=x"), None);
        assert_eq!(tag_in("https://github.com/x/y/releases/tag/nightly"), None);
        assert_eq!(page(RELEASES, "v1.5.0"), "https://github.com/SuperDuperSoft/RustyPresenter/releases/tag/v1.5.0");
    }

    #[test]
    fn once_a_day_and_each_version_announced_once() {
        let now = Utc::now();
        let mut s = State::default();
        assert!(s.due(now), "never asked");
        s.checked = Some(now - chrono::Duration::hours(23));
        assert!(!s.due(now));
        s.checked = Some(now - chrono::Duration::hours(24));
        assert!(s.due(now));
        s.checked = Some(now + chrono::Duration::hours(5));
        assert!(s.due(now), "the clock was set back");
        assert_eq!(s.newer_than("1.4.6"), None, "no release known");
        s.latest = Some("v1.5.0".into());
        assert_eq!(s.newer_than("1.4.6").as_deref(), Some("1.5.0"));
        assert_eq!(s.to_tell("1.4.6").as_deref(), Some("1.5.0"));
        s.told = Some("1.5.0".into());
        assert_eq!(s.to_tell("1.4.6"), None, "told already");
        assert_eq!(s.newer_than("1.4.6").as_deref(), Some("1.5.0"), "Help and about still says so");
        s.latest = Some("v1.6.0".into());
        assert_eq!(s.to_tell("1.4.6").as_deref(), Some("1.6.0"), "a newer one again");
        assert_eq!(s.to_tell("1.6.0"), None, "installed");
    }

    #[tokio::test]
    async fn github_is_asked_and_the_answer_kept() {
        let web = FakeReleases::start().await;
        let base = web.base("/SuperDuperSoft/RustyPresenter/releases");
        let dir = std::env::temp_dir().join(format!("rp-update-check-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = Utc::now();
        // The project is not public yet: nothing to be seen, and that is no error.
        assert_eq!(latest(&base).await.unwrap(), None);
        let s = check(&dir, &base, now).await.unwrap();
        assert_eq!((s.checked, s.latest.clone()), (Some(now), None));
        // Public, no release yet: GitHub shows the releases page.
        web.put("/SuperDuperSoft/RustyPresenter/releases/latest", b"<html>".to_vec());
        assert_eq!(latest(&base).await.unwrap(), None);
        // A release.
        web.point("/SuperDuperSoft/RustyPresenter/releases/latest", "/SuperDuperSoft/RustyPresenter/releases/tag/v1.5.0");
        assert_eq!(latest(&base).await.unwrap().as_deref(), Some("v1.5.0"));
        let s = check(&dir, &base, now).await.unwrap();
        assert_eq!(s.newer_than("1.4.6").as_deref(), Some("1.5.0"));
        assert_eq!(load(&dir), s, "kept on disk");
        mark_told(&dir, "1.5.0");
        assert_eq!(load(&dir).to_tell("1.4.6"), None);
        // Only the first hop is read: where it points is never fetched.
        let hits = web.hits();
        latest(&base).await.unwrap();
        assert_eq!(web.hits(), hits + 1);
        // A tag that is no version is no release to offer.
        web.point("/SuperDuperSoft/RustyPresenter/releases/latest", "/SuperDuperSoft/RustyPresenter/releases/tag/nightly");
        assert_eq!(latest(&base).await.unwrap(), None);
        // GitHub in trouble: an error, and what was known stays.
        web.fail_all(503);
        assert!(matches!(check(&dir, &base, now).await, Err(FetchError::Http(503, _))));
        assert_eq!(load(&dir).latest.as_deref(), Some("v1.5.0"));
        // No internet.
        assert!(matches!(latest("http://127.0.0.1:9/releases").await, Err(FetchError::Offline(_))));
        // A file that cannot be read is as no file.
        std::fs::write(dir.join(FILE), "{nonsense").unwrap();
        assert_eq!(load(&dir), State::default());
    }
}
