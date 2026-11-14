use super::*;
use crate::follow::lyrics::sound_key;
use crate::follow::testing::*;

#[test]
fn auto_shows_next_slide_via_command() {
    use crate::model::{ItemKind, ServiceItem, Song};
    let app = crate::app::test_app();
    let song = Song { id: Uuid::new_v4(), title: "Amazing Grace".into(), lyrics: format!("Verse 1\n{}\n\nVerse 2\n{}", SONG[0], SONG[1]), ..Default::default() };
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None };
    let item_id = item.id;
    {
        let mut d = app.data.lock().unwrap();
        d.songs.push(song);
        d.service_mut().unwrap().items.push(item);
        d.settings.lines_per_slide = 2;
        d.settings.follow.mode = Mode::Auto;
    }
    let out = app.command(Envelope { id: "op".into(), base: None, cmd: Command::Show { item_id, slide: 0 } });
    assert!(out.ok);
    let host = AppHost(Arc::downgrade(&app));
    let live = host.live().unwrap();
    assert!(live.is_song && live.words_up);
    assert_eq!((live.slide, live.mode), (0, Mode::Auto));
    let slides = live.slides.clone();
    let sung: Vec<String> = SongText::from_slides(&slides).words.iter().filter(|w| w.slide == 0).map(|w| w.text.clone()).collect();
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, &host, &sung.join(" "), &mut t);
    let cursor = app.live.lock().unwrap().cursor.clone().unwrap();
    assert_eq!((cursor.item_id, cursor.slide), (item_id, 1), "Auto took slide 2 live through the live engine");
    assert_eq!(e.state().light, Light::Sure);
}

#[test]
fn manual_move_wins_and_relocks() {
    let host = Fake::song(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "amazing grace how", &mut t);
    // Mid-line, the operator goes on to slide 2.
    host.operator_show(1);
    sing(&mut e, host.as_ref(), "sweet the sound that saved a wretch like me", &mut t);
    assert_eq!(host.shown(), Vec::<String>::new(), "no fighting for the rest of the verse");
    assert_eq!(host.slide(), 1);
    assert_eq!(e.state().light, Light::Holding);
    // The singers reach the operator's slide: Auto locks on again and carries on from there.
    sing(&mut e, host.as_ref(), VERSE_2, &mut t);
    assert_eq!(host.shown(), ["show 2"]);
    assert_eq!(host.slide(), 2);
}

#[test]
fn singers_on_an_earlier_slide_than_the_operators_are_followed_there() {
    // The operator clicks slide 3 as verse 1 ends, but the singers go on to verse 2 (slide 2).
    let host = Fake::song(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "amazing grace how sweet the sound that saved a wretch", &mut t);
    assert_eq!(host.shown(), ["show 1"]);
    host.operator_show(2);
    // The rest of verse 1 only finishes the old slide: the operator's slide stays.
    sing(&mut e, host.as_ref(), "like me", &mut t);
    assert_eq!((host.slide(), e.state().light), (2, Light::Holding));
    // Verse 2, surely sung for a line: Automatic locks on to the singers (it used to wait for words of the
    // operator's slide for ever) and brings their slide up.
    sing(&mut e, host.as_ref(), "I once was lost but now am found", &mut t);
    assert_eq!(host.slide(), 1, "{:?}", host.shown());
    assert_eq!(e.state().light, Light::Sure);
    // And goes on from there.
    sing(&mut e, host.as_ref(), "was blind but now I see", &mut t);
    assert_eq!(host.slide(), 2);
}

