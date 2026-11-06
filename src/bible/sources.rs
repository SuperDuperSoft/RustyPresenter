//! Copyrighted translations from licensed online sources (spec §8.5, F5).
//! Each church registers its own free key with the provider; nothing is bundled or published.
//! Keys live in `secrets.json` beside `data.json`: never in backups, the app state or the repo.
//! Fetched passages go into a small cache per provider and translation whose size and age follow each provider's terms.
use super::{Part, Reference, api_bible, canonical, display_name, esv, kjv, nlt, parse_reference, youversion};
use crate::model::VerseText;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io, path::Path, sync::{Mutex, OnceLock}, time::Duration};

/// (chapter, verse, text)
pub type Verse = (u16, u16, String);

/// A fetched passage and the API.Bible FUMS tokens to report each time it is shown (empty for other providers).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fetched { pub verses: Vec<Verse>, pub fums: Vec<String> }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider { Esv, Nlt, ApiBible, YouVersion }

pub const PROVIDERS: &[Provider] = &[Provider::Esv, Provider::Nlt, Provider::ApiBible, Provider::YouVersion];

pub struct Source {
    pub translation: &'static str,
    pub provider: Provider,
    /// Full notice for the credits page; slides show the short "(ESV)" label after each reference.
    pub copyright: &'static str,
}

const ESV: &str = "Scripture quotations are from the ESV® Bible (The Holy Bible, English Standard Version®), © 2001 by Crossway, a publishing ministry of Good News Publishers. Used by permission. All rights reserved. www.esv.org";
const NLT: &str = "Scripture quotations marked (NLT) are taken from the Holy Bible, New Living Translation, copyright © 1996, 2004, 2015 by Tyndale House Foundation. Used by permission of Tyndale House Publishers, Carol Stream, Illinois 60188. All rights reserved.";
const CSB: &str = "Scripture quotations marked CSB have been taken from the Christian Standard Bible®, Copyright © 2017 by Holman Bible Publishers. Used by permission. Christian Standard Bible® and CSB® are federally registered trademarks of Holman Bible Publishers.";
const HCSB: &str = "Scripture quotations marked HCSB are taken from the Holman Christian Standard Bible®, Copyright © 1999, 2000, 2002, 2003, 2009 by Holman Bible Publishers. Used by permission. Holman Christian Standard Bible®, Holman CSB®, and HCSB® are federally registered trademarks of Holman Bible Publishers.";
const NIV: &str = "Scripture quotations taken from The Holy Bible, New International Version® NIV®. Copyright © 1973, 1978, 1984, 2011 by Biblica, Inc.™ Used by permission. All rights reserved worldwide.";

/// Where each copyrighted translation can come from, best first. The first source whose key is saved is used.
pub const SOURCES: &[Source] = &[
    Source { translation: "ESV", provider: Provider::Esv, copyright: ESV },
    Source { translation: "NLT", provider: Provider::Nlt, copyright: NLT },
    Source { translation: "NLT", provider: Provider::ApiBible, copyright: NLT },
    Source { translation: "CSB", provider: Provider::ApiBible, copyright: CSB },
    Source { translation: "CSB", provider: Provider::YouVersion, copyright: CSB },
    Source { translation: "NIV", provider: Provider::YouVersion, copyright: NIV },
    Source { translation: "NIV", provider: Provider::ApiBible, copyright: NIV },
    Source { translation: "HCSB", provider: Provider::YouVersion, copyright: HCSB },
];

/// How much fetched text may be kept on disk.
#[derive(Clone, Copy, Debug)]
pub struct CachePolicy {
    pub max_verses: usize,
    /// Never hold more than half of any one book.
    pub half_book: bool,
    /// Entries older than this are fetched again.
    pub max_age_days: Option<u64>,
}

