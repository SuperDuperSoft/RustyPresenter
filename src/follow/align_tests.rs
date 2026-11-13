use super::*;
use crate::follow::lyrics::sound_key;
use crate::follow::testing::SONG;
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

const CHORUS: &str = "my chains are gone\nI've been set free";

/// A song whose chorus is in the plan three times, the last one after a bridge.
fn chorus_song() -> SongText {
    song(&["amazing grace how sweet the sound", CHORUS, "the earth shall soon dissolve like snow", CHORUS, "when we've been there ten thousand years",
        "through many dangers toils and snares", CHORUS])
}

/// `chorus_song`'s slides 0 to 4 as they are planned.
const TO_THE_BRIDGE: &str = "amazing grace how sweet the sound my chains are gone I've been set free the earth shall soon dissolve like snow \
    my chains are gone I've been set free when we've been there ten thousand years";

#[test]
fn jump_into_a_repeated_chorus_is_sure() {
    let s = chorus_song();
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    let p = sing(&mut a, TO_THE_BRIDGE, &mut t);
    assert_eq!((p.plan_index, p.off_script), (4, false));
    // Off plan: back to the chorus, skipping the next slide.
    let p = sing(&mut a, "my chains are", &mut t);
    assert_eq!(p.plan_index, 4, "three words stay");
    let p = sing(&mut a, "gone", &mut t);
    assert_eq!((p.plan_index, p.off_script), (6, false), "the copy next in the plan");
    assert!(p.confidence >= 0.6, "the other copies of the chorus are not rivals: {p:?}");
}

#[test]
fn twin_copy_follows_what_is_sung_next() {
    let s = chorus_song();
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, TO_THE_BRIDGE, &mut t);
    let p = sing(&mut a, "my chains are gone I've been set free", &mut t);
    assert_eq!((p.plan_index, p.word), (6, Some(index_of(&s, 6, "free"))));
    // What follows the first chorus is sung: it was that copy after all.
    let p = sing(&mut a, "the earth", &mut t);
    assert_eq!((p.plan_index, p.word, p.off_script), (2, Some(index_of(&s, 2, "earth")), false));
}

#[test]
fn shared_first_line_stays_unsure_until_the_words_differ() {
    let s = song(&["my hope is built on nothing less\nthan jesus blood and righteousness", "amazing grace how sweet the sound",
        "my hope is built on nothing less\non christ the solid rock I stand"]);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    a.reset_to(1);
    let p = sing(&mut a, "my hope is built", &mut t);
    assert!(p.confidence < 0.6, "two slides start with these words and they are not twins: {p:?}");
    let p = sing(&mut a, "on nothing less on christ the", &mut t);
    assert_eq!(p.plan_index, 2);
    assert!(p.confidence >= 0.6, "{p:?}");
}

#[test]
fn an_operator_seat_finds_the_singers_mid_slide() {
    let s = song(&SONG);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    a.reset_to(1);
    a.seat_anywhere();
    let p = sing(&mut a, "blind", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(index_of(&s, 1, "blind")), 1, false));
    let p = sing(&mut a, "but now I see", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(s.slides[1].end_word - 1), 1));
}

#[test]
fn without_the_seat_a_mid_slide_word_is_not_believed() {
    let s = song(&SONG);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    a.reset_to(1);
    let p = sing(&mut a, "blind", &mut t);
    assert_eq!((p.word, p.plan_index), (None, 1), "as at a song's start");
    // The seat ends at the next move.
    a.seat_anywhere();
    a.reset_to(1);
    assert_eq!(sing(&mut a, "blind", &mut t).word, None);
}

#[test]
fn a_seat_never_leaves_the_operators_slide() {
    let s = song(&SONG);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    a.reset_to(1);
    a.seat_anywhere();
    let p = sing(&mut a, "dangers", &mut t);
    assert_eq!((p.word, p.plan_index), (None, 1), "a word of the next slide only");
}

/// A refrain whose last line repeats its first two ("It Is Well"), between two slides that share no word with it.
fn well_song() -> SongText {
    song(&["amazing grace how sweet the sound", "it is well\nwith my soul\nit is well it is well with my soul", "through many dangers toils and snares"])
}

#[test]
fn a_seat_prefers_the_slides_opening_to_its_last_line() {
    // The operator puts the refrain up as the singers start it, and the ear misses "it is".
    let s = well_song();
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    a.reset_to(1);
    a.seat_anywhere();
    let p = sing(&mut a, "well", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 1, "well")), 1), "line 1, not the same word on the last line");
    let p = sing(&mut a, "with my soul", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 1, "soul")), 1), "line 2");
    assert!(p.word.unwrap() < s.slides[1].last_line_first_word, "the last line is still to be sung: {p:?}");
}

