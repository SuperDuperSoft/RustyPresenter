//! Verse numbering (Bible library spec §5.4). English Bibles (KJV-style) and the Hebrew Bible (Masoretic) number some
//! Old Testament verses differently: Malachi 4:1 in English is Malachi 3:19 in Hebrew, and a psalm's title is verse 1
//! in Hebrew, so Psalm 51:1 in English is Psalm 51:3. The words are the same; only the numbers move.
//!
//! This maps between the two, so a Hebrew Bible shown beside an English one lines up verse for verse. The table is
//! written from the documented differences between the two traditions (facts, not text), and is checked against a
//! real Hebrew Bible by `tests::live_hebrew_bible_lines_up_with_english` (run by hand).
use super::library::Vsys;
use crate::scripture::VerseId;

/// English verses `from..=to` of `chapter` are Hebrew `heb_chapter:heb_from` onwards, one for one.
struct Shift { book: u8, chapter: u16, from: u16, to: u16, heb_chapter: u16, heb_from: u16 }

const fn s(book: u8, chapter: u16, from: u16, to: u16, heb_chapter: u16, heb_from: u16) -> Shift { Shift { book, chapter, from, to, heb_chapter, heb_from } }

// Book numbers are places in `bible::BOOKS`.
const GEN: u8 = 0; const EXO: u8 = 1; const LEV: u8 = 2; const NUM: u8 = 3; const DEU: u8 = 4; const SA1: u8 = 8; const SA2: u8 = 9;
const KI1: u8 = 10; const KI2: u8 = 11; const CH1: u8 = 12; const CH2: u8 = 13; const NEH: u8 = 15; const JOB: u8 = 17; const PSA: u8 = 18;
const ECC: u8 = 20; const SNG: u8 = 21; const ISA: u8 = 22; const JER: u8 = 23; const EZK: u8 = 25; const DAN: u8 = 26; const HOS: u8 = 27;
const JOL: u8 = 28; const JON: u8 = 31; const MIC: u8 = 32; const NAM: u8 = 33; const ZEC: u8 = 37; const MAL: u8 = 38;

/// Where a chapter breaks at a different verse. In book order, English first.
const SHIFTS: &[Shift] = &[
    s(GEN, 31, 55, 55, 32, 1), s(GEN, 32, 1, 32, 32, 2),
    s(EXO, 8, 1, 4, 7, 26), s(EXO, 8, 5, 32, 8, 1), s(EXO, 22, 1, 1, 21, 37), s(EXO, 22, 2, 31, 22, 1),
    s(LEV, 6, 1, 7, 5, 20), s(LEV, 6, 8, 30, 6, 1),
    s(NUM, 16, 36, 50, 17, 1), s(NUM, 17, 1, 13, 17, 16), s(NUM, 29, 40, 40, 30, 1), s(NUM, 30, 1, 16, 30, 2),
    s(DEU, 12, 32, 32, 13, 1), s(DEU, 13, 1, 18, 13, 2), s(DEU, 22, 30, 30, 23, 1), s(DEU, 23, 1, 25, 23, 2),
    s(DEU, 29, 1, 1, 28, 69), s(DEU, 29, 2, 29, 29, 1),
    s(SA1, 21, 1, 15, 21, 2), s(SA1, 23, 29, 29, 24, 1), s(SA1, 24, 1, 22, 24, 2),
    s(SA2, 18, 33, 33, 19, 1), s(SA2, 19, 1, 43, 19, 2),
    s(KI1, 4, 21, 34, 5, 1), s(KI1, 5, 1, 18, 5, 15), s(KI1, 22, 44, 53, 22, 45),
    s(KI2, 11, 21, 21, 12, 1), s(KI2, 12, 1, 21, 12, 2),
    s(CH1, 6, 1, 15, 5, 27), s(CH1, 6, 16, 81, 6, 1), s(CH1, 12, 5, 40, 12, 6),
    s(CH2, 2, 1, 1, 1, 18), s(CH2, 2, 2, 18, 2, 1), s(CH2, 14, 1, 1, 13, 23), s(CH2, 14, 2, 15, 14, 1),
    s(NEH, 4, 1, 6, 3, 33), s(NEH, 4, 7, 23, 4, 1), s(NEH, 9, 38, 38, 10, 1), s(NEH, 10, 1, 39, 10, 2),
    s(JOB, 41, 1, 8, 40, 25), s(JOB, 41, 9, 34, 41, 1),
    s(ECC, 5, 1, 1, 4, 17), s(ECC, 5, 2, 20, 5, 1),
    s(SNG, 6, 13, 13, 7, 1), s(SNG, 7, 1, 13, 7, 2),
    s(ISA, 9, 1, 1, 8, 23), s(ISA, 9, 2, 21, 9, 1), s(ISA, 64, 2, 12, 64, 1),
    s(JER, 9, 1, 1, 8, 23), s(JER, 9, 2, 26, 9, 1),
    s(EZK, 20, 45, 49, 21, 1), s(EZK, 21, 1, 32, 21, 6),
    s(DAN, 4, 1, 3, 3, 31), s(DAN, 4, 4, 37, 4, 1), s(DAN, 5, 31, 31, 6, 1), s(DAN, 6, 1, 28, 6, 2),
    s(HOS, 1, 10, 11, 2, 1), s(HOS, 2, 1, 23, 2, 3), s(HOS, 11, 12, 12, 12, 1), s(HOS, 12, 1, 14, 12, 2),
    s(HOS, 13, 16, 16, 14, 1), s(HOS, 14, 1, 9, 14, 2),
    s(JOL, 2, 28, 32, 3, 1), s(JOL, 3, 1, 21, 4, 1),
    s(JON, 1, 17, 17, 2, 1), s(JON, 2, 1, 10, 2, 2),
    s(MIC, 5, 1, 1, 4, 14), s(MIC, 5, 2, 15, 5, 1),
    s(NAM, 1, 15, 15, 2, 1), s(NAM, 2, 1, 13, 2, 2),
    s(ZEC, 1, 18, 21, 2, 1), s(ZEC, 2, 1, 13, 2, 5),
    s(MAL, 4, 1, 6, 3, 19),
];

