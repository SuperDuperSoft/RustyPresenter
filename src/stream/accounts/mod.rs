//! Signing in to YouTube and Facebook (design `2026-11-09-livestreaming-design.md` §4 part 8, step 1.3e), instead
//! of pasting a stream key. Jake, November 16, 2026: the app is registered under the church's own Google and Meta
//! developer accounts; its ids come in when the app is built (`RP_GOOGLE_CLIENT_ID`, `RP_GOOGLE_CLIENT_SECRET`,
//! `RP_FACEBOOK_APP_ID`, `RP_FACEBOOK_CLIENT_TOKEN`). A copy built without them offers stream keys only.
//!
//! Signed in, Go Live makes the broadcast itself with the title and description from Settings, gets its stream
//! key, and, while live, how many are watching; after, the replay's link. What sign-in keeps (Google's refresh
//! token, the Facebook Page's token) is a password like a stream key: it is kept in the computer's password store
//! (`keys`) under `token_id`, never in the data.
use super::{Destination, Service, keys::KeyStore};
use std::time::Duration;
use uuid::Uuid;

pub mod facebook;
pub mod google;
pub mod twitch;

/// The app's own ids with Google and Meta, from the build. Empty: that service cannot be signed in to.
#[derive(Clone, Debug, Default)]
pub struct Ids {
    pub google_id: String,
    /// Google gives a desktop app a "secret" that is not one (anyone can read it out of the program); it is still
    /// kept out of the source.
    pub google_secret: String,
    pub facebook_app: String,
    pub facebook_token: String,
    /// Twitch's client id for the app (a public client: no secret).
    pub twitch_client: String,
}

impl Ids {
    /// This build's.
    pub fn built_in() -> Self {
        Ids {
            google_id: option_env!("RP_GOOGLE_CLIENT_ID").unwrap_or_default().trim().to_string(),
            google_secret: option_env!("RP_GOOGLE_CLIENT_SECRET").unwrap_or_default().trim().to_string(),
            facebook_app: option_env!("RP_FACEBOOK_APP_ID").unwrap_or_default().trim().to_string(),
            facebook_token: option_env!("RP_FACEBOOK_CLIENT_TOKEN").unwrap_or_default().trim().to_string(),
            twitch_client: option_env!("RP_TWITCH_CLIENT_ID").unwrap_or_default().trim().to_string(),
        }
    }

    /// Whether `service` can be signed in to with these ids.
    pub fn can_sign_in(&self, service: Service) -> bool {
        match service {
            Service::YouTube => !self.google_id.is_empty() && !self.google_secret.is_empty(),
            Service::Facebook => !self.facebook_app.is_empty() && !self.facebook_token.is_empty(),
            Service::Twitch => !self.twitch_client.is_empty(),
            _ => false,
        }
    }
}

/// Where the services are: the real ones, or a stand-in in a test.
#[derive(Clone, Debug)]
pub struct Ends {
    pub google_auth: String,
    pub google_token: String,
    pub youtube: String,
    /// YouTube's address for uploads (thumbnails).
    pub youtube_upload: String,
    pub graph: String,
    pub twitch_id: String,
    pub helix: String,
}

impl Ends {
    pub fn real() -> Self {
        Ends {
            google_auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            google_token: "https://oauth2.googleapis.com/token".into(),
            youtube: "https://www.googleapis.com/youtube/v3".into(),
            youtube_upload: "https://www.googleapis.com/upload/youtube/v3".into(),
            graph: format!("https://graph.facebook.com/{}", facebook::VERSION),
            twitch_id: "https://id.twitch.tv/oauth2".into(),
            helix: "https://api.twitch.tv/helix".into(),
        }
    }

    /// All four at one address (a test's stand-in server).
    pub fn at(base: &str) -> Self {
        Ends { google_auth: format!("{base}/auth"), google_token: format!("{base}/token"), youtube: format!("{base}/youtube/v3"), youtube_upload: format!("{base}/upload/youtube/v3"), graph: format!("{base}/graph"),
            twitch_id: format!("{base}/twitch-id"), helix: format!("{base}/helix") }
    }
}

