//! The fetch adapter (spec §5.1): looks a passage up in one translation, choosing the local
//! reader, the bounded text cache or the online reader by the translation's `bible_sources` status.
use super::cache::{TextCache, cache_cap, per_item_limit};
use super::sources::{SourceStatus, Sources, provider_of, status_of};
use super::strings::{needs_key, not_available, passage_too_long};
use crate::bible;
use crate::model::VerseText;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, RwLock};

/// A looked-up passage: its display label and verses, or a message for the operator.
pub type Lookup = Result<(String, Vec<VerseText>), String>;

/// The most verses one offline lookup may return.
const OFFLINE_LIMIT: usize = 400;

/// The Bible session's readers, reached only through this trait so `src/bible*` stays untouched.
pub trait BibleReaders: Send + Sync + 'static {
    /// A translation stored on this computer (KJV, bundled or imported). Blocking.
    fn local(&self, tr: &str, input: &str) -> Lookup;
    /// A translation read over the internet (`bible::sources::lookup`).
    fn online<'a>(&'a self, root: &'a Path, tr: &'a str, input: &'a str) -> Pin<Box<dyn Future<Output = Lookup> + Send + 'a>>;
}

/// The readers available before the Bible session's sources are wired in: the bundled KJV only.
pub struct KjvOnly;

impl BibleReaders for KjvOnly {
    fn local(&self, tr: &str, input: &str) -> Lookup {
        if tr.eq_ignore_ascii_case("KJV") { bible::kjv().lookup(input) } else { Err(not_available(tr)) }
    }

    fn online<'a>(&'a self, _root: &'a Path, tr: &'a str, _input: &'a str) -> Pin<Box<dyn Future<Output = Lookup> + Send + 'a>> {
        Box::pin(async move { Err(not_available(tr)) })
    }
}

/// Looks passages up by `bible_sources` status; online text goes through the bounded cache.
pub struct Fetcher {
    pub readers: Arc<dyn BibleReaders>,
    pub cache: Arc<TextCache>,
    pub root: PathBuf,
}

impl Fetcher {
    /// A fetcher reading only the bundled KJV, with a cache under `root`.
    pub fn kjv_only(root: PathBuf) -> Fetcher {
        Fetcher { readers: Arc::new(KjvOnly), cache: Arc::new(TextCache::new(root.clone())), root }
    }

    /// Looks `input` up in `tr` (spec §5.1's dispatch table).
    pub async fn fetch(&self, sources: &Sources, tr: &str, input: &str) -> Lookup {
        match status_of(sources, tr) {
            SourceStatus::Offline => {
                let readers = self.readers.clone();
                let (tr, input) = (tr.to_string(), input.to_string());
                let found = tokio::task::spawn_blocking(move || readers.local(&tr, &input)).await.map_err(|e| e.to_string())??;
                within(found, OFFLINE_LIMIT)
            }
            SourceStatus::Online => {
                if let Some(hit) = self.cache.get(tr, input) {
                    return Ok(hit);
                }
                let cap = cache_cap(provider_of(sources, tr));
                let (label, verses) = within(self.readers.online(&self.root, tr, input).await?, per_item_limit(cap))?;
                self.cache.put(tr, input, cap, &label, &verses);
                Ok((label, verses))
            }
            SourceStatus::NeedsKey => Err(needs_key(tr)),
            SourceStatus::Unavailable | SourceStatus::Unknown => Err(not_available(tr)),
        }
    }
}

/// Refuses a passage of more than `limit` verses.
fn within(found: (String, Vec<VerseText>), limit: usize) -> Lookup {
    if found.1.len() > limit { Err(passage_too_long(limit)) } else { Ok(found) }
}

/// The app's fetcher and a way to read the current `bible_sources`, installed at start (Part B).
pub struct FetchCtx {
    pub fetcher: Fetcher,
    pub sources: Arc<dyn Fn() -> Sources + Send + Sync>,
}

static CTX: RwLock<Option<Arc<FetchCtx>>> = RwLock::new(None);

/// Makes `ctx` the context behind [`fetch`].
pub fn install(ctx: Arc<FetchCtx>) { *CTX.write().unwrap_or_else(|e| e.into_inner()) = Some(ctx); }

/// Removes the installed context; [`fetch`] falls back to the bundled KJV.
pub fn uninstall() { *CTX.write().unwrap_or_else(|e| e.into_inner()) = None; }