/// Psalms whose title is verse 1 in Hebrew: every verse is one later than in English.
const TITLE_1: &[u16] = &[3, 4, 5, 6, 7, 8, 9, 12, 13, 18, 19, 20, 21, 22, 30, 31, 34, 36, 38, 39, 40, 41, 42, 44, 45, 46, 47, 48, 49, 53, 55, 56, 57,
    58, 59, 61, 62, 63, 64, 65, 67, 68, 69, 70, 75, 76, 77, 80, 81, 83, 84, 85, 88, 89, 92, 102, 108, 140, 142];
/// Psalms whose title is verses 1 and 2 in Hebrew: every verse is two later.
const TITLE_2: &[u16] = &[51, 52, 54, 60];

fn title_verses(psalm: u16) -> u16 { if TITLE_2.contains(&psalm) { 2 } else if TITLE_1.contains(&psalm) { 1 } else { 0 } }

/// An English verse's place in the Hebrew Bible.
pub fn english_to_hebrew(VerseId(book, c, v): VerseId) -> VerseId {
    if let Some(r) = SHIFTS.iter().find(|r| r.book == book && r.chapter == c && (r.from..=r.to).contains(&v)) {
        return VerseId(book, r.heb_chapter, r.heb_from + (v - r.from));
    }
    // Psalm 13 has one verse fewer in Hebrew than its title would give: English 13:5 and 13:6 are both Hebrew 13:6.
    if book == PSA && c == 13 && v == 6 { return VerseId(book, 13, 6); }
    // The last verse of Nehemiah 7 in English is the end of 7:72 in Hebrew.
    if book == NEH && c == 7 && v == 73 { return VerseId(book, 7, 72); }
    if book == PSA { return VerseId(book, c, v + title_verses(c)); }
    VerseId(book, c, v)
}

