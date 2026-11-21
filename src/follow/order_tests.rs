use super::*;
use crate::follow::engine::{Engine, FollowEvent, Light};
use crate::follow::review::Shown;
use crate::follow::rules::Mode;
use crate::follow::testing::{Fake, SONG, VERSE_1, VERSE_2, sing, text};

const VERSE_3: &str = "through many dangers toils and snares I have already come";

/// A take of a song with `n` slides where these slides went up by hand, each as (when, slide).
fn take(n: usize, clicks: &[(f64, usize)], until: f64) -> Take {
    Take { item: Some(uuid::Uuid::from_u128(7)), title: "Grace".into(), from: clicks[0].0, until, slides: (0..n).map(|i| format!("slide {i}")).collect(), labels: vec![], heard: vec![],
        screen: clicks.iter().map(|&(at, slide)| Shown { at, slide, words: true, by_hand: true }).collect() }
}

#[test]
fn the_order_slides_were_up_in_is_the_order_sung() {
    // Verse, chorus, verse 2, the chorus again, and once more.
    let t = take(3, &[(0.0, 0), (20.0, 1), (40.0, 2), (60.0, 1)], 100.0);
    assert_eq!(learn(&t), Some(vec![0, 1, 2, 1]));
    // A slide passed on the way (Next, Next) and a slip put right at once are not sung.
    let t = take(4, &[(0.0, 0), (20.0, 1), (20.4, 2), (21.0, 3), (40.0, 0), (40.5, 1)], 60.0);
    assert_eq!(learn(&t), Some(vec![0, 3, 1]));
    // The same slide again after a slip is still one stay.
    let t = take(3, &[(0.0, 0), (20.0, 2), (20.5, 1), (21.0, 2), (40.0, 0)], 60.0);
    assert_eq!(learn(&t), Some(vec![0, 2, 0]));
}

#[test]
fn a_song_sung_as_its_slides_are_listed_teaches_nothing() {
    assert_eq!(learn(&take(3, &[(0.0, 0), (20.0, 1), (40.0, 2)], 60.0)), None);
    assert_eq!(learn(&take(3, &[(0.0, 1), (20.0, 2)], 60.0)), None, "from the second slide on, still as listed");
    assert_eq!(learn(&take(3, &[(0.0, 0)], 60.0)), None, "one slide is no order");
    assert_eq!(learn(&take(3, &[(0.0, 0), (0.3, 1), (0.6, 2), (0.9, 0)], 1.2)), None, "clicked through in a second");
    // With its words off a slide is not being sung.
    let mut t = take(3, &[(0.0, 0), (20.0, 1), (40.0, 0)], 60.0);
    t.screen[2].words = false;
    assert_eq!(learn(&t), None);
}

