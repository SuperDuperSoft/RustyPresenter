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

#[test]
fn only_a_word_the_phoneme_ear_never_delivers_is_stepped_over() {
    assert!(undelivered("a") && undelivered("A"), "one neutral sound");
    // Two sounds or more, a sung "Ah", and words with no phonemes at all (another script, nothing).
    for w in ["I", "the", "oh", "ah", "grace", "主", ""] { assert!(!undelivered(w), "{w:?}") }
}

#[test]
fn a_jump_being_confirmed_steps_over_a_word_the_ear_never_delivers() {
    let s = song(&GRACE);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace", &mut t);
    // Off plan to slide 2, as the phoneme ear hears it: "a" never arrives. The jump must not end there.
    let p = sing(&mut a, "that saved wretch", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(1), 0), "three words heard stay: the step over \"a\" is no proof");
    let p = sing(&mut a, "like", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(index_of(&s, 1, "like")), 1, false), "the fourth word heard jumps");
}

#[test]
fn a_jump_survives_a_word_the_ear_missed() {
    // The leader goes to slide 3 and the ear misses "many". The count does not start again (it did, and with an
    // ear that misses one sung word in three a different slide was hardly ever noticed): four words heard, in
    // order, are the proof.
    let s = song(&GRACE);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace", &mut t);
    let p = sing(&mut a, "through dangers toils", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(1), 0), "three words heard stay");
    let p = sing(&mut a, "and", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(index_of(&s, 2, "and")), 2, false), "the fourth word heard jumps");
    assert!(p.confidence >= 0.6, "{p:?}");
}

#[test]
fn a_jump_survives_a_misheard_word() {
    let s = song(&GRACE);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace", &mut t);
    // "dangers" is heard as something else; the words around it still prove the jump.
    let p = sing(&mut a, "through many zzzq toils", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(1), 0), "three good words stay");
    let p = sing(&mut a, "and", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(index_of(&s, 2, "and")), 2, false));
    assert!(p.confidence >= 0.6, "{p:?}");
    // A missed word and a misheard one: still four good words among six.
    let mut b = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut b, "amazing grace", &mut t);
    let p = sing(&mut b, "through dangers zzzq toils", &mut t);
    assert_eq!(p.plan_index, 0, "three good words stay: {p:?}");
    let p = sing(&mut b, "and", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 2, "and")), 2));
}

/// Amazing Grace, a verse a slide.
const HYMN: [&str; 5] = [
    "amazing grace how sweet the sound\nthat saved a wretch like me\nI once was lost but now am found\nwas blind but now I see",
    "twas grace that taught my heart to fear\nand grace my fears relieved\nhow precious did that grace appear\nthe hour I first believed",
    "through many dangers toils and snares\nI have already come\ntis grace hath brought me safe thus far\nand grace will lead me home",
    "the Lord has promised good to me\nhis word my hope secures\nhe will my shield and portion be\nas long as life endures",
    "when we've been there ten thousand years\nbright shining as the sun\nwe've no less days to sing God's praise\nthan when we first begun",
];

/// The next number of a fixed, repeatable sequence, 0 to 1 (xorshift).
fn roll(seed: &mut u64) -> f64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

/// The singers are three words into verse 1 when the leader goes to another verse and sings it through. The ear
/// loses `lost` of the words: half of those never arrive, half arrive as some other word. Over 300 such tries:
/// how many times the aligner was sure of the new slide by the end of its second line, and by its end.
fn noticed(lost: f64) -> (usize, usize, usize) {
    let s = song(&HYMN);
    let (mut two_lines, mut by_end, tries) = (0, 0, 300);
    for n in 0..tries {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(n as u64 + 1);
        let target = 1 + n % 4;
        let mut a = Aligner::new(&s, &plan(&s));
        let mut t = 0.0;
        sing(&mut a, "amazing grace how", &mut t);
        let span = &s.slides[target];
        let line_3 = span.words().find(|&w| s.words[w].line == 2).unwrap();
        let mut at = None;
        for w in span.words() {
            let x = roll(&mut seed);
            if x < lost / 2.0 { continue }
            let word = if x < lost { "zzzq" } else { s.words[w].text.as_str() };
            let p = sing(&mut a, word, &mut t);
            if at.is_none() && p.plan_index == target && p.confidence >= 0.6 && !p.off_script { at = Some(w) }
        }
        if at.is_some_and(|w| w < line_3) { two_lines += 1 }
        if at.is_some() { by_end += 1 }
    }
    (two_lines, by_end, tries)
}