/// A Hebrew verse's place in an English Bible. `None` for a psalm's title, which English Bibles do not number.
pub fn hebrew_to_english(VerseId(book, c, v): VerseId) -> Option<VerseId> {
    if let Some(r) = SHIFTS.iter().find(|r| r.book == book && r.heb_chapter == c && (r.heb_from..=r.heb_from + (r.to - r.from)).contains(&v)) {
        return Some(VerseId(book, r.chapter, r.from + (v - r.heb_from)));
    }
    // Hebrew Numbers 25:19 is the start of English 26:1.
    if book == NUM && c == 25 && v == 19 { return Some(VerseId(book, 26, 1)); }
    if book == PSA {
        let t = title_verses(c);
        return (v > t).then(|| VerseId(book, c, v - t));
    }
    Some(VerseId(book, c, v))
}

/// A verse of a Bible numbered `from`, as a Bible numbered `to` numbers it. Only the Hebrew and English ways are
/// told apart so far; the others read as English. `None` when the other Bible has no such verse.
pub fn convert(id: VerseId, from: Vsys, to: Vsys) -> Option<VerseId> {
    let hebrew = |v: Vsys| v == Vsys::Original;
    match (hebrew(from), hebrew(to)) {
        (true, false) => hebrew_to_english(id),
        (false, true) => Some(english_to_hebrew(id)),
        _ => Some(id),
    }
}