#[test]
fn the_leader_going_back_to_the_slide_before_is_followed_after_a_click() {
    // The operator presses Next early, in the chorus's first line; the singers finish the chorus, and then the
    // leader sings it again from its top.
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.slides = vec![text("my chains are gone I've been set free\nmy God my Savior has ransomed me"),
        text("the earth shall soon dissolve like snow\nthe sun forbear to shine")]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "my chains are gone I've been", &mut t);
    host.operator_show(1);
    sing(&mut e, host.as_ref(), "set free my God my Savior has ransomed me", &mut t);
    assert_eq!(host.shown(), Vec::<String>::new(), "the rest of the chorus only finishes it");
    assert_eq!((host.slide(), e.state().light), (1, Light::Holding));
    sing(&mut e, host.as_ref(), "my chains are gone I've been set free my God my", &mut t);
    assert_eq!(host.slide(), 0, "the chorus came back: {:?}", host.shown());
}

#[test]
fn leaving_the_song_stops_auto() {
    let host = Fake::song(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "amazing grace how sweet the sound", &mut t);
    // The operator goes to the sermon.
    host.change(|l| { l.item_id = Uuid::new_v4(); l.is_song = false; l.slides.clear(); l.slide = 0 });
    sing(&mut e, host.as_ref(), "that saved a wretch like me", &mut t);
    for i in 0..100 { e.step(host.as_ref(), t + i as f64 * 0.1, Some(FollowEvent::Voice(Voice::Music, t))) }
    assert_eq!(host.shown(), Vec::<String>::new());
    assert_eq!(e.state().light, Light::Off);
    // A move the follower decided before the change is refused by the live version.
    let stale = host.live().unwrap().version - 1;
    assert!(!host.show(host.item(), 1, stale));
}

#[test]
fn hold_stops_moves() {
    let host = Fake::song(Mode::Auto);
    let mut e = Engine::new();
    e.set_hold(true);
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    for i in 0..60 { e.step(host.as_ref(), t + i as f64 * 0.1, None) }
    assert_eq!(host.shown(), Vec::<String>::new());
    assert_eq!(e.state().light, Light::Holding);
    assert!(e.state().holding);
    e.set_hold(false);
    sing(&mut e, host.as_ref(), "I once was lost but now am found", &mut t);
    assert_eq!(host.shown(), ["show 1"], "the singers are on slide 2: it follows again");
}

#[test]
fn video_playing_never_moves() {
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.video_playing = true);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    sing(&mut e, host.as_ref(), VERSE_2, &mut t);
    for i in 0..60 { e.step(host.as_ref(), t + i as f64 * 0.1, Some(FollowEvent::Voice(Voice::Music, t))) }
    assert_eq!(host.shown(), Vec::<String>::new());
}

#[test]
fn non_song_item_idle() {
    let host = Fake::song(Mode::Auto);
    host.change(|l| { l.is_song = false; l.slides = vec![text("Welcome"), text("Announcements")] });
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "welcome announcements", &mut t);
    for i in 0..60 { e.step(host.as_ref(), t + i as f64 * 0.1, Some(FollowEvent::Voice(Voice::Music, t))) }
    assert_eq!(host.shown(), Vec::<String>::new());
    assert_eq!(e.state(), FollowState::default());
    // Nothing live at all.
    *host.live.lock().unwrap() = None;
    e.step(host.as_ref(), 99.0, None);
    assert_eq!(e.state().light, Light::Off);
}

#[test]
fn suggest_sets_highlight_not_live() {
    let host = Fake::song(Mode::Suggest);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    assert_eq!(host.shown(), Vec::<String>::new());
    assert_eq!(host.slide(), 0);
    assert_eq!(e.state().suggest, Some(1));
    assert_eq!(e.state().mode, Mode::Suggest);
    assert_eq!(e.state().line.as_deref(), Some("that saved a wretch like me"));
    // The operator takes it: the highlight goes.
    host.operator_show(1);
    e.step(host.as_ref(), t + 0.1, None);
    assert_eq!(e.state().suggest, None);
}

#[test]
fn auto_blanks_and_brings_words_back() {
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.settings.blank_after_s = Some(3.0));
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    assert_eq!(host.shown(), ["show 1"]);
    for i in 0..40 { e.step(host.as_ref(), t + i as f64 * 0.1, Some(FollowEvent::Voice(Voice::Music, t))) }
    assert_eq!(host.shown(), ["show 1", "blank"]);
    assert_eq!(e.state().light, Light::Blank);
    t += 4.0;
    sing(&mut e, host.as_ref(), "I once", &mut t);
    assert_eq!(host.shown(), ["show 1", "blank", "show 1"]);
}