impl Provider {
    /// Name of the key in `secrets.json`.
    pub fn key_name(self) -> &'static str {
        match self { Provider::Esv => "esv", Provider::Nlt => "nlt", Provider::ApiBible => "api_bible", Provider::YouVersion => "youversion" }
    }

    pub fn from_key_name(name: &str) -> Option<Self> { PROVIDERS.iter().copied().find(|p| p.key_name() == name) }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Esv => "ESV API (Crossway)",
            Provider::Nlt => "NLT API (Tyndale)",
            Provider::ApiBible => "API.Bible (American Bible Society)",
            Provider::YouVersion => "YouVersion Platform",
        }
    }

    pub fn signup_url(self) -> &'static str {
        match self {
            Provider::Esv => "https://api.esv.org/account/create-application/",
            Provider::Nlt => "https://api.nlt.to/Account/Register",
            Provider::ApiBible => "https://api.bible/sign-up",
            Provider::YouVersion => "https://platform.youversion.com",
        }
    }

    /// What the church must do after signing up, beyond pasting the key.
    pub fn setup_note(self) -> &'static str {
        match self {
            Provider::Esv => "Free for churches. Create an “API application”, then copy its key.",
            Provider::Nlt => "Free for non-commercial use. Register, then copy the key from your account.",
            Provider::ApiBible => "The free Starter plan allows 3 copyrighted Bibles: under Plan, pick CSB (and NIV or NLT if you want them from here).",
            Provider::YouVersion => "Create an app, then accept each Bible’s license in the portal (NIV from Biblica; HCSB if it is offered).",
        }
    }

    /// Storage limits from each provider's terms (see each module).
    pub fn cache_policy(self) -> CachePolicy {
        match self {
            // ESV API: no more than 500 verses or half of any book stored locally.
            Provider::Esv => CachePolicy { max_verses: 500, half_book: true, max_age_days: None },
            // Tyndale fair use: 500 verses, never a whole book (half is the safe side).
            Provider::Nlt => CachePolicy { max_verses: 500, half_book: true, max_age_days: None },
            // API.Bible terms §11: stored text refreshed at least every 30 days.
            Provider::ApiBible => CachePolicy { max_verses: 500, half_book: true, max_age_days: Some(30) },
            // Platform terms are only shown at sign-in; follow the strictest publisher rule (Biblica: 500 verses).
            Provider::YouVersion => CachePolicy { max_verses: 500, half_book: true, max_age_days: Some(30) },
        }
    }

    /// A passage every account can read, used to check a key when it is saved.
    fn probe_translation(self) -> &'static str {
        match self { Provider::Esv => "ESV", Provider::Nlt => "NLT", Provider::ApiBible => "KJV", Provider::YouVersion => "BSB" }
    }

    async fn fetch(self, key: &str, translation: &str, book: u8, part: &Part) -> Result<Fetched, FetchError> {
        let client = client()?;
        let plain = |r: Result<Vec<Verse>, FetchError>| r.map(|verses| Fetched { verses, fums: Vec::new() });
        match self {
            Provider::Esv => plain(esv::fetch(client, key, book, part).await),
            Provider::Nlt => plain(nlt::fetch(client, key, book, part).await),
            Provider::ApiBible => api_bible::fetch(client, key, translation, book, part).await,
            Provider::YouVersion => plain(youversion::fetch(client, key, translation, book, part).await),
        }
    }

    /// API.Bible: report the passage to FUMS each time it is shown (fire and forget).
    fn report(self, root: &Path, fums: &[String]) {
        if self != Provider::ApiBible || fums.is_empty() { return; }
        api_bible::init_device_id(root);
        if let Ok(c) = client() { api_bible::report_fums(c, fums); }
    }
}

/// Every source for a translation, best first.
pub fn sources(translation: &str) -> impl Iterator<Item = &'static Source> { SOURCES.iter().filter(move |s| s.translation == translation) }

/// The source that will be used: the first with a saved key, else the first listed.
pub fn source_with(translation: &str, secrets: &Secrets) -> Option<&'static Source> {
    sources(translation).find(|s| secrets.key(s.provider).is_some()).or_else(|| sources(translation).next())
}

#[derive(Clone, Debug, PartialEq)]
pub enum FetchError {
    /// No answer from the provider (offline, DNS, timeout).
    Network,
    /// The provider refused the key.
    BadKey,
    RateLimited,
    /// The provider answered but had no text for this passage.
    NotFound,
    Other(String),
}