/// Where a destination's sign-in is kept in the password store: beside its stream key, under an id of its own.
pub fn token_id(destination: Uuid) -> Uuid { Uuid::from_u128(destination.as_u128() ^ 0x5349_474e_2d49_4e00_0000_0000_0000_0000) }

/// Who can watch a YouTube broadcast.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Privacy { #[default] Public, Unlisted }

impl Privacy {
    pub fn youtube(self) -> &'static str { match self { Privacy::Public => "public", Privacy::Unlisted => "unlisted" } }
    pub fn is_public(&self) -> bool { *self == Privacy::Public }
}

/// What a broadcast is called: `template` (empty: `default_title`) with the day's date and the service's name.
pub fn title_for(template: &str, date: chrono::NaiveDate, service: Service) -> String {
    let t = if template.trim().is_empty() { default_title() } else { template.trim().to_string() };
    super::title(&t, date, service.name())
}

/// The title a new church's broadcasts get, in the app's language, with `{date}` in it.
pub fn default_title() -> String { crate::tr!("Sunday Service — {date}", date = "{date}") }

/// What sign-in kept for `destination`: the account's name, and what the service is asked with (a password).
pub fn kept(store: &dyn KeyStore, destination: Uuid) -> Option<(String, String)> {
    let v = store.get(token_id(destination))?;
    let (name, token) = v.split_once('\n')?;
    (!token.trim().is_empty()).then(|| (name.trim().to_string(), token.trim().to_string()))
}

/// Keeps a sign-in for `destination`, with the account's name beside it.
pub fn keep(store: &dyn KeyStore, destination: Uuid, name: &str, token: &str) -> Result<(), String> {
    store.set(token_id(destination), &format!("{}\n{}", name.replace(['\n', '\r'], " ").trim(), token.trim()))
}

/// Signs `destination` out on this computer.
pub fn forget(store: &dyn KeyStore, destination: Uuid) { store.remove(token_id(destination)) }

/// The app's ids; a test's own stand-in ones in a test that sets them (`testing::use_server`).
pub fn app_ids() -> Ids {
    #[cfg(test)]
    if let Some((ids, _)) = testing::OVERRIDE.with(|o| o.borrow().clone()) { return ids }
    Ids::built_in()
}

/// Where the services are; in a test, its stand-in.
pub fn app_ends() -> Ends {
    #[cfg(test)]
    if let Some((_, ends)) = testing::OVERRIDE.with(|o| o.borrow().clone()) { return ends }
    Ends::real()
}

/// What a broadcast is told: its title and description, and on YouTube who can watch.
#[derive(Clone, Debug, Default)]
pub struct Details {
    pub title: String,
    pub description: String,
    pub privacy: Privacy,
    /// YouTube: a picture file for the broadcast's thumbnail; empty: none (YouTube takes a frame).
    pub thumbnail: String,
    /// YouTube: the broadcast made ahead for this service (`google::schedule`), used if it is still to come.
    pub scheduled: String,
}

/// A broadcast made for Go Live.
#[derive(Clone, Debug)]
pub struct Live {
    pub service: Service,
    /// The service's id for the broadcast.
    pub id: String,
    /// The full address to send to, key and all (a password: never shown or logged).
    pub address: String,
    /// Where it can be watched, and later the replay.
    pub watch: String,
    /// What sign-in keeps from now on, when the service gave a new one (Twitch: a refresh token works once).
    pub renewed: Option<String>,
    /// The destination it was made for (where a renewed sign-in is kept).
    pub destination: Uuid,
    /// YouTube: the broadcast's live chat; empty: none.
    pub chat: String,
    /// What the service's other questions are asked with, shared by every copy, renewed before it runs out.
    auth: std::sync::Arc<std::sync::Mutex<Auth>>,
}

/// An access (a password) and until when it works, and what renews it (empty: it does not run out).
#[derive(Debug)]
struct Auth {
    access: String,
    until: std::time::Instant,
    refresh: String,
    /// Twitch: what sign-in keeps before the refresh token ("id|login|").
    kept_before: String,
}

/// An access is renewed this long before it runs out (OBS does the same: renewed on use, near its end).
pub const RENEW_BEFORE: Duration = Duration::from_secs(300);
/// What a Page's token (which does not run out) is said to last.
const FOREVER: Duration = Duration::from_secs(100 * 24 * 3600);