#[test]
fn input_lost_goes_manual_and_tells() {
    let host = Fake::song(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "amazing grace", &mut t);
    e.step(host.as_ref(), t, Some(FollowEvent::InputLost("device unplugged".into())));
    e.step(host.as_ref(), t, Some(FollowEvent::InputLost("device unplugged".into())));
    assert_eq!(host.notices.lock().unwrap().len(), 1);
    assert!(host.notices.lock().unwrap()[0].contains("device unplugged"));
    assert_eq!(e.state().mode, Mode::Manual);
    sing(&mut e, host.as_ref(), "how sweet the sound that saved a wretch like me", &mut t);
    assert_eq!(host.shown(), Vec::<String>::new());
}

#[test]
fn follower_thread_moves_and_stops() {
    let host = Fake::song(Mode::Auto);
    let f = Follower::start_with(host.clone());
    for w in VERSE_1.split_whitespace() {
        let at = f.now();
        f.feed(FollowEvent::Voice(Voice::Singing, at));
        f.feed(FollowEvent::Heard(Heard { key: sound_key(w), at, conf: 1.0 }));
        std::thread::sleep(Duration::from_millis(40));
    }
    let until = Instant::now() + Duration::from_secs(5);
    while host.slide() != 1 && Instant::now() < until { std::thread::sleep(Duration::from_millis(20)) }
    assert_eq!(host.slide(), 1);
    f.set_hold(true);
    assert!(f.holding());
    f.set_mode(host.item(), Mode::Suggest);
    let until = Instant::now() + Duration::from_secs(5);
    while host.modes.lock().unwrap().is_empty() && Instant::now() < until { std::thread::sleep(Duration::from_millis(20)) }
    assert_eq!(host.modes.lock().unwrap().as_slice(), [(host.item(), Mode::Suggest)]);
    let until = Instant::now() + Duration::from_secs(5);
    while !f.state().holding && Instant::now() < until { std::thread::sleep(Duration::from_millis(20)) }
    assert!(f.state().holding);
    drop(f);
}

#[test]
fn log_records_reason_and_confidence() {
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.settings.blank_after_s = Some(3.0));
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    for i in 0..40 { e.step(host.as_ref(), t + i as f64 * 0.1, Some(FollowEvent::Voice(Voice::Music, t))) }
    let log = e.take_log();
    assert_eq!(log.iter().map(|l| l.action).collect::<Vec<_>>(), [LogAction::Show(1), LogAction::Blank]);
    assert!(log.iter().all(|l| l.item == host.item()));
    let show = &log[0];
    assert_eq!(show.reason, "heard \"sweet the sound that\"", "the last words heard when it changed, 2 s ahead");
    assert!(show.confidence >= 0.6 && show.confidence <= 1.0, "{}", show.confidence);
    assert_eq!(log[1].reason, "no singing for 3 s");
    assert!(log[1].at > show.at);
    let line = show.line();
    assert!(line.starts_with("0:0") && line.contains("Slide 2") && line.ends_with("%)"), "{line}");
    assert!(e.take_log().is_empty(), "taken once");
    // The follower thread keeps them for the session.
    let host = Fake::song(Mode::Auto);
    let f = Follower::start_with(host.clone());
    for w in VERSE_1.split_whitespace() {
        let at = f.now();
        f.feed(FollowEvent::Voice(Voice::Singing, at));
        f.feed(FollowEvent::Heard(Heard { key: sound_key(w), at, conf: 1.0 }));
    }
    let until = Instant::now() + Duration::from_secs(5);
    while f.log().is_empty() && Instant::now() < until { std::thread::sleep(Duration::from_millis(20)) }
    assert_eq!(f.log().first().map(|l| l.action), Some(LogAction::Show(1)));
}

