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
    assert_eq!(r.tick(20.0, &at(15), Voice::Singing, 2.0, 2), Some(Suggest(3)));
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
    // The last line sung again (a tag): the words come back.
    assert_eq!(r.tick(46.0, &at(3), Voice::Singing, 2.0, 1), Some(Unblank(1)));
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