impl Live {
    /// A broadcast whose questions are asked with `access`, good for `lasts`, renewed with `refresh` (empty: never).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(service: Service, id: String, address: String, watch: String, access: String, lasts: Duration, refresh: String, kept_before: String) -> Live {
        let until = std::time::Instant::now() + lasts;
        Live { service, id, address, watch, renewed: None, destination: Uuid::nil(), chat: String::new(), auth: std::sync::Arc::new(std::sync::Mutex::new(Auth { access, until, refresh, kept_before })) }
    }

    /// The access to ask with now: renewed first when it runs out within `RENEW_BEFORE`. A Twitch renewal (its
    /// refresh token works once) is kept in `store` at once, so the next sign-in still works.
    async fn access(&self, client: &reqwest::Client, ends: &Ends, ids: &Ids, store: &dyn KeyStore) -> Result<String, String> {
        let (access, due, refresh) = {
            let a = self.auth.lock().unwrap_or_else(|e| e.into_inner());
            (a.access.clone(), std::time::Instant::now() + RENEW_BEFORE >= a.until, a.refresh.clone())
        };
        if !due || refresh.is_empty() { return Ok(access) }
        let (access, lasts, new_refresh) = match self.service {
            Service::YouTube => { let (a, l) = google::access(client, ends, ids, &refresh).await?; (a, l, refresh) }
            Service::Twitch => twitch::renew(client, ends, ids, &refresh).await?,
            _ => return Ok(access),
        };
        let kept_before = {
            let mut a = self.auth.lock().unwrap_or_else(|e| e.into_inner());
            (a.access, a.until, a.refresh) = (access.clone(), std::time::Instant::now() + lasts, new_refresh.clone());
            a.kept_before.clone()
        };
        if self.service == Service::Twitch && !self.destination.is_nil() {
            let name = kept(store, self.destination).map(|(n, _)| n).unwrap_or_default();
            if let Err(e) = keep(store, self.destination, &name, &format!("{kept_before}{new_refresh}")) { eprintln!("twitch: the renewed sign-in could not be kept: {e}") }
        }
        Ok(access)
    }

    /// The access to ask with now (renewed when needed, kept in the computer's password store).
    pub async fn access_now(&self, client: &reqwest::Client, ends: &Ends, ids: &Ids) -> Result<String, String> {
        self.access(client, ends, ids, super::keys::app_store().as_ref()).await
    }

    /// Twitch: the channel's login, for its chat.
    pub fn login(&self) -> Option<String> {
        if self.service != Service::Twitch { return None }
        self.auth.lock().unwrap_or_else(|e| e.into_inner()).kept_before.split('|').nth(1).map(str::to_string)
    }

    /// The access as it is, not renewed (a Page's, which does not run out).
    pub fn current(&self) -> String { self.auth.lock().unwrap_or_else(|e| e.into_inner()).access.clone() }

    /// Tests: as if `by` had passed.
    #[cfg(test)]
    pub fn age_by(&self, by: Duration) {
        let mut a = self.auth.lock().unwrap();
        a.until = a.until.checked_sub(by).unwrap_or_else(std::time::Instant::now);
    }
}

/// Makes a broadcast for `destination` (signed in with `saved`, what sign-in kept) with `details`.
pub async fn prepare(client: &reqwest::Client, ends: &Ends, ids: &Ids, destination: &Destination, saved: &str, details: &Details) -> Result<Live, String> {
    let made = match destination.service {
        Service::YouTube => google::prepare(client, ends, ids, saved, details).await,
        Service::Facebook => facebook::prepare(client, ends, saved, details).await,
        Service::Twitch => twitch::prepare(client, ends, ids, saved, details).await,
        _ => Err("this service has no sign-in".into()),
    };
    made.map(|l| Live { destination: destination.id, ..l })
}

/// How many are watching `live` now; `None` when the service does not say. The sign-in is renewed first when it
/// is about to run out (a service longer than YouTube's hour).
pub async fn viewers(client: &reqwest::Client, ends: &Ends, ids: &Ids, live: &Live) -> Option<u64> {
    viewers_kept(client, ends, ids, live, super::keys::app_store().as_ref()).await
}