#[test]
fn no_words_only_blanks_never_moves() {
    // The ear can't tell words yet: with a words-off timer on, singing, beats and quiet only blank and bring the
    // words back.
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.settings.blank_after_s = Some(3.0));
    let mut e = Engine::new();
    e.step(host.as_ref(), 0.0, Some(FollowEvent::Listening { words: false }));
    let mut t = 0.0;
    let run = |e: &mut Engine, voice: Voice, secs: f64, t: &mut f64| {
        for _ in 0..(secs * 10.0) as usize {
            *t += 0.1;
            e.step(host.as_ref(), *t, Some(FollowEvent::Voice(voice, *t)));
            e.step(host.as_ref(), *t, Some(FollowEvent::Onsets(3.0, *t)));
        }
    };
    run(&mut e, Voice::Singing, 30.0, &mut t);
    assert_eq!(host.shown(), Vec::<String>::new(), "a minute of singing at a steady beat never moves on by pace alone");
    assert_eq!(e.state().light, Light::Listening);
    run(&mut e, Voice::Music, 4.0, &mut t);
    assert_eq!(host.shown(), ["blank"]);
    run(&mut e, Voice::Singing, 1.0, &mut t);
    assert_eq!(host.shown(), ["blank", "show 0"], "words back on the same slide as singing starts");
    // The operator moves on by hand: no re-lock is needed for blanking to go on working.
    host.operator_show(1);
    run(&mut e, Voice::Singing, 20.0, &mut t);
    run(&mut e, Voice::Silence, 4.0, &mut t);
    run(&mut e, Voice::Singing, 1.0, &mut t);
    assert_eq!(host.shown(), ["blank", "show 0", "blank", "show 1"]);
    assert_eq!(host.slide(), 1);
    let log = e.take_log();
    assert_eq!(log[1].reason, "singing again");
    // A real ear later: words count again.
    e.step(host.as_ref(), t, Some(FollowEvent::Listening { words: true }));
}

/// Verse 1 with a 1 s lead. Word i reaches the follower at i/3 + 0.5 s on its own clock, `lag` seconds after it
/// was sung, and the sound input's clock reads `clock` seconds more than the follower's. What was shown, and
/// when the first change was made (the follower's clock).
fn sung_late(clock: f64, lag: f64) -> (Vec<String>, f64) {
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.settings.lead_s = 1.0);
    let mut e = Engine::new();
    for (i, w) in VERSE_1.split_whitespace().enumerate() {
        let d = i as f64 / 3.0 + 0.5;
        e.step(host.as_ref(), d, Some(FollowEvent::Voice(Voice::Singing, d + clock)));
        e.step(host.as_ref(), d, Some(FollowEvent::Heard(Heard { key: sound_key(w), at: d + clock - lag, conf: 1.0 })));
    }
    let log = e.take_log();
    (host.shown(), log.first().expect("a change").at)
}

#[test]
fn words_stamped_when_sung_change_sooner() {
    // The same words reach the follower at the same moments. In A the ear says they were sung just then; in B,
    // that they were sung half a second earlier (what a real ear's delay looks like).
    let (shown_a, at_a) = sung_late(0.0, 0.0);
    let (shown_b, at_b) = sung_late(0.0, 0.5);
    assert_eq!(shown_a, ["show 1"]);
    assert_eq!(shown_b, ["show 1"]);
    // B's singers are half a second further on than the arrival times say: the next slide is due sooner.
    assert!(at_b < at_a, "{at_b} should be before {at_a}");
}

#[test]
fn the_sound_clock_may_start_anywhere() {
    let (shown, at) = sung_late(0.0, 0.5);
    // The sound input's clock is 100 s behind the follower's: nothing changes.
    let (shown_far, at_far) = sung_late(-100.0, 0.5);
    assert_eq!(shown_far, shown);
    assert!((at_far - at).abs() < 1e-9, "{at_far} should be {at}");
}