impl FetchError {
    pub fn from_status(status: u16) -> Self {
        match status { 401 | 403 => Self::BadKey, 429 => Self::RateLimited, 404 => Self::NotFound, s => Self::Other(format!("the service answered {s}")) }
    }

    fn message(&self, translation: &str, provider: Provider) -> String {
        match self {
            Self::Network => format!("Couldn't reach {} for {translation}. Check the internet connection.", provider.label()),
            Self::BadKey => format!("{} refused the key. Check it in Settings → Bible.", provider.label()),
            Self::RateLimited => format!("{} says too many lookups. Wait a minute and try again.", provider.label()),
            Self::NotFound => "No verses found.".into(),
            Self::Other(e) => format!("{translation} lookup failed: {e}."),
        }
    }
}

impl From<reqwest::Error> for FetchError {
    fn from(e: reqwest::Error) -> Self {
        if let Some(s) = e.status() { Self::from_status(s.as_u16()) }
        else if e.is_connect() || e.is_timeout() || e.is_request() { Self::Network }
        else { Self::Other(e.to_string()) }
    }
}

/// One shared HTTPS client. rustls with the ring backend (builds with the MinGW toolchain), OS certificate store.
pub fn client() -> Result<&'static reqwest::Client, FetchError> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("RustyPresenter/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())
    }).as_ref().map_err(|e| FetchError::Other(e.clone()))
}

// ---------- keys ----------

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
#[serde(default)]
pub struct Secrets { pub keys: BTreeMap<String, String> }

impl Secrets {
    pub fn load(root: &Path) -> Self {
        std::fs::read(root.join("secrets.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> io::Result<()> {
        crate::store::write_atomic(&root.join("secrets.json"), &serde_json::to_vec_pretty(self).map_err(io::Error::other)?)
    }

    pub fn key(&self, provider: Provider) -> Option<&str> {
        self.keys.get(provider.key_name()).map(|k| k.trim()).filter(|k| !k.is_empty())
    }
}

/// Saves (or, when empty, removes) a provider's key after checking it with a real lookup.
pub async fn set_key(root: &Path, provider: Provider, key: &str) -> Result<(), String> {
    let key = key.trim();
    if !key.is_empty() {
        let probe = parse_reference("John 3:16")?;
        let t = provider.probe_translation();
        let f = provider.fetch(key, t, probe.book, &probe.parts[0]).await.map_err(|e| e.message(t, provider))?;
        provider.report(root, &f.fums);
    }
    save_key(root, provider, key).map_err(|e| e.to_string())
}

/// Stores the key. Removing or changing it also deletes the text cached under the old one (API.Bible terms §10).
fn save_key(root: &Path, provider: Provider, key: &str) -> io::Result<()> {
    let _guard = FILES.lock().unwrap();
    let mut s = Secrets::load(root);
    let old = s.key(provider).map(str::to_string);
    if key.is_empty() { s.keys.remove(provider.key_name()); } else { s.keys.insert(provider.key_name().into(), key.into()); }
    s.save(root)?;
    if key.is_empty() || old.as_deref() != Some(key) {
        clear_cache(root, provider);
        // What the key may read changed too: its Bible list is fetched again.
        super::catalog::forget(root, provider);
    }
    Ok(())
}

/// Deletes every cache file of one provider, catalog Bibles (`yv:`, `ab:`) included. Call with `FILES` held.
fn clear_cache(root: &Path, provider: Provider) {
    let prefix = format!("{}-", provider.key_name());
    let Ok(rd) = std::fs::read_dir(root.join("bible-cache")) else { return };
    for e in rd.flatten() {
        if e.file_name().to_str().is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".json")) { let _ = std::fs::remove_file(e.path()); }
    }
}

