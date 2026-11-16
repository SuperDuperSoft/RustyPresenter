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

/// The app's own ids with Google and Meta, from the build. Empty: that service cannot be signed in to.
#[derive(Clone, Debug, Default)]
pub struct Ids {
    pub google_id: String,
    /// Google gives a desktop app a "secret" that is not one (anyone can read it out of the program); it is still
    /// kept out of the source.
    pub google_secret: String,
    pub facebook_app: String,
    pub facebook_token: String,
}

impl Ids {
    /// This build's.
    pub fn built_in() -> Self {
        Ids {
            google_id: option_env!("RP_GOOGLE_CLIENT_ID").unwrap_or_default().trim().to_string(),
            google_secret: option_env!("RP_GOOGLE_CLIENT_SECRET").unwrap_or_default().trim().to_string(),
            facebook_app: option_env!("RP_FACEBOOK_APP_ID").unwrap_or_default().trim().to_string(),
            facebook_token: option_env!("RP_FACEBOOK_CLIENT_TOKEN").unwrap_or_default().trim().to_string(),
        }
    }

    /// Whether `service` can be signed in to with these ids.
    pub fn can_sign_in(&self, service: Service) -> bool {
        match service {
            Service::YouTube => !self.google_id.is_empty() && !self.google_secret.is_empty(),
            Service::Facebook => !self.facebook_app.is_empty() && !self.facebook_token.is_empty(),
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
    pub graph: String,
}

impl Ends {
    pub fn real() -> Self {
        Ends {
            google_auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            google_token: "https://oauth2.googleapis.com/token".into(),
            youtube: "https://www.googleapis.com/youtube/v3".into(),
            graph: format!("https://graph.facebook.com/{}", facebook::VERSION),
        }
    }

    /// All four at one address (a test's stand-in server).
    pub fn at(base: &str) -> Self {
        Ends { google_auth: format!("{base}/auth"), google_token: format!("{base}/token"), youtube: format!("{base}/youtube/v3"), graph: format!("{base}/graph") }
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
pub struct Details { pub title: String, pub description: String, pub privacy: Privacy }

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
    /// What the service's other questions are asked with (a password).
    token: String,
}

/// Makes a broadcast for `destination` (signed in with `saved`, what sign-in kept) with `details`.
pub async fn prepare(client: &reqwest::Client, ends: &Ends, ids: &Ids, destination: &Destination, saved: &str, details: &Details) -> Result<Live, String> {
    match destination.service {
        Service::YouTube => google::prepare(client, ends, ids, saved, details).await,
        Service::Facebook => facebook::prepare(client, ends, saved, details).await,
        _ => Err("this service has no sign-in".into()),
    }
}

/// How many are watching `live` now; `None` when the service does not say.
pub async fn viewers(client: &reqwest::Client, ends: &Ends, live: &Live) -> Option<u64> {
    match live.service {
        Service::YouTube => google::viewers(client, ends, live).await,
        Service::Facebook => facebook::viewers(client, ends, live).await,
        _ => None,
    }
}

/// Ends `live` at the service (YouTube ends its own when the stream stops).
pub async fn finish(client: &reqwest::Client, ends: &Ends, live: &Live) {
    if live.service == Service::Facebook { facebook::finish(client, ends, live).await }
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
    pub fn facebook_code(code: &str, at: &str) -> String { tr!("Go to {at} on any phone or computer and type the code {code}.", at = at, code = code) }
    pub fn sign_in_failed(why: &str) -> String { tr!("Signing in did not work: {why}", why = why) }
    pub fn no_page() -> String { tr!("This Facebook account manages no Page. A stream can only go to a Page.") }
    pub fn prepare_failed(name: &str, why: &str) -> String { tr!("{name}: the broadcast could not be made ({why}). Sign in again in Settings → Media → Livestream.", name = name, why = why) }
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
        let ids = Ids { google_id: "app-id".into(), google_secret: "secret".into(), facebook_app: "fb-app".into(), facebook_token: "fb-token".into() };
        OVERRIDE.with(|o| *o.borrow_mut() = Some((ids, Ends::at(base))));
    }
    pub use super::tests::{Seen, serve};
}
