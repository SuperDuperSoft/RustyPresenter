//! Copyrighted translations from licensed online sources (spec §8.5, F5).
//! Each church registers its own free key with the provider; nothing is bundled or published.
//! Keys live in `secrets.json` beside `data.json`: never in backups, the app state or the repo.
//! Fetched passages go into a small cache per provider and translation whose size and age follow each provider's terms.
use super::{Part, Reference, api_bible, blocklist, canonical, display_name, esv, kjv, nlt, parse_reference, youversion};
use crate::{model::VerseText, scripture::strings::BLOCKED};
use crate::tr;
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
    pub fn setup_note(self) -> String {
        match self {
            Provider::Esv => tr!("Free for churches. Create an “API application”, then copy its key."),
            Provider::Nlt => tr!("Free for non-commercial use. Register, then copy the key from your account."),
            Provider::ApiBible => tr!("The free Starter plan allows 3 copyrighted Bibles: under Plan, pick CSB (and NIV or NLT if you want them from here)."),
            Provider::YouVersion => tr!("Create an app, then accept each Bible’s license in the portal (NIV from Biblica; HCSB if it is offered)."),
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

/// True for a Bible read over the internet (ESV, NIV, a YouVersion or API.Bible id): its text is only ever kept in
/// the capped cache, never with a service (Nov 5 spec §5). A file the church imported under the same name is
/// checked by the caller.
pub fn is_online(translation: &str) -> bool {
    let t = translation.trim();
    t.starts_with("yv:") || t.starts_with("ab:") || SOURCES.iter().any(|s| super::id::same_id(s.translation, t))
}

/// The source that will be used: the first with a saved key, else the first listed.
pub fn source_with(translation: &str, secrets: &Secrets) -> Option<&'static Source> {
    sources(translation).find(|s| secrets.key(s.provider).is_some()).or_else(|| sources(translation).next())
}

#[derive(Debug, PartialEq)]
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
        match status { 401 | 403 => Self::BadKey, 429 => Self::RateLimited, 404 => Self::NotFound, s => Self::Other(tr!("the service answered {status}", status = s)) }
    }

    fn message(&self, translation: &str, provider: Provider) -> String {
        match self {
            Self::Network => tr!("Couldn't reach {service} for {translation}. Check the internet connection.", service = provider.label(), translation = translation),
            Self::BadKey => tr!("{service} refused the key. Check it in Settings → Bible.", service = provider.label()),
            Self::RateLimited => tr!("{service} says too many lookups. Wait a minute and try again.", service = provider.label()),
            Self::NotFound => tr!("No verses found."),
            Self::Other(e) => tr!("{translation} lookup failed: {error}.", translation = translation, error = e),
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
    if key.is_empty() || old.as_deref() != Some(key) { clear_cache(root, provider); }
    Ok(())
}

/// Deletes every cache file of one provider. Call with `FILES` held.
fn clear_cache(root: &Path, provider: Provider) {
    for src in SOURCES.iter().filter(|s| s.provider == provider) { let _ = std::fs::remove_file(Cache::path(root, provider, src.translation)); }
}

// ---------- cache ----------

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct Cache {
    entries: Vec<Entry>,
    /// Verses of the open service (book, chapter, verse): the passages holding them are evicted last (Nov 5 spec §5).
    #[serde(skip)]
    pinned: std::collections::BTreeSet<(u8, u16, u16)>,
}

/// The open service's verses in each online Bible, by id (`pin`).
static PINNED: Mutex<BTreeMap<String, std::collections::BTreeSet<(u8, u16, u16)>>> = Mutex::new(BTreeMap::new());

/// Keeps `verses` of `translation` in its cache as long as the caps allow: other passages are evicted first. Replaces
/// that Bible's earlier pins.
pub fn pin(translation: &str, verses: impl IntoIterator<Item = crate::scripture::VerseId>) {
    PINNED.lock().unwrap().insert(super::id::normalise_id(translation), verses.into_iter().map(|v| (v.0, v.1, v.2)).collect());
}

fn pinned(translation: &str) -> std::collections::BTreeSet<(u8, u16, u16)> {
    PINNED.lock().unwrap().get(&super::id::normalise_id(translation)).cloned().unwrap_or_default()
}

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
        root.join("bible-cache").join(format!("{}-{}.json", provider.key_name(), super::id::file_stem(translation)))
    }

    /// Also deletes the old provider-less `bible-cache/{translation}.json`, whose source is unknown.
    pub fn load(root: &Path, provider: Provider, translation: &str) -> Self {
        let _ = std::fs::remove_file(root.join("bible-cache").join(format!("{}.json", super::id::file_stem(translation))));
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

    /// True when the entry holds a verse of the open service.
    fn is_pinned(&self, e: &Entry) -> bool { e.verses.iter().any(|(c, v, _)| self.pinned.contains(&(e.book, *c, *v))) }

    /// The entry to evict first among those `of` accepts: the oldest one not pinned, else the oldest.
    fn victim(&self, of: impl Fn(&Entry) -> bool) -> Option<usize> {
        self.entries.iter().position(|e| of(e) && !self.is_pinned(e)).or_else(|| self.entries.iter().position(of))
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
        while self.verse_count() > policy.max_verses {
            let Some(i) = self.victim(|_| true) else { break };
            self.entries.remove(i);
        }
        if policy.half_book {
            while self.entries.iter().filter(|e| e.book == book).map(|e| e.verses.len()).sum::<usize>() > half {
                let Some(i) = self.victim(|e| e.book == book) else { break };
                self.entries.remove(i);
            }
        }
    }
}

