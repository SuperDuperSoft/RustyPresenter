//! The slide → words mapping and every edit made from a slide.
use super::*;
use crate::model::{Settings, Slide, Song};

const SONG: &str = "Verse 1
[G]Amazing [C]grace how sweet
That saved a wretch

Like me

Chorus
[D]My chains are gone
I've been set free

Verse 2
'Twas grace that taught";

fn settings(lps: u8) -> Settings { Settings { credits: "none".into(), lines_per_slide: lps, ..Default::default() } }

fn song(lyrics: &str, order: &[&str]) -> Song {
    Song { lyrics: lyrics.into(), arrangement: order.iter().map(|s| s.to_string()).collect(), ..Default::default() }
}

fn slides(lyrics: &str, order: &[&str], lps: u8) -> Vec<Slide> { crate::slides::song_slides(&song(lyrics, order), &settings(lps)) }

/// Slide `i` of the song as it plays, found in its words.
fn at(lyrics: &str, order: &[&str], lps: u8, i: usize) -> (Doc, At) {
    let d = Doc::read(lyrics);
    let s = slides(lyrics, order, lps);
    let order: Vec<String> = order.iter().map(|s| s.to_string()).collect();
    let a = d.find(&order, s[i].section.as_ref().unwrap(), usize::from(lps)).unwrap();
    (d, a)
}

fn texts(lyrics: &str, order: &[&str], lps: u8) -> Vec<String> {
    slides(lyrics, order, lps).iter().map(|s| match &s.content { crate::model::SlideContent::Text { text, .. } => text.clone(), _ => String::new() }).collect()
}

fn v(s: &[&str]) -> Vec<String> { s.iter().map(|x| x.to_string()).collect() }

#[test]
fn every_slide_finds_its_own_lines() {
    for lps in [1u8, 2, 4] {
        let s = slides(SONG, &[], lps);
        let d = Doc::read(SONG);
        for (i, slide) in s.iter().enumerate() {
            let a = d.find(&[], slide.section.as_ref().unwrap(), usize::from(lps)).unwrap();
            let words = crate::slides::strip_chords(&d.slide_lines(&a).join("\n")).unwrap_or_else(|| d.slide_lines(&a).join("\n"));
            assert_eq!(words, texts(SONG, &[], lps)[i], "slide {i} at {lps} lines");
        }
    }
    assert_eq!(Doc::read(SONG).text(), SONG, "read and written back unchanged");
    let crlf = SONG.replace('\n', "\r\n");
    assert_eq!(Doc::read(&crlf).text(), crlf, "Windows line endings kept");
}

#[test]
fn editing_a_slide_changes_only_its_lines() {
    let (d, a) = at(SONG, &[], 4, 2);
    assert_eq!(d.slide_lines(&a), v(&["[D]My chains are gone", "I've been set free"]));
    let c = d.set_lines(&a, &v(&["[D]My chains are gone", "I've been set free!"])).unwrap();
    assert_eq!(c.lyrics, SONG.replace("set free\n", "set free!\n"));
    assert!(c.fixes.is_empty());
    // A blank line typed in the slide starts another slide.
    let c = d.set_lines(&a, &v(&["one", "", "two"])).unwrap();
    assert_eq!(texts(&c.lyrics, &[], 4), ["Amazing grace how sweet\nThat saved a wretch", "Like me", "one", "two", "'Twas grace that taught"]);
}

#[test]
fn a_repeated_part_changes_everywhere_it_plays() {
    let order = ["Verse 1", "Chorus", "Verse 2", "Chorus"];
    let (d, a) = at(SONG, &order, 4, 4);
    assert_eq!(d.plays(&v(&order), a.section), 2);
    let c = d.set_lines(&a, &v(&["Free!"])).unwrap();
    let t = texts(&c.lyrics, &order, 4);
    assert_eq!((t[2].as_str(), t[4].as_str()), ("Free!", "Free!"));
}

#[test]
fn split_join_add_delete_and_duplicate() {
    let (d, a) = at(SONG, &[], 4, 0);
    // Split before the second line.
    let c = d.split(&a, 1).unwrap();
    assert_eq!(texts(&c.lyrics, &[], 4)[..3], ["Amazing grace how sweet".to_string(), "That saved a wretch".into(), "Like me".into()]);
    assert_eq!(d.split(&a, 0), Err(Refused::NoSplit));
    // Join the first slide with the next one of Verse 1.
    let c = d.join_next(&a, 4).unwrap();
    assert_eq!(texts(&c.lyrics, &[], 4)[0], "Amazing grace how sweet\nThat saved a wretch\nLike me");
    let (d2, last) = at(SONG, &[], 4, 1);
    assert_eq!(d2.join_next(&last, 4), Err(Refused::LastOfPart));
    // Two lines a slide: the slides of one group cannot be joined in the words.
    let (d3, a3) = at(SONG, &[], 1, 0);
    assert_eq!(d3.join_next(&a3, 1), Err(Refused::SameGroup { lines: 1 }));
    // Add a slide after the chorus's slide.
    let (d, a) = at(SONG, &[], 4, 2);
    let c = d.add_after(&a, &v(&["New words"])).unwrap();
    assert_eq!(texts(&c.lyrics, &[], 4)[2..4], ["My chains are gone\nI've been set free".to_string(), "New words".into()]);
    assert!(c.lyrics.contains("set free\n\nNew words\n\nVerse 2"), "{}", c.lyrics);
    // Duplicate it: a copy right after, chords and all.
    let c = d.duplicate(&a).unwrap();
    assert!(c.lyrics.contains("set free\n\n[D]My chains are gone\nI've been set free\n\nVerse 2"), "{}", c.lyrics);
    // Delete Verse 1's second slide.
    let (d, a) = at(SONG, &[], 4, 1);
    let c = d.delete(&a).unwrap();
    assert_eq!(c.lyrics, SONG.replace("\n\nLike me", ""));
    assert!(c.fixes.is_empty());
}

