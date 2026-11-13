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
