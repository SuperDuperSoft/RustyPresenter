//! The bounded text cache for translations that are not stored on this computer (spec §5.1a).
//! Memory plus `<root>/scripture-cache/<TR>.json`, at most `cap` verses on disk; verses of the
//! open service are pinned and evicted last.
use super::VerseId;
use super::input::verse_id_of;
use crate::model::VerseText;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const CACHE_CAP_DEFAULT: usize = 500;
const FILE_VERSION: u32 = 1;

/// How many verses of a translation may be kept on disk (500 for every provider today).
pub fn cache_cap(_provider: &str) -> usize { CACHE_CAP_DEFAULT }

/// The most verses one item may hold in a cached translation.
pub fn per_item_limit(cap: usize) -> usize { cap.min(400) }

#[derive(Serialize, Deserialize, Clone)]
struct FileVerse {
    id: VerseId,
    reference: String,
    text: String,
    used: u64,
}

#[derive(Serialize, Deserialize, Clone)]
struct FilePassage {
    label: String,
    ids: Vec<VerseId>,
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    v: u32,
    verses: Vec<FileVerse>,
    passages: BTreeMap<String, FilePassage>,
}

#[derive(Default)]
struct TrCache {
    verses: HashMap<VerseId, (VerseText, u64)>,
    passages: HashMap<String, FilePassage>,
    pinned: BTreeSet<VerseId>,
}

impl TrCache {
    /// Drop passages that lost a verse.
    fn prune_passages(&mut self) {
        let verses = &self.verses;
        self.passages.retain(|_, p| p.ids.iter().all(|id| verses.contains_key(id)));
    }

    /// Drop least-recently-used unpinned verses until at most `cap` remain (pinned ones stay).
    fn evict(&mut self, cap: usize) {
        if self.verses.len() <= cap { return; }
        let mut unpinned: Vec<(u64, VerseId)> = self.verses.iter()
            .filter(|(id, _)| !self.pinned.contains(id))
            .map(|(id, (_, used))| (*used, *id))
            .collect();
        unpinned.sort();
        let excess = self.verses.len() - cap;
        for (_, id) in unpinned.into_iter().take(excess) { self.verses.remove(&id); }
        self.prune_passages();
    }

    /// The file image: the `cap` most recently used verses and the passages they cover.
    fn file(&self, cap: usize) -> CacheFile {
        let mut vs: Vec<FileVerse> = self.verses.iter()
            .map(|(id, (v, used))| FileVerse { id: *id, reference: v.reference.clone(), text: v.text.clone(), used: *used })
            .collect();
        vs.sort_by(|a, b| b.used.cmp(&a.used));
        vs.truncate(cap);
        vs.sort_by_key(|v| v.id);
        let kept: BTreeSet<VerseId> = vs.iter().map(|v| v.id).collect();
        let passages = self.passages.iter()
            .filter(|(_, p)| p.ids.iter().all(|id| kept.contains(id)))
            .map(|(k, p)| (k.clone(), p.clone()))
            .collect();
        CacheFile { v: FILE_VERSION, verses: vs, passages }
    }
}

#[derive(Default)]
struct Inner {
    /// Monotonic recency counter.
    tick: u64,
    trs: HashMap<String, TrCache>,
}

/// The text cache. `Send + Sync`; every method takes `&self`.
pub struct TextCache {
    root: PathBuf,
    inner: Mutex<Inner>,
}

fn tr_key(tr: &str) -> String { tr.trim().to_uppercase() }

fn passage_key(reference: &str) -> String { reference.trim().to_lowercase() }

fn file_path(root: &Path, tr: &str) -> PathBuf {
    let name: String = tr_key(tr).chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    root.join("scripture-cache").join(format!("{name}.json"))
}

/// The file's contents; `None` when missing, corrupt or from another version.
fn read_file(path: &Path) -> Option<CacheFile> {
    let f: CacheFile = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    (f.v == FILE_VERSION).then_some(f)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir)?; }
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

impl TextCache {
    /// No I/O; call `load` per translation.
    pub fn new(root: PathBuf) -> Self { TextCache { root, inner: Mutex::new(Inner::default()) } }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> { self.inner.lock().unwrap_or_else(|e| e.into_inner()) }

    /// Read the translation's file into memory. A missing or corrupt file leaves it empty
    /// (the next `put` replaces the bad file).
    pub fn load(&self, tr: &str) {
        let Some(file) = read_file(&file_path(&self.root, tr)) else { return };
        let mut inner = self.lock();
        let max_used = file.verses.iter().map(|v| v.used).max().unwrap_or(0);
        inner.tick = inner.tick.max(max_used);
        let c = inner.trs.entry(tr_key(tr)).or_default();
        for v in file.verses {
            c.verses.entry(v.id).or_insert((VerseText { reference: v.reference, text: v.text }, v.used));
        }
        for (k, p) in file.passages { c.passages.entry(k).or_insert(p); }
        c.prune_passages();
    }

    /// The stored passage (label, verses), from memory only; a hit marks its verses used.
    pub fn get(&self, tr: &str, reference: &str) -> Option<(String, Vec<VerseText>)> {
        let mut inner = self.lock();
        let Inner { tick, trs } = &mut *inner;
        let c = trs.get_mut(&tr_key(tr))?;
        let p = c.passages.get(&passage_key(reference))?;
        if !p.ids.iter().all(|id| c.verses.contains_key(id)) { return None; }
        let mut out = Vec::with_capacity(p.ids.len());
        for id in &p.ids {
            *tick += 1;
            let entry = c.verses.get_mut(id)?;
            entry.1 = *tick;
            out.push(entry.0.clone());
        }
        Some((p.label.clone(), out))
    }