#[test]
fn deleting_the_last_slide_of_a_part_takes_the_part_out_of_the_order() {
    let order = ["Verse 1", "Chorus", "Verse 2", "Chorus"];
    let (d, a) = at(SONG, &order, 4, 3);
    assert_eq!(d.slide_lines(&a), v(&["'Twas grace that taught"]));
    let c = d.delete(&a).unwrap();
    assert!(!c.lyrics.contains("Verse 2"), "{}", c.lyrics);
    assert!(!c.lyrics.ends_with('\n'));
    assert_eq!(c.fixes, vec![OrderFix::Remove { label: "Verse 2".into() }]);
    let mut o = v(&order);
    fix_order(&mut o, &c.fixes);
    assert_eq!(o, v(&["Verse 1", "Chorus", "Chorus"]));
    let only = "Verse 1\nalone";
    let (d, a) = at(only, &[], 4, 0);
    assert_eq!(d.delete(&a), Err(Refused::OnlySlide));
}

#[test]
fn changing_the_section_renames_or_splits_the_part() {
    let order = ["Verse 1", "Chorus", "Verse 1"];
    // On the part's first slide: the part is renamed, and the order follows.
    let (d, a) = at(SONG, &order, 4, 0);
    let c = d.change_section(&a, "Verse 3").unwrap();
    assert!(c.lyrics.starts_with("Verse 3\n[G]Amazing"));
    let mut o = v(&order);
    fix_order(&mut o, &c.fixes);
    assert_eq!(o, v(&["Verse 3", "Chorus", "Verse 3"]));
    // Further in: from this slide on, the part is a Bridge, played after Verse 1 wherever Verse 1 plays.
    let (d, a) = at(SONG, &order, 4, 1);
    let c = d.change_section(&a, "Bridge").unwrap();
    assert!(c.lyrics.contains("That saved a wretch\n\nBridge\nLike me"), "{}", c.lyrics);
    let mut o = v(&order);
    fix_order(&mut o, &c.fixes);
    assert_eq!(o, v(&["Verse 1", "Bridge", "Chorus", "Verse 1", "Bridge"]));
    let s = slides(&c.lyrics, &["Verse 1", "Bridge", "Chorus", "Verse 1", "Bridge"], 4);
    assert_eq!(s.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), ["Verse 1", "Bridge", "Chorus", "Verse 1", "Bridge"]);
    // A name the slides would read as words gets brackets.
    let c = d.change_section(&a, "Hook").unwrap();
    assert!(c.lyrics.contains("\n[Hook]\nLike me"), "{}", c.lyrics);
    assert_eq!(Doc::read(SONG).section_choices(0)[..2], ["Verse 3".to_string(), "Chorus 2".into()]);
}

#[test]
fn chords_stay_when_only_the_words_are_edited() {
    let (d, a) = at(SONG, &[], 4, 0);
    let lines = d.slide_lines(&a);
    assert_eq!(words::words(&lines), "Amazing grace how sweet\nThat saved a wretch");
    let merged = words::merge_words(&lines, "Amazing grace, how sweet\nThat saved a wretch like me");
    assert_eq!(merged, v(&["[G]Amazing [C]grace, how sweet", "That saved a wretch like me"]));
    let c = d.set_lines(&a, &merged).unwrap();
    assert!(c.lyrics.starts_with("Verse 1\n[G]Amazing [C]grace, how sweet\nThat saved a wretch like me\n\nLike me"));
}

#[test]
fn a_slide_of_chords_alone_is_found_and_kept() {
    let lyrics = "Intro\n[G] [D] [Em] [C]\n\nVerse 1\n[G]Amazing grace";
    let (d, a) = at(lyrics, &[], 2, 0);
    assert_eq!(d.slide_lines(&a), v(&["[G] [D] [Em] [C]"]));
    assert_eq!(words::words(&d.slide_lines(&a)), "", "no words to show");
    let c = d.set_lines(&a, &v(&["[G] [D]"])).unwrap();
    assert!(c.lyrics.starts_with("Intro\n[G] [D]\n\nVerse 1"));
    // Editing the next slide's words keeps the intro as it is.
    let (d, a) = at(lyrics, &[], 2, 1);
    let c = d.set_lines(&a, &words::merge_words(&d.slide_lines(&a), "Amazing grace!")).unwrap();
    assert_eq!(c.lyrics, "Intro\n[G] [D] [Em] [C]\n\nVerse 1\n[G]Amazing grace!");
}

#[test]
fn a_labelled_slide_keeps_its_label() {
    let lyrics = "Verse 1\na\n\n{Instrumental}\nb\nc";
    let (d, a) = at(lyrics, &[], 4, 1);
    assert_eq!(a.tag, Some(3));
    assert_eq!(d.slide_lines(&a), v(&["b", "c"]));
    let c = d.set_lines(&a, &v(&["b!"])).unwrap();
    assert_eq!(c.lyrics, "Verse 1\na\n\n{Instrumental}\nb!");
    let c = d.delete(&a).unwrap();
    assert_eq!(c.lyrics, "Verse 1\na", "the label goes with its slide");
}