#[test]
fn a_different_slide_is_noticed_by_an_ear_that_loses_words() {
    // An ear that hears everything: always, within two lines.
    assert_eq!(noticed(0.0), (300, 300, 300));
    // One word in four lost: 278 times within two lines, every time by the slide's end. (Four exact words one
    // right after the other, as the proof was: 234 and 294.)
    let (two_lines, by_end, tries) = noticed(0.25);
    assert!(two_lines * 100 >= tries * 90 && by_end * 100 >= tries * 99, "{two_lines} within two lines, {by_end} by the end, of {tries}");
    // One word in three lost: 255 and 293. (As it was: 190 and 273.)
    let (two_lines, by_end, tries) = noticed(0.33);
    assert!(two_lines * 100 >= tries * 82 && by_end * 100 >= tries * 95, "{two_lines} within two lines, {by_end} by the end, of {tries}");
}

#[test]
fn talk_with_the_songs_words_in_it_never_jumps() {
    // Three words into verse 1 someone talks for a hundred words, and every third word they say is a word of
    // the song, picked from anywhere in it. Scattered words are no place in the song: over 300 such talks the
    // aligner is never sure of another slide.
    let s = song(&HYMN);
    for n in 0..300u64 {
        let mut seed = 0xD1B5_4A32_D192_ED03u64.wrapping_mul(n + 1);
        let mut a = Aligner::new(&s, &plan(&s));
        let mut t = 0.0;
        sing(&mut a, "amazing grace how", &mut t);
        for i in 0..100 {
            let x = roll(&mut seed);
            let word = if i % 3 == 0 { s.words[(x * s.words.len() as f64) as usize].text.as_str() } else { "zzzq" };
            let p = sing(&mut a, word, &mut t);
            assert!(p.plan_index == 0 || p.confidence < 0.6 || p.off_script, "talk {n}, word {i} ({word}): {p:?}");
        }
    }
}

#[test]
fn a_jump_has_only_two_words_to_spare() {
    let s = song(&GRACE);
    // Three words in a row that slide 3 can't explain end the jump: the count starts again at "many".
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace", &mut t);
    let p = sing(&mut a, "through zzzq zzzq zzzq many dangers toils", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(1), 0), "{p:?}");
    let p = sing(&mut a, "and", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 2, "and")), 2), "four in order from \"many\"");
    // Talk that happens to hold a few of another slide's words, scattered, never jumps.
    let mut b = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut b, "amazing grace", &mut t);
    let p = sing(&mut b, "we go through many things and dangers in this life every single day", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(1), 0), "{p:?}");
    // A stricter setting has more to spare, and asks for more.
    let mut strict = Aligner::new(&s, &plan(&s)).with_jump_words(6);
    let mut t = 0.0;
    sing(&mut strict, "amazing grace", &mut t);
    assert_eq!(sing(&mut strict, "through many zzzq dangers toils and", &mut t).plan_index, 0, "five good words");
    assert_eq!(sing(&mut strict, "snares", &mut t).plan_index, 2, "the sixth");
    // A laxer one: two good words, one to spare.
    let mut lax = Aligner::new(&s, &plan(&s)).with_jump_words(2);
    let mut t = 0.0;
    sing(&mut lax, "amazing grace", &mut t);
    assert_eq!(sing(&mut lax, "through zzzq zzzq many", &mut t).plan_index, 0, "the first \"through\" is given up after two strays");
    assert_eq!(sing(&mut lax, "dangers", &mut t).plan_index, 2);
}