#[test]
fn operator_back_mid_slide_locks_on_again() {
    let host = Fake::song(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    assert_eq!(host.shown(), ["show 1"]);
    sing(&mut e, host.as_ref(), "I once was lost", &mut t);
    // The operator goes back a slide by mistake, then forward again: the singers are mid-slide.
    host.operator_show(0);
    e.step(host.as_ref(), t, None);
    host.operator_show(1);
    sing(&mut e, host.as_ref(), "blind but now I see", &mut t);
    assert_eq!(e.state().light, Light::Sure);
    assert_eq!(host.shown().last().map(String::as_str), Some("show 2"));
}

#[test]
fn a_place_heard_during_a_hold_is_old_when_the_hold_ends() {
    let host = Fake::song(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    sing(&mut e, host.as_ref(), VERSE_2, &mut t);
    assert_eq!(host.slide(), 2);
    let before = host.shown();
    // Hold. The leader goes back to the first verse for a line (the aligner follows him there: the light shows
    // that line); then the band plays for 26 s.
    e.set_hold(true);
    sing(&mut e, host.as_ref(), "amazing grace how sweet the sound", &mut t);
    assert_eq!(e.state().line.as_deref(), Some("amazing grace how sweet the sound"));
    for _ in 0..260 {
        t += 0.1;
        e.step(host.as_ref(), t, Some(FollowEvent::Voice(Voice::Music, t)));
    }
    // Hold is released with nobody singing. That place was heard 26 s ago, not just now: the screen stays.
    e.set_hold(false);
    e.step(host.as_ref(), t, None);
    for _ in 0..20 {
        t += 0.1;
        e.step(host.as_ref(), t, Some(FollowEvent::Voice(Voice::Music, t)));
    }
    assert_eq!(host.shown(), before, "an old place moved the screen as the hold ended");
    assert_eq!(host.slide(), 2);
    // The singers come in again where the leader was: a word heard now moves it.
    sing(&mut e, host.as_ref(), "that", &mut t);
    assert_eq!(host.shown().last().map(String::as_str), Some("show 0"));
}

#[test]
fn the_old_slides_last_line_does_not_prove_the_operators_slide() {
    // The chorus is on two slides in a row, then a verse.
    let chorus = "my chains are gone\nI've been set free";
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.slides = vec![text(chorus), text(chorus), text("the earth shall soon\ndissolve like snow")]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "my chains are gone", &mut t);
    // A line early, the operator goes on to the second copy. The singers finish the first one: the very words
    // of the operator's slide's last line, but they are not singing that slide yet.
    host.operator_show(1);
    sing(&mut e, host.as_ref(), "I've been set free", &mut t);
    assert_eq!(host.shown(), Vec::<String>::new(), "the verse came up while the first chorus was being finished");
    assert_eq!(host.slide(), 1);
    assert_eq!(e.state().light, Light::Holding);
    // The chorus again, now on the operator's slide: Auto locks on, and its last line brings the verse up.
    sing(&mut e, host.as_ref(), "my chains are gone", &mut t);
    assert_eq!(host.shown(), Vec::<String>::new());
    sing(&mut e, host.as_ref(), "I've been set free", &mut t);
    assert_eq!(host.shown(), ["show 2"]);
}

#[test]
fn a_last_line_shared_with_the_slide_before_does_not_prove_the_operators_slide() {
    // Two verses that end on the same line, then a third slide.
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.slides = vec![text("glory be to God on high\ncome and praise the Lord"),
        text("holy is his name forever\ncome and praise the Lord"), text("sing with joy tonight")]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "glory be to God on high", &mut t);
    // The operator presses Next as the first verse's last line begins.
    host.operator_show(1);
    sing(&mut e, host.as_ref(), "come and praise the Lord", &mut t);
    assert_eq!(host.shown(), Vec::<String>::new(), "Auto went one slide past the operator's");
    assert_eq!(host.slide(), 1);
    assert_eq!(e.state().light, Light::Holding);
}

