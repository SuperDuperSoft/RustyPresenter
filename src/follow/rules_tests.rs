use super::*;
use crate::model::{Slide, SlideContent};
use FollowAction::*;

fn song(texts: &[&str]) -> SongText {
    let slides: Vec<Slide> = texts.iter().map(|t| Slide::new("", SlideContent::Text { text: (*t).into(), footer: None, reference: None, size: 0, parallel: None })).collect();
    SongText::from_slides(&slides)
}

/// Slide 0: a 6-word line, then an 8-word last line, one syllable each; slide 1 after it.
fn hymn() -> SongText { song(&["we sing we sing we sing\nour god is great and strong and true", "come now", "{Instrumental}", "be still"]) }

fn auto(lead: f32) -> Settings { Settings { mode: Mode::Auto, lead_s: lead, ..Settings::default() } }

/// Auto with the two words-off timers Settings can switch on: 3 s of no singing, 8 s of talking.
fn timed(lead: f32) -> Settings { Settings { blank_after_s: Some(3.0), talk_blank_after_s: Some(8.0), ..auto(lead) } }

/// The ear has heard no word yet.
fn nowhere() -> Position { Position { word: None, plan_index: 0, confidence: 0.0, off_script: false } }

fn at(word: usize) -> Position { Position { word: Some(word), plan_index: 0, confidence: 1.0, off_script: false } }

/// Sings words `0..=last` one syllable every third of a second (pace 3/s); the time of the last one.
fn sing_to(r: &mut Rules, last: usize, live: usize) -> (f64, Vec<FollowAction>) {
    let mut acts = vec![];
    let mut t = 0.0;
    for w in 0..=last {
        t = w as f64 / 3.0;
        acts.extend(r.tick(t, &at(w), Voice::Singing, 2.0, live));
    }
    (t, acts)
}

#[test]
fn switches_lead_seconds_before_next_line() {
    let s = hymn();
    let mut r = Rules::new(auto(1.0), &s);
    // "god" (7): six one-syllable words are left, at 3 a second the next line starts 2 s after it was heard.
    let (t7, acts) = sing_to(&mut r, 7, 0);
    assert_eq!(acts, []);
    assert!((r.pace().unwrap() - 3.0).abs() < 0.01);
    assert_eq!(r.tick(t7 + 0.9, &at(7), Voice::Singing, 2.0, 0), None);
    assert_eq!(r.tick(t7 + 1.0, &at(7), Voice::Singing, 2.0, 0), Some(Show(1)), "2 s − 1 s lead");
    // With a 2 s lead it changes as soon as "god" is heard.
    let mut r = Rules::new(auto(2.0), &s);
    let (_, acts) = sing_to(&mut r, 7, 0);
    assert_eq!(acts, [Show(1)]);
}

#[test]
fn lead_is_clamped_0_5_to_4() {
    assert_eq!(auto(10.0).lead(), 4.0);
    assert_eq!(auto(0.1).lead(), 0.5);
    assert_eq!(auto(f32::NAN).lead(), 2.0);
    assert_eq!(auto(2.5).lead(), 2.5);
    assert_eq!(Settings::default().lead(), 2.0);
    // 0.1 s acts as 0.5 s.
    let s = hymn();
    let mut r = Rules::new(auto(0.1), &s);
    let (t7, _) = sing_to(&mut r, 7, 0);
    assert_eq!(r.tick(t7 + 0.5, &at(7), Voice::Singing, 2.0, 0), None);
    let mut r = Rules::new(auto(0.1), &s);
    let (t7, _) = sing_to(&mut r, 7, 0);
    assert_eq!(r.tick(t7 + 0.4, &at(7), Voice::Singing, 2.0, 0), None);
    assert_eq!(r.tick(t7 + 0.75, &at(7), Voice::Singing, 2.0, 0), None);
    assert_eq!(r.tick(t7 + 1.0, &at(7), Voice::Singing, 2.0, 0), None);
    assert_eq!(r.tick(t7 + 1.5, &at(7), Voice::Singing, 2.0, 0), Some(Show(1)));
}

#[test]
fn never_before_last_line_started() {
    let s = song(&["glory glory\nhallelujah", "his truth is marching on"]);
    let mut r = Rules::new(auto(4.0), &s);
    // Six syllables left at the default pace is well inside 4 s, but the last line has not started.
    for t in [0.0, 1.0, 2.0, 3.0] { assert_eq!(r.tick(t, &at(0), Voice::Singing, 2.0, 0), None) }
    assert_eq!(r.tick(3.5, &at(1), Voice::Singing, 2.0, 0), None);
    assert_eq!(r.tick(4.0, &at(2), Voice::Singing, 2.0, 0), Some(Show(1)));
}

#[test]
fn no_timer_takes_the_words_off_unless_settings_ask() {
    assert_eq!((Settings::default().blank_after_s, Settings::default().talk_blank_after_s), (None, None));
    let s = hymn();
    // A long pause in the middle of a slide, music or silence or talk: the words stay.
    let mut r = Rules::new(auto(2.0), &s);
    sing_to(&mut r, 3, 0);
    for i in 0..1200 {
        let voice = [Voice::Music, Voice::Silence, Voice::Speaking][i / 400];
        assert_eq!(r.tick(1.0 + i as f64 * 0.1, &at(3), voice, 4.0, 0), None, "{i}");
    }
    assert!(!r.blanked());
    // The singers carry on where they stopped.
    assert_eq!(r.tick(125.0, &at(4), Voice::Singing, 2.0, 0), None);
    assert_eq!(r.tick(126.0, &at(7), Voice::Singing, 2.0, 0), Some(Show(1)));
}

#[test]
fn next_words_wait_through_a_long_pause() {
    let s = hymn();
    let mut r = Rules::new(auto(2.0), &s);
    let (t7, acts) = sing_to(&mut r, 7, 0);
    assert_eq!(acts, [Show(1)]);
    // Slide 1 is up. The singers finish slide 0 and hold its last word; the band then plays for a minute.
    for (i, w) in (8..=13).enumerate() { assert_eq!(r.tick(t7 + i as f64 * 0.3, &at(w), Voice::Singing, 2.0, 1), None) }
    for i in 0..80 { assert_eq!(r.tick(t7 + 2.0 + i as f64 * 0.1, &at(13), Voice::Singing, 0.5, 1), None, "held note {i}") }
    for i in 0..600 { assert_eq!(r.tick(t7 + 10.0 + i as f64 * 0.1, &at(13), Voice::Music, 4.0, 1), None, "break {i}") }
    assert!(!r.blanked());
    // They come in on slide 1: it is already there.
    assert_eq!(r.tick(t7 + 71.0, &at(14), Voice::Singing, 2.0, 1), None);
}

#[test]
fn heard_words_count_as_singing_under_a_loud_band() {
    // The room's label says "music" all the way through (drums and guitars hide the voice from it).
    let s = hymn();
    let mut r = Rules::new(auto(2.0), &s);
    let mut acts = vec![];
    for w in 0..=7 { acts.extend(r.tick(w as f64 / 3.0, &at(w), Voice::Music, 4.0, 0)) }
    assert_eq!(acts, [Show(1)]);
    // The last word, however late it is heard and however short the lead: the next words go up at once.
    let mut r = Rules::new(auto(0.5), &s);
    sing_to(&mut r, 3, 0);
    assert_eq!(r.tick(30.0, &at(13), Voice::Music, 4.0, 0), Some(Show(1)));
}

