//! Changing the order a song plays in (its arrangement) from the Slides panel (Jake: "It should be as easy as humanly
//! possible to rearrange the song in any order you like."). Each change gives the new order and, for each part as it
//! played before, where it plays now (`moved`), so a song on screen keeps its live part live while its order changes
//! (`Live::reconcile` with an order hint).
use crate::sections::{SectionKind, arrangement, resolve_order};
use crate::slides::parse_sections;

/// A change to the order, by entry (an entry is one name in the order as typed).
#[derive(Clone, Debug, PartialEq)]
pub enum OrderOp {
    /// Entry `from` to place `to` (`to` = the number of entries: the end).
    Move { from: usize, to: usize },
    Remove { at: usize },
    /// Entry `at` played again right after itself.
    Duplicate { at: usize },
    /// Section `label` played at place `at` (the number of entries: the end).
    Insert { label: String, at: usize },
    /// The part at played place `occurrence` one place earlier or later in the order.
    Step { occurrence: usize, later: bool },
    /// Every verse followed by the chorus.
    ChorusAfterVerses,
    /// The chorus once more at the end.
    ChorusAtEnd,
}

/// A new order, and where each played part went (by played place, old → new; None: taken out).
#[derive(Clone, Debug, PartialEq)]
pub struct Reordered { pub order: Vec<String>, pub moved: Vec<Option<usize>> }

/// The first chorus of the song as written, if it has one.
pub fn chorus(lyrics: &str) -> Option<String> {
    parse_sections(lyrics).into_iter().map(|s| s.label).find(|l| matches!(SectionKind::from_label(l).0, SectionKind::Chorus))
}

/// The entry of `order` that plays at played place `occurrence` (entries that match no part are skipped in play).
pub fn entry_of(order: &[String], labels: &[String], occurrence: usize) -> Option<usize> {
    order.iter().enumerate().filter(|(_, e)| labels.iter().any(|l| l.eq_ignore_ascii_case(e.trim()))).map(|(i, _)| i).nth(occurrence)
}

/// `op` on `order` (an empty order is the written one, spelled out first). None when nothing would change or the
/// change cannot be made (no chorus).
pub fn apply(lyrics: &str, order: &[String], op: &OrderOp) -> Option<Reordered> {
    let labels: Vec<String> = parse_sections(lyrics).into_iter().map(|s| s.label).collect();
    let mut entries: Vec<(String, Option<usize>)> = {
        let mut v = order.to_vec();
        arrangement::ensure_real(&mut v, lyrics);
        v.into_iter().enumerate().map(|(i, e)| (e, Some(i))).collect()
    };
    let before: Vec<(String, Option<usize>)> = entries.clone();
    let n = entries.len();
    match op {
        OrderOp::Move { from, to } => {
            if *from >= n { return None }
            let e = entries.remove(*from);
            let to = (*to).min(entries.len());
            entries.insert(to, e);
        }
        OrderOp::Remove { at } => { if *at >= n { return None } entries.remove(*at); }
        OrderOp::Duplicate { at } => { let e = entries.get(*at)?.0.clone(); entries.insert(at + 1, (e, None)) }
        OrderOp::Insert { label, at } => { entries.insert((*at).min(n), (label.clone(), None)) }
        OrderOp::Step { occurrence, later } => {
            let names: Vec<String> = entries.iter().map(|e| e.0.clone()).collect();
            let from = entry_of(&names, &labels, *occurrence)?;
            // The next or previous entry that plays.
            let plays = |i: usize| labels.iter().any(|l| l.eq_ignore_ascii_case(entries[i].0.trim()));
            let to = if *later { (from + 1..n).find(|&i| plays(i))? } else { (0..from).rev().find(|&i| plays(i))? };
            let e = entries.remove(from);
            entries.insert(to, e);
        }
        OrderOp::ChorusAfterVerses => {
            let c = chorus(lyrics)?;
            let is = |l: &str, k: SectionKind| SectionKind::from_label(l).0 == k;
            let mut out: Vec<(String, Option<usize>)> = vec![];
            for (i, e) in entries.iter().enumerate() {
                out.push(e.clone());
                let next_is_chorus = entries.get(i + 1).is_some_and(|x| is(&x.0, SectionKind::Chorus));
                if is(&e.0, SectionKind::Verse) && !next_is_chorus { out.push((c.clone(), None)) }
            }
            entries = out;
        }
        OrderOp::ChorusAtEnd => entries.push((chorus(lyrics)?, None)),
    }
    if entries.iter().map(|e| &e.0).eq(before.iter().map(|e| &e.0)) { return None }
    // Played places, old and new, by entry.
    let played = |v: &[(String, Option<usize>)]| -> Vec<Option<usize>> {
        v.iter().filter(|e| labels.iter().any(|l| l.eq_ignore_ascii_case(e.0.trim()))).map(|e| e.1).collect()
    };
    let (old, new) = (played(&before), played(&entries));
    let moved = old.iter().map(|id| id.and_then(|id| new.iter().position(|x| *x == Some(id)))).collect();
    let mut order: Vec<String> = entries.into_iter().map(|e| e.0).collect();
    // Back to exactly the written order: stored as "the written order" (empty), as Use written order does.
    if order.iter().map(|e| e.trim().to_lowercase()).eq(arrangement::written_order(lyrics).iter().map(|l| l.to_lowercase())) && order_is_plain(order.as_slice(), &labels) { order.clear() }
    Some(Reordered { order, moved })
}