#[test]
fn an_instrumental_slide_put_up_by_hand_leaves_on_the_first_sure_word() {
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.slides = vec![text(SONG[0]), text("{Instrumental}"), text(SONG[2])]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    assert_eq!(host.shown(), Vec::<String>::new());
    // The verse's last note is still held (the room's label says singing) when the operator puts the
    // instrumental slide up. Then the band plays, and the label says singing again as the singers come in.
    // The label alone never undoes the operator's click.
    host.operator_show(1);
    for (voice, looks) in [(Voice::Singing, 20), (Voice::Music, 100), (Voice::Singing, 3)] {
        for _ in 0..looks {
            t += 0.1;
            e.step(host.as_ref(), t, Some(FollowEvent::Voice(voice, t)));
        }
    }
    assert_eq!(host.shown(), Vec::<String>::new(), "the operator's slide stays until a word is heard");
    assert_eq!(e.state().light, Light::Holding);
    // The next slide's first word, surely heard: its words come up now, not four words into it.
    sing(&mut e, host.as_ref(), "through", &mut t);
    assert_eq!(host.shown(), ["show 2"]);
}

#[test]
fn a_new_song_on_its_intro_slide_waits_for_singing_to_start() {
    let host = Fake::song(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "amazing grace", &mut t);
    // The last note of this song is still held when the operator takes the next song live, on its intro slide.
    host.change(|l| {
        l.item_id = Uuid::new_v4();
        l.slides = vec![text("{Instrumental}"), text(SONG[1]), text(SONG[2])];
        l.slide = 0;
    });
    for _ in 0..30 {
        t += 0.1;
        e.step(host.as_ref(), t, Some(FollowEvent::Voice(Voice::Singing, t)));
    }
    assert_eq!(host.shown(), Vec::<String>::new(), "Auto left the intro slide the operator had just put up");
    // The note ends and the band plays the intro: nothing. The singers come in: the first words go up.
    for _ in 0..50 {
        t += 0.1;
        e.step(host.as_ref(), t, Some(FollowEvent::Voice(Voice::Music, t)));
    }
    assert_eq!(host.shown(), Vec::<String>::new());
    t += 0.1;
    e.step(host.as_ref(), t, Some(FollowEvent::Voice(Voice::Singing, t)));
    assert_eq!(host.shown(), ["show 1"]);
}

#[test]
fn one_stray_word_does_not_end_the_old_slides_last_line() {
    // The song of `the_old_slides_last_line_does_not_prove_the_operators_slide`. This time the ear delivers one
    // word that is not on the old slide's last line (misheard, an ad-lib) before that line: the line still only
    // finishes the old slide.
    let chorus = "my chains are gone\nI've been set free";
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.slides = vec![text(chorus), text(chorus), text("the earth shall soon\ndissolve like snow")]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "my chains are gone", &mut t);
    host.operator_show(1);
    sing(&mut e, host.as_ref(), "zzzq I've been set free", &mut t);
    assert_eq!(host.shown(), Vec::<String>::new(), "one stray word, and the verse came up while the first chorus was being finished");
    assert_eq!(host.slide(), 1);
    assert_eq!(e.state().light, Light::Holding);
    // The same with two verses that end on the same line.
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.slides = vec![text("glory be to God on high\ncome and praise the Lord"),
        text("holy is his name forever\ncome and praise the Lord"), text("sing with joy tonight")]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "glory be to God on high", &mut t);
    host.operator_show(1);
    sing(&mut e, host.as_ref(), "zzzq come and praise the Lord", &mut t);
    assert_eq!(host.shown(), Vec::<String>::new(), "one stray word, and Auto went one slide past the operator's");
    assert_eq!(host.slide(), 1);
    assert_eq!(e.state().light, Light::Holding);
}