#[test]
fn a_timer_blanks_when_settings_ask() {
    let s = hymn();
    let mut r = Rules::new(timed(2.0), &s);
    sing_to(&mut r, 3, 0);
    let last = 1.0;
    assert_eq!(r.tick(last + 1.0, &at(3), Voice::Music, 4.0, 0), None);
    assert_eq!(r.tick(last + 2.9, &at(3), Voice::Music, 4.0, 0), None);
    assert_eq!(r.tick(last + 3.0, &at(3), Voice::Music, 4.0, 0), Some(Blank));
    assert!(r.blanked());
    assert_eq!(r.blank_why(), (false, 3.0));
    assert_eq!(r.tick(last + 6.0, &at(3), Voice::Silence, 0.0, 0), None, "once");
    // Off: never blanks.
    let mut r = Rules::new(Settings { blank_after_s: None, ..timed(2.0) }, &s);
    sing_to(&mut r, 3, 0);
    assert_eq!(r.tick(60.0, &at(3), Voice::Music, 4.0, 0), None);
}

#[test]
fn instrumental_slide_comes_up_when_quiet_and_leaves_as_singing_starts() {
    // Slide 1 "come now" is sung, then slide 2 is an instrumental before slide 3 "be still".
    let s = hymn();
    let mut r = Rules::new(auto(4.0), &s);
    assert_eq!(r.tick(10.0, &at(14), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(10.4, &at(15), Voice::Singing, 2.0, 1), None, "the last word is being sung: its words stay");
    // "now" is held for three seconds: still singing, the words stay.
    for i in 0..30 { assert_eq!(r.tick(10.5 + i as f64 * 0.1, &at(15), Voice::Singing, 0.5, 1), None, "{i}") }
    // The band takes over: two quiet seconds later the instrumental slide comes up.
    assert_eq!(r.tick(14.0, &at(15), Voice::Music, 4.0, 1), None);
    assert_eq!(r.tick(15.3, &at(15), Voice::Music, 4.0, 1), None);
    assert_eq!(r.tick(15.5, &at(15), Voice::Music, 4.0, 1), Some(Show(2)));
    // On the instrumental slide, however long the band plays or the leader talks: nothing.
    for i in 0..600 {
        let voice = if i < 300 { Voice::Music } else { Voice::Speaking };
        assert_eq!(r.tick(16.0 + i as f64 * 0.1, &at(15), voice, 4.0, 2), None, "{i}");
    }
    // Singing starts: the next words come up before a word of them is heard.
    assert_eq!(r.tick(80.0, &at(15), Voice::Singing, 2.0, 2), Some(Show(3)));
    assert_eq!(r.tick(80.5, &at(15), Voice::Singing, 2.0, 3), None, "and it stays there");
    assert_eq!(r.tick(81.0, &at(16), Voice::Singing, 2.0, 3), None);
    // Not while the last word is not yet sung: a pause before it keeps the words.
    let mut r = Rules::new(auto(4.0), &s);
    r.tick(10.0, &at(14), Voice::Singing, 2.0, 1);
    for i in 0..300 { assert_eq!(r.tick(10.5 + i as f64 * 0.1, &at(14), Voice::Music, 4.0, 1), None, "{i}") }
    // Suggest only suggests it.
    let mut r = Rules::new(Settings { mode: Mode::Suggest, ..auto(4.0) }, &s);
    r.tick(10.0, &at(15), Voice::Singing, 2.0, 1);
    assert_eq!(r.tick(13.0, &at(15), Voice::Music, 4.0, 1), Some(Suggest(2)));
    // (The band is heard on the instrumental slide, for a second or more, before the singing starts: see
    // `an_instrumental_slide_waits_for_singing_to_start_on_it`.)
    assert_eq!(r.tick(16.0, &at(15), Voice::Music, 4.0, 2), None);
    assert_eq!(r.tick(17.5, &at(15), Voice::Music, 4.0, 2), None);
    assert_eq!(r.tick(20.0, &at(15), Voice::Singing, 2.0, 2), Some(Suggest(3)));
}

#[test]
fn an_instrumental_slide_waits_for_singing_to_start_on_it() {
    // The operator takes the next song live on its intro slide while the last note of the song before is still
    // held. The room's label says singing, but nothing has started: the intro slide stays.
    let s = song(&["{Instrumental}", "amazing grace", "how sweet the sound"]);
    let mut r = Rules::new(auto(2.0), &s);
    for i in 0..30 { assert_eq!(r.tick(i as f64 * 0.1, &nowhere(), Voice::Singing, 2.0, 0), None, "held note {i}") }
    // The note ends and the band plays the intro. Then the singers come in: the first words go up.
    for i in 0..100 { assert_eq!(r.tick(3.0 + i as f64 * 0.1, &nowhere(), Voice::Music, 4.0, 0), None, "intro {i}") }
    assert_eq!(r.tick(13.0, &nowhere(), Voice::Singing, 2.0, 0), Some(Show(1)));
    // A sure word heard on another slide does not wait for the label.
    let mut r = Rules::new(auto(2.0), &s);
    assert_eq!(r.tick(0.0, &nowhere(), Voice::Singing, 2.0, 0), None);
    assert_eq!(r.tick(0.5, &at(0), Voice::Singing, 2.0, 0), Some(Show(1)));
    // It is asked of each slide: the band was heard on the intro slide, then the screen goes to a break slide
    // while someone sings. Nothing has started on that one yet. It does once the room has been quiet there for
    // a second (two looks a second apart: one look alone is only a flicker of the label).
    let s = song(&["{Instrumental}", "amazing grace", "{Instrumental}", "how sweet the sound"]);
    let mut r = Rules::new(auto(2.0), &s);
    assert_eq!(r.tick(0.0, &nowhere(), Voice::Music, 4.0, 0), None);
    assert_eq!(r.tick(1.0, &nowhere(), Voice::Singing, 2.0, 2), None);
    assert_eq!(r.tick(2.0, &nowhere(), Voice::Silence, 0.0, 2), None);
    assert_eq!(r.tick(3.0, &nowhere(), Voice::Silence, 0.0, 2), None);
    assert_eq!(r.tick(4.0, &nowhere(), Voice::Singing, 2.0, 2), Some(Show(3)));
}

#[test]
fn a_flicker_of_the_label_in_a_held_note_is_not_singing_starting() {
    // The intro slide goes live while the last note of the song before is held. The room's label flickers in
    // the note: one look says music, later two say talking. The note is still the note: the intro slide stays.
    let s = song(&["{Instrumental}", "amazing grace", "how sweet the sound"]);
    let mut r = Rules::new(auto(2.0), &s);
    for i in 0..60 {
        let voice = match i { 15 => Voice::Music, 30 | 31 => Voice::Speaking, _ => Voice::Singing };
        assert_eq!(r.tick(i as f64 * 0.1, &nowhere(), voice, 2.0, 0), None, "held note {i}");
    }
    // The flicker may be the very first look after the operator's click.
    let mut r = Rules::new(auto(2.0), &s);
    assert_eq!(r.tick(0.0, &nowhere(), Voice::Music, 4.0, 0), None);
    for i in 1..30 { assert_eq!(r.tick(i as f64 * 0.1, &nowhere(), Voice::Singing, 2.0, 0), None, "held note {i}") }
    // The note ends and the band plays for a second and a half: now the room has stopped singing, and the
    // singers coming in is a start.
    for i in 0..15 { assert_eq!(r.tick(3.0 + i as f64 * 0.1, &nowhere(), Voice::Music, 4.0, 0), None, "intro {i}") }
    assert_eq!(r.tick(4.5, &nowhere(), Voice::Singing, 2.0, 0), Some(Show(1)));
    // Under a second of band between two sung phrases is not a stop either.
    let mut r = Rules::new(auto(2.0), &s);
    for i in 0..10 { assert_eq!(r.tick(i as f64 * 0.1, &nowhere(), Voice::Singing, 2.0, 0), None, "note {i}") }
    for i in 10..15 { assert_eq!(r.tick(i as f64 * 0.1, &nowhere(), Voice::Music, 4.0, 0), None, "band {i}") }
    for i in 15..30 { assert_eq!(r.tick(i as f64 * 0.1, &nowhere(), Voice::Singing, 2.0, 0), None, "note again {i}") }
}

/// A verse (slide 0), an instrumental slide, the next verse. The verse's last word has been sung and it has been
/// quiet for two seconds: the instrumental slide is up.
fn on_the_break() -> Rules {
    let s = song(&["I once was lost but now am found\nwas blind but now I see", "{Instrumental}", "twas grace that taught"]);
    let mut r = Rules::new(auto(2.0), &s);
    assert_eq!(r.tick(40.0, &at(13), Voice::Singing, 2.0, 0), None);
    assert_eq!(r.tick(42.5, &at(13), Voice::Silence, 0.0, 0), Some(Show(1)));
    r
}

#[test]
fn on_an_instrumental_slide_one_stray_word_does_not_bring_the_verse_back() {
    // The leader says something over the break. The ear finds a song word in it ("I", the start of the verse's
    // first line) and delivers it when the room is quiet again: the instrumental slide stays, however long.
    let mut r = on_the_break();
    for i in 0..50 { assert_eq!(r.tick(50.0 + i as f64 * 0.1, &at(0), Voice::Silence, 0.0, 1), None, "{i}") }
    // The leader quotes the line: three words in a row, but they came while the room's label said talking.
    let mut r = on_the_break();
    for w in 0..=2 { assert_eq!(r.tick(50.0 + w as f64 * 0.3, &at(w), Voice::Speaking, 0.0, 1), None, "word {w}") }
    assert_eq!(r.tick(50.8, &at(2), Voice::Silence, 0.0, 1), None, "a quoted line is not singing");
    // Three words in a row of which only the last was surely heard: one sung word is not two.
    let mut r = on_the_break();
    for w in 0..=1 {
        assert_eq!(r.tick(60.0 + w as f64 * 0.3, &Position { confidence: 0.3, ..at(w) }, Voice::Music, 4.0, 1), None, "word {w}");
    }
    assert_eq!(r.tick(60.6, &at(2), Voice::Music, 4.0, 1), None);
    assert_eq!(r.tick(60.9, &at(3), Voice::Music, 4.0, 1), Some(Show(0)), "the second sure word in a row");
    // The verse really is sung again (a loud band hides the voice from the room's label): its third word in a
    // row brings it back, as from a slide with words.
    let mut r = on_the_break();
    assert_eq!(r.tick(60.0, &at(0), Voice::Music, 4.0, 1), None);
    assert_eq!(r.tick(60.3, &at(1), Voice::Music, 4.0, 1), None);
    assert_eq!(r.tick(60.6, &at(2), Voice::Music, 4.0, 1), Some(Show(0)));
}

#[test]
fn an_intro_slide_leaves_as_singing_starts() {
    // Slide 0 is an intro with nothing to sing; no word has been heard yet (or there is no speech model).
    let s = song(&["{Instrumental}", "amazing grace", "how sweet the sound"]);
    let mut r = Rules::new(auto(2.0), &s);
    for i in 0..300 { assert_eq!(r.tick(i as f64 * 0.1, &nowhere(), Voice::Music, 4.0, 0), None) }
    assert_eq!(r.tick(30.0, &nowhere(), Voice::Speaking, 3.0, 0), None, "a welcome is not singing");
    assert_eq!(r.tick(31.0, &nowhere(), Voice::Singing, 2.0, 0), Some(Show(1)));
    // The singers are heard somewhere else: that wins.
    let mut r = Rules::new(auto(2.0), &s);
    assert_eq!(r.tick(1.0, &at(3), Voice::Singing, 2.0, 0), Some(Show(2)));
    // Manual: nothing.
    let mut r = Rules::new(Settings { mode: Mode::Manual, ..auto(2.0) }, &s);
    assert_eq!(r.tick(1.0, &nowhere(), Voice::Singing, 2.0, 0), None);
}

#[test]
fn talk_holds_then_blanks() {
    let s = hymn();
    let mut r = Rules::new(timed(4.0), &s);
    sing_to(&mut r, 2, 0);
    // The singers' place is on the last line, but the leader talks: no change.
    let mut acts = vec![];
    for i in 0..79 { acts.extend(r.tick(1.0 + i as f64 * 0.1, &at(7), Voice::Speaking, 3.0, 0)) }
    assert_eq!(acts, []);
    assert_eq!(r.tick(9.0, &at(7), Voice::Speaking, 3.0, 0), Some(Blank), "8 s of talking");
    assert_eq!(r.blank_why(), (true, 8.0));
    // Off: talk only holds.
    let mut r = Rules::new(Settings { talk_blank_after_s: None, ..timed(4.0) }, &s);
    sing_to(&mut r, 2, 0);
    for i in 0..300 { assert_eq!(r.tick(1.0 + i as f64 * 0.1, &at(7), Voice::Speaking, 3.0, 0), None) }
}

#[test]
fn unblanks_when_song_words_return() {
    let s = hymn();
    let mut r = Rules::new(timed(2.0), &s);
    sing_to(&mut r, 13, 0);
    let mut acts = vec![];
    for i in 0..40 { acts.extend(r.tick(5.0 + i as f64 * 0.1, &at(13), Voice::Music, 4.0, 0)) }
    assert!(acts.contains(&Blank));
    // Music with no new words, or singing with no new words: still blank.
    assert_eq!(r.tick(9.5, &at(13), Voice::Singing, 2.0, 0), None);
    // Unsure words don't bring them back.
    let unsure = Position { confidence: 0.2, ..at(14) };
    assert_eq!(r.tick(9.8, &unsure, Voice::Singing, 2.0, 0), None);
    assert_eq!(r.tick(10.0, &at(14), Voice::Singing, 2.0, 0), Some(Unblank(1)));
    assert!(!r.blanked());
    // Repeating the last line brings back the same slide.
    let mut r = Rules::new(timed(2.0), &s);
    sing_to(&mut r, 13, 0);
    for i in 0..40 { r.tick(5.0 + i as f64 * 0.1, &at(13), Voice::Music, 4.0, 0); }
    assert_eq!(r.tick(10.0, &at(6), Voice::Singing, 2.0, 0), Some(Unblank(0)));
}

#[test]
fn suggest_mode_only_suggests() {
    let s = hymn();
    let mut r = Rules::new(Settings { mode: Mode::Suggest, ..auto(2.0) }, &s);
    let (_, acts) = sing_to(&mut r, 7, 0);
    assert_eq!(acts, [Suggest(1)]);
    // Never blanks, never shows.
    for i in 0..100 { assert!(matches!(r.tick(5.0 + i as f64 * 0.1, &at(13), Voice::Music, 4.0, 0), None | Some(Suggest(1))), "{i}") }
    // Off script ahead: a suggestion only.
    assert_eq!(r.tick(20.0, &at(14), Voice::Singing, 2.0, 0), Some(Suggest(1)));
    // Manual: nothing at all.
    let mut r = Rules::new(Settings { mode: Mode::Manual, ..auto(2.0) }, &s);
    let (_, acts) = sing_to(&mut r, 13, 0);
    assert_eq!(acts, []);
    assert_eq!(r.tick(30.0, &at(14), Voice::Music, 0.0, 0), None);
}

#[test]
fn low_confidence_holds() {
    let s = hymn();
    let mut r = Rules::new(auto(4.0), &s);
    let mut t = 0.0;
    for w in 0..=13 {
        t = w as f64 / 3.0;
        assert_eq!(r.tick(t, &Position { confidence: 0.3, ..at(w) }, Voice::Singing, 2.0, 0), None);
    }
    // Ahead on another slide but unsure, or off script: hold.
    assert_eq!(r.tick(t + 0.3, &Position { confidence: 0.3, ..at(14) }, Voice::Singing, 2.0, 0), None);
    assert_eq!(r.tick(t + 0.6, &Position { off_script: true, ..at(14) }, Voice::Singing, 2.0, 0), None);
    // Sure again: it follows.
    assert_eq!(r.tick(t + 0.9, &at(14), Voice::Singing, 2.0, 0), Some(Show(1)));
}

#[test]
fn last_slide_holds_then_blanks_never_next_item() {
    let s = song(&["amazing grace", "how sweet the sound"]);
    let mut r = Rules::new(auto(4.0), &s);
    let mut acts = vec![];
    for (i, w) in (2..=5).enumerate() { acts.extend(r.tick(i as f64 * 0.3, &at(w), Voice::Singing, 2.0, 1)) }
    for i in 0..300 { acts.extend(r.tick(2.0 + i as f64 * 0.1, &at(5), Voice::Music, 3.0, 1)) }
    assert_eq!(acts, [Blank], "the last slide holds, then blanks; nothing past it");
    assert_eq!(r.blank_why(), (false, 5.0), "the song's last word, then five quiet seconds");
    for i in 0..50 { assert_eq!(r.tick(40.0 + i as f64 * 0.1, &at(5), Voice::Singing, 2.0, 1), None) }
    // The last line sung again (a tag): the words come back, on its third word in a row (after the song's end
    // one matching word could as well be the next song starting).
    assert_eq!(r.tick(46.0, &at(2), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(46.3, &at(3), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(46.6, &at(4), Voice::Singing, 2.0, 1), Some(Unblank(1)));
    // Not before the last word: a pause on the last slide keeps the words.
    let mut r = Rules::new(auto(4.0), &s);
    r.tick(0.0, &at(4), Voice::Singing, 2.0, 1);
    for i in 0..600 { assert_eq!(r.tick(1.0 + i as f64 * 0.1, &at(4), Voice::Music, 3.0, 1), None, "{i}") }
    // A held last note is singing: the words stay until it is quiet for five seconds.
    let mut r = Rules::new(auto(4.0), &s);
    r.tick(0.0, &at(5), Voice::Singing, 2.0, 1);
    for i in 0..100 { assert_eq!(r.tick(0.1 + i as f64 * 0.1, &at(5), Voice::Singing, 0.5, 1), None, "{i}") }
    assert_eq!(r.tick(14.9, &at(5), Voice::Silence, 0.0, 1), None);
    assert_eq!(r.tick(15.2, &at(5), Voice::Silence, 0.0, 1), Some(Blank));
}

#[test]
fn just_behind_after_an_early_change_holds() {
    let s = hymn();
    let mut r = Rules::new(auto(2.0), &s);
    let (t7, _) = sing_to(&mut r, 7, 0);
    // Slide 1 went up early; the singers finish slide 0's last line.
    for (i, w) in (8..=13).enumerate() { assert_eq!(r.tick(t7 + i as f64 * 0.3, &at(w), Voice::Singing, 2.0, 1), None) }
}

/// The two-slide song's last slide live, its words 2..=5 sung, then music until the words come off.
fn ended_song() -> (SongText, Rules) {
    let s = song(&["amazing grace", "how sweet the sound"]);
    let mut r = Rules::new(auto(4.0), &s);
    for (i, w) in (2..=5).enumerate() { r.tick(i as f64 * 0.3, &at(w), Voice::Singing, 2.0, 1); }
    let mut acts = vec![];
    for i in 0..60 { acts.extend(r.tick(1.0 + i as f64 * 0.1, &at(5), Voice::Music, 3.0, 1)) }
    assert_eq!(acts, [Blank]);
    (s, r)
}

#[test]
fn a_tag_under_a_loud_band_brings_the_words_back() {
    // Words taken off by a Settings timer (the song is not over). The label still says music: one heard word is
    // not enough, two in a row are.
    let s = hymn();
    let mut r = Rules::new(timed(2.0), &s);
    sing_to(&mut r, 3, 0);
    let mut acts = vec![];
    for i in 0..40 { acts.extend(r.tick(1.0 + i as f64 * 0.1, &at(3), Voice::Music, 4.0, 0)) }
    assert_eq!(acts, [Blank]);
    assert_eq!(r.tick(9.0, &at(4), Voice::Music, 3.0, 0), None);
    assert_eq!(r.tick(9.4, &at(5), Voice::Music, 3.0, 0), Some(Unblank(0)));
    // Under a singing label one word is enough.
    let mut r = Rules::new(timed(2.0), &s);
    sing_to(&mut r, 3, 0);
    for i in 0..40 { r.tick(1.0 + i as f64 * 0.1, &at(3), Voice::Music, 4.0, 0); }
    assert_eq!(r.tick(9.0, &at(4), Voice::Singing, 3.0, 0), Some(Unblank(0)));
    // After the song's end (the band may be starting the next song before the next item is clicked) it takes
    // three words in a row, whatever the label says.
    let (_s, mut r) = ended_song();
    assert_eq!(r.tick(9.0, &at(2), Voice::Music, 3.0, 1), None);
    assert_eq!(r.tick(9.4, &at(3), Voice::Music, 3.0, 1), None);
    assert_eq!(r.tick(9.8, &at(4), Voice::Music, 3.0, 1), Some(Unblank(1)));
}

#[test]
fn after_the_end_one_stray_word_does_not_bring_the_words_back() {
    let (_s, mut r) = ended_song();
    // The band starts the next song before the next item is clicked. One of its words matches the old slide,
    // under a singing label; two seconds later another one does.
    assert_eq!(r.tick(9.0, &at(3), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(11.0, &at(2), Voice::Singing, 2.0, 1), None);
    assert!(r.blanked());
}

#[test]
fn after_the_end_a_tag_brings_the_words_back() {
    let (_s, mut r) = ended_song();
    // The last slide sung again: three of its words in a row.
    assert_eq!(r.tick(9.0, &at(2), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(9.3, &at(3), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(9.6, &at(4), Voice::Singing, 2.0, 1), Some(Unblank(1)));
    assert!(!r.blanked());
}

#[test]
fn a_spoken_word_then_a_flicker_of_singing_keeps_the_words_off() {
    let (_s, mut r) = ended_song();
    // In the prayer a song word is heard, under a talking label; later the label flickers to singing.
    assert_eq!(r.tick(112.0, &at(2), Voice::Speaking, 0.0, 1), None);
    for i in 0..40 { assert_eq!(r.tick(112.1 + i as f64 * 0.1, &at(2), Voice::Singing, 0.0, 1), None, "{i}") }
    assert!(r.blanked());
    // A place left over from long ago does not count either.
    let (_s, mut r) = ended_song();
    assert_eq!(r.tick(9.0, &at(2), Voice::Music, 3.0, 1), None);
    for i in 0..40 { assert_eq!(r.tick(20.0 + i as f64 * 0.1, &at(2), Voice::Singing, 0.0, 1), None, "{i}") }
}

#[test]
fn talk_straight_after_the_last_word_still_ends_the_song() {
    let s = song(&["amazing grace", "how sweet the sound"]);
    let mut r = Rules::new(auto(4.0), &s);
    for (i, w) in (2..=5).enumerate() { r.tick(i as f64 * 0.3, &at(w), Voice::Singing, 2.0, 1); }
    // "Amen, let's pray": the place goes off-script before the five seconds are up.
    let lost = Position { confidence: 0.0, off_script: true, ..at(5) };
    let mut acts = vec![];
    let mut when = 0.0;
    for i in 0..80 {
        let t = 2.0 + i as f64 * 0.1;
        if let Some(a) = r.tick(t, &lost, Voice::Speaking, 0.0, 1) { acts.push(a); when = t }
    }
    assert_eq!(acts, [Blank]);
    assert!((5.8..=6.1).contains(&when), "five seconds after the last word (0.9 s), was {when}");
}

#[test]
fn the_quiet_counts_from_the_last_word_not_from_an_older_label() {
    let s = song(&["amazing grace", "how sweet the sound"]);
    let mut r = Rules::new(auto(4.0), &s);
    r.tick(20.0, &at(2), Voice::Singing, 2.0, 1);
    // The last line arrives late and unsure under a music label, then turns sure.
    let unsure = Position { confidence: 0.2, ..at(5) };
    assert_eq!(r.tick(26.0, &unsure, Voice::Music, 3.0, 1), None);
    assert_eq!(r.tick(26.1, &at(5), Voice::Music, 3.0, 1), None, "not the moment the last word arrives");
    assert_eq!(r.tick(30.9, &at(5), Voice::Music, 3.0, 1), None);
    assert_eq!(r.tick(31.1, &at(5), Voice::Music, 3.0, 1), Some(Blank));
}

#[test]
fn the_pace_is_measured_across_the_ears_batches() {
    // Six one-syllable words a second, delivered three at a time every half second.
    let s = hymn();
    let mut r = Rules::new(Settings { mode: Mode::Suggest, ..auto(0.5) }, &s);
    for batch in 0..4 {
        for k in 0..3 { r.tick(batch as f64 * 0.5, &at(batch * 3 + k), Voice::Singing, 2.0, 0); }
    }
    let pace = r.pace().expect("a pace");
    assert!((5.5..=6.5).contains(&pace), "about 6 syllables a second, was {pace}");
}

#[test]
fn switching_to_automatic_in_the_pause_puts_the_next_words_up() {
    let s = hymn();
    let mut r = Rules::new(Settings { mode: Mode::Suggest, ..auto(2.0) }, &s);
    let (t, _) = sing_to(&mut r, 13, 0);
    // A long held chord after the slide's last word; then the operator switches Suggest to Automatic.
    for i in 0..30 { r.tick(t + 0.3 + i as f64 * 0.1, &at(13), Voice::Music, 3.0, 0); }
    r.set_settings(auto(2.0));
    assert_eq!(r.tick(t + 4.0, &at(13), Voice::Music, 3.0, 0), Some(Show(1)));
}

#[test]
fn a_timer_starts_again_after_the_operators_move_and_after_a_hold() {
    let s = hymn();
    // The operator puts the words back long after the last singing: a full timer runs before the next blank.
    let mut r = Rules::new(timed(2.0), &s);
    r.tick(0.0, &nowhere(), Voice::Singing, 2.0, 0);
    assert_eq!(r.tick(100.0, &nowhere(), Voice::Music, 3.0, 0), Some(Blank));
    r.reset();
    assert_eq!(r.tick(100.1, &nowhere(), Voice::Music, 3.0, 0), None);
    assert_eq!(r.tick(103.0, &nowhere(), Voice::Music, 3.0, 0), None);
    assert_eq!(r.tick(103.2, &nowhere(), Voice::Music, 3.0, 0), Some(Blank));
    // Hold was on through a minute of singing the rules never saw; released in a breath.
    let mut r = Rules::new(timed(2.0), &s);
    r.tick(0.0, &nowhere(), Voice::Singing, 2.0, 0);
    r.resume();
    assert_eq!(r.tick(60.5, &nowhere(), Voice::Music, 3.0, 0), None);
    assert_eq!(r.tick(63.6, &nowhere(), Voice::Music, 3.0, 0), Some(Blank));
}

#[test]
fn a_refused_blank_or_unblank_is_tried_again() {
    let (_s, mut r) = ended_song();
    // The screen did not take the blank: the rules ask again.
    r.set_blanked(false);
    assert_eq!(r.tick(8.0, &at(5), Voice::Music, 3.0, 1), Some(Blank));
    // The screen did not take the un-blank (after the song's end: on the tag's third word): asked again on the
    // next look.
    assert_eq!(r.tick(9.0, &at(2), Voice::Singing, 3.0, 1), None);
    assert_eq!(r.tick(9.3, &at(3), Voice::Singing, 3.0, 1), None);
    assert_eq!(r.tick(9.6, &at(4), Voice::Singing, 3.0, 1), Some(Unblank(1)));
    r.set_blanked(true);
    assert_eq!(r.tick(9.7, &at(4), Voice::Singing, 3.0, 1), Some(Unblank(1)));
}

/// Slide 0's last line is being sung and slide 1 went up early (2 s lead) as "god" was heard; the time of "god".
fn early_change() -> (Rules, f64) {
    let s = hymn();
    let mut r = Rules::new(auto(2.0), &s);
    let (t7, acts) = sing_to(&mut r, 7, 0);
    assert_eq!(acts, [Show(1)]);
    (r, t7)
}

/// After the early change the singers finish slide 0, then sing its last line again (a tag): on the tag's third
/// word the screen goes back to slide 0. The time of that word.
fn tagged() -> (Rules, f64) {
    let (mut r, mut t) = early_change();
    for w in 8..=13 {
        t += 0.3;
        assert_eq!(r.tick(t, &at(w), Voice::Singing, 2.0, 1), None, "word {w}");
    }
    for w in 6..=7 {
        t += 0.3;
        assert_eq!(r.tick(t, &at(w), Voice::Singing, 2.0, 1), None, "tag word {w}");
    }
    t += 0.3;
    assert_eq!(r.tick(t, &at(8), Voice::Singing, 2.0, 1), Some(Show(0)), "the tag's third word");
    (r, t)
}

#[test]
fn a_held_word_stops_the_clock() {
    // A slow song with a long last line, one syllable a word.
    let s = song(&["one two three\nwe will sing of your love for all our days", "come now"]);
    let mut r = Rules::new(auto(2.0), &s);
    // A word every two thirds of a second. "we" (3) starts the last line: nine words, 6 s, are left.
    for w in 0..=3 { assert_eq!(r.tick(w as f64 / 1.5, &at(w), Voice::Singing, 2.0, 0), None, "word {w}") }
    assert!((r.pace().unwrap() - 1.5).abs() < 0.01);
    // The leader holds the word, stretches it or sings something else over it: singing, but no new word, for
    // 20 s. The clock alone never takes the unsung line away (it used to, 4 s in).
    for i in 1..=200 { assert_eq!(r.tick(2.0 + i as f64 * 0.1, &at(3), Voice::Singing, 2.0, 0), None, "held {i}") }
    // The line goes on. The change comes on the word that leaves three words, 2 s, to go.
    for (i, w) in (4..=8).enumerate() {
        assert_eq!(r.tick(22.0 + (i + 1) as f64 / 1.5, &at(w), Voice::Singing, 2.0, 0), None, "word {w}");
    }
    assert_eq!(r.tick(22.0 + 6.0 / 1.5, &at(9), Voice::Singing, 2.0, 0), Some(Show(1)));
}

#[test]
fn a_normal_line_coasts_as_before() {
    // The clock still stands in for the words between: here for 1.5 s after "god", well inside the limit.
    let s = hymn();
    let mut r = Rules::new(auto(0.5), &s);
    let (t7, acts) = sing_to(&mut r, 7, 0);
    assert_eq!(acts, []);
    assert_eq!(r.tick(t7 + 1.0, &at(7), Voice::Singing, 2.0, 0), None);
    assert_eq!(r.tick(t7 + 1.5, &at(7), Voice::Singing, 2.0, 0), Some(Show(1)));
}

#[test]
fn one_word_never_goes_back_a_slide() {
    let (mut r, t7) = early_change();
    // Slide 1 is live and the singers are finishing slide 0. One stray word (an ad-lib, a misheard word) lands
    // on slide 0's first line: the screen does not flip back and forward again.
    for (i, &w) in [8, 9, 0, 10, 11].iter().enumerate() {
        assert_eq!(r.tick(t7 + (i + 1) as f64 * 0.3, &at(w), Voice::Singing, 2.0, 1), None, "word {w}");
    }
}

#[test]
fn three_words_back_go_back() {
    let (mut r, t7) = early_change();
    // The singers really went back to the top of slide 0 (the verse again): three words in a row say so.
    assert_eq!(r.tick(t7 + 0.3, &at(0), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(t7 + 0.6, &at(1), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(t7 + 0.9, &at(2), Voice::Singing, 2.0, 1), Some(Show(0)));
}

#[test]
fn tag_of_the_last_line_brings_the_slide_back() {
    // All of it is in `tagged`: the first time through the last line holds; sung again from its end, it brings
    // slide 0 back on the third word.
    let (r, _) = tagged();
    assert!(!r.blanked());
}

#[test]
fn after_a_tag_the_next_slide_waits_for_the_last_word() {
    let (mut r, mut t) = tagged();
    // Slide 0 is live again and the tag is already inside the lead: the next slide must not flip up again at
    // once. It waits for the slide's last word.
    for w in 9..=12 {
        t += 0.3;
        assert_eq!(r.tick(t, &at(w), Voice::Singing, 2.0, 0), None, "word {w}");
    }
    assert_eq!(r.tick(t + 0.3, &at(13), Voice::Singing, 2.0, 0), Some(Show(1)));
}

#[test]
fn a_step_back_in_mid_line_is_not_a_tag() {
    let (mut r, t7) = early_change();
    // One misheard word puts the place back from the middle of the last line (not from its end): the words
    // after it are still the tail of the early change, and the screen stays on slide 1.
    for (i, &w) in [8, 9, 6, 10, 11, 12].iter().enumerate() {
        assert_eq!(r.tick(t7 + (i + 1) as f64 * 0.3, &at(w), Voice::Singing, 2.0, 1), None, "word {w}");
    }
}

#[test]
fn stale_place_does_not_move_when_talk_pauses() {
    let s = hymn();
    let mut r = Rules::new(auto(2.0), &s);
    // In a talk the aligner drifts onto slide 1 ("come"), under a talking label: hold.
    for i in 0..=100 { assert_eq!(r.tick(10.0 + i as f64 * 0.1, &at(14), Voice::Speaking, 0.0, 0), None, "{i}") }
    // The talk pauses. The place is ten seconds old: no reason to move the screen.
    assert_eq!(r.tick(20.1, &at(14), Voice::Silence, 0.0, 0), None);
    // A word heard now is.
    assert_eq!(r.tick(25.0, &at(15), Voice::Singing, 2.0, 0), Some(Show(1)));
}

#[test]
fn pace_uses_when_words_were_sung() {
    // Three one-syllable words a second, delivered two at a time when the second one is over.
    let s = hymn();
    let mut r = Rules::new(auto(1.0), &s);
    for w in 0..=7usize { r.tick_sung((w | 1) as f64 / 3.0, w as f64 / 3.0, &at(w), Voice::Singing, 2.0, 0); }
    let pace = r.pace().expect("a pace");
    assert!((pace - 3.0).abs() < 0.05, "three syllables a second, was {pace}");
    // Delivered late and unevenly (a slow run of the model, then a quick one): still three a second.
    let delivered = [1.0, 1.0, 1.0, 1.5, 1.5, 3.0, 3.0, 3.0];
    let mut r = Rules::new(auto(1.0), &s);
    for (w, &now) in delivered.iter().enumerate() { r.tick_sung(now, w as f64 / 3.0, &at(w), Voice::Singing, 2.0, 0); }
    let pace = r.pace().expect("a pace");
    assert!((pace - 3.0).abs() < 0.05, "three syllables a second, was {pace}");
    // Timed by when they arrived, the same words give a pace that is far off.
    let mut r = Rules::new(auto(1.0), &s);
    for (w, &now) in delivered.iter().enumerate() { r.tick(now, &at(w), Voice::Singing, 2.0, 0); }
    let pace = r.pace().expect("a pace");
    assert!((pace - 3.0).abs() > 0.5, "the arrival times mislead, was {pace}");
}

#[test]
fn a_late_ear_does_not_delay_the_change() {
    // Every word reaches the rules 0.8 s after it was sung.
    let s = hymn();
    let mut r = Rules::new(auto(1.0), &s);
    for w in 0..=7usize {
        let sung = w as f64 / 3.0;
        assert_eq!(r.tick_sung(sung + 0.8, sung, &at(w), Voice::Singing, 2.0, 0), None, "word {w}");
    }
    // "god" (7) was sung with 2 s to go: the change is due 1 s after it was sung, not 1 s after it arrived.
    let t7 = 7.0 / 3.0;
    assert_eq!(r.tick(t7 + 0.9, &at(7), Voice::Singing, 2.0, 0), None);
    assert_eq!(r.tick(t7 + 1.0, &at(7), Voice::Singing, 2.0, 0), Some(Show(1)));
}

#[test]
fn a_believed_jump_back_a_slide_goes_back_at_once() {
    // The chorus sits just before the song's last slide, which is live and has been sung to its end.
    let s = song(&["how great is our god\nsing with me", "then sings my soul"]);
    let last_verse = |r: &mut Rules| {
        for (i, w) in (8..=11).enumerate() { assert_eq!(r.tick(i as f64 * 0.3, &at(w), Voice::Singing, 2.0, 1), None, "word {w}") }
    };
    let mut r = Rules::new(auto(2.0), &s);
    last_verse(&mut r);
    // The chorus again from its top. The aligner only believes a move to another slide after four words in a
    // row, so the first place it reports there is "our" (3). That is proof enough: no three words more (which
    // would reach the chorus's last line, where the place is held as the tail of an early change).
    assert_eq!(r.tick(2.4, &at(3), Voice::Singing, 2.0, 1), Some(Show(0)));
    // Not when someone was talking as those words came (the leader quotes the chorus): three words in a row.
    let mut r = Rules::new(auto(2.0), &s);
    last_verse(&mut r);
    assert_eq!(r.tick(2.2, &at(11), Voice::Speaking, 0.0, 1), None);
    assert_eq!(r.tick(2.4, &at(3), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(2.7, &at(4), Voice::Singing, 2.0, 1), None);
    // Nor when the place there is unsure.
    let mut r = Rules::new(auto(2.0), &s);
    last_verse(&mut r);
    assert_eq!(r.tick(2.4, &Position { confidence: 0.3, ..at(3) }, Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(2.5, &at(3), Voice::Singing, 2.0, 1), None, "turning sure later is one word, not a jump");
}

#[test]
fn a_tag_of_a_short_last_line_does_not_flash_the_old_slide() {
    // A last line of three words, and one of four: by the tag's third word (when it is known to be a tag) the
    // line is over or nearly over. Going back would put the old slide up for a moment and the next one straight
    // after it.
    for (lines, last) in [("we sing we sing\nour god reigns", 6), ("we sing we sing\nour great god reigns", 7)] {
        let s = song(&[lines, "come now"]);
        let mut r = Rules::new(auto(2.0), &s);
        let (mut t, acts) = sing_to(&mut r, 4, 0);
        assert_eq!(acts, [Show(1)], "{lines:?}");
        // Slide 1 is up. The singers finish the line, then sing it twice more.
        for w in (5..=last).chain(4..=last).chain(4..=last) {
            t += 1.0 / 3.0;
            assert_eq!(r.tick(t, &at(w), Voice::Singing, 2.0, 1), None, "{lines:?} word {w}");
            assert_eq!(r.tick(t + 0.1, &at(w), Voice::Singing, 2.0, 1), None, "{lines:?} word {w}, the next look");
        }
    }
}

#[test]
fn a_slide_sung_again_from_the_top_gets_its_lead_again() {
    let (mut r, mut t) = early_change();
    // The singers go back to the top of slide 0 and the screen follows (`three_words_back_go_back`).
    for w in 0..=1 {
        t += 0.3;
        assert_eq!(r.tick(t, &at(w), Voice::Singing, 2.0, 1), None, "word {w}");
    }
    t += 0.3;
    assert_eq!(r.tick(t, &at(2), Voice::Singing, 2.0, 1), Some(Show(0)));
    // They sing the whole slide again. The next slide comes up on the last line, 2 s ahead as the first time,
    // not only once the slide's last word has been heard.
    for w in 3..=6 {
        t += 0.3;
        assert_eq!(r.tick(t, &at(w), Voice::Singing, 2.0, 0), None, "word {w}");
    }
    assert_eq!(r.tick(t + 0.3, &at(7), Voice::Singing, 2.0, 0), Some(Show(1)));
    // A tag brought slide 0 back, and then it is sung again from the top: the same.
    let (mut r, mut t) = tagged();
    for w in 0..=6 {
        t += 0.3;
        assert_eq!(r.tick(t, &at(w), Voice::Singing, 2.0, 0), None, "after the tag, word {w}");
    }
    assert_eq!(r.tick(t + 0.3, &at(7), Voice::Singing, 2.0, 0), Some(Show(1)));
}

#[test]
fn switching_to_automatic_after_the_next_slides_first_word() {
    let s = hymn();
    let suggest = Settings { mode: Mode::Suggest, ..auto(2.0) };
    // The operator does not take the suggestion. The singers go on to slide 1's first word and pause after it;
    // the band plays for three seconds. Then the operator switches Suggest to Automatic.
    let mut r = Rules::new(suggest.clone(), &s);
    let (t, _) = sing_to(&mut r, 13, 0);
    assert_eq!(r.tick(t + 0.3, &at(14), Voice::Singing, 2.0, 0), Some(Suggest(1)));
    for i in 0..30 { r.tick(t + 0.6 + i as f64 * 0.1, &at(14), Voice::Music, 3.0, 0); }
    r.set_settings(auto(2.0));
    assert_eq!(r.tick(t + 4.0, &at(14), Voice::Music, 3.0, 0), Some(Show(1)), "the singers are on slide 1: it goes up");
    // Someone talked in the pause: the place may be left over from the talk. It waits for a word heard now.
    let mut r = Rules::new(suggest.clone(), &s);
    let (t, _) = sing_to(&mut r, 13, 0);
    r.tick(t + 0.3, &at(14), Voice::Singing, 2.0, 0);
    for i in 0..30 { r.tick(t + 0.6 + i as f64 * 0.1, &at(14), Voice::Speaking, 0.0, 0); }
    r.set_settings(auto(2.0));
    assert_eq!(r.tick(t + 4.0, &at(14), Voice::Music, 3.0, 0), None);
    assert_eq!(r.tick(t + 5.0, &at(15), Voice::Singing, 2.0, 0), Some(Show(1)));
    // The word itself came under a talking label: the same.
    let mut r = Rules::new(suggest, &s);
    let (t, _) = sing_to(&mut r, 13, 0);
    r.tick(t + 0.3, &at(14), Voice::Speaking, 0.0, 0);
    for i in 0..30 { r.tick(t + 0.6 + i as f64 * 0.1, &at(14), Voice::Music, 3.0, 0); }
    r.set_settings(auto(2.0));
    assert_eq!(r.tick(t + 4.0, &at(14), Voice::Music, 3.0, 0), None);
}

/// `texts`' last slide is live and its words `first..=last` are sung; then music until the words come off.
fn ended(settings: Settings, texts: &[&str], first: usize, last: usize) -> Rules {
    let s = song(texts);
    let live = texts.len() - 1;
    let mut r = Rules::new(settings, &s);
    for (i, w) in (first..=last).enumerate() { r.tick(i as f64 * 0.3, &at(w), Voice::Singing, 2.0, live); }
    let mut acts = vec![];
    for i in 0..80 { acts.extend(r.tick(2.0 + i as f64 * 0.1, &at(last), Voice::Music, 3.0, live)) }
    assert_eq!(acts, [Blank]);
    r
}

#[test]
fn after_the_end_a_short_last_line_comes_back() {
    // Two words ("be still"): three sung words in a row are there once the tag is sung round a second time.
    let mut r = ended(auto(2.0), &["amazing grace", "be still"], 2, 3);
    assert_eq!(r.tick(20.0, &at(2), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(20.4, &at(3), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(20.8, &at(2), Voice::Singing, 2.0, 1), Some(Unblank(1)));
    assert!(!r.blanked());
    // Three words: the line's own last word is the third.
    let mut r = ended(auto(2.0), &["amazing grace", "lord god almighty"], 2, 4);
    assert_eq!(r.tick(20.0, &at(2), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(20.4, &at(3), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(20.8, &at(4), Voice::Singing, 2.0, 1), Some(Unblank(1)));
    // Four words, and the ear misses one of them.
    let (_s, mut r) = ended_song();
    assert_eq!(r.tick(20.0, &at(2), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(20.4, &at(4), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(20.8, &at(5), Voice::Singing, 2.0, 1), Some(Unblank(1)));
    // Still not on one or two matching words of the next song.
    let mut r = ended(auto(2.0), &["amazing grace", "be still"], 2, 3);
    assert_eq!(r.tick(20.0, &at(3), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(23.0, &at(2), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(26.0, &at(3), Voice::Singing, 2.0, 1), None);
    assert!(r.blanked());
}

#[test]
fn after_the_end_spoken_and_old_words_are_not_words_in_a_row() {
    // The pastor quotes the last line; on its third word the room's label flickers to singing.
    let (_s, mut r) = ended_song();
    assert_eq!(r.tick(20.0, &at(2), Voice::Speaking, 0.0, 1), None);
    assert_eq!(r.tick(20.4, &at(3), Voice::Speaking, 0.0, 1), None);
    assert_eq!(r.tick(20.8, &at(4), Voice::Singing, 2.0, 1), None);
    assert!(r.blanked());
    // Three matching words minutes apart are not in a row either.
    let (_s, mut r) = ended_song();
    assert_eq!(r.tick(9.0, &at(2), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(60.0, &at(3), Voice::Singing, 2.0, 1), None);
    assert_eq!(r.tick(200.0, &at(4), Voice::Singing, 2.0, 1), None);
    assert!(r.blanked());
}

#[test]
fn after_the_end_singing_on_to_another_slide_brings_its_words_up() {
    let texts = ["amazing grace how sweet the sound", "how great is our god", "be still my soul"];
    // After the last slide and a pause the congregation goes into the chorus (slide 1) again. The aligner
    // believes that after four words in a row and reports "our" (9): the chorus comes up on it, not three
    // words later.
    let mut r = ended(auto(2.0), &texts, 11, 14);
    assert_eq!(r.tick(20.0, &at(9), Voice::Singing, 2.0, 2), Some(Unblank(1)));
    assert!(!r.blanked());
    // Someone was talking as those words came (a quoted line, then the label flickers to singing): three sung
    // words in a row, as on the last slide itself.
    let mut r = ended(auto(2.0), &texts, 11, 14);
    assert_eq!(r.tick(19.8, &at(14), Voice::Speaking, 0.0, 2), None);
    assert_eq!(r.tick(20.0, &at(9), Voice::Singing, 2.0, 2), None);
    assert_eq!(r.tick(20.3, &at(10), Voice::Singing, 2.0, 2), None);
    assert!(r.blanked());
    // Jumps set to two words in a row (Settings): that is less than three, so three are still asked for.
    let mut r = ended(Settings { jump_words: 2, ..auto(2.0) }, &texts, 11, 14);
    assert_eq!(r.tick(20.0, &at(7), Voice::Singing, 2.0, 2), None);
    assert_eq!(r.tick(20.3, &at(8), Voice::Singing, 2.0, 2), None);
    assert_eq!(r.tick(20.6, &at(9), Voice::Singing, 2.0, 2), Some(Unblank(1)));
    // The place had drifted off the song's last word before the words came off (talk): no jump is known, so
    // one word on another slide is one word.
    let s = song(&texts);
    let mut r = Rules::new(auto(2.0), &s);
    for (i, w) in (11..=14).enumerate() { r.tick(i as f64 * 0.3, &at(w), Voice::Singing, 2.0, 2); }
    let drifted = Position { confidence: 0.2, ..at(8) };
    let mut acts = vec![];
    for i in 0..80 { acts.extend(r.tick(2.0 + i as f64 * 0.1, &drifted, Voice::Speaking, 0.0, 2)) }
    assert_eq!(acts, [Blank]);
    assert_eq!(r.tick(20.0, &at(9), Voice::Singing, 2.0, 2), None);
    assert!(r.blanked());
}

#[test]
fn a_place_heard_while_not_looking_is_old_when_they_look_again() {
    let s = song(&["amazing grace how sweet", "the sound that saved", "a wretch like me"]);
    let mut r = Rules::new(auto(2.0), &s);
    assert_eq!(r.tick(0.0, &at(8), Voice::Singing, 2.0, 2), None);
    // Hold is on (the engine calls `missed`, not `tick`). The leader sings slide 0's line; then the band plays
    // for 26 s.
    for w in 0..=3 {
        let t = 1.0 + w as f64 * 0.3;
        r.missed(t, t, &at(w), Voice::Singing);
    }
    for i in 0..260 {
        let t = 2.0 + i as f64 * 0.1;
        r.missed(t, t, &at(3), Voice::Music);
    }
    // Hold is released. The place on slide 0 was heard 26 s ago: it moves nothing, whatever the label says.
    assert_eq!(r.tick(28.0, &at(3), Voice::Music, 3.0, 2), None);
    assert_eq!(r.tick(28.5, &at(3), Voice::Singing, 2.0, 2), None);
    // A word heard now does.
    assert_eq!(r.tick(40.0, &at(1), Voice::Singing, 2.0, 2), Some(Show(0)));
    // Released a moment after the word was heard, the place is fresh: it follows.
    let mut r = Rules::new(auto(2.0), &s);
    r.tick(0.0, &at(8), Voice::Singing, 2.0, 2);
    for w in 0..=3 {
        let t = 1.0 + w as f64 * 0.3;
        r.missed(t, t, &at(w), Voice::Singing);
    }
    assert_eq!(r.tick(2.5, &at(3), Voice::Singing, 2.0, 2), Some(Show(0)));
}

#[test]
fn an_intro_of_chords_waits_for_the_singing_like_any_instrumental_slide() {
    // A song whose first slide is its intro chords (no words in the room): auto-follow treats it as a slide with
    // nothing to sing, so the intro stays up while the band plays, and the first words come up as singing starts.
    let song = crate::model::Song { lyrics: "Intro\n[G] [D] [Em] [C]\n\nVerse 1\n[G]Amazing grace\nhow sweet the sound".into(), ..Default::default() };
    let slides = crate::slides::song_slides(&song, &crate::model::Settings { credits: "none".into(), ..Default::default() });
    let s = SongText::from_slides(&slides);
    assert!(s.slides[0].instrumental && !s.slides[1].instrumental, "the intro has nothing to sing");
    assert_eq!(s.words[0].text, "Amazing", "no chord is taken for a word");
    let mut r = Rules::new(auto(2.0), &s);
    for i in 0..100 { assert_eq!(r.tick(i as f64 * 0.1, &nowhere(), Voice::Music, 4.0, 0), None, "intro {i}") }
    assert_eq!(r.tick(12.0, &nowhere(), Voice::Singing, 2.0, 0), Some(Show(1)));
}
