//! One entry per played section of a song's slides: the section bar, the phone chips and key hints (spec §3.1).
use super::{SectionKind, strings};
use crate::model::Slide;
use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Occurrence {
    pub occurrence: u16,
    pub label: String,
    pub kind: SectionKind,
    pub number: Option<u8>,
    pub verse_nth: Option<u8>,
    pub first_slide: usize,
    pub slides: usize,
}

/// One per distinct `SectionRef.occurrence`, in slide order. Empty for anything but a song.
pub fn occurrences(slides: &[Slide]) -> Vec<Occurrence> {
    let mut out: Vec<Occurrence> = Vec::new();
    for (i, s) in slides.iter().enumerate() {
        let Some(r) = &s.section else { continue };
        match out.last_mut() {
            Some(o) if o.occurrence == r.occurrence => o.slides += 1,
            _ => out.push(Occurrence { occurrence: r.occurrence, label: r.label.clone(), kind: r.kind, number: r.number, verse_nth: r.verse_nth, first_slide: i, slides: 1 }),
        }
    }
    out
}

/// True when any verse of the song has a number.
pub fn numbered_verses(occ: &[Occurrence]) -> bool { occ.iter().any(|o| o.kind == SectionKind::Verse && o.number.is_some()) }

/// The key that reaches this occurrence: a verse's digit (its number, or its place among plain verses), else the
/// kind's letter, upper case. None for Verse 10 and up, and for Other.
pub fn key_hint(o: &Occurrence, numbered: bool) -> Option<char> {
    if o.kind == SectionKind::Verse {
        let n = if numbered { o.number } else { o.verse_nth };
        return n.filter(|n| (1..=9).contains(n)).and_then(|n| char::from_digit(u32::from(n), 10));
    }
    o.kind.key().map(|c| c.to_ascii_uppercase())
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct OccurrenceView {
    pub occurrence: u16,
    pub label: String,
    pub short: String,
    pub kind: SectionKind,
    pub color: String,
    pub first_slide: usize,
    pub slides: usize,
    /// The live slide is in this occurrence.
    pub live: bool,
    pub key: Option<char>,
}

pub fn occurrence_views(slides: &[Slide], live_slide: Option<usize>) -> Vec<OccurrenceView> {
    let occ = occurrences(slides);
    let numbered = numbered_verses(&occ);
    occ.iter().map(|o| OccurrenceView {
        occurrence: o.occurrence, label: o.label.clone(), short: o.kind.short(o.number), kind: o.kind, color: o.kind.color(),
        first_slide: o.first_slide, slides: o.slides, live: live_slide.is_some_and(|l| (o.first_slide..o.first_slide + o.slides).contains(&l)),
        key: key_hint(o, numbered),
    }).collect()
}

/// "Chorus · slide 2 of 3" and the kind, for the slide under the live preview; None for a slide not from a song.
pub fn live_section_line(slides: &[Slide], live_slide: usize) -> Option<(String, SectionKind)> {
    let r = slides.get(live_slide)?.section.as_ref()?;
    Some((strings::live_section_line(&r.label, r.page + 1, r.pages), r.kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::refs::kit;

    const SONG: &str = "Verse 1\na\nb\nChorus\nx\ny\nz\nVerse 2\nc\nd";

    #[test]
    fn occurrences_follow_played_order() {
        let o = occurrence_views(&kit::song(SONG, &["Verse 1", "Chorus", "Verse 2", "Chorus"], 2), None);
        assert_eq!(o.iter().map(|o| o.short.as_str()).collect::<Vec<_>>(), ["V1", "C", "V2", "C"]);
        assert_eq!(o.iter().map(|o| o.first_slide).collect::<Vec<_>>(), [0, 1, 3, 4]);
        assert_eq!(o.iter().map(|o| o.slides).collect::<Vec<_>>(), [1, 2, 1, 2]);
    }

    #[test]
    fn non_song_has_no_occurrences() { assert!(occurrences(&kit::plain(&["a", "b"])).is_empty()) }

    #[test]
    fn live_flag_marks_only_the_live_occurrence() {
        let o = occurrence_views(&kit::song(SONG, &["Verse 1", "Chorus", "Verse 2", "Chorus"], 2), Some(2));
        assert_eq!(o.iter().map(|o| o.live).collect::<Vec<_>>(), [false, true, false, false]);
    }

    #[test]
    fn key_hints_use_numbers_or_verse_nth() {
        let o = occurrence_views(&kit::song(SONG, &[], 4), None);
        assert_eq!(o.iter().map(|o| o.key).collect::<Vec<_>>(), [Some('1'), Some('C'), Some('2')]);
        let hymn = occurrence_views(&kit::song("Verse\na\nChorus\nx\nVerse\nb", &[], 4), None);
        assert_eq!(hymn[2].key, Some('2'));
    }

    #[test]
    fn verse_ten_has_no_key_hint() {
        let o = occurrence_views(&kit::song("Verse 1\na\nVerse 10\nb", &[], 4), None);
        assert_eq!((o[1].short.as_str(), o[1].key), ("V10", None));
        assert_eq!(o[0].key, Some('1'));
    }

    #[test]
    fn other_has_no_key_hint() {
        let o = occurrence_views(&kit::song("a\n\nb", &[], 4), None);
        assert_eq!((o[0].short.as_str(), o[0].key, o[0].kind), ("•", None, SectionKind::Other));
    }

    #[test]
    fn live_section_line_text() {
        let s = kit::song(SONG, &[], 1);
        assert_eq!(live_section_line(&s, 3), Some(("Chorus · slide 2 of 3".to_string(), SectionKind::Chorus)));
        assert_eq!(live_section_line(&kit::plain(&["a"]), 0), None);
    }
}
