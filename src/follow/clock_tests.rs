use super::*;
use crate::follow::align::Heard;
use crate::follow::engine::{Engine, FollowEvent, Host, Light};
use crate::follow::lyrics::sound_key;
use crate::follow::rules::{Mode, Voice};
use crate::follow::testing::{Fake, SONG, VERSE_1, VERSE_2, sing, text};

fn song() -> SongText { SongText::from_slides(&SONG.iter().map(|t| text(t)).collect::<Vec<_>>()) }
/// Practice: the first slide up for 8 s, the second for 10.
fn route() -> Route { Route::new(3, &[0, 1, 2]).timed(&[0, 1, 2], &[8.0, 10.0, 12.0]) }

#[test]
fn the_clock_counts_from_the_first_word_heard_on_the_slide() {
    let (text, route) = (song(), route());
    let mut c = Clock::default();
    assert_eq!(c.at(&route, 0), None, "nobody heard yet");
    // "grace" (the slide's fourth syllable) sung at 11 s, four syllables a second: the slide began at 10.
    c.heard(&route, &text, 0, 1, 11.0, 4.0);
    assert_eq!(c.at(&route, 0), Some(10.0 + 8.0 - CLOCK_LEAD));
    assert_eq!(c.at(&route, 1), None, "only the slide the singers were heard on");
    // Later words of the slide do not move its beginning.
    c.heard(&route, &text, 0, 3, 12.0, 3.0);
    assert_eq!(c.at(&route, 0), Some(10.0 + 8.0 - CLOCK_LEAD));
    assert!(!c.due(&route, 0, 16.9));
    assert!(c.due(&route, 0, 17.0));
    // The last slide is never left, and a slide practice has no time for neither.
    c.heard(&route, &text, 2, text.slides[2].first_word, 40.0, 3.0);
    assert_eq!(c.at(&route, 2), None);
    c.heard(&route, &text, 0, 0, 50.0, 3.0);
    assert_eq!(c.at(&Route::new(3, &[]), 0), None);
    c.reset();
    assert_eq!(c.at(&route, 0), None);
}

#[test]
fn words_heard_early_on_the_slide_hold_it() {
    let (text, route) = (song(), route());
    let mut c = Clock::default();
    c.heard(&route, &text, 0, 0, 10.5, 3.0);
    // Sunday takes longer: the first line again, heard a moment ago.
    c.heard(&route, &text, 0, 2, 16.5, 3.0);
    assert!(!c.due(&route, 0, 17.5), "they are surely not through with it");
    assert!(c.due(&route, 0, 19.6), "nothing heard for three seconds: by the clock");
    // On the last line the clock is free to go.
    c.heard(&route, &text, 0, text.slides[0].last_line_first_word, 20.0, 3.0);
    assert!(c.due(&route, 0, 20.1));
}

#[test]
fn sundays_tempo_stretches_practices_times() {
    let (text, route) = (song(), route());
    let mut c = Clock::default();
    c.heard(&route, &text, 0, 0, 11.0, 3.0);
    // The second slide begins after 9.6 s where practice took 8: a fifth slower, believed by half.
    c.heard(&route, &text, 1, text.slides[1].first_word, 19.6 + 1.0 / 3.0, 3.0);
    assert!((c.ratio() - 1.1).abs() < 1e-6, "{}", c.ratio());
    assert!((c.at(&route, 1).unwrap() - (19.6 + 11.0 - CLOCK_LEAD)).abs() < 1e-6);
    // A slide that took twice as long was not sung the same way: no tempo from it.
    let mut c = Clock::default();
    c.heard(&route, &text, 0, 0, 10.0, 3.0);
    c.heard(&route, &text, 1, text.slides[1].first_word, 30.0, 3.0);
    assert_eq!(c.ratio(), 1.0);
    // Nor from a jump: only from one slide to the next.
    c.heard(&route, &text, 0, 0, 40.0, 3.0);
    assert_eq!(c.ratio(), 1.0);
}

fn timed(mode: Mode) -> std::sync::Arc<Fake> {
    let host = Fake::song(mode);
    // Practice had the first verse up for 6 s and the second for 6.
    host.change(|l| { l.order = vec![0, 1, 2]; l.secs = vec![6.0, 6.0, 6.0] });
    host
}

fn wait(e: &mut Engine, host: &dyn Host, t: &mut f64, secs: f64) {
    let until = *t + secs;
    while *t < until {
        *t += 0.1;
        e.step(host, *t, Some(FollowEvent::Voice(Voice::Singing, *t)));
    }
}

/// The ear makes out the first line and then nothing more: the next slide still comes up when practice had it up.
#[test]
fn when_the_ear_loses_the_singers_the_next_slide_comes_up_by_the_clock() {
    let host = timed(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "amazing grace how sweet", &mut t);
    wait(&mut e, host.as_ref(), &mut t, 2.5);
    assert!(host.shown().is_empty(), "not yet: {t}");
    wait(&mut e, host.as_ref(), &mut t, 1.0);
    assert_eq!(host.shown(), ["show 1"]);
    let trace = e.take_trace().join("\n");
    assert!(trace.contains("practice knows how long 2 of its slides take"), "{trace}");
    assert!(trace.contains("SHOW slide 2: as long as it took at practice"), "{trace}");
    // It does not run on by itself: the second slide waits until somebody is heard singing it.
    wait(&mut e, host.as_ref(), &mut t, 20.0);
    assert_eq!(host.shown(), ["show 1"]);
    // Heard there, the words lead again: the third slide as the second verse ends.
    sing(&mut e, host.as_ref(), VERSE_2, &mut t);
    assert_eq!(host.shown(), ["show 1", "show 2"]);
    assert_eq!(e.state().light, Light::Sure);
}

/// Without times from practice nothing moves by the clock, and Suggest only points.
#[test]
fn no_times_no_clock_and_suggest_only_points() {
    let host = Fake::song(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "amazing grace how sweet", &mut t);
    wait(&mut e, host.as_ref(), &mut t, 20.0);
    assert!(host.shown().is_empty());
    let host = timed(Mode::Suggest);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "amazing grace how sweet", &mut t);
    wait(&mut e, host.as_ref(), &mut t, 5.0);
    assert!(host.shown().is_empty());
    assert_eq!(e.state().suggest, Some(1));
}

/// The band was wrong-footed by the clock (Sunday took longer): the old slide's words bring it back.
#[test]
fn still_on_the_old_slide_after_all_its_words_bring_it_back() {
    let host = timed(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "amazing grace how sweet", &mut t);
    wait(&mut e, host.as_ref(), &mut t, 4.5);
    assert_eq!(host.shown(), ["show 1"]);
    // They start the first verse over.
    sing(&mut e, host.as_ref(), "amazing grace how sweet the sound", &mut t);
    assert_eq!(host.shown(), ["show 1", "show 0"], "{}", e.take_trace().join("
"));
}

/// A move by hand voids the count: the clock waits for the singers to be heard on the operator's slide.
#[test]
fn a_move_by_hand_stops_the_clock() {
    let host = timed(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "amazing grace how sweet", &mut t);
    host.operator_show(0);
    host.change(|l| l.words_up = true);
    host.operator_show(1);
    e.step(host.as_ref(), t, None);
    host.operator_show(0);
    e.step(host.as_ref(), t, None);
    wait(&mut e, host.as_ref(), &mut t, 20.0);
    assert!(host.shown().is_empty());
    let _ = (VERSE_1, sound_key("x"), Heard { key: String::new(), at: 0.0, conf: 0.0 });
}