/// Every entry names a part (nothing typed that plays nothing).
fn order_is_plain(order: &[String], labels: &[String]) -> bool { resolve_order(order, labels).len() == order.len() }

#[cfg(test)]
mod tests {
    use super::*;

    const L: &str = "Verse 1\nv1\n\nChorus\nc\n\nVerse 2\nv2\n\nBridge\nb";

    fn v(s: &[&str]) -> Vec<String> { s.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn moving_and_removing_say_where_each_part_went() {
        let r = apply(L, &[], &OrderOp::Move { from: 0, to: 4 }).unwrap();
        assert_eq!(r.order, v(&["Chorus", "Verse 2", "Bridge", "Verse 1"]));
        assert_eq!(r.moved, vec![Some(3), Some(0), Some(1), Some(2)]);
        let r = apply(L, &v(&["Verse 1", "Chorus", "Verse 2", "Chorus"]), &OrderOp::Remove { at: 1 }).unwrap();
        assert_eq!(r.order, v(&["Verse 1", "Verse 2", "Chorus"]));
        assert_eq!(r.moved, vec![Some(0), None, Some(1), Some(2)], "the chorus taken out is gone; the other is still itself");
    }

    #[test]
    fn repeating_a_part_anywhere() {
        let r = apply(L, &[], &OrderOp::Insert { label: "Chorus".into(), at: 3 }).unwrap();
        assert_eq!(r.order, v(&["Verse 1", "Chorus", "Verse 2", "Chorus", "Bridge"]));
        assert_eq!(r.moved, vec![Some(0), Some(1), Some(2), Some(4)]);
        let r = apply(L, &[], &OrderOp::Duplicate { at: 1 }).unwrap();
        assert_eq!(r.order, v(&["Verse 1", "Chorus", "Chorus", "Verse 2", "Bridge"]));
        assert_eq!(r.moved[1], Some(1), "the original stays first; the copy is new");
    }

    #[test]
    fn a_part_steps_earlier_or_later_past_names_that_play_nothing() {
        let order = v(&["Verse 1", "Coda", "Chorus", "Verse 2"]);
        let r = apply(L, &order, &OrderOp::Step { occurrence: 1, later: false }).unwrap();
        assert_eq!(r.order, v(&["Chorus", "Verse 1", "Coda", "Verse 2"]));
        assert!(apply(L, &r.order, &OrderOp::Step { occurrence: 0, later: false }).is_none(), "already first");
    }

    #[test]
    fn one_tap_patterns() {
        let r = apply(L, &[], &OrderOp::ChorusAfterVerses).unwrap();
        assert_eq!(r.order, v(&["Verse 1", "Chorus", "Verse 2", "Chorus", "Bridge"]));
        let r = apply(L, &r.order, &OrderOp::ChorusAtEnd).unwrap();
        assert_eq!(r.order.last().map(String::as_str), Some("Chorus"));
        assert!(apply("Verse 1\na\n\nVerse 2\nb", &[], &OrderOp::ChorusAtEnd).is_none(), "no chorus");
    }

    #[test]
    fn back_to_the_written_order_is_stored_as_the_written_order() {
        let r = apply(L, &v(&["Chorus", "Verse 1", "Verse 2", "Bridge"]), &OrderOp::Move { from: 0, to: 1 }).unwrap();
        assert!(r.order.is_empty());
    }
}
