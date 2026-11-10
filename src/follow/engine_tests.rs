use super::*;
use crate::follow::lyrics::sound_key;

fn text(t: &str) -> Slide { Slide::new("", SlideContent::Text { text: t.into(), footer: None, reference: None, size: 0, parallel: None }) }

const SONG: [&str; 3] = [
    "amazing grace how sweet the sound\nthat saved a wretch like me",
    "I once was lost but now am found\nwas blind but now I see",
    "through many dangers toils and snares\nI have already come",
];

/// A live engine stand-in: records the follower's own changes; `operator` changes as a click would.
struct Fake { live: Mutex<Option<LiveSong>>, shown: Mutex<Vec<String>>, notices: Mutex<Vec<String>>, modes: Mutex<Vec<(Uuid, Mode)>> }

impl Fake {
    fn song(mode: Mode) -> Arc<Fake> {
        let live = LiveSong { item_id: Uuid::new_v4(), is_song: true, slides: SONG.iter().map(|t| text(t)).collect(), slide: 0, words_up: true,
            video_playing: false, version: 1, mode, settings: Settings::default() };
        Arc::new(Fake { live: Mutex::new(Some(live)), shown: Mutex::default(), notices: Mutex::default(), modes: Mutex::default() })
    }
    fn change(&self, f: impl FnOnce(&mut LiveSong)) {
        let mut l = self.live.lock().unwrap();
        let l = l.as_mut().unwrap();
        f(l);
        l.version += 1;
    }
    fn operator_show(&self, slide: usize) { self.change(|l| { l.slide = slide; l.words_up = true }) }
    fn slide(&self) -> usize { self.live.lock().unwrap().as_ref().unwrap().slide }
    fn item(&self) -> Uuid { self.live.lock().unwrap().as_ref().unwrap().item_id }
    fn shown(&self) -> Vec<String> { self.shown.lock().unwrap().clone() }
}

impl Host for Fake {
    fn live(&self) -> Option<LiveSong> { self.live.lock().unwrap().clone() }
    fn show(&self, item: Uuid, slide: usize, base: u64) -> bool {
        let mut l = self.live.lock().unwrap();
        let Some(l) = l.as_mut().filter(|l| l.version == base && l.item_id == item) else { return false };
        l.slide = slide;
        l.words_up = true;
        l.version += 1;
        self.shown.lock().unwrap().push(format!("show {slide}"));
        true
    }
    fn clear_slide(&self, base: u64) -> bool {
        let mut l = self.live.lock().unwrap();
        let Some(l) = l.as_mut().filter(|l| l.version == base) else { return false };
        l.words_up = false;
        l.version += 1;
        self.shown.lock().unwrap().push("blank".into());
        true
    }
    fn notice(&self, text: &str) { self.notices.lock().unwrap().push(text.into()) }
    fn save_mode(&self, item: Uuid, mode: Mode) { self.modes.lock().unwrap().push((item, mode)) }
}

/// Sings `words` a third of a second apart (each one heard as it is sung); the clock moves on.
fn sing(e: &mut Engine, host: &dyn Host, words: &str, t: &mut f64) {
    for w in words.split_whitespace() {
        *t += 1.0 / 3.0;
        e.step(host, *t, Some(FollowEvent::Voice(Voice::Singing, *t)));
        e.step(host, *t, Some(FollowEvent::Heard(Heard { key: sound_key(w), at: *t, conf: 1.0 })));
    }
}

const VERSE_1: &str = "amazing grace how sweet the sound that saved a wretch like me";
const VERSE_2: &str = "I once was lost but now am found was blind but now I see";

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