/// A translation id as part of a file name (design §5.1): `:` → `~`, other characters outside `[A-Za-z0-9._-]` →
/// `%XX`, so `yv:111` is `yv~111` and today's `CSB` stays `CSB`.
pub fn file_part(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for b in id.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => out.push(b as char),
            b':' => out.push('~'),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------- cache ----------

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct Cache { entries: Vec<Entry> }

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Entry {
    book: u8,
    part: (u16, Option<u16>, u16, Option<u16>),
    fetched: u64,
    verses: Vec<Verse>,
    /// API.Bible FUMS tokens, reported again on every cache hit.
    #[serde(default)]
    fums: Vec<String>,
}

fn part_key(p: &Part) -> (u16, Option<u16>, u16, Option<u16>) { (p.start.0, p.start.1, p.end.0, p.end.1) }

/// Serialises cache and secrets file access between concurrent lookups.
static FILES: Mutex<()> = Mutex::new(());

fn fresh(e: &Entry, now: u64, policy: CachePolicy) -> bool { policy.max_age_days.is_none_or(|d| now.saturating_sub(e.fetched) < d * 86_400) }

impl Cache {
    /// `bible-cache/api_bible-CSB.json`: one file per provider, so text is only ever served under its own provider's terms.
    fn path(root: &Path, provider: Provider, translation: &str) -> std::path::PathBuf {
        root.join("bible-cache").join(format!("{}-{}.json", provider.key_name(), file_part(translation)))
    }

    /// Also deletes the old provider-less `bible-cache/{translation}.json`, whose source is unknown.
    pub fn load(root: &Path, provider: Provider, translation: &str) -> Self {
        let _ = std::fs::remove_file(root.join("bible-cache").join(format!("{}.json", file_part(translation))));
        std::fs::read(Self::path(root, provider, translation)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, root: &Path, provider: Provider, translation: &str) -> io::Result<()> {
        let p = Self::path(root, provider, translation);
        std::fs::create_dir_all(p.parent().unwrap())?;
        crate::store::write_atomic(&p, &serde_json::to_vec(self).map_err(io::Error::other)?)
    }

    pub fn verse_count(&self) -> usize { self.entries.iter().map(|e| e.verses.len()).sum() }

    /// Drops entries older than the policy allows; true if any went.
    fn prune(&mut self, now: u64, policy: CachePolicy) -> bool {
        let n = self.entries.len();
        self.entries.retain(|e| fresh(e, now, policy));
        self.entries.len() != n
    }

    fn get(&self, book: u8, part: &Part, now: u64, policy: CachePolicy) -> Option<&Entry> {
        self.entries.iter().find(|e| e.book == book && e.part == part_key(part) && fresh(e, now, policy))
    }

    /// Adds a passage, then drops the oldest entries until the policy holds. A passage that alone breaks it is not kept.
    fn put(&mut self, book: u8, part: &Part, now: u64, f: Fetched, policy: CachePolicy, book_len: usize) {
        self.entries.retain(|e| !(e.book == book && e.part == part_key(part)));
        self.prune(now, policy);
        let half = book_len / 2;
        let Fetched { verses, fums } = f;
        if verses.len() > policy.max_verses || (policy.half_book && verses.len() > half) { return; }
        self.entries.push(Entry { book, part: part_key(part), fetched: now, verses, fums });
        while self.verse_count() > policy.max_verses { self.entries.remove(0); }
        if policy.half_book {
            while self.entries.iter().filter(|e| e.book == book).map(|e| e.verses.len()).sum::<usize>() > half {
                let i = self.entries.iter().position(|e| e.book == book).unwrap();
                self.entries.remove(i);
            }
        }
    }
}

// ---------- lookup ----------

/// Where an online translation's text comes from: today's fixed routes (`SOURCES`), or a Bible added from a
/// catalog (`yv:111`, `ab:…`, design §3.3.1).
struct Route { provider: Provider, label: String }

fn route(root: &Path, translation: &str, secrets: &Secrets) -> Result<Route, String> {
    if super::catalog::provider_of(translation).is_some() {
        let b = super::catalog::find_added(root, translation).ok_or_else(|| super::strings::REMOVED.to_string())?;
        let provider = super::catalog::provider_of(&b.id).unwrap_or(Provider::YouVersion);
        return Ok(Route { provider, label: b.abbr });
    }
    let src = source_with(translation, secrets)
        .ok_or_else(|| format!("{translation} has no online source. Paste its verses with Add → Bible, or import a licensed file in Settings → Bible."))?;
    Ok(Route { provider: src.provider, label: translation.to_string() })
}

/// Looks up a passage in a copyrighted translation: cache first, then the provider.
pub async fn lookup(root: &Path, translation: &str, input: &str) -> Result<(String, Vec<VerseText>), String> {
    if super::blocklist::blocked_id(translation) { return Err(super::strings::BLOCKED.into()); }
    let secrets = Secrets::load(root);
    let Route { provider, label } = route(root, translation, &secrets)?;
    let r: Reference = parse_reference(input)?;
    let key = secrets.key(provider).map(str::to_string).ok_or_else(|| if label == translation {
        format!("{translation} needs a free key from {}. Add it in Settings → Bible, or paste the verses with Add → Bible.", provider.label())
    } else { super::strings::needs_key(&label, provider.label()) })?;
    let policy = provider.cache_policy();
    let display = display_name(r.book);
    let now = crate::app::now_secs();
    let mut out = Vec::new();
    for part in &r.parts {
        let cached = {
            let _g = FILES.lock().unwrap();
            let mut cache = Cache::load(root, provider, translation);
            if cache.prune(now, policy) { let _ = cache.save(root, provider, translation); }
            cache.get(r.book, part, now, policy).map(|e| Fetched { verses: e.verses.clone(), fums: e.fums.clone() })
        };
        let f = match cached {
            Some(f) => f,
            None => {
                let f = match provider.fetch(&key, translation, r.book, part).await {
                    Ok(f) => f,
                    Err(e) => {
                        // A refused key: what it fetched may no longer be kept (API.Bible terms §10).
                        if e == FetchError::BadKey { let _g = FILES.lock().unwrap(); clear_cache(root, provider); }
                        return Err(e.message(&label, provider));
                    }
                };
                if f.verses.is_empty() { return Err("No verses found.".into()); }
                // A blocked translation under another name: its text gives it away (design §4).
                if f.verses.iter().any(|(c, v, t)| super::blocklist::blocked_verse(r.book, *c, *v, t)) { return Err(super::strings::BLOCKED.into()); }
                remember(root, provider, translation, &key, now, (r.book, part), &f);
                f
            }
        };
        provider.report(root, &f.fums);
        out.extend(f.verses.into_iter().map(|(c, v, text)| VerseText { reference: format!("{display} {c}:{v}"), text }));
        if out.len() > 400 { return Err("That passage is too long. Choose up to about 400 verses.".into()); }
    }
    Ok((canonical(&r, display), out))
}

/// Caches a fetched passage, unless the key it was fetched with was removed or changed meanwhile.
fn remember(root: &Path, provider: Provider, translation: &str, key: &str, now: u64, (book, part): (u8, &Part), f: &Fetched) {
    let _g = FILES.lock().unwrap();
    if Secrets::load(root).key(provider) != Some(key) { return; }
    let mut cache = Cache::load(root, provider, translation);
    cache.put(book, part, now, f.clone(), provider.cache_policy(), kjv().book_len(book));
    let _ = cache.save(root, provider, translation);
}

/// "John 3:16-18", "Psalm 23", "John 1:51-2:2" — the plain form every provider's search accepts.
pub fn human_query(book: u8, part: &Part) -> String {
    let name = super::BOOKS[book as usize].1;
    match (part.start, part.end) {
        ((c, None), (e, None)) if c == e => format!("{name} {c}"),
        ((c, None), (e, None)) => format!("{name} {c}-{e}"),
        ((c, Some(v)), (e, Some(w))) if (c, v) == (e, w) => format!("{name} {c}:{v}"),
        ((c, Some(v)), (e, Some(w))) if c == e => format!("{name} {c}:{v}-{w}"),
        ((c, v), (e, w)) => format!("{name} {c}:{}-{e}:{}", v.unwrap_or(1), w.map_or("999".into(), |w| w.to_string())),
    }
}

/// Collapses whitespace runs the providers leave between words and lines.
pub fn tidy(text: &str) -> String { text.split_whitespace().collect::<Vec<_>>().join(" ") }

#[cfg(test)]
mod tests {
    use super::*;

    fn part(s: &str) -> (u8, Part) { let r = parse_reference(s).unwrap(); (r.book, r.parts[0].clone()) }
    fn verses(c: u16, from: u16, n: u16) -> Fetched { Fetched { verses: (from..from + n).map(|v| (c, v, format!("v{v}"))).collect(), fums: vec![] } }
    const ESV: CachePolicy = CachePolicy { max_verses: 500, half_book: true, max_age_days: None };

    #[test]
    fn human_queries() {
        let q = |s| { let (b, p) = part(s); human_query(b, &p) };
        assert_eq!(q("jn 3:16-18"), "John 3:16-18");
        assert_eq!(q("Ps 23"), "Psalms 23");
        assert_eq!(q("Ps 23-24"), "Psalms 23-24");
        assert_eq!(q("John 3:16"), "John 3:16");
        assert_eq!(q("John 1:51-2:2"), "John 1:51-2:2");
    }

    #[test]
    fn cache_returns_what_was_put() {
        let mut c = Cache::default();
        let (b, p) = part("John 3:16-18");
        c.put(b, &p, 100, verses(3, 16, 3), ESV, 879);
        assert_eq!(c.get(b, &p, 100, ESV).unwrap().verses.len(), 3);
        let (b2, p2) = part("John 3:16-17");
        assert!(c.get(b2, &p2, 100, ESV).is_none());
    }

    #[test]
    fn cache_never_holds_more_than_500_verses() {
        let mut c = Cache::default();
        for ch in 1..=30u16 { let (b, p) = part(&format!("Isaiah {ch}")); c.put(b, &p, ch as u64, verses(ch, 1, 30), CachePolicy { half_book: false, ..ESV }, 1292); }
        assert!(c.verse_count() <= 500);
        let (b, p) = part("Isaiah 30");
        assert!(c.get(b, &p, 0, ESV).is_some(), "newest kept");
        let (b, p) = part("Isaiah 1");
        assert!(c.get(b, &p, 0, ESV).is_none(), "oldest dropped");
    }

    #[test]
    fn cache_never_holds_more_than_half_a_book() {
        let mut c = Cache::default();
        let (b, p1) = part("Jude 1:1-8");
        c.put(b, &p1, 1, verses(1, 1, 8), ESV, 25);
        let (_, p2) = part("Jude 1:9-14");
        c.put(b, &p2, 2, verses(1, 9, 6), ESV, 25);
        assert!(c.get(b, &p1, 2, ESV).is_none(), "8 + 6 > 12: oldest dropped");
        assert!(c.get(b, &p2, 2, ESV).is_some());
        let (_, all) = part("Jude 1");
        c.put(b, &all, 3, verses(1, 1, 25), ESV, 25);
        assert!(c.get(b, &all, 3, ESV).is_none(), "a whole book is never stored");
    }

    #[test]
    fn cache_expires_old_entries() {
        let policy = CachePolicy { max_age_days: Some(30), ..ESV };
        let mut c = Cache::default();
        let (b, p) = part("John 3:16");
        c.put(b, &p, 0, verses(3, 16, 1), policy, 879);
        assert!(c.get(b, &p, 29 * 86_400, policy).is_some());
        assert!(c.get(b, &p, 31 * 86_400, policy).is_none());
        assert!(!c.prune(29 * 86_400, policy));
        assert!(c.prune(31 * 86_400, policy), "old text is dropped, not just hidden");
        assert_eq!(c.verse_count(), 0);
        assert!(!Cache::default().prune(0, ESV));
    }

    #[test]
    fn cache_files_are_per_provider() {
        let root = crate::store::test_dir();
        let (b, p) = part("John 3:16");
        let mut c = Cache::default();
        c.put(b, &p, 1, Fetched { verses: vec![(3, 16, "x".into())], fums: vec!["tok".into()] }, ESV, 879);
        c.save(&root, Provider::ApiBible, "NLT").unwrap();
        assert!(root.join("bible-cache/api_bible-NLT.json").exists());
        assert!(Cache::load(&root, Provider::Nlt, "NLT").get(b, &p, 1, ESV).is_none(), "API.Bible text never served as Tyndale's");
        let e = Cache::load(&root, Provider::ApiBible, "NLT");
        assert_eq!(e.get(b, &p, 1, ESV).unwrap().fums, vec!["tok".to_string()], "FUMS tokens kept for cache hits");
        // Entries written before tokens were kept still load.
        let old: Cache = serde_json::from_str(r#"{"entries":[{"book":42,"part":[3,16,3,16],"fetched":1,"verses":[[3,16,"x"]]}]}"#).unwrap();
        assert!(old.entries[0].fums.is_empty());
    }

    #[test]
    fn legacy_cache_files_are_deleted() {
        let root = crate::store::test_dir();
        std::fs::create_dir_all(root.join("bible-cache")).unwrap();
        std::fs::write(root.join("bible-cache/CSB.json"), r#"{"entries":[]}"#).unwrap();
        let _ = Cache::load(&root, Provider::ApiBible, "CSB");
        assert!(!root.join("bible-cache/CSB.json").exists());
    }

    fn cache_file(root: &Path, provider: Provider, translation: &str) -> bool {
        let (b, p) = part("John 3:16");
        let mut c = Cache::default();
        c.put(b, &p, 1, verses(3, 16, 1), ESV, 879);
        c.save(root, provider, translation).unwrap();
        Cache::path(root, provider, translation).exists()
    }

    #[test]
    fn changing_or_removing_a_key_deletes_its_cache() {
        let root = crate::store::test_dir();
        save_key(&root, Provider::ApiBible, "a").unwrap();
        assert!(cache_file(&root, Provider::ApiBible, "CSB"));
        assert!(cache_file(&root, Provider::Esv, "ESV"));
        save_key(&root, Provider::ApiBible, "a").unwrap();
        assert!(Cache::path(&root, Provider::ApiBible, "CSB").exists(), "same key: kept");
        save_key(&root, Provider::ApiBible, "b").unwrap();
        assert!(!Cache::path(&root, Provider::ApiBible, "CSB").exists(), "new key: deleted");
        assert!(Cache::path(&root, Provider::Esv, "ESV").exists(), "other providers untouched");
        assert!(cache_file(&root, Provider::ApiBible, "NIV"));
        save_key(&root, Provider::ApiBible, "").unwrap();
        assert!(!Cache::path(&root, Provider::ApiBible, "NIV").exists(), "removed key: deleted");
        assert_eq!(Secrets::load(&root).key(Provider::ApiBible), None);
    }

    #[test]
    fn a_bad_key_clears_only_its_provider() {
        let root = crate::store::test_dir();
        assert!(cache_file(&root, Provider::ApiBible, "CSB"));
        assert!(cache_file(&root, Provider::YouVersion, "CSB"));
        clear_cache(&root, Provider::ApiBible);
        assert!(!Cache::path(&root, Provider::ApiBible, "CSB").exists());
        assert!(Cache::path(&root, Provider::YouVersion, "CSB").exists());
    }

    #[test]
    fn a_fetch_finishing_after_its_key_was_removed_is_not_cached() {
        let root = crate::store::test_dir();
        let p_ = Provider::ApiBible;
        let (b, p) = part("John 3:16");
        let f = verses(3, 16, 1);
        remember(&root, p_, "CSB", "old", 1, (b, &p), &f);
        assert!(!Cache::path(&root, p_, "CSB").exists(), "no key saved");
        save_key(&root, Provider::ApiBible, "new").unwrap();
        remember(&root, p_, "CSB", "old", 1, (b, &p), &f);
        assert!(!Cache::path(&root, p_, "CSB").exists(), "key changed");
        remember(&root, p_, "CSB", "new", 1, (b, &p), &f);
        assert!(Cache::load(&root, p_, "CSB").get(b, &p, 1, p_.cache_policy()).is_some());
    }

    #[test]
    fn catalog_ids_make_safe_cache_files_that_a_key_change_clears() {
        assert_eq!(file_part("yv:111"), "yv~111");
        assert_eq!(file_part("ab:de4e12af7f28f599-02"), "ab~de4e12af7f28f599-02");
        assert_eq!(file_part("CSB"), "CSB");
        assert_eq!(file_part("a/b\\c"), "a%2Fb%5Cc");
        let root = crate::store::test_dir();
        save_key(&root, Provider::YouVersion, "a").unwrap();
        assert!(cache_file(&root, Provider::YouVersion, "yv:149"));
        assert!(root.join("bible-cache/youversion-yv~149.json").exists());
        super::super::catalog::store(&root, Provider::YouVersion, Default::default()).unwrap();
        save_key(&root, Provider::YouVersion, "b").unwrap();
        assert!(!Cache::path(&root, Provider::YouVersion, "yv:149").exists(), "a catalog Bible's cache goes with the key");
        assert!(super::super::catalog::cached(&root, Provider::YouVersion).is_none(), "and so does the Bible list");
    }

    #[tokio::test]
    async fn added_catalog_bible_is_looked_up_from_its_cache_with_its_own_label() {
        let root = crate::store::test_dir();
        let b = super::super::catalog::OnlineBible { id: "yv:149".into(), abbr: "RVR1960".into(), copyright: "© SBAL".into(), lang: "es".into(), ..Default::default() };
        let err = lookup(&root, "yv:149", "John 3:16").await.unwrap_err();
        assert_eq!(err, super::super::strings::REMOVED, "not added: a plain message, not an id");
        super::super::catalog::add(&root, b).unwrap();
        let err = lookup(&root, "yv:149", "John 3:16").await.unwrap_err();
        assert!(err.starts_with("RVR1960 needs the internet"), "{err}");
        let mut s = Secrets::default();
        s.keys.insert("youversion".into(), "k".into());
        s.save(&root).unwrap();
        let (b, p) = part("John 3:16");
        let mut c = Cache::default();
        c.put(b, &p, crate::app::now_secs(), Fetched { verses: vec![(3, 16, "Porque de tal manera amó Dios al mundo".into())], fums: vec![] }, ESV, 879);
        c.save(&root, Provider::YouVersion, "yv:149").unwrap();
        let (label, v) = lookup(&root, "yv:149", "Jn 3:16").await.unwrap();
        assert_eq!(label, "John 3:16");
        assert!(v[0].text.starts_with("Porque"));
        assert_eq!(lookup(&root, "yv:1849", "John 3:16").await.unwrap_err(), super::super::strings::BLOCKED);
    }

    #[test]
    fn secrets_round_trip_and_trim() {
        let root = crate::store::test_dir();
        let mut s = Secrets::default();
        s.keys.insert("esv".into(), "  abc  ".into());
        s.save(&root).unwrap();
        assert_eq!(Secrets::load(&root).key(Provider::Esv), Some("abc"));
        s.keys.insert("esv".into(), "   ".into());
        assert_eq!(s.key(Provider::Esv), None);
    }

    #[tokio::test]
    async fn lookup_without_a_key_says_where_to_add_one() {
        let root = crate::store::test_dir();
        let err = lookup(&root, "ESV", "John 3:16").await.unwrap_err();
        assert!(err.contains("Settings → Bible"), "{err}");
    }

    #[tokio::test]
    async fn lookup_uses_the_cache_without_calling_out() {
        let root = crate::store::test_dir();
        let mut s = Secrets::default();
        s.keys.insert("esv".into(), "not-a-real-key".into());
        s.save(&root).unwrap();
        let (b, p) = part("John 3:16");
        let mut c = Cache::default();
        c.put(b, &p, crate::app::now_secs(), Fetched { verses: vec![(3, 16, "For God so loved the world".into())], fums: vec![] }, ESV, 879);
        c.save(&root, Provider::Esv, "ESV").unwrap();
        let (label, v) = lookup(&root, "ESV", "jn 3:16").await.unwrap();
        assert_eq!(label, "John 3:16");
        assert_eq!(v[0].reference, "John 3:16");
        assert_eq!(v[0].text, "For God so loved the world");
    }
}
