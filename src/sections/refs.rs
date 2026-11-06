//! Where a song slide came from (`SectionRef`), and the played order of a song's sections (spec §4.1, §5.1).
use super::SectionKind;
use serde::{Deserialize, Serialize};

/// Built with the slides, never stored in `data.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SectionRef {
    /// As written: "Chorus", "Verse 2".
    pub label: String,
    pub kind: SectionKind,
    /// Verse 2 → 2.
    pub number: Option<u8>,
    /// The nth distinct Verse section in written order (1-based), for hymns with plain "Verse" headings.
    pub verse_nth: Option<u8>,
    /// Index in the played order (0-based).
    pub occurrence: u16,
    /// The nth time this label is played (0 = the first chorus).
    pub ordinal: u16,
    /// Slide index inside this occurrence, counted across the section's blank-line groups.
    pub page: u16,
    pub pages: u16,
}

/// The written sections each entry of `order` plays: entries are trimmed and matched case-insensitively against
/// the first section with that label; unknown entries are skipped; with none left, the written order.
pub fn resolve_order(order: &[String], labels: &[String]) -> Vec<usize> {
    let find = |l: &str| labels.iter().position(|s| s.eq_ignore_ascii_case(l.trim()));
    let played: Vec<usize> = order.iter().filter_map(|l| find(l)).collect();
    if played.is_empty() { (0..labels.len()).collect() } else { played }
}

/// One `SectionRef` per slide, in played order. `sections` holds each written section's label and page count.
pub fn song_refs(sections: &[(&str, usize)], played: &[usize]) -> Vec<SectionRef> {
    let mut verse_nth = vec![None; sections.len()];
    let mut n = 0u8;
    for (i, (label, _)) in sections.iter().enumerate() {
        if SectionKind::from_label(label).0 == SectionKind::Verse {
            n = n.saturating_add(1);
            verse_nth[i] = Some(n);
        }
    }
    let mut refs = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (occurrence, &idx) in played.iter().enumerate() {
        let (label, pages) = sections[idx];
        let lower = label.trim().to_lowercase();
        let ordinal = seen.iter().filter(|l| **l == lower).count() as u16;
        seen.push(lower);
        let (kind, number) = SectionKind::from_label(label);
        for page in 0..pages {
            refs.push(SectionRef {
                label: label.to_string(), kind, number, verse_nth: verse_nth[idx], occurrence: occurrence as u16, ordinal,
                page: page as u16, pages: pages as u16,
            });
        }
    }
    refs
}

#[cfg(test)]
pub(crate) mod kit {
    use crate::model::{Settings, Slide, SlideContent, Song};

    /// A song's slides as the app builds them, with no credit line.
    pub fn song(lyrics: &str, order: &[&str], lps: u8) -> Vec<Slide> {
        let song = Song { lyrics: lyrics.into(), arrangement: order.iter().map(|s| s.to_string()).collect(), ..Default::default() };
        crate::slides::song_slides(&song, &Settings { credits: "none".into(), lines_per_slide: lps, ..Default::default() })
    }

    /// Slides that are not from a song.
    pub fn plain(texts: &[&str]) -> Vec<Slide> {
        texts.iter().map(|t| Slide::new(*t, SlideContent::Text { text: t.to_string(), footer: None, reference: None, size: 0, parallel: None })).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SONG: &str = "Verse 1\na\nb\nChorus\nx\ny\nz\nVerse 2\nc\nd";

    fn refs(s: &[crate::model::Slide]) -> Vec<SectionRef> { s.iter().map(|s| s.section.clone().unwrap()).collect() }

    #[test]
    fn song_slides_tag_section_ref() {
        // V1 C V2 C, two lines per slide: the chorus has 2 pages.
        let r = refs(&kit::song(SONG, &["Verse 1", "Chorus", "Verse 2", "Chorus"], 2));
        assert_eq!(r.len(), 6);
        assert_eq!((r[1].label.as_str(), r[1].occurrence, r[1].ordinal, r[1].page, r[1].pages), ("Chorus", 1, 0, 0, 2));
        assert_eq!((r[2].page, r[2].pages), (1, 2));
        assert_eq!((r[3].label.as_str(), r[3].occurrence, r[3].number), ("Verse 2", 2, Some(2)));
        assert_eq!((r[4].occurrence, r[4].ordinal), (3, 1));
    }

    #[test]
    fn repeated_chorus_gets_distinct_ordinals() {
        let r = refs(&kit::song(SONG, &["Chorus", "Verse 1", "Chorus", "Chorus"], 4));
        let chorus: Vec<_> = r.iter().filter(|r| r.kind == SectionKind::Chorus).map(|r| r.ordinal).collect();
        assert_eq!(chorus, [0, 1, 2]);
    }

    #[test]
    fn unnumbered_verses_get_verse_nth() {
        let r = refs(&kit::song("Verse\na\nChorus\nx\nVerse\nb", &[], 4));
        assert_eq!(r.iter().map(|r| r.verse_nth).collect::<Vec<_>>(), [Some(1), None, Some(2)]);
    }

    #[test]
    fn duplicate_plain_verse_headings_get_distinct_verse_nth() {
        let r = refs(&kit::song("Verse\na\nVerse\nb\nVerse\nc\nVerse\nd", &[], 4));
        assert_eq!(r.iter().map(|r| r.verse_nth).collect::<Vec<_>>(), [Some(1), Some(2), Some(3), Some(4)]);
        assert_eq!(r.iter().map(|r| r.ordinal).collect::<Vec<_>>(), [0, 1, 2, 3]);
    }

    #[test]
    fn section_refs_count_pages_across_groups() {
        // Verse 1 has two blank-line groups, [a b c] and [d]; 2 lines per slide → pages a/b, c, d.
        let s = kit::song("Verse 1\na\nb\nc\n\nd", &[], 2);
        assert_eq!(refs(&s).iter().map(|r| (r.page, r.pages)).collect::<Vec<_>>(), [(0, 3), (1, 3), (2, 3)]);
        assert_eq!(s.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["Verse 1 (1/2)", "Verse 1 (2/2)", "Verse 1"], "labels unchanged");
    }

    #[test]
    fn order_entries_match_trimmed_case_insensitive() {
        let labels = vec!["Verse 1".to_string(), "Chorus".to_string()];
        assert_eq!(resolve_order(&[" chorus ".into(), "VERSE 1".into(), "Nope".into()], &labels), [1, 0]);
        assert_eq!(resolve_order(&["Nope".into()], &labels), [0, 1]);
    }

    #[test]
    fn section_ref_json_round_trip() {
        let r = kit::song(SONG, &[], 4)[1].section.clone().unwrap();
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["kind"], "chorus");
        assert_eq!(serde_json::from_value::<SectionRef>(j).unwrap(), r);
    }

    #[test]
    fn non_song_slides_have_no_section() {
        assert!(kit::plain(&["a"])[0].section.is_none());
    }
}
