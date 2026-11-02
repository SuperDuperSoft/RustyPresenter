//! Copyrighted translations from licensed online sources (spec §8.5, F5).
//! Each church registers its own free key with the provider; nothing is bundled or published.
//! Keys live in `secrets.json` beside `data.json`: never in backups, the app state or the repo.
//! Fetched passages go into a small per-translation cache whose size and age follow each provider's terms.
use super::{Part, Reference, api_bible, canonical, display_name, esv, kjv, nlt, parse_reference, youversion};
use crate::model::VerseText;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io, path::Path, sync::{Mutex, OnceLock}, time::Duration};

/// (chapter, verse, text)
pub type Verse = (u16, u16, String);

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

    async fn fetch(self, key: &str, translation: &str, book: u8, part: &Part) -> Result<Vec<Verse>, FetchError> {
        let client = client()?;
        match self {
            Provider::Esv => esv::fetch(client, key, book, part).await,
            Provider::Nlt => nlt::fetch(client, key, book, part).await,
            Provider::ApiBible => api_bible::fetch(client, key, translation, book, part).await,
            Provider::YouVersion => youversion::fetch(client, key, translation, book, part).await,
        }
    }
}

/// Every source for a translation, best first.
pub fn sources(translation: &str) -> impl Iterator<Item = &'static Source> { SOURCES.iter().filter(move |s| s.translation == translation) }

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
/// Removing a key also deletes the text cached from that provider (API.Bible terms §10).
pub async fn set_key(root: &Path, provider: Provider, key: &str) -> Result<(), String> {
    let key = key.trim();
    if !key.is_empty() {
        let probe = parse_reference("John 3:16")?;
        let t = provider.probe_translation();
        provider.fetch(key, t, probe.book, &probe.parts[0]).await.map_err(|e| e.message(t, provider))?;
    }
    let _guard = FILES.lock().unwrap();
    let mut s = Secrets::load(root);
    if key.is_empty() { s.keys.remove(provider.key_name()); } else { s.keys.insert(provider.key_name().into(), key.into()); }
    s.save(root).map_err(|e| e.to_string())?;
    if key.is_empty() {
        for src in SOURCES.iter().filter(|s| s.provider == provider) { let _ = std::fs::remove_file(Cache::path(root, src.translation)); }
    }
    Ok(())
}

// ---------- cache ----------

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct Cache { entries: Vec<Entry> }

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Entry { book: u8, part: (u16, Option<u16>, u16, Option<u16>), fetched: u64, verses: Vec<Verse> }

fn part_key(p: &Part) -> (u16, Option<u16>, u16, Option<u16>) { (p.start.0, p.start.1, p.end.0, p.end.1) }

/// Serialises cache and secrets file access between concurrent lookups.
static FILES: Mutex<()> = Mutex::new(());

impl Cache {
    fn path(root: &Path, translation: &str) -> std::path::PathBuf { root.join("bible-cache").join(format!("{translation}.json")) }

    pub fn load(root: &Path, translation: &str) -> Self {
        std::fs::read(Self::path(root, translation)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, root: &Path, translation: &str) -> io::Result<()> {
        let p = Self::path(root, translation);
        std::fs::create_dir_all(p.parent().unwrap())?;
        crate::store::write_atomic(&p, &serde_json::to_vec(self).map_err(io::Error::other)?)
    }

    pub fn verse_count(&self) -> usize { self.entries.iter().map(|e| e.verses.len()).sum() }

    fn get(&self, book: u8, part: &Part, now: u64, policy: CachePolicy) -> Option<&[Verse]> {
        let fresh = |e: &Entry| policy.max_age_days.is_none_or(|d| now.saturating_sub(e.fetched) < d * 86_400);
        self.entries.iter().find(|e| e.book == book && e.part == part_key(part) && fresh(e)).map(|e| e.verses.as_slice())
    }

    /// Adds a passage, then drops the oldest entries until the policy holds. A passage that alone breaks it is not kept.
    fn put(&mut self, book: u8, part: &Part, now: u64, verses: Vec<Verse>, policy: CachePolicy, book_len: usize) {
        self.entries.retain(|e| !(e.book == book && e.part == part_key(part)));
        if let Some(d) = policy.max_age_days { self.entries.retain(|e| now.saturating_sub(e.fetched) < d * 86_400); }
        let half = book_len / 2;
        if verses.len() > policy.max_verses || (policy.half_book && verses.len() > half) { return; }
        self.entries.push(Entry { book, part: part_key(part), fetched: now, verses });
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

/// Looks up a passage in a copyrighted translation: cache first, then the provider.
pub async fn lookup(root: &Path, translation: &str, input: &str) -> Result<(String, Vec<VerseText>), String> {
    let secrets = Secrets::load(root);
    let src = source_with(translation, &secrets)
        .ok_or_else(|| format!("{translation} has no licensed source. Import a Bible file you're licensed to use in Settings → Bible."))?;
    let r: Reference = parse_reference(input)?;
    let key = secrets.key(src.provider).map(str::to_string)
        .ok_or_else(|| format!("{translation} needs a free key from {}. Add it in Settings → Bible.", src.provider.label()))?;
    let policy = src.provider.cache_policy();
    let book_len = kjv().book_len(r.book);
    let display = display_name(r.book);
    let now = crate::app::now_secs();
    let mut out = Vec::new();
    for part in &r.parts {
        let cached = { let _g = FILES.lock().unwrap(); Cache::load(root, translation).get(r.book, part, now, policy).map(<[Verse]>::to_vec) };
        let verses = match cached {
            Some(v) => v,
            None => {
                let v = src.provider.fetch(&key, translation, r.book, part).await.map_err(|e| e.message(translation, src.provider))?;
                if v.is_empty() { return Err("No verses found.".into()); }
                let _g = FILES.lock().unwrap();
                let mut cache = Cache::load(root, translation);
                cache.put(r.book, part, now, v.clone(), policy, book_len);
                let _ = cache.save(root, translation);
                v
            }
        };
        out.extend(verses.into_iter().map(|(c, v, text)| VerseText { reference: format!("{display} {c}:{v}"), text }));
        if out.len() > 400 { return Err("That passage is too long. Choose up to about 400 verses.".into()); }
    }
    Ok((canonical(&r, display), out))
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
    fn verses(c: u16, from: u16, n: u16) -> Vec<Verse> { (from..from + n).map(|v| (c, v, format!("v{v}"))).collect() }
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
        assert_eq!(c.get(b, &p, 100, ESV).unwrap().len(), 3);
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
        c.put(b, &p, crate::app::now_secs(), vec![(3, 16, "For God so loved the world".into())], ESV, 879);
        c.save(&root, "ESV").unwrap();
        let (label, v) = lookup(&root, "ESV", "jn 3:16").await.unwrap();
        assert_eq!(label, "John 3:16");
        assert_eq!(v[0].reference, "John 3:16");
        assert_eq!(v[0].text, "For God so loved the world");
    }
}
