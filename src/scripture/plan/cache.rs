//! Plan keys and the plan cache (spec §5.4): a scripture item reuses its measured plan while
//! nothing that planning reads has changed, and otherwise shows a character estimate.
use crate::model::VerseText;
use crate::scripture::cache::TextCache;
use crate::scripture::planning::{CharMeasure, PlanTheme, PlannedScripture, ScriptureInput, plan_scripture};
use crate::scripture::sources::SourceStatus;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// A hash of everything planning reads for one item; equal keys mean the same plan.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PlanKey(pub u64);

fn hash_verses(vs: &[VerseText], h: &mut DefaultHasher) {
    vs.len().hash(h);
    for v in vs {
        v.reference.hash(h);
        v.text.hash(h);
        v.id.hash(h);
    }
}

/// The key of `i`: every Bible's verses, codes and notices, marks, `per_slide`, layout,
/// theme and canvas.
pub fn plan_key<T: PlanTheme>(i: &ScriptureInput<T>) -> PlanKey {
    let mut h = DefaultHasher::new();
    hash_verses(&i.main, &mut h);
    i.main_code.hash(&mut h);
    i.main_notice.hash(&mut h);
    i.also.len().hash(&mut h);
    for a in &i.also {
        hash_verses(&a.verses, &mut h);
        a.code.hash(&mut h);
        a.notice.hash(&mut h);
    }
    i.per_slide.hash(&mut h);
    i.layout.hash(&mut h);
    i.breaks.hash(&mut h);
    i.theme.hash(&mut h);
    i.canvas.hash(&mut h);
    PlanKey(h.finish())
}

/// Measured plans by item id, each with the key it was planned for.
#[derive(Default)]
pub struct PlanCache {
    map: RwLock<HashMap<Uuid, (PlanKey, Arc<PlannedScripture>)>>,
}

impl PlanCache {
    /// The plan stored for `id`, only when it was planned for `key`.
    pub fn get(&self, id: Uuid, key: PlanKey) -> Option<Arc<PlannedScripture>> {
        let map = self.map.read().unwrap_or_else(|e| e.into_inner());
        map.get(&id).filter(|(k, _)| *k == key).map(|(_, p)| p.clone())
    }

    /// Store (or replace) the plan for `id`.
    pub fn insert(&self, id: Uuid, key: PlanKey, plan: Arc<PlannedScripture>) {
        self.map.write().unwrap_or_else(|e| e.into_inner()).insert(id, (key, plan));
    }

    /// Drop the plans of items not in `ids`.
    pub fn retain(&self, ids: &HashSet<Uuid>) {
        self.map.write().unwrap_or_else(|e| e.into_inner()).retain(|id, _| ids.contains(id));
    }

    /// Every stored item's key.
    pub fn keys(&self) -> HashMap<Uuid, PlanKey> {
        self.map.read().unwrap_or_else(|e| e.into_inner()).iter().map(|(id, (k, _))| (*id, *k)).collect()
    }
}

/// The cached measured plan for `id` (`fitting` false), else a character estimate
/// (`fitting` true) shown until the measured plan arrives.
pub fn plan_or_estimate<T: PlanTheme>(cache: &PlanCache, id: Uuid, input: &ScriptureInput<T>) -> (Arc<PlannedScripture>, bool) {
    if let Some(plan) = cache.get(id, plan_key(input)) {
        return (plan, false);
    }
    (Arc::new(plan_scripture(input, &mut CharMeasure::for_input(&input.theme, input.canvas))), true)
}

/// A translation's verses for an item: the stored text when it is on this computer, else the
/// text cache (memory only). `None`: the text is missing and the item builds no slides.
pub fn resolve_verses(stored: &[VerseText], status: SourceStatus, cache: &TextCache, tr: &str, reference: &str) -> Option<Vec<VerseText>> {
    if status == SourceStatus::Offline {
        return Some(stored.to_vec());
    }
    cache.get(tr, reference).map(|(_, vs)| vs)
}

/// The ids in `now`, in order, whose key is new or differs from `prev`.
pub fn changed(prev: &HashMap<Uuid, PlanKey>, now: &[(Uuid, PlanKey)]) -> Vec<Uuid> {
    now.iter().filter(|(id, k)| prev.get(id) != Some(k)).map(|(id, _)| *id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripture::VerseId;
    use crate::scripture::planning::AlsoInput;
    use crate::scripture::test_support::{TestTheme, input, tmp_root, v, verses};

    fn sample() -> ScriptureInput<TestTheme> { input(&[v("John 3:16", "a"), v("John 3:17", "b")], None) }

    #[test]
    fn build_item_uses_cached_plan_when_key_matches() {
        let (c, id, i) = (PlanCache::default(), Uuid::new_v4(), sample());
        let plan = Arc::new(plan_scripture(&i, &mut CharMeasure::with_limits(10, 20)));
        c.insert(id, plan_key(&i), plan.clone());
        let (got, fitting) = plan_or_estimate(&c, id, &i);
        assert!(Arc::ptr_eq(&got, &plan) && !fitting);
    }
    #[test]
    fn build_item_falls_back_to_char_measure_and_marks_fitting() {
        let (c, id, i) = (PlanCache::default(), Uuid::new_v4(), sample());
        let want = plan_scripture(&i, &mut CharMeasure::for_input(&i.theme, i.canvas));
        let (got, fitting) = plan_or_estimate(&c, id, &i);
        assert!(fitting);
        assert_eq!(*got, want);
        let mut other = sample();
        other.per_slide = 1; // stale key in the cache is a miss too
        c.insert(id, plan_key(&other), Arc::new(plan_scripture(&other, &mut CharMeasure::with_limits(10, 20))));
        assert!(plan_or_estimate(&c, id, &i).1);
    }
    #[test]
    fn build_item_with_missing_online_text_has_no_slides() {
        let cache = TextCache::new(tmp_root());
        let stored = verses(42, 3, 16..=16);
        assert_eq!(resolve_verses(&[], SourceStatus::Online, &cache, "ESV", "John 3:16"), None);
        assert_eq!(resolve_verses(&stored, SourceStatus::Offline, &cache, "KJV", "John 3:16"), Some(stored.clone()));
        cache.put("ESV", "John 3:16", 500, "John 3:16", &stored);
        assert_eq!(resolve_verses(&[], SourceStatus::Online, &cache, "ESV", "John 3:16"), Some(stored));
    }
    #[test]
    fn plan_key_changes_with_theme_breaks_canvas_also() {
        let base = sample();
        let mut a = base.clone();
        a.theme.size = 80;
        let mut b = base.clone();
        b.breaks.before.insert(VerseId(42, 3, 17));
        let mut c = base.clone();
        c.canvas = (1024, 768);
        let mut d = base.clone();
        d.also = vec![AlsoInput { code: "NIV".into(), verses: vec![v("John 3:16", "n")], notice: None }];
        let keys: HashSet<_> = [&base, &a, &b, &c, &d].iter().map(|i| plan_key(i)).collect();
        assert_eq!(keys.len(), 5);
        assert_eq!(plan_key(&base), plan_key(&base.clone()));
    }
    #[test]
    fn only_changed_keys_are_resubmitted() {
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let prev = HashMap::from([(a, PlanKey(1)), (b, PlanKey(2))]);
        assert_eq!(changed(&prev, &[(a, PlanKey(1)), (b, PlanKey(3)), (c, PlanKey(1))]), vec![b, c]);
        assert!(changed(&prev, &[(a, PlanKey(1))]).is_empty());
    }
}