// ---------- lookup ----------

/// Looks up a passage in a copyrighted translation: cache first, then the provider.
pub async fn lookup(root: &Path, translation: &str, input: &str) -> Result<(String, Vec<VerseText>), String> {
    if blocklist::blocked(translation, &[], &[], &[translation, &format!("yv:{}", translation.trim())]) { return Err(BLOCKED.into()); }
    let secrets = Secrets::load(root);
    let src = source_with(translation, &secrets)
        .ok_or_else(|| tr!("{translation} has no online source. Paste its verses with Add → Bible, or import a licensed file in Settings → Bible.", translation = translation))?;
    let r: Reference = parse_reference(input)?;
    let key = secrets.key(src.provider).map(str::to_string)
        .ok_or_else(|| tr!("{translation} needs a free key from {service}. Add it in Settings → Bible, or paste the verses with Add → Bible.", translation = translation, service = src.provider.label()))?;
    let (provider, policy) = (src.provider, src.provider.cache_policy());
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
                        return Err(e.message(translation, provider));
                    }
                };
                if f.verses.is_empty() { return Err(tr!("No verses found.")); }
                if refused(provider, translation, r.book, &f) { return Err(BLOCKED.into()); }
                remember(root, src, &key, now, (r.book, part), &f);
                f
            }
        };
        if refused(provider, translation, r.book, &f) { return Err(BLOCKED.into()); }
        provider.report(root, &f.fums);
        out.extend(f.verses.into_iter().map(|(c, v, text)| VerseText { reference: format!("{display} {c}:{v}"), text, id: Some(crate::scripture::VerseId(r.book, c, v)) }));
        if out.len() > 400 { return Err(tr!("That passage is too long. Choose up to about 400 verses.")); }
    }
    Ok((canonical(&r, display), out))
}

/// A passage from the cache alone, never the network: what an online Bible can show offline (Nov 5 spec §5).
/// `None` when any part of it is missing or too old.
pub fn cached(root: &Path, translation: &str, input: &str) -> Option<Vec<VerseText>> {
    let src = source_with(translation, &Secrets::load(root))?;
    let r = parse_reference(input).ok()?;
    let (policy, display, now) = (src.provider.cache_policy(), display_name(r.book), crate::app::now_secs());
    let _g = FILES.lock().unwrap();
    let cache = Cache::load(root, src.provider, translation);
    let mut out = Vec::new();
    for part in &r.parts {
        let e = cache.get(r.book, part, now, policy)?;
        out.extend(e.verses.iter().map(|(c, v, text)| VerseText { reference: format!("{display} {c}:{v}"), text: text.clone(), id: Some(crate::scripture::VerseId(r.book, *c, *v)) }));
    }
    Some(out)
}

/// Puts one passage of `text` in the cache, as a fetch would (tests of what the app does offline).
#[cfg(test)]
pub(crate) fn put_cached(root: &Path, translation: &str, input: &str, text: &str) {
    let src = source_with(translation, &Secrets::load(root)).unwrap();
    let r = parse_reference(input).unwrap();
    let part = &r.parts[0];
    let verses = (part.start.1.unwrap_or(1)..=part.end.1.unwrap_or(1)).map(|v| (part.start.0, v, text.to_string())).collect();
    let mut cache = Cache::load(root, src.provider, translation);
    cache.put(r.book, part, crate::app::now_secs(), Fetched { verses, fums: vec![] }, src.provider.cache_policy(), kjv().book_len(r.book));
    cache.save(root, src.provider, translation).unwrap();
}

/// Text that turns out to be a blocked translation, whatever its label says: by its verses, or by the notice API.Bible
/// sent with them. It is neither kept nor reported as used.
fn refused(provider: Provider, translation: &str, book: u8, f: &Fetched) -> bool {
    f.verses.iter().any(|(c, v, text)| blocklist::blocked_verse(book, *c, *v, text))
        || (provider == Provider::ApiBible && api_bible::copyright(translation).is_some_and(|n| blocklist::blocked("", &[], &[&n], &[])))
}