/// A verse slide whose last line is easily lost, and the chorus after it.
const PRECIOUS: [&str; 2] = ["how precious did that grace appear\nthe hour I first believed", "my chains are gone\nI've been set free"];

#[test]
fn a_place_lost_for_three_words_is_found_again_on_its_slide() {
    let s = song(&PRECIOUS);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    let p = sing(&mut a, "how precious did that grace", &mut t);
    assert_eq!(p.word, Some(index_of(&s, 0, "grace")));
    // The ear misses "appear the hour". "I" is too short to place anyone: it is a word the place can't explain.
    let p = sing(&mut a, "I first", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 0, "grace")), 0), "one word a little ahead proves nothing");
    let p = sing(&mut a, "believed", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(index_of(&s, 0, "believed")), 0, false), "two exact words in a row");
    assert!(p.confidence >= 0.6, "sure, so the chorus goes up before it is sung: {p:?}");
    let p = sing(&mut a, "my chains", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 1, "chains")), 1));
}

#[test]
fn a_refind_needs_a_miss_before_it_and_a_second_word_after_it() {
    let s = song(&PRECIOUS);
    let grace = Some(index_of(&s, 0, "grace"));
    // The second word does not follow: the place stays.
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "how precious did that grace", &mut t);
    let p = sing(&mut a, "I first zzzq", &mut t);
    assert_eq!((p.word, p.plan_index), (grace, 0));
    // The place had explained every word so far: a word four ahead starts an ordinary jump, four words long.
    let mut b = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut b, "how precious did that grace", &mut t);
    let p = sing(&mut b, "first believed", &mut t);
    assert_eq!((p.word, p.plan_index), (grace, 0), "two words are not the full proof");
    let p = sing(&mut b, "my chains", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 1, "chains")), 1), "the fourth word in a row");
}

#[test]
fn a_refind_never_reaches_another_slide() {
    let s = song(&PRECIOUS);
    // A word of the NEXT slide, a little ahead of a place that has just missed a word: the full proof.
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    let p = sing(&mut a, "how precious did that grace appear the hour", &mut t);
    assert_eq!(p.word, Some(index_of(&s, 0, "hour")));
    let p = sing(&mut a, "zzzq my chains are", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 0, "hour")), 0), "three words of the next slide stay");
    let p = sing(&mut a, "gone", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 1, "gone")), 1), "the fourth word jumps");
    // A re-find begun on this slide is not finished on the next one. "first" begins it; the slide's last word is
    // heard one sound off ("relieved"), which is no proof; "my" is the next slide's.
    let mut b = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut b, "how precious did that grace", &mut t);
    let p = sing(&mut b, "zzzq first relieved my", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 0, "grace")), 0), "the next slide asks for the full proof");
    let p = sing(&mut b, "chains are gone", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 1, "gone")), 1));
}

/// A two-line chorus that is in the plan twice: the first copy goes on to a verse, the second to the bridge that
/// ends the song. Invented words, so that no two sound alike.
const TWICE: [&str; 5] = ["alpha bravo charlie delta", "echo foxtrot golf hotel india juliet\nkilo lima mike november", "oscar papa quebec romeo",
    "echo foxtrot golf hotel india juliet\nkilo lima mike november", "sierra tango uniform victor"];

#[test]
fn a_refind_does_not_put_one_copy_of_a_repeated_chorus_ahead_of_the_other() {
    let s = song(&TWICE);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    let p = sing(&mut a, &TWICE.join(" "), &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(s.slides[4].end_word - 1), 4, false));
    // The chorus once more, which the plan does not have: its words go up at once, whichever copy it is.
    let p = sing(&mut a, "echo foxtrot golf hotel", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(index_of(&s, 3, "hotel")), 3, false), "the copy nearest in the plan");
    assert!(p.confidence >= 0.6, "{p:?}");
    // The ear loses "india juliet kilo" and hears one word nothing explains; then the last line. One copy goes
    // on to the bridge, the other to a verse, and which one this is was only a guess. Found again cheaply, the
    // guessed copy alone would be on its last line, far ahead of the other, and so sure: the bridge would come
    // back up as the song ends, and stay.
    let p = sing(&mut a, "zzzq lima mike", &mut t);
    assert_eq!(p.plan_index, 3);
    assert!(p.confidence < 0.6, "one copy of the chorus was found again without the other: {p:?}");
    let p = sing(&mut a, "november", &mut t);
    assert_eq!(p.plan_index, 3);
    assert!(p.confidence < 0.6, "{p:?}");
}