#[test]
fn a_stray_word_in_the_middle_of_the_old_last_line_does_not_end_it() {
    // The chorus is on two slides in a row, then a verse. Its last line is long.
    let chorus = "come and praise our mighty king\nlift up his name in every land and sea";
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.slides = vec![text(chorus), text(chorus), text("the earth shall soon\ndissolve like snow")]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "come and praise our mighty king", &mut t);
    // A line early, the operator goes on to the second copy. As the singers finish the first one, the leader
    // throws in "king" (a word of the song, but not of the line being finished): the rest of the line is still
    // the old slide's, not proof that the operator's slide is being sung.
    host.operator_show(1);
    sing(&mut e, host.as_ref(), "lift up king his name in every land and sea", &mut t);
    assert_eq!(host.shown(), Vec::<String>::new(), "the verse came up while the first chorus was being finished");
    assert_eq!(host.slide(), 1);
    assert_eq!(e.state().light, Light::Holding);
    // The chorus again, now on the operator's slide: Auto locks on, and only its last line brings the verse up.
    sing(&mut e, host.as_ref(), "come and praise our mighty king", &mut t);
    assert_eq!(host.shown(), Vec::<String>::new());
    sing(&mut e, host.as_ref(), "lift up his name in every land and sea", &mut t);
    assert_eq!(host.shown(), ["show 2"]);
}

#[test]
fn more_hand_moves_do_not_forget_the_old_slides_last_line() {
    let chorus = "my chains are gone\nI've been set free";
    // The chorus on two (or three) slides in a row, then a verse. The singers have sung its first line on
    // slide 0 when the operator clicks: Next, Next, Back; Next, Back, Next; a double-tapped Next over three
    // copies. Every click is seen on its own, with no word heard in between. The singers are still finishing
    // slide 0, wherever the clicks ended.
    for (copies, moves) in [(2, vec![1, 2, 1]), (2, vec![1, 0, 1]), (3, vec![1, 2])] {
        let host = Fake::song(Mode::Auto);
        host.change(|l| l.slides = (0..copies).map(|_| text(chorus)).chain([text("the earth shall soon\ndissolve like snow")]).collect());
        let mut e = Engine::new();
        let mut t = 0.0;
        sing(&mut e, host.as_ref(), "my chains are gone", &mut t);
        for &slide in &moves {
            host.operator_show(slide);
            e.step(host.as_ref(), t, None);
        }
        sing(&mut e, host.as_ref(), "I've been set free", &mut t);
        assert_eq!(host.shown(), Vec::<String>::new(), "{moves:?}: the old slide's last line took Auto past the operator's slide");
        assert_eq!(host.slide(), moves[moves.len() - 1], "{moves:?}");
        assert_eq!(e.state().light, Light::Holding, "{moves:?}");
    }
}

#[test]
fn clear_or_logo_after_autos_own_blank_is_the_operators_click() {
    let host = Fake::song(Mode::Auto);
    host.change(|l| {
        l.slides = vec![text("amazing grace"), text("how sweet the sound")];
        l.slide = 1;
    });
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "how sweet the sound", &mut t);
    // The song is over: after five quiet seconds Auto takes the words off itself.
    for _ in 0..60 {
        t += 0.1;
        e.step(host.as_ref(), t, Some(FollowEvent::Voice(Voice::Music, t)));
    }
    assert_eq!(host.shown(), ["blank"]);
    assert_eq!(e.state().light, Light::Blank);
    // The operator presses Logo (or Clear) for the prayer. The slide is the same and its words were off
    // already: only the screen's version tells of the click.
    host.change(|_| {});
    // The band tags the last line, twice. Before Auto's own blank that click would have stopped Auto; after
    // it, it does too: the lyrics do not come back over the operator's logo.
    sing(&mut e, host.as_ref(), "how sweet the sound how sweet the sound", &mut t);
    assert_eq!(host.shown(), ["blank"], "Auto put the lyrics back over the operator's Clear or Logo");
    assert_eq!(e.state().light, Light::Holding);
}

