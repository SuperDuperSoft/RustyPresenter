//! Manual slide-break marks between verses: gap states, clicks, save/clean and remap (spec §5.4).
use super::{VerseBreaks, VerseId};
use std::collections::BTreeSet;

/// What the gap before a verse shows in the breaks dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapState {
    /// No new slide here.
    None,
    /// The automatic plan starts a new slide here.
    Auto,
    /// The operator asked for a new slide here.
    Added,
    /// The operator kept an automatic break together.
    Kept,
}

/// The spec §5.4 table: a manual `before` always wins; `join` only matters on an automatic break.
pub fn gap_state(auto_break: bool, before: bool, join: bool) -> GapState {
    match (auto_break, before, join) {
        (_, true, _) => GapState::Added,
        (true, false, true) => GapState::Kept,
        (true, false, false) => GapState::Auto,
        (false, false, _) => GapState::None,
    }
}

/// The change a click on a gap makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Before,
    Join,
    Clear,
}

/// None → add a break; Auto → keep together; Added or Kept → back to automatic.
pub fn click(state: GapState) -> Mark {
    match state {
        GapState::None => Mark::Before,
        GapState::Auto => Mark::Join,
        GapState::Added | GapState::Kept => Mark::Clear,
    }
}

/// Store `m` for the gap before `at`; a verse is never in both sets.
pub fn apply_mark(b: &mut VerseBreaks, at: VerseId, m: Mark) {
    match m {
        Mark::Before => {
            b.before.insert(at);
            b.join.remove(&at);
        }
        Mark::Join => {
            b.join.insert(at);
            b.before.remove(&at);
        }
        Mark::Clear => {
            b.before.remove(&at);
            b.join.remove(&at);
        }
    }
}

/// The marks to save: `join` only where the automatic plan breaks, and `before` wins over `join`.
pub fn for_save(b: &VerseBreaks, auto_gaps: &BTreeSet<VerseId>) -> VerseBreaks {
    VerseBreaks {
        before: b.before.clone(),
        join: b.join.iter().filter(|id| auto_gaps.contains(id) && !b.before.contains(id)).copied().collect(),
    }
}

/// Drop marks on verses not in `union`, `before` on the first verse, and `join` where `before` is set.
pub fn clean(b: &VerseBreaks, union: &[VerseId]) -> VerseBreaks {
    let known: BTreeSet<VerseId> = union.iter().copied().collect();
    let first = union.first().copied();
    let before: BTreeSet<VerseId> = b.before.iter().filter(|id| known.contains(id) && Some(**id) != first).copied().collect();
    let join = b.join.iter().filter(|id| known.contains(id) && !before.contains(id)).copied().collect();
    VerseBreaks { before, join }
}

/// After a retranslation: each `before` on a missing verse moves to the nearest existing boundary
/// (same chapter first, then the smaller verse-number distance, ties to the later verse);
/// `join` on a missing verse is dropped. Then [`clean`].
pub fn remap(b: &VerseBreaks, union: &[VerseId]) -> VerseBreaks {
    let known: BTreeSet<VerseId> = union.iter().copied().collect();
    let before = b.before.iter().filter_map(|&id| if known.contains(&id) { Some(id) } else { nearest(&known, id) }).collect();
    let join = b.join.iter().filter(|id| known.contains(id)).copied().collect();
    clean(&VerseBreaks { before, join }, union)
}

fn nearest(known: &BTreeSet<VerseId>, id: VerseId) -> Option<VerseId> {
    let below = known.range(..id).next_back().copied();
    let above = known.range(id..).next().copied();
    let same = |c: VerseId| (c.book(), c.chapter()) == (id.book(), id.chapter());
    let dist = |c: VerseId| c.verse().abs_diff(id.verse());
    match (below, above) {
        (Some(lo), Some(hi)) => {
            let pick_lo = match (same(lo), same(hi)) {
                (true, false) => true,
                (false, true) => false,
                _ => dist(lo) < dist(hi),
            };
            Some(if pick_lo { lo } else { hi })
        }
        (lo, hi) => lo.or(hi),
    }
}

/// One gap in the breaks dialog: the gap before `verse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GapRow {
    pub verse: Option<VerseId>,
    pub state: GapState,
    /// False when the verse has no id, so no mark can be stored for it.
    pub enabled: bool,
}