#[test]
fn a_copy_that_goes_on_to_the_same_slide_is_still_found_again() {
    // Pre-chorus and chorus are in the plan twice: either copy of the pre-chorus goes on to the chorus, so it
    // does not matter which one is found again.
    let pre = "was blind but now\nonce lost then found";
    let s = song(&["amazing grace how sweet the sound", pre, CHORUS, "the earth shall soon dissolve like snow", pre, CHORUS,
        "through many dangers toils and snares"]);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    let p = sing(&mut a, "amazing grace how sweet the sound was", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(index_of(&s, 1, "was")), 1));
    // The ear misses "blind but now".
    let p = sing(&mut a, "zzzq once lost", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(index_of(&s, 1, "lost")), 1, false), "two exact words in a row");
    assert!(p.confidence >= 0.6, "{p:?}");
}

#[test]
fn a_one_letter_word_does_not_put_the_singers_on_the_last_line_by_itself() {
    let s = song(&["all my days I give to you\noh I will sing your praise", "through many dangers toils and snares"]);
    let oh = s.slides[0].last_line_first_word;
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    let p = sing(&mut a, "all my days I give to you", &mut t);
    assert_eq!((p.word, p.plan_index), (Some(oh - 1), 0));
    assert!(p.confidence >= 0.6, "{p:?}");
    // The singers hold "you". The phoneme ear writes the held vowel again, twice, and each time it sounds like
    // the "oh" and the "I" the last line starts with. Sure, that place would bring the next slide up a line early.
    for _ in 0..2 {
        t += 0.5;
        let p = a.push(&Heard { key: sound_key("ooh"), at: t, conf: 0.9 });
        assert_eq!((p.plan_index, p.off_script), (0, false));
        assert!(p.confidence < 0.6, "a held vowel put the singers on the last line: {p:?}");
    }
    // The line is sung. Its one-letter words prove nothing; the first longer word does.
    let p = sing(&mut a, "oh I", &mut t);
    assert!(p.confidence < 0.6, "{p:?}");
    let p = sing(&mut a, "will", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(oh + 2), 0, false));
    assert!(p.confidence >= 0.6, "{p:?}");
}

#[test]
fn a_last_line_of_one_letter_words_is_proved_by_the_next_slides_first_word() {
    let s = song(&["all my days I give to you\noh oh oh", "through many dangers toils and snares"]);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    // The place goes along the last line, but nothing on it can prove it.
    let p = sing(&mut a, "all my days I give to you oh oh oh", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(s.slides[0].end_word - 1), 0, false));
    assert!(p.confidence < 0.6, "{p:?}");
    // The next slide waits for its own first word: a word late, never a line early.
    let p = sing(&mut a, "through", &mut t);
    assert_eq!((p.word, p.plan_index, p.off_script), (Some(index_of(&s, 1, "through")), 1, false));
    assert!(p.confidence >= 0.6, "{p:?}");
}

/// A chorus written out twice, one slide right after the other, then a verse.
const DOUBLED: [&str; 4] = ["alpha bravo charlie delta", "echo foxtrot golf hotel
india juliet kilo lima", "echo foxtrot golf hotel
india juliet kilo lima",
    "mike november oscar papa"];

