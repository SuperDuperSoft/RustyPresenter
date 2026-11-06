//! The search service (spec §3.1, §5.5): builds a local translation's index lazily on its first
//! search or ahead of time (pre-warm), one build at a time, and answers `Pending` until it is ready.
use super::index::{Hit, Order, Scope, SearchIndex, build};
use super::query::SearchQuery;
use crate::scripture::fetch::BibleReaders;
use crate::scripture::sources::{SourceStatus, Sources, status_of};
use crate::scripture::strings::{SEARCH_CHIP_ESV, SEARCH_CHIP_READY, search_chip_none, search_unavailable};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::runtime::Handle;

/// How a translation can be searched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchAccess {
    /// Stored on this computer: the local index.
    Local,
    /// ESV online, through the Bible session's `/search` (not built yet).
    EsvOnline,
    None,
}

/// Offline translations search locally; ESV searches online when `esv_search` is available.
/// Online translations are never indexed.
pub fn search_access(sources: &Sources, tr: &str, esv_search: bool) -> SearchAccess {
    match status_of(sources, tr) {
        SourceStatus::Offline => SearchAccess::Local,
        SourceStatus::Online if esv_search && tr.trim().eq_ignore_ascii_case("ESV") => SearchAccess::EsvOnline,
        _ => SearchAccess::None,
    }
}

/// The search status chip under the search box.
pub fn search_chip(access: SearchAccess, tr: &str) -> String {
    match access {
        SearchAccess::Local => SEARCH_CHIP_READY.to_string(),
        SearchAccess::EsvOnline => SEARCH_CHIP_ESV.to_string(),
        SearchAccess::None => search_chip_none(tr),
    }
}

/// One search from the search panel.
#[derive(Deserialize, Clone, Debug)]
pub struct SearchRequest {
    pub query: String,
    pub translation: String,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub order: Order,
}

/// A search's answer: results, or `Pending` while the translation's index is being built.
#[derive(Serialize, Debug)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SearchReply {
    Ready { total: usize, hits: Vec<Hit> },
    Pending,
}

/// One translation's index: building (with its cancel flag) or ready.
enum Slot {
    Building(Arc<AtomicBool>),
    Ready(Arc<SearchIndex>),
}

type Slots = Arc<Mutex<HashMap<String, Slot>>>;

/// Builds and keeps the local indexes. Indexes live in memory for the service's lifetime and are
/// never written to disk. Safe to call from any thread: builds run on the captured runtime's
/// blocking pool, so the caller needs no Tokio context.
pub struct SearchService {
    rt: Handle,
    readers: Arc<dyn BibleReaders>,
    on_ready: Arc<dyn Fn(String) + Send + Sync>,
    slots: Slots,
}

/// The map key for a translation code.
fn key(tr: &str) -> String { crate::bible::normalise_id(tr) }