/// One row per gap (before every verse after the first). `auto_at[i]` is true when union verse `i`
/// starts a slide in the automatic plan.
pub fn gap_rows(union: &[Option<VerseId>], b: &VerseBreaks, auto_at: &[bool]) -> Vec<GapRow> {
    union
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, &verse)| {
            let auto = auto_at.get(i).copied().unwrap_or(false);
            match verse {
                Some(id) => GapRow { verse, state: gap_state(auto, b.before.contains(&id), b.join.contains(&id)), enabled: true },
                None => GapRow { verse, state: gap_state(auto, false, false), enabled: false },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripture::test_support::{ids, set};

    #[test]
    fn gap_state_table() {
        use GapState::*;
        let rows = [
            ((false, false, false), None),
            ((true, false, false), Auto),
            ((false, true, false), Added),
            ((true, true, false), Added),
            ((false, false, true), None),
            ((true, false, true), Kept),
            ((false, true, true), Added),
            ((true, true, true), Added),
        ];
        for ((a, b, j), want) in rows {
            assert_eq!(gap_state(a, b, j), want, "{a} {b} {j}");
        }
    }

    #[test]
    fn clicks_cycle_states() {
        assert_eq!(
            (click(GapState::None), click(GapState::Auto), click(GapState::Kept), click(GapState::Added)),
            (Mark::Before, Mark::Join, Mark::Clear, Mark::Clear)
        );
        let at = VerseId(42, 3, 17);
        let state = |b: &VerseBreaks| gap_state(true, b.before.contains(&at), b.join.contains(&at));
        let mut b = VerseBreaks::default();
        let m = click(state(&b));
        apply_mark(&mut b, at, m); // Auto → Join
        assert_eq!(state(&b), GapState::Kept);
        let m = click(state(&b));
        apply_mark(&mut b, at, m); // Kept → Clear
        assert!(b.is_empty());
        assert_eq!(state(&b), GapState::Auto);
        apply_mark(&mut b, at, Mark::Before);
        assert_eq!((state(&b), b.join.len()), (GapState::Added, 0));
        let m = click(state(&b));
        apply_mark(&mut b, at, m); // Added → Clear, shows Auto again
        assert_eq!(state(&b), GapState::Auto);
    }

    #[test]
    fn save_drops_stale_joins() {
        let b = VerseBreaks { before: set(&[]), join: set(&[(42, 3, 17), (42, 3, 18)]) };
        assert_eq!(for_save(&b, &set(&[(42, 3, 18)])).join, set(&[(42, 3, 18)]));
        assert!(for_save(&b, &set(&[])).join.is_empty());
    }

    #[test]
    fn before_on_first_verse_is_ignored() {
        let u = ids(&[(42, 3, 16), (42, 3, 17)]);
        let b = VerseBreaks { before: set(&[(42, 3, 16), (42, 3, 17)]), join: set(&[]) };
        assert_eq!(clean(&b, &u).before, set(&[(42, 3, 17)]));
    }

    #[test]
    fn verse_in_before_and_join_keeps_only_before() {
        let u = ids(&[(42, 3, 16), (42, 3, 17)]);
        let b = VerseBreaks { before: set(&[(42, 3, 17)]), join: set(&[(42, 3, 17)]) };
        let c = clean(&b, &u);
        assert_eq!((c.before, c.join), (set(&[(42, 3, 17)]), set(&[])));
        let s = for_save(&b, &set(&[(42, 3, 17)]));
        assert_eq!((s.before, s.join), (set(&[(42, 3, 17)]), set(&[])));
    }

    #[test]
    fn unknown_break_verses_dropped() {
        let u = ids(&[(42, 3, 16), (42, 3, 17), (42, 3, 18)]);
        let b = VerseBreaks { before: set(&[(42, 4, 1)]), join: set(&[(42, 4, 2)]) };
        assert!(clean(&b, &u).is_empty());
    }

    #[test]
    fn breaks_move_to_nearest_existing_boundary_on_retranslate() {
        let u = ids(&[(42, 3, 16), (42, 3, 17), (42, 3, 19), (42, 3, 20)]);
        let b = VerseBreaks { before: set(&[(42, 3, 18), (42, 3, 21)]), join: set(&[(42, 3, 18)]) };
        let r = remap(&b, &u);
        assert_eq!(r.before, set(&[(42, 3, 19), (42, 3, 20)])); // 18: tie 17/19 → later; 21 → 20
        assert!(r.join.is_empty());
    }

    #[test]
    fn remap_prefers_same_chapter() {
        let u = ids(&[(42, 3, 35), (42, 3, 36), (42, 4, 5)]);
        let b = VerseBreaks { before: set(&[(42, 4, 1)]), join: set(&[]) };
        assert_eq!(remap(&b, &u).before, set(&[(42, 4, 5)])); // 3:36 is closer in order but another chapter
    }

    #[test]
    fn gap_rows_disable_unparseable_verse() {
        let u = [Some(VerseId(42, 3, 16)), None, Some(VerseId(42, 3, 18))];
        let r = gap_rows(&u, &VerseBreaks::default(), &[false, true, true]);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].verse, r[0].state, r[0].enabled), (None, GapState::Auto, false));
        assert_eq!((r[1].verse, r[1].state, r[1].enabled), (Some(VerseId(42, 3, 18)), GapState::Auto, true));
        let r2 = gap_rows(&u, &VerseBreaks { before: set(&[(42, 3, 18)]), join: set(&[]) }, &[false, false, false]);
        assert_eq!((r2[0].state, r2[1].state), (GapState::None, GapState::Added));
    }
}