/// Looks `input` up in `translation` (spec §5.1). Uses the installed context when there is one
/// (`root` is then ignored: the installed fetcher owns the data root), else the bundled KJV only.
pub async fn fetch(root: &Path, translation: &str, input: &str) -> Lookup {
    let ctx = CTX.read().unwrap_or_else(|e| e.into_inner()).clone();
    match ctx {
        Some(ctx) => ctx.fetcher.fetch(&(ctx.sources)(), translation, input).await,
        None => Fetcher::kjv_only(root.to_path_buf()).fetch(&Sources::kjv_only(), translation, input).await,
    }
}

/// [`fetch`] for code already on the blocking pool. Must be called from a
/// `tokio::task::spawn_blocking` thread: it blocks on the current runtime's handle.
pub fn fetch_blocking(root: &Path, translation: &str, input: &str) -> Lookup {
    tokio::runtime::Handle::current().block_on(fetch(root, translation, input))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripture::test_support::*;

    #[tokio::test]
    async fn fetch_dispatches_by_status() {
        let (f, fake) = fetcher_with_fake();
        let src = rows(&[("KJV", "offline"), ("WEB", "offline"), ("NIV", "needs_key"), ("CSB", "unavailable"), ("ESV", "online")]);
        f.fetch(&src, "WEB", "John 3:16").await.unwrap();
        f.fetch(&src, "ESV", "John 3:16").await.unwrap();
        assert_eq!((fake.local_calls(), fake.online_calls()), (1, 1));
        assert_eq!(f.fetch(&src, "NIV", "John 3:16").await.unwrap_err(), "NIV needs a key. Add one in Settings → Bible.");
        assert_eq!(f.fetch(&src, "CSB", "John 3:16").await.unwrap_err(), "CSB isn't available.");
        assert_eq!(f.fetch(&src, "XYZ", "John 3:16").await.unwrap_err(), "XYZ isn't available.");
    }

    #[tokio::test]
    async fn online_fetch_writes_cache_and_hits_it_next_time() {
        let (f, fake) = fetcher_with_fake();
        let src = rows(&[("ESV", "online")]);
        let a = f.fetch(&src, "ESV", "John 3:16").await.unwrap();
        let b = f.fetch(&src, "ESV", "John 3:16").await.unwrap();
        assert_eq!((fake.online_calls(), &a), (1, &b));
        assert!(f.root.join("scripture-cache/ESV.json").exists());
    }

    #[tokio::test]
    async fn offline_text_is_never_written_to_cache() {
        let (f, fake) = fetcher_with_fake();
        let src = rows(&[("KJV", "offline")]);
        f.fetch(&src, "KJV", "John 3:16").await.unwrap();
        f.fetch(&src, "KJV", "John 3:16").await.unwrap();
        assert_eq!(fake.local_calls(), 2);
        assert!(!f.root.join("scripture-cache/KJV.json").exists());
    }

    #[tokio::test]
    async fn kjv_only_reads_bundled_kjv() {
        let (label, vs) = Fetcher::kjv_only(tmp_root()).fetch(&Sources::kjv_only(), "KJV", "Psalm 46:10").await.unwrap();
        assert_eq!((label.as_str(), vs.len()), ("Psalm 46:10", 1));
    }

    #[tokio::test]
    async fn online_result_over_limit_is_refused() {
        let (f, fake) = fetcher_with_fake();
        fake.set_len(401);
        let want = "That passage is too long. Choose up to about 400 verses.";
        assert_eq!(f.fetch(&rows(&[("ESV", "online")]), "ESV", "Psalm 119").await.unwrap_err(), want);
        assert_eq!(f.fetch(&rows(&[("WEB", "offline")]), "WEB", "Psalm 119").await.unwrap_err(), want);
        assert_eq!(f.cache.get("ESV", "Psalm 119"), None); // refused text is not cached
    }

    #[tokio::test]
    async fn free_fetch_uses_installed_context_else_kjv() {
        // one test fn, so no other test observes the installed global
        let root = tmp_root();
        assert_eq!(fetch(&root, "KJV", "John 3:16").await.unwrap().0, "John 3:16");
        assert_eq!(fetch(&root, "NIV", "John 3:16").await.unwrap_err(), "NIV isn't available.");
        let (f, fake) = fetcher_with_fake();
        install(Arc::new(FetchCtx { fetcher: f, sources: Arc::new(|| rows(&[("ESV", "online")])) }));
        fetch(&root, "ESV", "John 3:16").await.unwrap();
        assert_eq!(fake.online_calls(), 1);
        uninstall();
        assert_eq!(fetch(&root, "ESV", "John 3:16").await.unwrap_err(), "ESV isn't available.");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fetch_blocking_works_on_the_blocking_pool() {
        let root = tmp_root();
        let r = tokio::task::spawn_blocking(move || fetch_blocking(&root, "KJV", "Psalm 46:10")).await.unwrap();
        assert_eq!(r.unwrap().1.len(), 1);
    }
}