impl SearchService {
    /// `on_ready(code)` is called on the blocking pool when a translation's index is ready.
    pub fn new(rt: Handle, readers: Arc<dyn BibleReaders>, on_ready: Arc<dyn Fn(String) + Send + Sync>) -> Self {
        SearchService { rt, readers, on_ready, slots: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Searches `req.translation`, or starts (or keeps) its index build and answers `Pending`.
    pub fn search(&self, sources: &Sources, req: &SearchRequest) -> Result<SearchReply, String> {
        let tr = req.translation.trim();
        if search_access(sources, tr, false) != SearchAccess::Local {
            return Err(search_unavailable(tr));
        }
        let q = SearchQuery::parse(&req.query)?;
        let code = key(tr);
        let mut slots = self.lock();
        match slots.get(&code) {
            Some(Slot::Ready(ix)) => {
                let (total, hits) = ix.page(&ix.rank(&ix.find(&q, req.scope), &q, req.order), &q);
                Ok(SearchReply::Ready { total, hits })
            }
            Some(Slot::Building(_)) => Ok(SearchReply::Pending),
            None => {
                self.start(&mut slots, code);
                Ok(SearchReply::Pending)
            }
        }
    }

    /// Builds `tr`'s index ahead of its first search, only for a translation stored on this
    /// computer and only when nothing is building.
    pub fn prewarm(&self, sources: &Sources, tr: &str) {
        if search_access(sources, tr, false) != SearchAccess::Local {
            return;
        }
        let code = key(tr);
        let mut slots = self.lock();
        if slots.contains_key(&code) || slots.values().any(|s| matches!(s, Slot::Building(_))) {
            return;
        }
        self.start(&mut slots, code);
    }

    pub fn is_ready(&self, tr: &str) -> bool { matches!(self.lock().get(&key(tr)), Some(Slot::Ready(_))) }

    pub fn is_building(&self, tr: &str) -> bool { matches!(self.lock().get(&key(tr)), Some(Slot::Building(_))) }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Slot>> { self.slots.lock().unwrap_or_else(|e| e.into_inner()) }

    /// Cancels any other build (it is rebuilt lazily on its next search) and starts `code`'s.
    fn start(&self, slots: &mut HashMap<String, Slot>, code: String) {
        slots.retain(|_, s| match s {
            Slot::Building(cancel) => {
                cancel.store(true, Ordering::SeqCst);
                false
            }
            Slot::Ready(_) => true,
        });
        let cancel = Arc::new(AtomicBool::new(false));
        slots.insert(code.clone(), Slot::Building(cancel.clone()));
        let (readers, on_ready, all) = (self.readers.clone(), self.on_ready.clone(), self.slots.clone());
        self.rt.spawn_blocking(move || {
            // A reader that panics ends the build like a cancel, so the slot never stays `Building`.
            let read = std::panic::AssertUnwindSafe(|| build(&mut |chapter| readers.local(&code, chapter), &cancel));
            let built = std::panic::catch_unwind(read).ok().flatten();
            let mut slots = all.lock().unwrap_or_else(|e| e.into_inner());
            // Only this build's own slot is replaced; a cancelled build leaves no trace.
            let mine = matches!(slots.get(&code), Some(Slot::Building(c)) if Arc::ptr_eq(c, &cancel));
            match built {
                Some(ix) if mine => {
                    slots.insert(code.clone(), Slot::Ready(Arc::new(ix)));
                    drop(slots);
                    on_ready(code);
                }
                None if mine => {
                    slots.remove(&code);
                }
                _ => {}
            }
        });
    }
}

impl Drop for SearchService {
    /// Stops any build still running, so shutting down never waits for a whole Bible to be read.
    fn drop(&mut self) {
        for slot in self.lock().values() {
            if let Slot::Building(cancel) = slot {
                cancel.store(true, Ordering::SeqCst);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripture::fetch::{KjvOnly, Lookup};
    use crate::scripture::test_support::rows;
    use std::future::Future;
    use std::path::Path;
    use std::pin::Pin;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use std::time::Duration;
    use tokio::runtime::Handle;

    const TEN_S: Duration = Duration::from_secs(10);

    fn on_ready(tx: mpsc::Sender<String>) -> Arc<dyn Fn(String) + Send + Sync> {
        let tx = Mutex::new(tx);
        Arc::new(move |code| { let _ = tx.lock().unwrap().send(code); })
    }

    fn noop() -> Arc<dyn Fn(String) + Send + Sync> { Arc::new(|_| {}) }

    fn req(query: &str, tr: &str) -> SearchRequest {
        SearchRequest { query: query.into(), translation: tr.into(), scope: Scope::All, order: Order::Best }
    }

    fn no_online<'a>(tr: &'a str) -> Pin<Box<dyn Future<Output = Lookup> + Send + 'a>> {
        Box::pin(async move { Err(format!("no online {tr}")) })
    }

    /// Panics on any read of `"NIV"` or `"ESV"`: those must never be indexed.
    struct PanicReaders;

    impl BibleReaders for PanicReaders {
        fn local(&self, tr: &str, input: &str) -> Lookup {
            if matches!(tr, "NIV" | "ESV") { panic!("local read of {tr} {input}") }
            crate::bible::kjv().lookup(input)
        }

        fn online<'a>(&'a self, _root: &'a Path, tr: &'a str, input: &'a str) -> Pin<Box<dyn Future<Output = Lookup> + Send + 'a>> {
            if matches!(tr, "NIV" | "ESV") { panic!("online read of {tr} {input}") }
            no_online(tr)
        }
    }

    /// `"WEB"` serves KJV text after a 20 ms sleep per chapter; `"KJV"` serves it at once.
    struct SlowReaders;

    impl BibleReaders for SlowReaders {
        fn local(&self, tr: &str, input: &str) -> Lookup {
            if tr == "WEB" { std::thread::sleep(Duration::from_millis(20)); }
            crate::bible::kjv().lookup(input)
        }

        fn online<'a>(&'a self, _root: &'a Path, tr: &'a str, _input: &'a str) -> Pin<Box<dyn Future<Output = Lookup> + Send + 'a>> {
            no_online(tr)
        }
    }

    /// KJV text; counts the reads of `"Genesis 1"`.
    #[derive(Default)]
    struct CountingReaders {
        genesis_1: AtomicUsize,
    }

    impl CountingReaders {
        fn genesis_1_calls(&self) -> usize { self.genesis_1.load(Ordering::SeqCst) }
    }

    impl BibleReaders for CountingReaders {
        fn local(&self, _tr: &str, input: &str) -> Lookup {
            if input == "Genesis 1" { self.genesis_1.fetch_add(1, Ordering::SeqCst); }
            crate::bible::kjv().lookup(input)
        }

        fn online<'a>(&'a self, _root: &'a Path, tr: &'a str, _input: &'a str) -> Pin<Box<dyn Future<Output = Lookup> + Send + 'a>> {
            no_online(tr)
        }
    }

    #[test]
    fn search_access_local_only_for_offline_status() {
        let src = rows(&[("KJV", "offline"), ("ESV", "online"), ("NIV", "online")]);
        assert_eq!(search_access(&src, "KJV", false), SearchAccess::Local);
        assert_eq!(search_access(&src, "ESV", false), SearchAccess::None);
        assert_eq!(search_access(&src, "ESV", true), SearchAccess::EsvOnline);
        assert_eq!(search_access(&src, "NIV", true), SearchAccess::None);
    }

    #[test]
    fn search_chip_wording() {
        assert_eq!(search_chip(SearchAccess::Local, "KJV"), "Search: Ready");
        assert_eq!(search_chip(SearchAccess::EsvOnline, "ESV"), "Search: online (ESV)");
        assert_eq!(search_chip(SearchAccess::None, "NIV"), "Search isn't available for NIV");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn first_search_is_pending_then_ready() {
        let (tx, rx) = mpsc::channel();
        let s = SearchService::new(Handle::current(), Arc::new(KjvOnly), on_ready(tx));
        let (src, r) = (Sources::kjv_only(), req("be still and know", "KJV"));
        assert!(matches!(s.search(&src, &r).unwrap(), SearchReply::Pending));
        assert_eq!(rx.recv_timeout(TEN_S).unwrap(), "KJV");
        match s.search(&src, &r).unwrap() {
            SearchReply::Ready { total, hits } => {
                assert_eq!(total, 3);
                assert_eq!(hits[0].reference, "Psalm 46:10");
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn online_translation_never_indexed_or_prewarmed() {
        let s = SearchService::new(Handle::current(), Arc::new(PanicReaders), noop());
        let src = rows(&[("NIV", "online")]);
        s.prewarm(&src, "NIV");
        assert_eq!(
            s.search(&src, &req("love", "NIV")).unwrap_err(),
            "Search isn't available for NIV. Try a translation stored on this computer, like KJV."
        );
        assert!(!s.is_building("NIV") && !s.is_ready("NIV"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn prewarm_skips_online_bible_box_translation() {
        let s = SearchService::new(Handle::current(), Arc::new(PanicReaders), noop());
        s.prewarm(&rows(&[("ESV", "online")]), "ESV");
        assert!(!s.is_building("ESV") && !s.is_ready("ESV"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn prewarm_cancels_when_other_translation_searched() {
        let (tx, rx) = mpsc::channel();
        let s = SearchService::new(Handle::current(), Arc::new(SlowReaders), on_ready(tx));
        let src = rows(&[("KJV", "offline"), ("WEB", "offline")]);
        s.prewarm(&src, "WEB");
        assert!(s.is_building("WEB"));
        assert!(matches!(s.search(&src, &req("love", "KJV")).unwrap(), SearchReply::Pending));
        assert!(!s.is_building("WEB")); // cancelled synchronously
        assert_eq!(rx.recv_timeout(TEN_S).unwrap(), "KJV");
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err()); // WEB never reports ready
        assert!(matches!(s.search(&src, &req("love", "WEB")).unwrap(), SearchReply::Pending)); // lazy rebuild
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn typing_while_building_stays_pending() {
        let (tx, rx) = mpsc::channel();
        let readers = Arc::new(CountingReaders::default());
        let s = SearchService::new(Handle::current(), readers.clone(), on_ready(tx));
        let src = Sources::kjv_only();
        assert!(matches!(s.search(&src, &req("lo", "KJV")).unwrap(), SearchReply::Pending));
        assert!(matches!(s.search(&src, &req("love", "KJV")).unwrap(), SearchReply::Pending));
        assert_eq!(rx.recv_timeout(TEN_S).unwrap(), "KJV");
        assert_eq!(readers.genesis_1_calls(), 1); // one build
    }

    #[test]
    fn search_from_plain_thread_uses_captured_handle() {
        // no Tokio context on the calling thread, like the Slint UI thread
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let (tx, rx) = mpsc::channel();
        let s = Arc::new(SearchService::new(rt.handle().clone(), Arc::new(KjvOnly), on_ready(tx)));
        let s2 = s.clone();
        let r = std::thread::spawn(move || s2.search(&Sources::kjv_only(), &req("love", "KJV"))).join().unwrap();
        assert!(matches!(r.unwrap(), SearchReply::Pending));
        assert_eq!(rx.recv_timeout(TEN_S).unwrap(), "KJV");
    }

    #[test]
    fn search_reply_json_shapes() {
        assert_eq!(serde_json::to_value(SearchReply::Pending).unwrap(), serde_json::json!({"state":"pending"}));
        let v = serde_json::to_value(SearchReply::Ready { total: 0, hits: vec![] }).unwrap();
        assert_eq!(v, serde_json::json!({"state":"ready","total":0,"hits":[]}));
    }

    #[test]
    fn search_request_defaults_scope_and_order() {
        let r: SearchRequest = serde_json::from_str(r#"{"query":"love","translation":"KJV"}"#).unwrap();
        assert_eq!((r.scope, r.order), (Scope::All, Order::Best));
    }
}