/// `viewers`, a renewed sign-in kept in `store`.
pub async fn viewers_kept(client: &reqwest::Client, ends: &Ends, ids: &Ids, live: &Live, store: &dyn KeyStore) -> Option<u64> {
    let access = live.access(client, ends, ids, store).await.map_err(|e| eprintln!("stream: renewing the sign-in: {e}")).ok()?;
    match live.service {
        Service::YouTube => google::viewers(client, ends, live, &access).await,
        Service::Facebook => facebook::viewers(client, ends, live, &access).await,
        Service::Twitch => twitch::viewers(client, ends, ids, live, &access).await,
        _ => None,
    }
}

/// Ends `live` at the service (YouTube ends its own when the stream stops).
pub async fn finish(client: &reqwest::Client, ends: &Ends, live: &Live) {
    if live.service == Service::Facebook { facebook::finish(client, ends, live, &live.current()).await }
}

/// How long the operator has to finish signing in, in the browser.
pub const SIGN_IN_LIMIT: Duration = Duration::from_secs(300);

/// A service's error, in a few words for a log or a notice: its own message when it gives one.
async fn failed(r: reqwest::Response) -> String {
    let status = r.status();
    let body: serde_json::Value = r.json().await.unwrap_or_default();
    let said = body.pointer("/error/message").or_else(|| body.pointer("/error_description")).or_else(|| body.get("error")).and_then(|v| v.as_str()).unwrap_or("");
    if said.is_empty() { status.to_string() } else { format!("{status}: {said}") }
}

/// The answer as JSON, or what went wrong.
async fn json(r: Result<reqwest::Response, reqwest::Error>) -> Result<serde_json::Value, String> {
    let r = r.map_err(|e| e.to_string())?;
    if !r.status().is_success() { return Err(failed(r).await) }
    r.json().await.map_err(|e| e.to_string())
}

/// What the operator is told about signing in.
pub mod tr {
    use crate::tr;
    pub fn signed_in(name: &str) -> String { tr!("Signed in as {name}.", name = name) }
    pub fn open_browser() -> String { tr!("Finish signing in in the web browser that opened.") }
    pub fn twitch_code(code: &str, at: &str) -> String { tr!("Go to {at} on any phone or computer and type the code {code}.", at = at, code = code) }
    pub fn facebook_code(code: &str, at: &str) -> String { tr!("Go to {at} on any phone or computer and type the code {code}.", at = at, code = code) }
    pub fn sign_in_failed(why: &str) -> String { tr!("Signing in did not work: {why}", why = why) }
    pub fn no_page() -> String { tr!("This Facebook account manages no Page. A stream can only go to a Page.") }
    pub fn prepare_failed(name: &str, why: &str) -> String { tr!("{name}: the broadcast could not be made ({why}). Sign in again in Settings → Livestream → Where to stream.", name = name, why = why) }
    pub fn scheduling() -> String { tr!("Making the YouTube broadcast…") }
    pub fn scheduled(when: &str, link: &str) -> String { tr!("The YouTube broadcast is ready for {when}. Share its link: {link}", when = when, link = link) }
    pub fn schedule_failed(why: &str) -> String { tr!("The YouTube broadcast could not be made: {why}", why = why) }
    pub fn link_copied() -> String { tr!("The link is copied.") }
    pub fn replay(name: &str, link: &str) -> String { tr!("{name}: the replay is at {link}", name = name, link = link) }
}

/// Base64 without padding, URL-safe (`-` and `_`): what PKCE's challenge is written in.
pub(crate) fn base64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 2);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..=chunk.len() { out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char) }
    }
    out
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod testing {
    use super::{Ends, Ids};
    use std::cell::RefCell;
    thread_local! {
        pub(super) static OVERRIDE: RefCell<Option<(Ids, Ends)>> = const { RefCell::new(None) };
    }
    /// This thread's sign-ins go to the stand-in at `base`, with made-up ids.
    pub fn use_server(base: &str) {
        let ids = Ids { google_id: "app-id".into(), google_secret: "secret".into(), facebook_app: "fb-app".into(), facebook_token: "fb-token".into(), twitch_client: "tw-app".into() };
        OVERRIDE.with(|o| *o.borrow_mut() = Some((ids, Ends::at(base))));
    }
    pub use super::tests::{Seen, serve};
}