/// Caches a fetched passage, unless the key it was fetched with was removed or changed meanwhile.
fn remember(root: &Path, src: &Source, key: &str, now: u64, (book, part): (u8, &Part), f: &Fetched) {
    let _g = FILES.lock().unwrap();
    if Secrets::load(root).key(src.provider) != Some(key) { return; }
    let mut cache = Cache::load(root, src.provider, src.translation);
    cache.pinned = pinned(src.translation);
    cache.put(book, part, now, f.clone(), src.provider.cache_policy(), kjv().book_len(book));
    let _ = cache.save(root, src.provider, src.translation);
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

    #[test]
    fn a_prefixed_id_has_its_own_safe_cache_file() {
        let root = crate::store::test_dir();
        let (b, p) = part("John 3:16");
        let mut c = Cache::default();
        c.put(b, &p, 1, Fetched { verses: vec![(3, 16, "x".into())], fums: vec![] }, ESV, 879);
        c.save(&root, Provider::ApiBible, "ab:de4e12af7f28f599-02").unwrap();
        assert!(root.join("bible-cache/api_bible-ab~de4e12af7f28f599-02.json").exists());
        assert!(Cache::load(&root, Provider::ApiBible, "ab:de4e12af7f28f599-02").get(b, &p, 1, ESV).is_some());
        c.save(&root, Provider::ApiBible, "ab:../../x").unwrap();
        assert!(root.join("bible-cache/api_bible-ab~..%2F..%2Fx.json").exists());
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
        let src = SOURCES.iter().find(|s| s.translation == "CSB" && s.provider == Provider::ApiBible).unwrap();
        let (b, p) = part("John 3:16");
        let f = verses(3, 16, 1);
        remember(&root, src, "old", 1, (b, &p), &f);
        assert!(!Cache::path(&root, src.provider, "CSB").exists(), "no key saved");
        save_key(&root, Provider::ApiBible, "new").unwrap();
        remember(&root, src, "old", 1, (b, &p), &f);
        assert!(!Cache::path(&root, src.provider, "CSB").exists(), "key changed");
        remember(&root, src, "new", 1, (b, &p), &f);
        assert!(Cache::load(&root, src.provider, "CSB").get(b, &p, 1, src.provider.cache_policy()).is_some());
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

    #[tokio::test]
    async fn blocked_names_are_refused_before_any_key_is_needed() {
        let root = crate::store::test_dir();
        for t in ["TPT", "nwt", "1849"] { assert_eq!(lookup(&root, t, "John 3:16").await.unwrap_err(), BLOCKED, "{t}"); }
    }

    #[tokio::test]
    async fn text_that_reads_like_a_blocked_translation_is_refused_whatever_its_label() {
        let root = crate::store::test_dir();
        let mut s = Secrets::default();
        s.keys.insert("esv".into(), "not-a-real-key".into());
        s.save(&root).unwrap();
        let (b, p) = part("John 1:1");
        let mut c = Cache::default();
        c.put(b, &p, crate::app::now_secs(), Fetched { verses: vec![(1, 1, "and the Word was a god".into())], fums: vec![] }, ESV, 879);
        c.save(&root, Provider::Esv, "ESV").unwrap();
        assert_eq!(lookup(&root, "ESV", "John 1:1").await.unwrap_err(), BLOCKED);
    }

    #[test]
    fn pinned_verses_evicted_last() {
        let tiny = CachePolicy { max_verses: 3, half_book: false, max_age_days: None };
        let mut c = Cache { pinned: [(42, 3, 16)].into_iter().collect(), ..Cache::default() };
        let (b, p) = part("John 3:16");
        c.put(b, &p, 1, verses(3, 16, 1), tiny, 879);
        for (t, v) in [(2, 1), (3, 2), (4, 3), (5, 4)] { let (b, p) = part(&format!("Romans 1:{v}")); c.put(b, &p, t, verses(1, v, 1), tiny, 433) }
        assert!(c.get(b, &p, 5, tiny).is_some(), "the open service's verse stays");
        assert_eq!(c.verse_count(), 3);
    }

    #[test]
    fn cached_reads_the_cache_only() {
        let root = crate::store::test_dir();
        assert!(cache_file(&root, Provider::Esv, "ESV"));
        let got = cached(&root, "ESV", "John 3:16").expect("in the cache");
        assert_eq!((got[0].reference.as_str(), got[0].text.as_str(), got[0].id), ("John 3:16", "v16", Some(crate::scripture::VerseId(42, 3, 16))));
        assert_eq!(cached(&root, "ESV", "John 3:17"), None, "never the network");
        assert_eq!(cached(&root, "KJV", "John 3:16"), None, "no online source");
    }

    #[test]
    fn online_bibles() {
        assert!(is_online("NIV") && is_online("esv") && is_online("yv:111") && is_online("ab:de4e12af7f28f599-02"));
        assert!(!is_online("KJV") && !is_online("ebible:spaRV1909"));
    }
}
