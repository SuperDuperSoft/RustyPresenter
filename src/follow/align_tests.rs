use super::*;
use crate::follow::lyrics::sound_key;
use crate::model::{Slide, SlideContent};

fn song(texts: &[&str]) -> SongText {
    let slides: Vec<Slide> = texts.iter().map(|t| Slide::new("", SlideContent::Text { text: (*t).into(), footer: None, reference: None, size: 0, parallel: None })).collect();
    SongText::from_slides(&slides)
}

fn plan(song: &SongText) -> Vec<usize> { (0..song.slides.len()).collect() }

/// Sings `words` (space-separated), a third of a second apart, from `at`; the last place.
fn sing(a: &mut Aligner, words: &str, at: &mut f64) -> Position {
    let mut last = a.position().clone();
    for w in words.split_whitespace() {
        *at += 0.33;
        last = a.push(&Heard { key: sound_key(w), at: *at, conf: 1.0 });
    }
    last
}

fn index_of(s: &SongText, slide: usize, text: &str) -> usize { s.words.iter().position(|w| w.slide == slide && w.text == text).unwrap() }

const GRACE: [&str; 3] = ["amazing grace how sweet the sound", "that saved a wretch like me", "through many dangers toils and snares"];

#[test]
fn follows_forward_in_plan() {
    let s = song(&GRACE);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    assert_eq!(a.position().word, None);
    let p = sing(&mut a, "amazing grace how", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(2), 0));
    let p = sing(&mut a, "sweet the sound that saved a", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(index_of(&s, 1, "a")), 1, false));
    assert!(p.confidence > 0.5, "{p:?}");
    let p = sing(&mut a, "wretch like me through many", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 2, "many")), 2));
}

#[test]
fn one_matching_word_never_jumps() {
    let s = song(&GRACE);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace", &mut t);
    let p = sing(&mut a, "dangers", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(1), 0), "one word from slide 3 is not enough");
    let p = sing(&mut a, "how sweet", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(3), 0, false));
}

#[test]
fn out_of_plan_jump_needs_four_words() {
    let s = song(&GRACE);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace", &mut t);
    let p = sing(&mut a, "through many dangers", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(1), 0), "three words stay");
    let p = sing(&mut a, "toils", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 2, "toils")), 2), "the fourth word jumps");
    let p = sing(&mut a, "and snares", &mut t);
    assert_eq!((p.plan_index, p.off_script), (2, false));
    // A stricter setting needs more.
    let mut strict = Aligner::new(&s, &plan(&s)).with_jump_words(6);
    let mut t = 0.0;
    sing(&mut strict, "amazing grace", &mut t);
    assert_eq!(sing(&mut strict, "through many dangers toils", &mut t).plan_index, 0);
}

#[test]
fn repeated_chorus_follows_plan_order() {
    let chorus = "my chains are gone\nI've been set free";
    let s = song(&["amazing grace how sweet the sound", chorus, "the earth shall soon dissolve like snow", chorus, "when we've been there ten thousand years"]);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace how sweet the sound", &mut t);
    let p = sing(&mut a, "my chains are gone I've been set free", &mut t);
    assert_eq!(p.plan_index, 1);
    let p = sing(&mut a, "the earth shall soon dissolve like snow", &mut t);
    assert_eq!(p.plan_index, 2);
    for (words, want) in [("my", 3), ("my chains are gone", 3), ("my chains are gone I've been set free", 3)] {
        let mut b = Aligner::new(&s, &plan(&s));
        let mut t = 0.0;
        sing(&mut b, "amazing grace how sweet the sound my chains are gone I've been set free the earth shall soon dissolve like snow", &mut t);
        let p = sing(&mut b, words, &mut t);
        assert_eq!(p.plan_index, want, "{words}: the next chorus, not the first");
        assert!(!p.off_script);
    }
    // Sung again after the last chorus is planned, the repeat is still the last chorus (a repeated line).
    let p = sing(&mut a, "my chains are gone I've been set free", &mut t);
    assert_eq!(p.plan_index, 3);
    let p = sing(&mut a, "I've been set free", &mut t);
    assert_eq!((p.plan_index, p.word), (3, Some(index_of(&s, 3, "free"))));
}

#[test]
fn off_script_talk_reports_off_script() {
    let s = song(&GRACE);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace how", &mut t);
    let p = sing(&mut a, "please everybody join us welcome friends today morning church family", &mut t);
    assert!(p.off_script, "{p:?}");
    assert_eq!((p.word, p.plan_index), (Some(2), 0), "the place is unchanged");
    assert_eq!(p.confidence, 0.0);
    // Singing again picks up where it was.
    let p = sing(&mut a, "sweet the sound", &mut t);
    assert_eq!((p.word, p.off_script), (Some(5), false));
}

#[test]
fn misheard_words_tolerated() {
    let s = song(&GRACE);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    let all: Vec<&str> = GRACE.iter().flat_map(|l| l.split_whitespace()).collect();
    let mut p = a.position().clone();
    for (i, w) in all.iter().enumerate() {
        let w = if i % 4 == 3 { "zzzq" } else { w };
        p = sing(&mut a, w, &mut t);
    }
    assert_eq!(p.plan_index, 2);
    assert!(p.word >= Some(all.len() - 2), "{p:?}");
    assert!(!p.off_script);
}

#[test]
fn repeat_last_line_stays() {
    let s = song(&["amazing grace how sweet\nthe sound that saved", "a wretch like me"]);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace how sweet the sound that saved", &mut t);
    let p = sing(&mut a, "the sound that saved", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(7), 0, false));
    let p = sing(&mut a, "sound that saved", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(7), 0), "a repeat without its first word too");
    let p = sing(&mut a, "a wretch", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(9), 1));
}

#[test]
fn reset_to_starts_from_the_operators_slide() {
    let s = song(&GRACE);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace", &mut t);
    a.reset_to(2);
    assert_eq!((a.position().word, a.position().plan_index), (None, 2));
    let p = sing(&mut a, "through many", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 2, "many")), 2));
}

#[test]
fn empty_song_stays_put() {
    let s = song(&["", "{Instrumental}"]);
    let mut a = Aligner::new(&s, &plan(&s));
    let p = a.push(&Heard { key: sound_key("grace"), at: 1.0, conf: 1.0 });
    assert_eq!((p.word, p.confidence), (None, 0.0));
    let mut none = Aligner::new(&s, &[]);
    none.reset_to(5);
    assert_eq!(none.push(&Heard { key: String::new(), at: 1.0, conf: 0.0 }).word, None);
}