    /// Store a passage in memory and rewrite the translation's file, keeping at most `cap` verses.
    /// A passage with any verse whose label has no `VerseId` is not stored.
    pub fn put(&self, tr: &str, reference: &str, cap: usize, label: &str, verses: &[VerseText]) {
        let Some(ids) = verses.iter().map(|v| verse_id_of(&v.reference)).collect::<Option<Vec<_>>>() else { return };
        if ids.is_empty() { return; }
        let file = {
            let mut inner = self.lock();
            let Inner { tick, trs } = &mut *inner;
            let c = trs.entry(tr_key(tr)).or_default();
            for (id, v) in ids.iter().zip(verses) {
                *tick += 1;
                c.verses.insert(*id, (v.clone(), *tick));
            }
            c.passages.insert(passage_key(reference), FilePassage { label: label.to_string(), ids });
            c.evict(cap);
            c.file(cap)
        };
        if let Ok(json) = serde_json::to_vec(&file) {
            let _ = write_atomic(&file_path(&self.root, tr), &json);
        }
    }

    /// The verses of the open service: evicted last.
    pub fn set_pinned(&self, tr: &str, ids: BTreeSet<VerseId>) { self.lock().trs.entry(tr_key(tr)).or_default().pinned = ids; }

    /// How many verses the translation's file on disk holds; 0 when missing or corrupt.
    pub fn file_len(&self, tr: &str) -> usize { read_file(&file_path(&self.root, tr)).map_or(0, |f| f.verses.len()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::VerseText;
    use crate::scripture::test_support::{ids_of, tmp_root, verses, verses_many};

    #[test]
    fn per_item_limit_is_min_of_400_and_cap() { assert_eq!(per_item_limit(500), 400); assert_eq!(per_item_limit(300), 300); }
    #[test]
    fn cache_file_capped_at_500_verses_for_esv() {
        let c = TextCache::new(tmp_root());
        c.put("ESV", "Psalm 119", cache_cap("esv"), "Psalm 119", &verses(18, 119, 1..=176));
        c.put("ESV", "Isaiah 1-8", cache_cap("esv"), "Isaiah 1–8", &verses_many(22, 1..=8, 50));   // 400 verses; 176 + 400 > 500
        assert_eq!(c.file_len("ESV"), 500);
    }
    #[test]
    fn cache_evicts_outside_open_service_first() {
        let root = tmp_root();
        let c = TextCache::new(root.clone());
        let pinned = verses(18, 119, 1..=100);
        c.set_pinned("ESV", ids_of(&pinned));
        c.put("ESV", "Psalm 119:1-100", 500, "Psalm 119:1–100", &pinned);
        for ch in 1..=6 { c.put("ESV", &format!("Isaiah {ch}"), 500, &format!("Isaiah {ch}"), &verses(22, ch, 1..=100)); }
        let fresh = TextCache::new(root);
        fresh.load("ESV");
        assert_eq!(fresh.file_len("ESV"), 500);
        assert_eq!(fresh.get("ESV", "Psalm 119:1-100").unwrap().1.len(), 100);   // every pinned verse survived
        assert!(fresh.get("ESV", "Isaiah 1").is_none() && fresh.get("ESV", "Isaiah 2").is_none());   // oldest unpinned went first
        assert!(fresh.get("ESV", "Isaiah 6").is_some());
    }
    #[test]
    fn pinned_overflow_kept_in_memory_only() {
        let c = TextCache::new(tmp_root());
        let vs = verses_many(19, 1..=6, 100);   // 600 verses
        c.set_pinned("ESV", ids_of(&vs));
        c.put("ESV", "Proverbs 1-6", 500, "Proverbs 1–6", &vs);
        assert_eq!(c.file_len("ESV"), 500);
        assert_eq!(c.get("ESV", "Proverbs 1-6").unwrap().1.len(), 600);
    }
    #[test]
    fn get_is_memory_only_until_load() {
        let root = tmp_root();
        TextCache::new(root.clone()).put("ESV", "John 3:16", 500, "John 3:16", &verses(42, 3, 16..=16));
        let c2 = TextCache::new(root);
        assert!(c2.get("ESV", "John 3:16").is_none());
        c2.load("ESV");
        assert_eq!(c2.get("ESV", "  JOHN 3:16 ").unwrap().0, "John 3:16");   // key is trimmed + lowercased
    }
    #[test]
    fn corrupt_cache_file_starts_empty() {
        for bad in ["not json", r#"{"v":99,"verses":[],"passages":{}}"#] {
            let root = tmp_root();
            std::fs::create_dir_all(root.join("scripture-cache")).unwrap();
            std::fs::write(root.join("scripture-cache/ESV.json"), bad).unwrap();
            let c = TextCache::new(root.clone());
            c.load("ESV");
            assert!(c.get("ESV", "John 3:16").is_none());
            c.put("ESV", "John 3:16", 500, "John 3:16", &verses(42, 3, 16..=16));
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("scripture-cache/ESV.json")).unwrap()).unwrap();
            assert_eq!((v["v"].as_u64(), c.file_len("ESV")), (Some(1), 1));
        }
    }
    #[test]
    fn verses_without_ids_are_not_cached() {
        let c = TextCache::new(tmp_root());
        c.put("ESV", "Odd", 500, "Odd", &[VerseText { reference: "Odd label".into(), text: "x".into() }]);
        assert!(c.get("ESV", "Odd").is_none());
        assert_eq!(c.file_len("ESV"), 0);
    }
}