/// How a Bible's own verses show it is numbered: the Hebrew way has no Malachi 4 (its last verses are 3:19–24) and
/// a fourth chapter of Joel. A Bible without those books reads as English.
pub fn detect(has: &dyn Fn(u8, u16, u16) -> bool) -> Vsys {
    let malachi = has(MAL, 3, 19) && !has(MAL, 4, 1);
    let joel = has(JOL, 4, 1) && !has(JOL, 2, 28);
    if malachi || joel { Vsys::Original } else { Vsys::English }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e2h(b: u8, c: u16, v: u16) -> (u16, u16) { let VerseId(_, c, v) = english_to_hebrew(VerseId(b, c, v)); (c, v) }
    fn h2e(b: u8, c: u16, v: u16) -> Option<(u16, u16)> { hebrew_to_english(VerseId(b, c, v)).map(|VerseId(_, c, v)| (c, v)) }

    #[test]
    fn the_known_differences() {
        assert_eq!(e2h(MAL, 4, 1), (3, 19));
        assert_eq!(e2h(MAL, 4, 6), (3, 24));
        assert_eq!(e2h(JOL, 2, 28), (3, 1));
        assert_eq!(e2h(JOL, 3, 1), (4, 1));
        assert_eq!(e2h(PSA, 51, 1), (51, 3));
        assert_eq!(e2h(PSA, 3, 1), (3, 2));
        assert_eq!(e2h(PSA, 23, 1), (23, 1), "no numbered title");
        assert_eq!(e2h(GEN, 31, 55), (32, 1));
        assert_eq!(e2h(GEN, 32, 1), (32, 2));
        assert_eq!(e2h(ISA, 9, 6), (9, 5));
        assert_eq!(e2h(JON, 1, 17), (2, 1));
        assert_eq!(e2h(GEN, 1, 1), (1, 1));
        assert_eq!(e2h(42, 3, 16), (3, 16), "the New Testament is the same");
        assert_eq!(h2e(MAL, 3, 19), Some((4, 1)));
        assert_eq!(h2e(JOL, 4, 21), Some((3, 21)));
        assert_eq!(h2e(PSA, 51, 3), Some((51, 1)));
        assert_eq!(h2e(PSA, 51, 1), None, "the title");
        assert_eq!(h2e(PSA, 51, 2), None);
        assert_eq!(h2e(PSA, 23, 1), Some((23, 1)));
        assert_eq!(h2e(DEU, 28, 69), Some((29, 1)));
        assert_eq!(e2h(NEH, 7, 73), (7, 72));
        assert_eq!(h2e(NUM, 25, 19), Some((26, 1)));
    }

    /// Every shifted English verse goes to the Hebrew and comes back, and no two rules claim the same verse.
    #[test]
    fn every_rule_goes_there_and_back() {
        let mut english = std::collections::BTreeSet::new();
        let mut hebrew = std::collections::BTreeSet::new();
        for r in SHIFTS {
            for v in r.from..=r.to {
                let id = VerseId(r.book, r.chapter, v);
                let there = english_to_hebrew(id);
                assert_eq!(hebrew_to_english(there), Some(id), "{id:?} -> {there:?}");
                assert!(english.insert(id), "two rules for English {id:?}");
                assert!(hebrew.insert(there), "two rules for Hebrew {there:?}");
            }
        }
        for psalm in 1..=150u16 {
            for v in 1..=10u16 {
                if psalm == 13 && v == 6 { continue }
                let id = VerseId(PSA, psalm, v);
                assert_eq!(hebrew_to_english(english_to_hebrew(id)), Some(id), "{id:?}");
            }
        }
    }

    #[test]
    fn convert_only_moves_between_hebrew_and_english() {
        let id = VerseId(MAL, 4, 1);
        assert_eq!(convert(id, Vsys::English, Vsys::Original), Some(VerseId(MAL, 3, 19)));
        assert_eq!(convert(VerseId(MAL, 3, 19), Vsys::Original, Vsys::English), Some(id));
        assert_eq!(convert(VerseId(PSA, 51, 1), Vsys::Original, Vsys::English), None);
        for (a, b) in [(Vsys::English, Vsys::English), (Vsys::Original, Vsys::Original), (Vsys::English, Vsys::Septuagint), (Vsys::Vulgate, Vsys::English)] {
            assert_eq!(convert(id, a, b), Some(id), "{a:?} {b:?}");
        }
    }

    #[test]
    fn a_bible_shows_how_it_is_numbered() {
        let hebrew = |b: u8, c: u16, v: u16| !(b == MAL && c == 4) && !(b == JOL && c == 2 && v > 27);
        let english = |b: u8, c: u16, v: u16| !(b == MAL && c == 3 && v > 18) && !(b == JOL && c == 4);
        assert_eq!(detect(&hebrew), Vsys::Original);
        assert_eq!(detect(&english), Vsys::English);
        assert_eq!(detect(&|b, _, _| b >= 39), Vsys::English, "a New Testament alone");
    }

    /// Downloads the Hebrew Bible (WLC) from eBible and checks the whole table against it and the built-in KJV: every
    /// English Old Testament verse has its Hebrew verse, and every Hebrew verse but a psalm's title comes back to
    /// an English one. `cargo test live_hebrew -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_hebrew_bible_lines_up_with_english() {
        use crate::bible::catalog::{CatalogSource, Ctx};
        let root = std::env::temp_dir().join(format!("rp-test-{}", uuid::Uuid::new_v4()));
        let ctx = Ctx::new(root.clone(), CatalogSource::Ebible);
        let listing = CatalogSource::Ebible.refresh(&ctx).await.expect("the eBible list loads");
        let row = listing.rows.iter().find(|r| r.source_id() == "hboWLC").expect("the Hebrew Bible is listed");
        let meta = CatalogSource::Ebible.install(&ctx, row, &tokio_util::sync::CancellationToken::new(), &|_| {}).await.expect("it downloads");
        assert_eq!((meta.versification, meta.dir), (Vsys::Original, crate::bible::library::Dir::Rtl));
        let wlc = crate::bible::import::get(&root, &meta.id).unwrap();
        let kjv = crate::bible::kjv();
        let (mut missing, mut back, mut titles) = (Vec::new(), Vec::new(), 0);
        for &(b, c, v) in kjv.verses.keys().filter(|k| k.0 < 39) {
            let VerseId(_, hc, hv) = english_to_hebrew(VerseId(b, c, v));
            if !wlc.verses.contains_key(&(b, hc, hv)) { missing.push(format!("{} {c}:{v} -> {hc}:{hv}", crate::bible::display_name(b))) }
        }
        for &(b, c, v) in wlc.verses.keys() {
            match hebrew_to_english(VerseId(b, c, v)) {
                None => titles += 1,
                Some(VerseId(_, ec, ev)) if !kjv.verses.contains_key(&(b, ec, ev)) => back.push(format!("{} {c}:{v} -> {ec}:{ev}", crate::bible::display_name(b))),
                Some(_) => {}
            }
        }
        println!("English verses with no Hebrew verse: {} {:?}", missing.len(), missing);
        println!("Hebrew verses with no English verse: {} {:?}", back.len(), back);
        println!("psalm titles: {titles}");
        let _ = std::fs::remove_dir_all(root);
        assert!(missing.is_empty() && back.is_empty(), "the table is off");
    }
}