#[test]
fn equal_seats_take_the_earliest_line() {
    // "with my soul" ends line 2 and the last line. Heard first, it is taken as line 2: late at worst, never the
    // next slide while the refrain's last line is still to come.
    let s = well_song();
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    a.reset_to(1);
    a.seat_anywhere();
    let p = sing(&mut a, "with", &mut t);
    assert_eq!(p.word, Some(index_of(&s, 1, "with")));
    let p = sing(&mut a, "my soul", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 1, "soul")), 1));
    assert!(p.word.unwrap() < s.slides[1].last_line_first_word, "{p:?}");
    // It was the last line after all: the next slide's first word says so.
    let p = sing(&mut a, "through", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(index_of(&s, 2, "through")), 2, false));
    assert!(p.confidence >= 0.6, "{p:?}");
}

#[test]
fn a_seat_is_not_sure_until_it_is_proved_like_a_jump() {
    let s = song(&SONG);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    a.reset_to(1);
    a.seat_anywhere();
    // A word in mid-slide places the singers at once, but one or two words prove nothing yet.
    for w in ["blind", "but", "now", "I"] {
        let p = sing(&mut a, w, &mut t);
        assert_eq!((p.plan_index, p.off_script), (1, false));
        assert_eq!(p.confidence, 0.0, "{w}: {p:?}");
    }
    let p = sing(&mut a, "see", &mut t);
    assert_eq!(p.word, Some(s.slides[1].end_word - 1));
    assert!(p.confidence >= 0.6, "four exact words in a row (\"I\" is too short to count): {p:?}");
    // The slide's opening needs no such proof.
    a.reset_to(1);
    a.seat_anywhere();
    let p = sing(&mut a, "I once", &mut t);
    assert_eq!(p.word, Some(index_of(&s, 1, "once")));
    assert!(p.confidence >= 0.6, "{p:?}");
    // A laxer jump setting asks for fewer words.
    let mut lax = Aligner::new(&s, &plan(&s)).with_jump_words(2);
    lax.reset_to(1);
    lax.seat_anywhere();
    assert_eq!(sing(&mut lax, "blind", &mut t).confidence, 0.0);
    assert!(sing(&mut lax, "but", &mut t).confidence >= 0.6);
}

#[test]
fn twin_copies_with_different_next_slides_are_rivals_on_the_last_line() {
    let s = chorus_song();
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, TO_THE_BRIDGE, &mut t);
    // Off plan into the chorus: its own words go up at once, whichever copy it is.
    let p = sing(&mut a, "my chains are gone", &mut t);
    assert_eq!(p.plan_index, 6);
    assert!(p.confidence >= 0.6, "{p:?}");
    // On its last line the next slide would go up. One copy ends the song, the others go on to a verse:
    // which it is, is not known yet.
    for w in ["I've", "been", "set", "free"] {
        let p = sing(&mut a, w, &mut t);
        assert_eq!((p.plan_index, p.off_script), (6, false));
        assert!(p.confidence < 0.6, "{w}: {p:?}");
    }
    // The words that follow tell the copies apart.
    let p = sing(&mut a, "the earth", &mut t);
    assert_eq!((p.plan_index, p.word), (2, Some(index_of(&s, 2, "earth"))));
    assert!(p.confidence >= 0.6, "{p:?}");
}

#[test]
fn twin_copies_followed_by_the_same_words_are_not_rivals() {
    // Pre-chorus and chorus are in the plan twice; the first chorus goes on to verse 2, the second to the bridge.
    let pre = "was blind but now\nonce lost then found";
    let s = song(&["amazing grace how sweet the sound", pre, CHORUS, "the earth shall soon dissolve like snow", pre, CHORUS,
        "through many dangers toils and snares"]);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    let p = sing(&mut a, "amazing grace how sweet the sound was blind but now once lost then found my chains are gone I've been set free", &mut t);
    assert_eq!((p.plan_index, p.off_script), (2, false));
    // Off plan: the leader goes back to the pre-chorus. Either copy is followed by the chorus, so the place
    // stays sure on its last line too and the chorus goes up in time.
    let p = sing(&mut a, "was blind but now", &mut t);
    assert_eq!(p.plan_index, 4, "the copy next in the plan");
    assert!(p.confidence >= 0.6, "{p:?}");
    let p = sing(&mut a, "once lost then found", &mut t);
    assert_eq!(p.plan_index, 4);
    assert!(p.confidence >= 0.6, "both copies go on to the chorus: {p:?}");
    let p = sing(&mut a, "my chains are gone", &mut t);
    assert_eq!(p.plan_index, 5);
    assert!(p.confidence >= 0.6, "{p:?}");
    // The chorus's last line: the bridge or verse 2 next? Not known until one of them is sung.
    let p = sing(&mut a, "I've been set free", &mut t);
    assert_eq!(p.plan_index, 5);
    assert!(p.confidence < 0.6, "the copies go on to different slides: {p:?}");
    let p = sing(&mut a, "the earth", &mut t);
    assert_eq!((p.plan_index, p.word, p.off_script), (3, Some(index_of(&s, 3, "earth")), false));
    assert!(p.confidence >= 0.6, "{p:?}");
}

#[test]
fn a_is_stepped_over() {
    // The phoneme ear never hears "a" (one sound): the aligner steps over it.
    let s = song(&GRACE);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    let p = sing(&mut a, "amazing grace how sweet the sound that saved wretch like", &mut t);
    assert_eq!((p.word, p.off_script), (Some(index_of(&s, 1, "like")), false));
}