#[test]
fn how_long_each_slide_was_up_is_learned_too() {
    // To when the next sung slide came up; the last one to the end of the take. A slip in between is not a stay.
    let t = take(3, &[(0.0, 0), (20.0, 2), (20.5, 1), (41.0, 0)], 70.0);
    assert_eq!(learn_timed(&t), Some((vec![0, 1, 0], vec![20.5, 20.5, 29.0])));
    // A song sung as listed has times as well (and no order of its own).
    let t = take(3, &[(0.0, 0), (20.0, 1), (45.0, 2)], 60.0);
    assert_eq!(learn_timed(&t), Some((vec![0, 1, 2], vec![20.0, 25.0, 15.0])));
    assert_eq!(learn(&t), None);
    assert_eq!(lessons(&[t.clone()])[0].2, SungOrder { slides: vec![0, 1, 2], mark: mark(&t.slides), secs: vec![20.0, 25.0, 15.0] });
    assert_eq!(learn_timed(&take(3, &[(0.0, 0)], 60.0)), None);
    // An order saved before 1.4.5 has none.
    let old: SungOrder = serde_json::from_str(r#"{"slides":[0,1,0],"mark":5}"#).unwrap();
    assert!(old.secs.is_empty());
}

#[test]
fn the_route_knows_how_long_each_place_takes() {
    // The slide the order leaves out stands first and has no time; the last place has none to give (nothing after it).
    let r = Route::new(4, &[0, 1, 2, 1]).timed(&[0, 1, 2, 1], &[20.0, 10.0, 0.0, 30.0]);
    assert_eq!((0..r.len()).map(|p| r.secs(p)).collect::<Vec<_>>(), [None, Some(20.0), Some(10.0), None, None]);
    assert_eq!(r.timed_places(), 2);
    // As listed, from the second slide on: a place is its slide.
    let r = Route::new(4, &[1, 2, 3]).timed(&[1, 2, 3], &[20.0, 10.0, 30.0]);
    assert!(r.plain());
    assert_eq!((0..r.len()).map(|p| r.secs(p)).collect::<Vec<_>>(), [None, Some(20.0), Some(10.0), None]);
    // Times that do not fit the order, or no times: none.
    for r in [Route::new(4, &[0, 1, 2, 1]).timed(&[0, 1, 2, 1], &[20.0]), Route::new(4, &[0, 1, 2, 1]), Route::new(4, &[0, 9]).timed(&[0, 9], &[5.0, 5.0])] {
        assert_eq!(r.timed_places(), 0);
    }
}

#[test]
fn of_two_goes_at_a_song_the_longer_one_counts() {
    let full = take(3, &[(0.0, 0), (20.0, 1), (40.0, 2), (60.0, 1)], 100.0);
    let bridge_again = take(3, &[(200.0, 2), (220.0, 1), (240.0, 2)], 260.0);
    let other = Take { item: Some(uuid::Uuid::from_u128(8)), title: "Other".into(), ..take(2, &[(300.0, 1), (320.0, 0)], 340.0) };
    let nameless = Take { item: None, ..take(2, &[(400.0, 1), (420.0, 0)], 440.0) };
    let got = lessons(&[full.clone(), bridge_again, other, nameless]);
    assert_eq!(got.len(), 2);
    assert_eq!((got[0].1.as_str(), &got[0].2.slides), ("Grace", &vec![0, 1, 2, 1]));
    assert_eq!(got[0].2.mark, mark(&full.slides));
    assert_eq!((got[1].1.as_str(), &got[1].2.slides), ("Other", &vec![1, 0]));
    assert_eq!(spoken(&got[0].2.slides), "1 2 3 2");
}

#[test]
fn the_mark_changes_with_the_words_and_their_place() {
    assert_eq!(mark(&["a", "b"]), mark(&["a".to_string(), "b".to_string()]));
    assert_ne!(mark(&["a", "b"]), mark(&["b", "a"]));
    assert_ne!(mark(&["ab", ""]), mark(&["a", "b"]));
    assert_ne!(mark(&["a", "b"]), mark(&["a", "c"]));
    assert_eq!(mark_of(&[text("a"), text("b")]), mark(&["a", "b"]));
}

#[test]
fn the_route_lays_the_song_out_as_it_is_sung() {
    let r = Route::new(4, &[0, 1, 2, 1, 1]);
    assert!(!r.plain());
    assert_eq!(r.learned(), [0, 1, 2, 1, 1]);
    // The slide the order leaves out stands before it.
    assert_eq!((0..r.len()).map(|p| r.slide(p)).collect::<Vec<_>>(), [3, 0, 1, 2, 1, 1]);
    assert_eq!(r.start(0), 1);
    assert_eq!(r.start(3), 0);
    // The chorus after the second verse is the nearest one further on; back to the first verse is a place before.
    assert_eq!(r.place(3, 1), 4);
    assert_eq!(r.place(4, 1), 4, "still the slide that is up: the place stays");
    assert_eq!(r.place(4, 0), 1);
    assert_eq!(r.place(5, 3), 0);
    let slides: Vec<_> = ["a", "b", "c", "d"].iter().map(|t| text(t)).collect();
    assert_eq!(r.laid_out(&slides).iter().map(words).collect::<Vec<_>>(), ["d", "a", "b", "c", "b", "b"]);
}

#[test]
fn no_order_or_a_bad_one_is_the_slide_list() {
    for order in [vec![], vec![1], vec![0, 9], vec![0; MAX_LEN + 1]] {
        let r = Route::new(3, &order);
        assert!(r.plain() && r.learned().is_empty() && r.len() == 3, "{order:?}");
        assert_eq!((r.start(2), r.place(0, 2), r.slide(1)), (2, 2, 1));
    }
    assert!(Route::new(3, &[0, 1, 2]).plain(), "the list's own order");
}

/// Verse 1, verse 2, then verse 1 again, as learned at practice: at the end of verse 2 the first verse comes back
/// up by itself, before a word of it is sung. (Unlearned, that is a jump: four words late.)
#[test]
fn the_follower_expects_the_learned_order() {
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.order = vec![0, 1, 0, 2]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    assert_eq!(host.shown(), ["show 1"]);
    sing(&mut e, host.as_ref(), VERSE_2, &mut t);
    assert_eq!(host.shown(), ["show 1", "show 0"], "back to the first verse as the second ends");
    assert_eq!(e.state().light, Light::Sure);
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    assert_eq!(host.shown(), ["show 1", "show 0", "show 2"], "and on to the third, not the second again");
    let trace = e.take_trace().join("\n");
    assert!(trace.contains("expecting the order sung at practice: 1 2 1 3"), "{trace}");
    assert!(trace.contains("SHOW slide 1:") && trace.contains("SHOW slide 3:"), "the log names the song's own slides: {trace}");
}

/// A verse sung twice over: the singers go on along the route, and the slide that is already up is left alone.
#[test]
fn a_slide_sung_twice_over_stays_up_and_then_moves_on() {
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.order = vec![0, 0, 1]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    assert!(host.shown().is_empty(), "the first verse comes again: nothing to change");
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    assert_eq!(host.shown(), ["show 1"]);
}

/// The band leaves the learned order after all: found as before, by the words.
#[test]
fn off_the_learned_order_the_singers_are_still_found() {
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.order = vec![0, 1, 0, 2]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    sing(&mut e, host.as_ref(), VERSE_3, &mut t);
    assert_eq!(host.slide(), 2, "{:?}", host.shown());
}

/// The operator's click is placed on the route: after the second verse by hand, the first verse is what comes next.
#[test]
fn a_click_is_placed_on_the_learned_order() {
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.order = vec![0, 1, 0, 2]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "amazing grace", &mut t);
    host.operator_show(1);
    e.step(host.as_ref(), t, None);
    sing(&mut e, host.as_ref(), VERSE_2, &mut t);
    assert_eq!(host.shown(), ["show 0"]);
    // Suggest names the song's own slide too.
    let host = Fake::song(Mode::Suggest);
    host.change(|l| l.order = vec![0, 1, 0, 2]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    assert_eq!(e.state().suggest, Some(1));
    host.operator_show(1);
    e.step(host.as_ref(), t, Some(FollowEvent::Voice(crate::follow::rules::Voice::Singing, t)));
    sing(&mut e, host.as_ref(), VERSE_2, &mut t);
    assert_eq!(e.state().suggest, Some(0));
    assert_eq!(SONG.len(), 3);
}