#[test]
fn a_label_flicker_in_the_held_note_does_not_leave_the_intro_slide() {
    let host = Fake::song(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), "amazing grace", &mut t);
    // The last note of this song is still held when the operator takes the next song live, on its intro slide.
    host.change(|l| {
        l.item_id = Uuid::new_v4();
        l.slides = vec![text("{Instrumental}"), text(SONG[1]), text(SONG[2])];
        l.slide = 0;
    });
    // In the held note the room's label flickers: one look says music, a later one says talking.
    for i in 0..30 {
        t += 0.1;
        let voice = match i { 10 => Voice::Music, 20 => Voice::Speaking, _ => Voice::Singing };
        e.step(host.as_ref(), t, Some(FollowEvent::Voice(voice, t)));
    }
    assert_eq!(host.shown(), Vec::<String>::new(), "one flicker of the label, and Auto left the intro slide the operator had just put up");
    assert_eq!(host.slide(), 0);
}

#[test]
fn a_held_vowel_does_not_undo_a_click_onto_an_instrumental_slide() {
    let host = Fake::song(Mode::Auto);
    host.change(|l| l.slides = vec![text(SONG[0]), text("{Instrumental}"), text("Oh how I love you\nmy heart will sing")]);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    assert_eq!(host.shown(), Vec::<String>::new());
    // The verse's last note is still held when the operator puts the instrumental slide up. The ear writes the
    // held vowel down again as a word of one sound, and that sounds like "Oh", the first word after the break.
    // It is not the one sure word that leaves an instrumental slide, however often it comes.
    host.operator_show(1);
    for n in 1..=2 {
        sing(&mut e, host.as_ref(), "oh", &mut t);
        assert_eq!(host.shown(), Vec::<String>::new(), "the held vowel, heard {n} time(s), undid the operator's click");
        assert_eq!(e.state().light, Light::Holding);
    }
    // A longer word of the next slide, surely heard, brings its words up.
    sing(&mut e, host.as_ref(), "love", &mut t);
    assert_eq!(host.shown(), ["show 2"]);
}

#[test]
fn the_listening_log_tells_what_was_heard_and_done() {
    let host = Fake::song(Mode::Auto);
    let mut e = Engine::new();
    let mut t = 0.0;
    sing(&mut e, host.as_ref(), VERSE_1, &mut t);
    let lines = e.take_trace();
    assert!(lines[0].contains("song: 3 slides, 36 words, Auto, on slide 1"), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("heard") && l.contains("-> slide 1 \"grace\"")), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("SHOW slide 2")), "{lines:?}");
    assert!(e.take_trace().is_empty(), "taken once");
    // The operator's moves, and a word that leaves the place where it was.
    host.operator_show(0);
    e.step(host.as_ref(), t, None);
    sing(&mut e, host.as_ref(), "zzzq", &mut t);
    let lines = e.take_trace();
    assert!(lines[0].contains("by hand: slide 1"), "{lines:?}");
    assert!(lines[1].contains("place unchanged") && lines[1].contains("waiting after a move by hand"), "{lines:?}");
    // In Manual nothing is written down.
    let host = Fake::song(Mode::Manual);
    let mut e = Engine::new();
    sing(&mut e, host.as_ref(), "amazing grace", &mut t);
    assert_eq!(e.take_trace(), Vec::<String>::new(), "a church that does not use auto-follow gets no log");
}

#[test]
fn the_listening_log_is_one_file_a_day_and_old_days_go() {
    let dir = std::env::temp_dir().join(format!("rp-follow-trace-{}", Uuid::new_v4()));
    let day = |d: u32| chrono::NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(10, 30, 5).unwrap();
    for d in 1..=12 { write_trace(&dir, day(d), &[format!("line {d}")]) }
    write_trace(&dir, day(12), &["more".into()]);
    let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names.len(), TRACE_DAYS, "{names:?}");
    assert_eq!((names[0].as_str(), names[TRACE_DAYS - 1].as_str()), ("follow-20261103.log", "follow-20261112.log"));
    assert_eq!(std::fs::read_to_string(dir.join("follow-20261112.log")).unwrap(), "10:30:05 line 12\n10:30:05 more\n");
    let _ = std::fs::remove_dir_all(&dir);
}