#[test]
fn a_jump_into_a_chorus_written_out_twice_lands_on_its_first_slide_and_stays_sure() {
    let s = song(&DOUBLED);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "alpha bravo", &mut t);
    // The leader goes straight to the chorus. Both copies hold these words: the first is where it begins.
    let p = sing(&mut a, "echo foxtrot golf hotel", &mut t);
    assert_eq!((p.plan_index, p.word), (1, Some(index_of(&s, 1, "hotel"))));
    // On its last line the next slide comes up by the lead: the place must be sure there.
    let p = sing(&mut a, "india juliet", &mut t);
    assert_eq!(p.plan_index, 1);
    assert!(p.confidence >= 0.6, "{p:?}");
    // The second time round is the second copy, and it is sure on its last line too: the verse comes up in time.
    let p = sing(&mut a, "kilo lima echo foxtrot golf hotel india juliet", &mut t);
    assert_eq!(p.plan_index, 2);
    assert!(p.confidence >= 0.6, "{p:?}");
    let p = sing(&mut a, "kilo lima mike", &mut t);
    assert_eq!((p.plan_index, p.word), (3, Some(index_of(&s, 3, "mike"))));
}

/// Sings `words` as `sing` does, each `gap` seconds after the one before.
fn sing_every(a: &mut Aligner, words: &str, at: &mut f64, gap: f64) -> Position {
    let mut last = a.position().clone();
    for w in words.split_whitespace() {
        *at += gap;
        last = a.push(&Heard { key: sound_key(w), at: *at, conf: 1.0 });
    }
    last
}

#[test]
fn the_place_is_found_again_further_on_when_there_was_time_to_sing_what_lies_between() {
    let s = song(&["amazing grace how sweet the sound
that saved a wretch like me", "I once was lost but now am found"]);
    // The ear loses "the sound that saved a wretch" (two seconds of singing), then hears the slide's last two words.
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace how sweet", &mut t);
    t += 2.0;
    let p = sing(&mut a, "like", &mut t);
    assert_eq!(p.word, Some(index_of(&s, 0, "sweet")), "one word is not enough");
    let p = sing(&mut a, "me", &mut t);
    assert_eq!(p.word, Some(index_of(&s, 0, "me")), "two exact words in a row are");
    assert!(p.confidence >= 0.6, "{p:?}");
    // The same two words with no time to have sung what lies between: a stray, the place stays.
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace how sweet", &mut t);
    let p = sing_every(&mut a, "like me", &mut t, 0.1);
    assert_eq!(p.word, Some(index_of(&s, 0, "sweet")));
    // The ear loses the whole last line and the breath after it: the next slide's first two words find the place.
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace how sweet the sound", &mut t);
    t += 3.0;
    let p = sing(&mut a, "once was", &mut t);
    assert_eq!((p.plan_index, p.word), (1, Some(index_of(&s, 1, "was"))));
}

#[test]
fn a_jump_is_believed_through_words_the_ear_lost() {
    let s = song(&crate::follow::sim_tests::HYMN);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "amazing grace how sweet", &mut t);
    // The leader goes to the third verse; of "through many dangers toils and snares I have already come" the ear
    // delivers four words, with others lost between them and one misheard.
    let p = sing_every(&mut a, "many dangers", &mut t, 0.4);
    assert_eq!(p.plan_index, 0);
    t += 0.6;
    let p = sing_every(&mut a, "snares zzzq", &mut t, 0.4);
    assert_eq!(p.plan_index, 0, "three words are not four");
    t += 0.4;
    let p = sing_every(&mut a, "already", &mut t, 0.4);
    assert_eq!((p.plan_index, p.word), (4, Some(index_of(&s, 4, "already"))), "the fourth exact word, whatever was lost on the way");
}

#[test]
fn a_place_a_short_word_left_unsure_is_proved_by_a_later_word_of_the_line() {
    let s = song(&["step by step and day by day
I will trust you all my life", "evening falls upon the hill"]);
    let mut a = Aligner::new(&s, &plan(&s));
    let mut t = 0.0;
    sing(&mut a, "step by step and day by day", &mut t);
    let p = sing(&mut a, "I", &mut t);
    assert_eq!(p.confidence, 0.0, "a lone vowel onto the last line proves nothing");
    // "will" is lost; "trust" is exact, and there was the time to sing "will".
    t += 0.4;
    let p = sing(&mut a, "trust", &mut t);
    assert_eq!(p.word, Some(index_of(&s, 0, "trust")));
    assert!(p.confidence >= 0.6, "{p:?}");
}
