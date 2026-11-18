use super::*;
use crate::model::{Slide, SlideContent};
use crate::sections::SectionKind;

fn text(t: &str) -> Slide { Slide::new("", SlideContent::Text { text: t.into(), footer: None, reference: None, size: 0, parallel: None }) }

fn tagged(tag: &str, t: &str) -> Slide {
    let mut s = text(t);
    s.tag = Some(tag.into());
    s
}

fn in_section(label: &str, t: &str) -> Slide {
    let mut s = text(t);
    s.section = Some(crate::sections::SectionRef { label: label.into(), kind: SectionKind::from_label(label).0, number: None, verse_nth: None,
        occurrence: 0, ordinal: 0, page: 0, pages: 1 });
    s
}

#[test]
fn words_carry_slide_and_line() {
    let song = SongText::from_slides(&[in_section("Verse 1", "Amazing grace, how sweet\nthe sound"), in_section("Chorus", "My chains are gone")]);
    let w: Vec<(&str, usize, usize)> = song.words.iter().map(|w| (w.text.as_str(), w.slide, w.line)).collect();
    assert_eq!(w, [("Amazing", 0, 0), ("grace", 0, 0), ("how", 0, 0), ("sweet", 0, 0), ("the", 0, 1), ("sound", 0, 1),
        ("My", 1, 0), ("chains", 1, 0), ("are", 1, 0), ("gone", 1, 0)]);
    assert_eq!(song.slides.len(), 2);
    let s0 = &song.slides[0];
    assert_eq!((s0.slide, s0.first_word, s0.last_line_first_word, s0.end_word), (0, 0, 4, 6));
    assert_eq!(s0.section.as_deref(), Some("Verse 1"));
    assert!(!s0.instrumental);
    let s1 = &song.slides[1];
    assert_eq!((s1.first_word, s1.last_line_first_word, s1.end_word), (6, 6, 10));
    assert_eq!(song.words[1].key, sound_key("grace"));
}

#[test]
fn sound_key_matches_alikes() {
    assert_eq!(sound_key("grace"), sound_key("grays"));
    assert_eq!(sound_key("night"), sound_key("knight"));
    assert_eq!(sound_key("Grace,"), sound_key("grace"));
    assert_eq!(sound_key("née"), sound_key("nee"));
    assert_ne!(sound_key("grace"), sound_key("sound"));
    assert_ne!(sound_key("chains"), sound_key("gone"));
    assert_eq!(sound_key("Иисус"), "иисус", "non-Latin words keep their lowercase form");
}

#[test]
fn syllables_counts() {
    assert_eq!(syllables("amazing"), 3);
    assert_eq!(syllables("grace"), 1);
    assert_eq!(syllables("hallelujah"), 4);
    assert_eq!(syllables("the"), 1);
    assert_eq!(syllables("I"), 1);
    assert_eq!(syllables("glorious"), 3);
}

#[test]
fn instrumental_tag_slides_have_no_words() {
    let song = SongText::from_slides(&[text("Amazing grace"), tagged("Instrumental", ""), text("{Instrumental}"), text("how sweet")]);
    assert_eq!(song.words.len(), 4);
    assert!(song.slides[1].instrumental && song.slides[2].instrumental);
    assert_eq!((song.slides[1].first_word, song.slides[1].end_word), (2, 2));
    assert!(!song.slides[0].instrumental && !song.slides[3].instrumental);
    assert_eq!(song.words[2].slide, 3);
}

#[test]
fn odd_songs_never_panic() {
    assert!(SongText::from_slides(&[]).words.is_empty());
    let one = SongText::from_slides(&[text("one big slide\nwith lines\n\nand gaps")]);
    assert_eq!(one.words.len(), 7);
    assert_eq!(one.slides[0].last_line_first_word, 5);
    let tags = SongText::from_slides(&[tagged("Instrumental", ""), text("{Intro}")]);
    assert!(tags.words.is_empty() && tags.slides.iter().all(|s| s.instrumental));
    let odd = SongText::from_slides(&[text("!!! — 🙏🙏 … ¿?"), text("(x2)"), text("")]);
    assert!(odd.words.is_empty());
    let picture = Slide::new("", SlideContent::Still { asset: "a.png".into() });
    assert!(SongText::from_slides(&[picture]).slides[0].instrumental);
    let cjk = SongText::from_slides(&[text("奇异恩典"), text("Святый Боже")]);
    assert_eq!(cjk.words.len(), 6, "each Han character is a word; Cyrillic splits on spaces");
    assert!(cjk.words.iter().all(|w| !w.key.is_empty() && w.syllables >= 1));
    for w in ["", "'", "x", "🙏", "ʼ", "ß", "æ", "ghost", "tchotchke", "schism"] { let _ = (sound_key(w), syllables(w)); }
}
