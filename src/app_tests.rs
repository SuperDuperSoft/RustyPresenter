use super::*;

fn block_on<F: std::future::Future>(f: F) -> F::Output { tokio::runtime::Runtime::new().unwrap().block_on(f) }

#[test]
fn old_test_leftovers_are_swept_and_nothing_else() {
    let dir = std::env::temp_dir().join(format!("sweep-check-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let day_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(24 * 3600);
    let file = |name: &str, old: bool| {
        let p = dir.join(name);
        std::fs::write(&p, b"x").unwrap();
        if old { std::fs::File::options().write(true).open(&p).unwrap().set_modified(day_ago).unwrap() }
    };
    // A day old: earlier runs' leftovers, and the app's own import folder (kept).
    file("rp-test-old.log", true);
    file("rp-panic-old.log", true);
    file("rp-import-old.zip", true);
    file("unrelated-old.txt", true);
    // Just made: a run going on now.
    file("rp-test-new.log", false);
    // A folder: removed with what is in it (its age is when it was last written, so it is just made here).
    std::fs::create_dir_all(dir.join("rp-themes-new").join("inside")).unwrap();
    assert_eq!(sweep_test_scraps(&dir, std::time::Duration::from_secs(2 * 3600)), 2);
    let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["rp-import-old.zip", "rp-test-new.log", "rp-themes-new", "unrelated-old.txt"]);
    // With no age limit the folder goes too, contents and all.
    assert_eq!(sweep_test_scraps(&dir, std::time::Duration::ZERO), 2);
    assert!(!dir.join("rp-themes-new").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn a_passage_added_without_a_choice_gets_the_settings_verses_per_slide() {
    let app = test_app();
    app.data().settings.bible_per_slide = 3;
    let add = |per_slide: &str| format!(r#"{{"item":{{"kind":"scripture","reference":"John 3:16","translation":"KJV","verses":[]{per_slide}}}}}"#);
    app.do_action("service/add", add("").as_bytes()).await.unwrap();
    app.do_action("service/add", add(r#","per_slide":1"#).as_bytes()).await.unwrap();
    let d = app.data();
    let per: Vec<u8> = d.services[0].items.iter().map(|i| match i.kind { ItemKind::Scripture { per_slide, .. } => per_slide, _ => 9 }).collect();
    assert_eq!(per, [3, 1], "a choice the client made is kept");
}

#[tokio::test]
async fn do_action_saves_a_song_and_returns_its_id() {
    let app = test_app();
    let v = app.do_action("song/save", br#"{"id":"00000000-0000-0000-0000-000000000000","title":"Amazing Grace","lyrics":"Verse 1\nAmazing grace"}"#).await.unwrap();
    let id: Uuid = serde_json::from_value(v["id"].clone()).unwrap();
    assert!(!id.is_nil());
    assert_eq!(app.data().songs[0].title, "Amazing Grace");
}

#[tokio::test]
async fn do_action_rejects_untitled_song_with_400() {
    let err = test_app().do_action("song/save", br#"{"id":"00000000-0000-0000-0000-000000000000","title":" ","lyrics":""}"#).await.unwrap_err();
    assert_eq!((err.status, err.message.as_str()), (400, "Give the song a title."));
}

fn on_air(app: &App) {
    let id = Uuid::new_v4();
    let mut live = app.live();
    struct One(Uuid);
    impl Plan for One {
        fn order(&self) -> Vec<Uuid> { vec![self.0] }
        fn build(&self, id: Uuid) -> Option<LiveItem> {
            Some(LiveItem { item_id: id, title: "T".into(), kind: "song".into(), background: Background::Color { color: "#000".into() },
                slides: vec![Slide::new("s", SlideContent::Text { text: "hello".into(), footer: None, reference: None, size: 0, parallel: None })], theme: Theme::default() })
        }
    }
    live.apply(Envelope { id: id.to_string(), base: None, cmd: Command::Show { item_id: id, slide: 0 } }, &One(id));
}

fn wait_for_state(root: &std::path::Path, seq: u64) -> state::LiveState {
    for _ in 0..200 {
        if let Some(s) = state::load(root) && s.seq >= seq { return s; }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("no saved state with seq {seq}");
}

#[test]
fn app_writes_state_after_commit() {
    let app = test_app();
    app.set_state_writer(state::StateWriter::start(app.store.root.clone()));
    on_air(&app);
    let version = app.live().version;
    assert_eq!(state::load(&app.store.root), None, "nothing is on air yet");
    app.room_event(RoomEvent::Committed { version });
    let s = wait_for_state(&app.store.root, 1);
    assert!(matches!(s.frame.slide, Some(crate::live::SlideLayer::Text { ref text, .. }) if text == "hello"));
    assert!(s.saved_at.len() > 8);
}

#[test]
fn a_report_that_commits_nothing_saves_nothing() {
    let app = test_app();
    app.set_state_writer(state::StateWriter::start(app.store.root.clone()));
    on_air(&app);
    let version = app.live().version;
    app.room_event(RoomEvent::Committed { version });
    app.room_event(RoomEvent::Committed { version: 99 });
    app.room_event(RoomEvent::Committed { version });
    app.stop_state_writer();
    assert_eq!(state::load(&app.store.root).unwrap().seq, 1);
}

#[test]
fn saving_records_the_open_windows_and_a_newer_seq() {
    struct Roles;
    impl Screens for Roles {
        fn open(&self, _: &str, _: Option<&str>) -> Result<(), String> { Ok(()) }
        fn close(&self, _: &str) {}
        fn open_roles(&self) -> Vec<String> { vec!["room".into(), "stage".into()] }
        fn monitors(&self) -> Vec<Monitor> { vec![] }
    }
    let app = test_app();
    app.set_screens(Arc::new(Roles));
    app.set_state_writer(state::StateWriter::start(app.store.root.clone()));
    on_air(&app);
    let version = app.live().version;
    app.room_event(RoomEvent::Committed { version });
    let first = wait_for_state(&app.store.root, 1);
    app.save_live_state();
    let second = wait_for_state(&app.store.root, first.seq + 1);
    assert_eq!(second.windows, ["room", "stage"]);
}

#[test]
fn moving_the_live_songs_key_changes_the_stage_chords_at_once_and_never_the_room() {
    let app = test_app();
    let song = Song { id: Uuid::new_v4(), title: "Grace".into(), key: "G".into(), lyrics: "Verse 1
[G]Amazing [C]grace".into(), ..Default::default() };
    let (song_id, item_id) = (song.id, Uuid::new_v4());
    {
        let mut d = app.data();
        d.songs.push(song);
        d.services[0].items.push(ServiceItem { id: item_id, kind: ItemKind::Song { song_id, arrangement: None }, background: None, notes: String::new(), theme: None });
    }
    app.command(Envelope::new(Command::Show { item_id, slide: 0 }));
    let frame = app.live_view().frame.clone();
    assert_eq!(app.live_view().slide.unwrap().chords.as_deref(), Some("[G]Amazing [C]grace"));
    let r = block_on(app.do_action("song/key", serde_json::json!({ "id": song_id, "step": 3 }).to_string().as_bytes())).unwrap();
    assert_eq!(r["transpose"], 3);
    let v = app.live_view();
    assert_eq!(v.slide.unwrap().chords.as_deref(), Some("[Bb]Amazing [Eb]grace"), "the stage has the moved chords");
    assert_eq!(format!("{:?}", v.frame), format!("{frame:?}"), "the room shows the same");
    assert!(v.changed.is_none(), "no Update screen banner");
    // Kept to −11 … +11, and the song's words and chords as written never change.
    block_on(app.do_action("song/key", serde_json::json!({ "id": song_id, "step": 40 }).to_string().as_bytes())).unwrap();
    let d = app.data();
    let s = d.songs.iter().find(|s| s.id == song_id).unwrap();
    assert_eq!((s.transpose, s.lyrics.as_str()), (11, "Verse 1
[G]Amazing [C]grace"));
}

#[test]
fn songs_saved_before_keys_load_with_no_key_and_no_move() {
    let s: Song = serde_json::from_str(r#"{"id":"00000000-0000-0000-0000-000000000001","title":"Old","lyrics":"Verse 1\nla"}"#).unwrap();
    assert_eq!((s.key.as_str(), s.transpose), ("", 0));
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("key") && !json.contains("transpose"), "nothing new is written for a song without them: {json}");
    let moved: Song = serde_json::from_str(&serde_json::to_string(&Song { key: "D".into(), transpose: -2, ..s }).unwrap()).unwrap();
    assert_eq!((moved.key.as_str(), moved.transpose), ("D", -2));
}

#[test]
fn saving_with_nothing_committed_writes_nothing() {
    let app = test_app();
    app.set_state_writer(state::StateWriter::start(app.store.root.clone()));
    app.save_live_state();
    app.stop_state_writer();
    assert_eq!(state::load(&app.store.root), None);
}

#[test]
fn playing_means_committed_video_or_audio() {
    let app = test_app();
    let item_id = Uuid::new_v4();
    let kind = ItemKind::Text { title: "t".into(), body: "text".into(), layout: Default::default() };
    app.data().services[0].items.push(ServiceItem { id: item_id, kind, background: None, notes: String::new(), theme: None });
    let version = app.command(Envelope::new(Command::Show { item_id, slide: 0 })).version;
    app.room_event(RoomEvent::Committed { version });
    assert!(!app.playing(), "a text slide does not play");

    let mut saved = state::snapshot(&app.live(), vec![], 1).unwrap();
    saved.frame.audio = Some(Inst { inst: 9, v: AudioLayer { asset: "a.mp3".into(), title: "A".into(), looped: false, paused: false } });
    app.live().restore(&saved, true);
    assert!(!app.playing(), "not on air until the room commits");
    let version = app.live().version;
    app.room_event(RoomEvent::Committed { version });
    assert!(app.playing());
}

#[test]
fn saving_without_a_writer_does_nothing() {
    let app = test_app();
    on_air(&app);
    let version = app.live().version;
    app.room_event(RoomEvent::Committed { version });
    app.save_live_state();
    app.stop_state_writer();
    assert_eq!(state::load(&app.store.root), None);
}

#[test]
fn resume_position_lasts_until_on_air() {
    let app = test_app();
    on_air(&app);
    let (version, inst) = {
        let live = app.live();
        (live.version, live.frame.instances().next().expect("the shown frame has an instance"))
    };
    app.set_resume(&[(inst, 12.5), (inst + 100, 3.0)]);
    assert_eq!(app.resume_position(inst), Some(12.5));
    // A newer plan may prepare the same instance again before the first one went on air.
    assert_eq!(app.resume_position(inst), Some(12.5));
    assert_eq!(app.resume_position(inst + 1000), None);
    app.room_event(RoomEvent::Committed { version });
    assert_eq!(app.resume_position(inst), None, "a later replay starts from the beginning");
    assert_eq!(app.resume_position(inst + 100), Some(3.0), "only what went on air is forgotten");
}

#[test]
fn a_stopped_writer_saves_nothing_more() {
    let app = test_app();
    app.set_state_writer(state::StateWriter::start(app.store.root.clone()));
    on_air(&app);
    app.stop_state_writer();
    assert_eq!(state::load(&app.store.root), None, "a requested frame is not saved");
    // A clean quit stops the writer, then clears: a later commit must not bring the file back.
    state::clear(&app.store.root);
    let version = app.live().version;
    app.room_event(RoomEvent::Committed { version });
    app.stop_state_writer();
    assert!(!state::path(&app.store.root).exists());
}

#[tokio::test]
async fn set_notice_shows_on_the_live_view() {
    let app = test_app();
    let mut rx = app.subscribe();
    app.set_notice("RustyPresenter restarted after a problem.");
    assert_eq!(app.live().notice.as_deref(), Some("RustyPresenter restarted after a problem."));
    assert!(matches!(rx.try_recv().unwrap(), Update::Live(_)));
}

/// Q6: a player's position report goes out as `Update::Playback`, with the last view and the new position; a
/// change of state is still a whole live view.
#[tokio::test]
async fn a_position_report_is_a_playback_update_not_a_whole_live_view() {
    let app = test_app();
    let id = Uuid::new_v4();
    {
        let mut d = app.data();
        let media = crate::model::MediaFile { id: Uuid::new_v4(), name: "clip".into(), kind: crate::model::MediaKind::Video, asset: "clip.mp4".into(),
            status: crate::model::MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
        d.services[0].items.push(ServiceItem { id, kind: ItemKind::Media { media_id: media.id, autoplay: true, end: crate::model::EndAction::Stop, looped: false }, background: None, notes: String::new(), theme: None });
        d.media.push(media);
    }
    assert!(app.command(Envelope { id: "a".into(), base: None, cmd: Command::Show { item_id: id, slide: 0 } }).ok);
    let inst = app.live().frame.media.as_ref().unwrap().inst;
    let report = |state: PlayState, position: f64| app.room_event(RoomEvent::Media { inst, state, position, duration: 60.0, error: None });
    let mut rx = app.subscribe();
    let mut heard = || { let mut all = vec![]; while let Ok(u) = rx.try_recv() { all.push(u) } all };
    // The first report (it started playing) is a change of state.
    report(PlayState::Playing, 0.0);
    assert!(matches!(heard().as_slice(), [Update::Live(v)] if v.view().media.as_ref().is_some_and(|m| m.position == 0.0)));
    // Then only the position moves.
    report(PlayState::Playing, 0.5);
    report(PlayState::Playing, 1.0);
    let updates = heard();
    assert!(matches!(updates.as_slice(), [Update::Playback(a), Update::Playback(b)]
        if a.view().media.as_ref().is_some_and(|m| m.position == 0.5) && b.view().media.as_ref().is_some_and(|m| m.position == 1.0 && m.duration == 60.0)), "{updates:?}");
    let Update::Playback(last) = &updates[1] else { unreachable!() };
    assert_eq!((last.view().version, last.view().cursor.as_ref().map(|c| c.item_id)), (app.live().version, Some(id)), "the same view otherwise");
    assert_eq!(app.live_view().media.map(|m| m.position), Some(1.0));
    // Paused: a change of state again.
    report(PlayState::Paused, 1.0);
    assert!(matches!(heard().as_slice(), [Update::Live(_)]));
}

/// P4: a screen that could not open is not saved as on.
#[tokio::test]
async fn a_screen_that_did_not_open_is_not_saved_as_on() {
    let app = test_app();
    let before = app.data().settings.screens.clone();
    // No windowing layer in a test: the window cannot open.
    let r = app.do_action("display/open", br#"{"role":"room"}"#).await;
    assert!(r.is_err());
    assert_eq!(app.data().settings.screens, before, "the setup is as it was");
    assert_eq!(app.store.load().unwrap().settings.screens, before, "on the disk too");
}

/// Q27: the copy the 30-second check works on shares the library's lists.
#[tokio::test]
async fn a_copy_of_the_data_for_a_check_shares_the_library() {
    let app = test_app();
    app.edit(|d| { d.songs.push(Song { title: "Amazing Grace".into(), ..Default::default() }); Ok(()) }).unwrap();
    let copy = app.data().clone();
    let held = app.data();
    assert!(copy.songs.same(&held.songs) && copy.media.same(&held.media) && copy.decks.same(&held.decks) && copy.services.same(&held.services));
}

/// Q14: a panic inside an edit puts the data back and poisons nothing: every screen carries on.
#[tokio::test]
async fn a_panic_inside_an_edit_leaves_the_data_as_it_was_and_the_locks_usable() {
    let app = test_app();
    app.edit(|d| { d.songs.push(Song { title: "Amazing Grace".into(), ..Default::default() }); Ok(()) }).unwrap();
    let hit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = app.edit(|d| { d.songs.clear(); let none: Option<()> = None; none.expect("a bug in a rarely used action"); Ok(()) });
    }));
    assert!(hit.is_err(), "the panic is still reported");
    assert_eq!(app.data().songs.len(), 1, "the half-made change was put back");
    assert!(!app.data.lock().is_err(), "the lock is not poisoned");
    // A lock poisoned some other way is taken all the same.
    let other = app.clone();
    let _ = std::thread::spawn(move || { let _held = other.live(); panic!("while holding the live lock") }).join();
    assert!(app.live.lock().is_err(), "poisoned");
    assert_eq!(app.live().version, 0, "and still readable");
    assert!(app.command(Envelope::new(Command::Logo)).ok, "the screen still takes commands");
}

/// Q2: an edit copies only the list it changes; the lists it leaves alone stay shared with the copy taken
/// before it (and with the snapshot that is saved).
#[tokio::test]
async fn an_edit_shares_the_lists_it_does_not_change() {
    let app = test_app();
    app.edit(|d| { d.songs.push(Song { title: "Amazing Grace".into(), ..Default::default() }); Ok(()) }).unwrap();
    let before = app.data().clone();
    app.edit(|d| { d.settings.translation = "WEB".into(); Ok(()) }).unwrap();
    {
        let now = app.data();
        assert!(now.songs.same(&before.songs) && now.services.same(&before.services) && now.media.same(&before.media), "a settings change copies no list");
    }
    app.edit(|d| { d.songs[0].title = "Amazing Grace (My Chains Are Gone)".into(); Ok(()) }).unwrap();
    let now = app.data().clone();
    assert!(!now.songs.same(&before.songs) && now.services.same(&before.services), "only the songs were copied");
    assert_eq!((before.songs[0].title.as_str(), now.songs[0].title.as_str()), ("Amazing Grace", "Amazing Grace (My Chains Are Gone)"), "the copy taken before is as it was");
    assert_eq!(app.store.load().unwrap().songs[0].title, "Amazing Grace (My Chains Are Gone)", "and the save has the change");
    // An edit that fails puts the data back as it was.
    assert!(app.edit(|d| { d.songs.clear(); Err::<(), _>("no".to_string()) }).is_err());
    assert_eq!(app.data().songs.len(), 1);
}

/// P1: memory holds an edit whose save failed, so every screen is told to read it.
#[tokio::test]
async fn an_edit_whose_save_failed_still_tells_every_screen() {
    let app = test_app();
    // Something in the way of data.json: the save cannot land.
    let _ = std::fs::remove_file(app.store.data_path());
    std::fs::create_dir_all(app.store.data_path().join("in the way")).unwrap();
    let mut rx = app.subscribe();
    let r = app.edit(|d| { d.settings.translation = "WEB".into(); Ok(()) });
    assert!(r.is_err(), "the save failed");
    assert_eq!(app.data().settings.translation, "WEB", "memory has the change");
    let mut heard = vec![];
    while let Ok(u) = rx.try_recv() { heard.push(u) }
    assert!(heard.iter().any(|u| matches!(u, Update::Data)), "the operator window is told to read the data again");
}

#[tokio::test]
async fn an_older_save_never_lands_over_a_newer_one() {
    let app = test_app();
    let named = |n: &str| { let mut d = app.data().clone(); d.settings.translation = n.into(); d };
    let (older, newer) = (app.take_save(&named("OLD")).unwrap(), app.take_save(&named("NEW")).unwrap());
    // The newer save reaches the disk first; the older one, written late, is dropped.
    app.write_save(newer).unwrap();
    app.write_save(older).unwrap();
    assert_eq!(app.store.load().unwrap().settings.translation, "NEW");
    // An edit still saves.
    app.edit(|d| { d.settings.translation = "WEB".into(); Ok(()) }).unwrap();
    assert_eq!(app.store.load().unwrap().settings.translation, "WEB");
}

#[tokio::test]
async fn a_live_command_publishes_one_live_update() {
    let app = test_app();
    let mut rx = app.subscribe();
    app.command(Envelope { id: "c1".into(), base: None, cmd: Command::ClearAll });
    assert!(matches!(rx.try_recv().unwrap(), Update::Live(_)));
    assert!(rx.try_recv().is_err());
}

#[test]
fn live_views_go_out_in_the_order_they_were_read() {
    use tokio::sync::broadcast::error::TryRecvError;
    let app = test_app();
    let mut rx = app.subscribe();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let other = {
        let (app, stop) = (app.clone(), stop.clone());
        std::thread::spawn(move || while !stop.load(Ordering::Relaxed) { app.broadcast_live() })
    };
    let mut newest = 0;
    for i in 0..2000 {
        app.command(Envelope { id: format!("c{i}"), base: None, cmd: Command::Logo });
        loop {
            match rx.try_recv() {
                Ok(Update::Live(u)) => {
                    assert!(u.version >= newest, "view {} went out after view {newest}", u.version);
                    newest = u.version;
                }
                Ok(_) | Err(TryRecvError::Lagged(_)) => {}
                Err(_) => break,
            }
        }
    }
    stop.store(true, Ordering::Relaxed);
    other.join().unwrap();
    assert!(newest > 1000, "only reached version {newest}");
}

#[test]
fn a_live_update_is_serialized_once_however_many_clients_receive_it() {
    let app = test_app();
    let (mut a, mut b) = (app.subscribe(), app.subscribe());
    app.broadcast_live();
    let (Update::Live(x), Update::Live(y)) = (a.try_recv().unwrap(), b.try_recv().unwrap()) else { panic!("expected live updates") };
    // Both receivers read the JSON; the second must get the first one's string, not a fresh serialization.
    assert!(std::ptr::eq(x.json(), y.json()), "each client re-serialized the live view");
    assert!(std::ptr::eq(x.json(), x.json()));
    // The typed view stays available to native screens.
    assert_eq!(x.version, app.live_view().version);
}

#[test]
fn preflight_looks_at_the_real_files() {
    let app = test_app();
    let good = app.store.import_file(&write_temp(b"picture"), "png").unwrap();
    let empty = app.store.import_file(&write_temp(b""), "jpg").unwrap();
    let check = |logo: &str| {
        app.data().settings.logo = Some(logo.to_string());
        app.preflight().contains(&crate::preflight::strings::logo_missing())
    };
    assert!(!check(&good));
    assert!(check(&empty), "an empty file cannot be shown");
    assert!(check("gone.png"));
}

fn write_temp(bytes: &[u8]) -> PathBuf {
    let p = std::env::temp_dir().join(format!("rp-preflight-{}", Uuid::new_v4()));
    std::fs::write(&p, bytes).unwrap();
    p
}

#[tokio::test]
async fn stage_message_goes_to_the_stage_view_at_once_and_trimmed() {
    let app = test_app();
    let mut rx = app.subscribe();
    app.do_action("stage/message", br#"{"text":"  Wrap up, please \n"}"#).await.unwrap();
    assert_eq!(app.live_view().stage_message, "Wrap up, please");
    let Ok(Update::Live(u)) = rx.try_recv() else { panic!("no live update") };
    assert_eq!(u.stage_message, "Wrap up, please");
}

#[test]
fn stage_only_slide_reaches_the_stage_and_never_the_room() {
    let app = test_app();
    let id = Uuid::new_v4();
    app.data().services[0].items.push(ServiceItem { id, kind: ItemKind::Text { title: "Band".into(), body: "Watch me\n\nOne more time".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None });
    let before = app.live_view();
    let cmd = |v: Value| app.command(serde_json::from_value(v).unwrap());
    let out = cmd(json!({ "id": "a", "cmd": "stage_only", "item_id": id, "slide": 1 }));
    assert!(out.ok, "{out:?}");
    let v = app.live_view();
    let only = v.stage_only.clone().expect("on the stage");
    assert_eq!((only.item_id, only.index, only.slide.label.is_empty()), (id, 1, false));
    assert_eq!((v.version, v.frame.slide.is_none()), (before.version, true), "the room's frame is untouched");
    assert!(serde_json::to_string(&v).unwrap().find("One more time").is_none(), "never in the JSON");
    let gone = cmd(json!({ "id": "b", "cmd": "stage_only", "item_id": Uuid::new_v4(), "slide": 0 }));
    assert!(!gone.ok && app.live_view().stage_only.is_some(), "an unknown item changes nothing");
    assert!(cmd(json!({ "id": "c", "cmd": "stage_only" })).ok);
    assert!(app.live_view().stage_only.is_none(), "no item takes it down");
}

#[test]
fn a_stage_only_slide_and_a_phone_drawing_are_on_the_stage_together() {
    use crate::live::drawing::{DrawCmd, Pen, PALETTE};
    let app = test_app();
    let id = Uuid::new_v4();
    app.data().services[0].items.push(ServiceItem { id, kind: ItemKind::Text { title: "Band".into(), body: "Watch me".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None });
    let phone = Uuid::new_v4();
    app.phones.lock().unwrap().push(Phone { id: phone, name: "iPad".into(), hash: String::new(), last_seen: 0, may_draw: true });
    app.draw(phone, &DrawCmd::Start { stroke: 1, color: PALETTE[1].into(), width: 20.0, pen: Pen::Pen, points: vec![[0.1, 0.5], [0.9, 0.5]], ink: Default::default() }).unwrap();
    assert!(app.command(serde_json::from_value(json!({ "id": "a", "cmd": "stage_only", "item_id": id, "slide": 0 })).unwrap()).ok);
    let v = app.live_view();
    assert!(v.stage_only.is_some() && !v.drawing.is_empty(), "the stage-only slide does not take the drawing down");
    let (mut canvas, mut slide, renderer) = (crate::render::drawing::DrawingCanvas::new(), None, crate::ui::thumbs::TextRenderer::new());
    assert!(crate::ui::stage::drawing_panel(&app, &mut canvas, &mut slide, &renderer).is_some(), "the corner panel still shows the drawing");
    assert!(app.command(serde_json::from_value(json!({ "id": "b", "cmd": "stage_only" })).unwrap()).ok);
    assert!(!app.live_view().drawing.is_empty(), "taking it down leaves the drawing too");
}

#[tokio::test]
async fn empty_stage_message_clears_it() {
    let app = test_app();
    app.do_action("stage/message", br#"{"text":"Wrap up"}"#).await.unwrap();
    app.do_action("stage/message", br#"{"text":"   "}"#).await.unwrap();
    assert_eq!(app.live_view().stage_message, "");
}

#[tokio::test]
async fn stage_message_never_reaches_the_json_phones_and_pages_read() {
    let app = test_app();
    app.do_action("stage/message", br#"{"text":"Wrap up"}"#).await.unwrap();
    let v = serde_json::to_string(&app.live_view()).unwrap();
    assert!(!v.contains("Wrap up"), "{v}");
    assert!(!app.state_json().to_string().contains("Wrap up"));
}

#[test]
fn live_view_serializes_every_field_the_screens_read() {
    let v = serde_json::to_value(test_app().live_view()).unwrap();
    for k in ["version", "committed", "pending", "frame", "cursor", "item", "slide", "next", "media", "audio", "notice", "theme", "logo"] {
        assert!(v.get(k).is_some(), "{k}");
    }
}

#[tokio::test]
async fn bible_lookup_built_in_and_needs_key() {
    let app = test_app();
    let v = app.do_action("bible/lookup", br#"{"reference":"John 3:16","translation":"BSB"}"#).await.unwrap();
    assert!(v["verses"][0]["text"].as_str().unwrap().starts_with("For God so loved"));
    let err = app.do_action("bible/lookup", br#"{"reference":"John 3:16","translation":"ESV"}"#).await.unwrap_err();
    assert!(err.message.contains("Settings → Bible"), "{}", err.message);
    let s = app.state_json();
    assert_eq!(s["available_translations"], json!(["KJV", "WEB", "BSB"]));
    let esv = s["bible_sources"].as_array().unwrap().iter().find(|b| b["translation"] == "ESV").unwrap();
    assert_eq!(esv["status"], "needs_key");
}

#[tokio::test]
async fn saved_keys_never_reach_the_state() {
    let app = test_app();
    let mut s = bible::sources::Secrets::default();
    s.keys.insert("esv".into(), "secret-key-123".into());
    s.save(&app.store.root).unwrap();
    let state = app.state_json();
    assert!(!state.to_string().contains("secret-key-123"));
    assert!(state["available_translations"].as_array().unwrap().contains(&json!("ESV")));
    let esv = state["bible_providers"].as_array().unwrap().iter().find(|p| p["id"] == "esv").unwrap();
    assert_eq!(esv["key_set"], true);
    assert!(!app.store.data_path().exists() || !std::fs::read_to_string(app.store.data_path()).unwrap().contains("secret-key-123"));
}

#[tokio::test]
async fn bible_key_rejects_unknown_service_and_empty_key_removes() {
    let app = test_app();
    assert_eq!(app.do_action("bible/key", br#"{"provider":"nope","key":"x"}"#).await.unwrap_err().message, "Unknown Bible service.");
    let mut s = bible::sources::Secrets::default();
    s.keys.insert("esv".into(), "k".into());
    s.save(&app.store.root).unwrap();
    app.do_action("bible/key", br#"{"provider":"esv","key":"  "}"#).await.unwrap();
    assert!(bible::sources::Secrets::load(&app.store.root).keys.is_empty());
}

#[tokio::test]
async fn imported_bible_is_looked_up_offline_and_removed() {
    let app = test_app();
    let file = br#"<?xml version="1.0"?><XMLBIBLE biblename="Test"><INFORMATION><title>Holman Test</title><identifier>HCSB</identifier><rights>(c) Holman</rights></INFORMATION>
            <BIBLEBOOK bnumber="43"><CHAPTER cnumber="3"><VERS vnumber="16">For God loved.</VERS><VERS vnumber="17">For God sent.</VERS></CHAPTER></BIBLEBOOK></XMLBIBLE>"#;
    let v = app.do_action("bible/import/HCSB", file).await.unwrap();
    assert_eq!(v["abbreviation"], "HCSB");
    let v = app.do_action("bible/lookup", br#"{"reference":"John 3:16-17","translation":"HCSB"}"#).await.unwrap();
    assert_eq!(v["verses"][1]["text"], "For God sent.");
    let hcsb = app.state_json()["bible_sources"].as_array().unwrap().iter().find(|b| b["translation"] == "HCSB").unwrap().clone();
    assert_eq!((hcsb["status"].as_str(), hcsb["imported"].as_bool()), (Some("offline"), Some(true)));
    assert!(app.do_action("bible/import/KJV", file).await.unwrap_err().message.contains("already built in"));
    app.do_action("bible/import-remove", br#"{"translation":"HCSB"}"#).await.unwrap();
    assert!(app.do_action("bible/lookup", br#"{"reference":"John 3:16","translation":"HCSB"}"#).await.is_err());
}

#[tokio::test]
async fn blocked_translations_are_refused_on_every_action() {
    let app = test_app();
    let refused = |action: &'static str, body: String| { let app = app.clone(); async move { app.do_action(action, body.as_bytes()).await.unwrap_err().message } };
    for t in ["TPT", "nwt", "JST"] {
        assert_eq!(refused("bible/lookup", format!(r#"{{"reference":"John 3:16","translation":"{t}"}}"#)).await, blocked(), "{t}");
        assert_eq!(refused("bible/paste", format!(r#"{{"reference":"John 3:16","translation":"{t}","text":"16 For God loved."}}"#)).await, blocked(), "{t}");
        assert_eq!(refused("settings/translation", format!(r#"{{"translation":"{t}"}}"#)).await, blocked(), "{t}");
    }
    let passage = |t: &str| format!(r#""kind":"scripture","reference":"John 3:16","translation":"{t}","verses":[]"#);
    assert_eq!(refused("service/add", format!(r#"{{"item":{{{}}}}}"#, passage("TPT"))).await, blocked());
    let item = format!(r#"{{"id":"{}",{}}}"#, Uuid::new_v4(), passage("NWT"));
    assert_eq!(refused("service/update", item).await, blocked());
    assert!(app.data().services.iter().all(|s| s.items.is_empty()), "nothing went in");
}

#[tokio::test]
async fn a_blocked_text_under_another_name_is_refused() {
    let app = test_app();
    let text = "1 In the beginning was the Word, and the Word was with God, and the Word was a god.";
    let body = json!({ "reference": "John 1:1", "translation": "ABC", "text": text });
    assert_eq!(app.do_action("bible/paste", body.to_string().as_bytes()).await.unwrap_err().message, blocked());
    let body = json!({ "item": { "kind": "scripture", "reference": "John 1:1", "translation": "ABC",
        "verses": [{ "reference": "John 1:1", "text": text }] } });
    assert_eq!(app.do_action("service/add", body.to_string().as_bytes()).await.unwrap_err().message, blocked());
}

#[tokio::test]
async fn restoring_a_backup_leaves_blocked_passages_behind() {
    let app = test_app();
    let mut service = Service::new("Sunday");
    service.items.push(ServiceItem { id: Uuid::new_v4(), background: None, notes: String::new(), theme: None,
        kind: ItemKind::Scripture { reference: "John 3:16".into(), translation: "TPT".into(), verses: vec![], per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() } });
    let data = Data { services: vec![service].into(), ..Data::default() };
    let name = app.store.backup(&data).unwrap();
    app.do_action("backup/restore", json!({ "name": name }).to_string().as_bytes()).await.unwrap();
    assert!(app.data().services[0].items.is_empty());
    assert!(app.store.load().unwrap().services[0].items.is_empty(), "saved as well");
    assert_eq!(app.live().notice.as_deref(), Some(passages_removed(1).as_str()));
}

#[tokio::test]
async fn bible_paste_splits_verses() {
    let v = test_app().do_action("bible/paste", br#"{"reference":"John 3:16-17","translation":"hcsb","text":"16 For God loved. 17 For God sent."}"#).await.unwrap();
    assert_eq!(v["translation"], "HCSB");
    assert_eq!(v["verses"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_prefixed_id_goes_through_paste_settings_and_a_saved_service() {
    let app = test_app();
    let id = "ebible:spaRV1909";
    let mut b = bible::import::parse(br#"<XMLBIBLE biblename="Reina Valera"><BIBLEBOOK bnumber="43"><CHAPTER cnumber="3"><VERS vnumber="16">De tal manera.</VERS></CHAPTER></BIBLEBOOK></XMLBIBLE>"#).unwrap();
    b.abbreviation = id.into();
    bible::import::save(&app.store.root, &b).unwrap();

    let v = app.do_action("bible/lookup", json!({ "reference": "John 3:16", "translation": id }).to_string().as_bytes()).await.unwrap();
    assert_eq!((v["translation"].as_str(), v["verses"][0]["text"].as_str()), (Some(id), Some("De tal manera.")));
    let v = app.do_action("bible/paste", json!({ "reference": "John 3:16", "translation": id, "text": "16 De tal." }).to_string().as_bytes()).await.unwrap();
    assert_eq!(v["translation"], id, "not uppercased");

    app.do_action("settings/translation", json!({ "translation": id }).to_string().as_bytes()).await.unwrap();
    assert_eq!(app.store.load().unwrap().settings.translation, id);
    let add = json!({ "item": { "kind": "scripture", "reference": "John 3:16", "translation": id, "verses": v["verses"] } });
    app.do_action("service/add", add.to_string().as_bytes()).await.unwrap();
    let saved = app.store.load().unwrap();
    let item = saved.services.iter().flat_map(|s| &s.items).next().unwrap();
    assert!(matches!(&item.kind, ItemKind::Scripture { translation, .. } if translation == id));
    assert!(app.state_json()["available_translations"].as_array().unwrap().contains(&json!(id)));
}

/// A downloaded RV1909 under CC BY-SA, saved straight into `app`'s Bible folder (no network).
fn downloaded_rv1909(app: &App, licence: bible::library::Licence, notice: &str) {
    let id = "ebible:spaRV1909";
    let mut b = bible::import::parse(br#"<XMLBIBLE biblename="Reina Valera"><BIBLEBOOK bnumber="19"><CHAPTER cnumber="23"><VERS vnumber="1">Jehova es mi pastor.</VERS><VERS vnumber="2">En lugares.</VERS></CHAPTER></BIBLEBOOK></XMLBIBLE>"#).unwrap();
    b.abbreviation = id.into();
    b.copyright = notice.into();
    b.meta = Some(bible::library::BibleMeta {
        id: id.into(), abbr: "RV1909".into(), name: "Reina Valera 1909".into(), lang: "spa".into(), copyright: notice.into(), licence,
        source: bible::library::Source::Ebible { update_date: "2026-01-01".into() }, ..Default::default()
    });
    bible::import::install(&app.store.root, b).unwrap();
}

#[tokio::test]
async fn a_downloaded_bible_credits_its_passage_and_names_it_by_abbreviation() {
    let app = test_app();
    let cc = bible::library::Licence::Cc { kind: "by-sa".into(), version: "4.0".into(), url: "https://creativecommons.org/licenses/by-sa/4.0/".into() };
    downloaded_rv1909(&app, cc, "© Sociedad Bíblica");
    assert!(app.data().bible_credit("ebible:spaRV1909").is_none(), "not known until the Bibles are read again");
    app.refresh_bible_credits();
    app.data().settings.credits = "none".into();
    let v = app.do_action("bible/lookup", br#"{"reference":"Psalm 23:1-2","translation":"ebible:spaRV1909"}"#).await.unwrap();
    let add = json!({ "item": { "kind": "scripture", "reference": v["label"], "translation": "ebible:spaRV1909", "verses": v["verses"], "per_slide": 1 } });
    app.do_action("service/add", add.to_string().as_bytes()).await.unwrap();
    let state = app.state_json();
    let item = state["items"].as_array().unwrap().last().unwrap();
    assert!(item["title"].as_str().unwrap().ends_with(" (RV1909)"), "{}", item["title"]);
    let slides = item["slides"].as_array().unwrap();
    assert_eq!(slides.len(), 2);
    assert!(slides[0]["content"]["reference"].as_str().unwrap().ends_with("23:1 (RV1909)"), "{}", slides[0]["content"]);
    assert_eq!(slides[0]["content"]["footer"], "RV1909: © Sociedad Bíblica (CC BY-SA 4.0)", "CC BY-SA requires the credit, even with credits off");
    assert!(slides[1]["content"].get("footer").is_none(), "once per passage");
}

#[tokio::test]
async fn which_bibles_must_show_their_credit() {
    let app = test_app();
    downloaded_rv1909(&app, bible::library::Licence::Pd, "Public domain");
    app.refresh_bible_credits();
    let data = app.data();
    let rv = data.bible_credit("ebible:spaRV1909").unwrap();
    assert!(!rv.required, "public domain: {rv:?}");
    assert_eq!(rv.abbr, "RV1909");
    let kjv = data.bible_credit("KJV").unwrap();
    assert_eq!((kjv.line.as_str(), kjv.required), ("", false), "built in: nothing to credit");
    let esv = data.bible_credit("ESV").unwrap();
    assert!(esv.required && esv.line.contains("ESV"), "{esv:?}");
}

/// A Bible from YouVersion's catalog: added to the list without a download, credited, and removed again. The
/// Passion Translation can't be added even when a saved list names it.
#[tokio::test]
async fn a_youversion_bible_is_added_listed_credited_and_removed() {
    let app = test_app();
    let dir = app.store.root.join("bibles/catalog");
    std::fs::create_dir_all(&dir).unwrap();
    let books: Vec<&str> = bible::USFM.to_vec();
    let list = json!({ "checked_at": now_secs(), "rows": [
        { "id": 1588, "abbreviation": "AMP", "title": "Amplified Bible", "language_tag": "en", "publisher_name": "The Lockman Foundation", "books": books },
        { "id": 1849, "abbreviation": "TPT", "title": "The Passion Translation", "language_tag": "en", "books": books },
    ] });
    std::fs::write(dir.join("youversion.json"), list.to_string()).unwrap();
    let added = app.do_action("bible/install", br#"{"translation":"yv:1588"}"#).await.unwrap();
    assert_eq!((added["id"].as_str(), added["abbr"].as_str()), (Some("yv:1588"), Some("AMP")));
    assert!(app.translations().contains(&"yv:1588".to_string()));
    let sources = app.bible_sources();
    let amp = sources.iter().find(|b| b["translation"] == "yv:1588").unwrap();
    assert_eq!((amp["abbr"].as_str(), amp["name"].as_str(), amp["language"].as_str()), (Some("AMP"), Some("Amplified Bible"), Some("English")));
    assert_eq!(amp["status"], "needs_key", "tests have no YouVersion key");
    assert!(crate::scripture::online::online_only(&app.store.root, "yv:1588"), "its text is never saved with a service");
    let credit = app.data().bible_credit("yv:1588").cloned().unwrap();
    assert!(credit.abbr == "AMP" && credit.required && credit.line.contains("Lockman"), "{credit:?}");
    assert!(app.do_action("bible/install", br#"{"translation":"yv:1849"}"#).await.is_err(), "blocked");
    assert!(!app.translations().iter().any(|t| t == "yv:1849"));
    app.do_action("bible/import-remove", br#"{"translation":"yv:1588"}"#).await.unwrap();
    assert!(!app.translations().iter().any(|t| t == "yv:1588") && app.data().bible_credit("yv:1588").is_none());
}

/// A Hebrew-numbered Bible beside an English one (spec §5.4): Malachi 4:1 in English is 3:19 there. It is found
/// under its own number and lines up with the English verse; the other way round too.
#[tokio::test]
async fn a_hebrew_numbered_bible_lines_up_beside_an_english_one() {
    use crate::scripture::VerseId;
    let app = test_app();
    let mut verses = bible::import::Verses::new();
    for v in 1..=24u16 { verses.insert((38, 3, v), format!("heb 3:{v}")); }
    for v in 1..=13u16 { verses.insert((37, 2, v), format!("zec 2:{v}")); }
    let meta = bible::library::BibleMeta { id: "ebible:hboWLC".into(), abbr: "WLC".into(), name: "Westminster Leningrad Codex".into(), licence: bible::library::Licence::Pd, ..Default::default() };
    let saved = bible::import::install(&app.store.root, bible::import::ImportedBible { abbreviation: meta.id.clone(), name: meta.name.clone(), copyright: "Public domain".into(), verses, meta: Some(meta) }).unwrap();
    assert_eq!(saved.versification, bible::library::Vsys::Original, "read from its verses");
    // English main, Hebrew beside it.
    let (found, left_out) = app.lookup_also("KJV", &["ebible:hboWLC".to_string()], "Malachi 4:1-2").await;
    assert!(left_out.is_empty(), "{left_out:?}");
    let v = &found[0].verses;
    assert_eq!(v.iter().map(|v| (v.text.as_str(), v.reference.as_str(), v.id)).collect::<Vec<_>>(),
        [("heb 3:19", "Malachi 3:19", Some(VerseId(38, 4, 1))), ("heb 3:20", "Malachi 3:20", Some(VerseId(38, 4, 2)))]);
    // Hebrew main (typed in its own numbering), English beside it.
    let (found, _) = app.lookup_also("ebible:hboWLC", &["KJV".to_string()], "Malachi 3:19").await;
    let v = &found[0].verses;
    assert_eq!((v.len(), v[0].reference.as_str(), v[0].id), (1, "Malachi 4:1", Some(VerseId(38, 3, 19))));
    assert!(v[0].text.starts_with("For, behold, the day cometh"), "{}", v[0].text);
    // Typed the English way with the Hebrew Bible as the main one: found under its own number.
    let (label, typed) = app.lookup_passage("ebible:hboWLC", "Malachi 4:1-2").await.unwrap();
    assert_eq!((label.as_str(), typed.iter().map(|v| v.text.as_str()).collect::<Vec<_>>()), ("Malachi 3:19–20", vec!["heb 3:19", "heb 3:20"]));
    assert!(app.lookup_passage("ebible:hboWLC", "Malachi 9:1").await.is_err());
    // Where the numbers agree, nothing moves.
    let (found, _) = app.lookup_also("KJV", &["ebible:hboWLC".to_string()], "Malachi 3:1").await;
    assert_eq!((found[0].verses[0].text.as_str(), found[0].verses[0].id), ("heb 3:1", Some(VerseId(38, 3, 1))));
}

/// B5, B16: where two English verses are one Hebrew verse, the Hebrew verse shows once and stands for both, the
/// slides keep the two together, and the other way round both English verses show beside the Hebrew one.
#[tokio::test]
async fn two_english_verses_that_are_one_hebrew_verse_stay_paired() {
    use crate::scripture::VerseId;
    let app = test_app();
    let mut verses = bible::import::Verses::new();
    for v in 1..=6u16 { verses.insert((18, 13, v), format!("heb ps 13:{v}")); }
    for v in 1..=19u16 { verses.insert((22, 63, v), format!("heb isa 63:{v}")); }
    for v in 1..=11u16 { verses.insert((22, 64, v), format!("heb isa 64:{v}")); }
    // What makes it read as Hebrew-numbered.
    for v in 1..=24u16 { verses.insert((38, 3, v), format!("heb mal 3:{v}")); }
    let meta = bible::library::BibleMeta { id: "ebible:hboWLC".into(), abbr: "WLC".into(), name: "Westminster Leningrad Codex".into(), licence: bible::library::Licence::Pd, ..Default::default() };
    bible::import::install(&app.store.root, bible::import::ImportedBible { abbreviation: meta.id.clone(), name: meta.name.clone(), copyright: "Public domain".into(), verses, meta: Some(meta) }).unwrap();
    let hebrew = ["ebible:hboWLC".to_string()];
    let shape = |v: &[VerseText]| v.iter().map(|v| (v.text.clone(), v.id, v.through)).collect::<Vec<_>>();

    let (found, left_out) = app.lookup_also("KJV", &hebrew, "Psalm 13:5-6").await;
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(shape(&found[0].verses), [("heb ps 13:6".to_string(), Some(VerseId(18, 13, 5)), Some(VerseId(18, 13, 6)))], "one Hebrew verse for both");
    // One verse to a slide would part them; the pair stays on one slide, so nothing is left unpaired.
    let (_, main) = app.lookup_passage("KJV", "Psalm 13:5-6").await.unwrap();
    let (slides, _) = crate::slides::parallel_slides(&main, "KJV", &[("WLC".into(), found[0].verses.as_slice())], 1, Default::default(), &crate::model::Theme::default());
    assert_eq!(slides.len(), 1, "13:5 and 13:6 share the slide with the one Hebrew verse");

    let (found, _) = app.lookup_also("KJV", &hebrew, "Isaiah 64:1-2").await;
    assert_eq!(shape(&found[0].verses), [("heb isa 63:19".to_string(), Some(VerseId(22, 64, 1)), None), ("heb isa 64:1".to_string(), Some(VerseId(22, 64, 2)), None)],
        "64:1 is the end of Hebrew 63:19, not Hebrew 64:1");
    let (found, _) = app.lookup_also("KJV", &hebrew, "Isaiah 63:19-64:1").await;
    assert_eq!(shape(&found[0].verses), [("heb isa 63:19".to_string(), Some(VerseId(22, 63, 19)), Some(VerseId(22, 64, 1)))]);

    // Hebrew main, English beside it: both English verses show beside the one Hebrew verse.
    let (found, _) = app.lookup_also("ebible:hboWLC", &["KJV".to_string()], "Psalm 13:6").await;
    let v = &found[0].verses;
    assert_eq!((v.len(), v[0].id, v[0].reference.as_str()), (1, Some(VerseId(18, 13, 6)), "Psalm 13:5-6"));
    assert!(v[0].text.starts_with("But I have trusted in thy mercy") && v[0].text.contains("I will sing unto the LORD"), "{}", v[0].text);
}

/// B4: an online Bible whose numbering is not known is left out where the numbers can differ.
#[tokio::test]
async fn an_online_bible_that_may_number_its_own_way_is_left_out_where_verses_move() {
    let app = test_app();
    let online = |id: &str, lang: &str| bible::online::OnlineBible { id: id.into(), abbr: id.into(), name: id.into(), lang: lang.into(), ..Default::default() };
    bible::online::add(&app.store.root, online("yv:400", "ru")).unwrap();
    bible::online::add(&app.store.root, online("yv:500", "es")).unwrap();
    let ids = ["yv:400".to_string(), "WEB".to_string()];
    let (found, left_out) = app.lookup_also("KJV", &ids, "Psalm 23:1").await;
    assert_eq!((found.iter().map(|a| a.translation.as_str()).collect::<Vec<_>>(), left_out), (vec!["WEB"], vec!["yv:400".to_string()]),
        "left out without being looked up: its Psalm 23 may be another psalm");
    // With that Bible as the main one, nothing can be lined up beside it there.
    let (found, left_out) = app.lookup_also("yv:400", &["WEB".to_string()], "Psalm 23:1").await;
    assert!(found.is_empty() && left_out == ["WEB"], "{left_out:?}");
    // A language that numbers the English way is looked up as before (and fails here only for want of a key).
    assert!(!bible::versify::language_may_number_its_own_way(&bible::online::get(&app.store.root, "yv:500").unwrap().lang));
}

/// B4: a Bible with the English chapters and the Greek psalm numbers shows the same psalm beside an English one.
#[tokio::test]
async fn a_bible_with_greek_psalm_numbers_lines_up_beside_an_english_one() {
    use crate::scripture::VerseId;
    let app = test_app();
    let mut verses = bible::import::Verses::new();
    for v in 1..=39u16 { verses.insert((18, 9, v), format!("syn ps 9:{v}")); }
    for v in 1..=6u16 { verses.insert((18, 22, v), format!("syn ps 22:{v}")); }
    for v in 1..=10u16 { verses.insert((18, 23, v), format!("syn ps 23:{v}")); }
    for v in 1..=6u16 { verses.insert((38, 4, v), format!("syn mal 4:{v}")); }
    let meta = bible::library::BibleMeta { id: "ebible:russyn".into(), abbr: "SYN".into(), name: "Synodal".into(), licence: bible::library::Licence::Pd, ..Default::default() };
    let saved = bible::import::install(&app.store.root, bible::import::ImportedBible { abbreviation: meta.id.clone(), name: meta.name.clone(), copyright: "Public domain".into(), verses, meta: Some(meta) }).unwrap();
    assert_eq!(saved.versification, bible::library::Vsys::Vulgate);
    let (found, left_out) = app.lookup_also("KJV", &["ebible:russyn".to_string()], "Psalm 23:1").await;
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!((found[0].verses[0].text.as_str(), found[0].verses[0].id), ("syn ps 22:1", Some(VerseId(18, 23, 1))), "the shepherd psalm, not the next one");
    let (found, _) = app.lookup_also("KJV", &["ebible:russyn".to_string()], "Malachi 4:1").await;
    assert_eq!(found[0].verses[0].text, "syn mal 4:1");
}

#[tokio::test]
async fn removing_a_bible_drops_its_credit() {
    let app = test_app();
    downloaded_rv1909(&app, bible::library::Licence::Pd, "Public domain");
    app.refresh_bible_credits();
    app.do_action("bible/import-remove", br#"{"translation":"ebible:spaRV1909"}"#).await.unwrap();
    assert!(app.data().bible_credit("ebible:spaRV1909").is_none());
}

#[tokio::test]
async fn settings_translation_saves_and_rejects_unknown() {
    let app = test_app();
    app.do_action("settings/translation", br#"{"translation":"NIV"}"#).await.unwrap();
    assert_eq!(app.data().settings.translation, "NIV");
    assert_eq!(app.store.load().unwrap().settings.translation, "NIV", "saved to disk");
    assert_eq!(app.do_action("settings/translation", br#"{"translation":"XYZ"}"#).await.unwrap_err().message, "Unknown translation.");
    assert_eq!(app.data().settings.translation, "NIV");
}

#[tokio::test]
async fn picking_a_bible_puts_it_first_in_recent() {
    let app = test_app();
    // NLT, CSB and HCSB are listed only for a church whose own key can supply them.
    for hidden in ["NLT", "CSB", "HCSB"] { assert!(!app.translations().iter().any(|t| t == hidden), "{hidden}") }
    assert!(app.do_action("settings/translation", br#"{"translation":"NLT"}"#).await.is_err());
    let mut keys = bible::sources::Secrets::default();
    for service in ["api_bible", "youversion"] { keys.keys.insert(service.into(), "k".into()); }
    keys.save(&app.store.root).unwrap();
    for t in ["NIV", "WEB", "NIV", "BSB", "ESV", "NLT", "CSB", "HCSB", "KJV", "WEB"] {
        app.do_action("settings/translation", json!({ "translation": t }).to_string().as_bytes()).await.unwrap();
    }
    let recent = app.store.load().unwrap().settings.recent;
    assert_eq!(recent, ["WEB", "KJV", "HCSB", "CSB", "NLT", "ESV", "BSB", "NIV"], "newest first, once each, saved");
    assert_eq!(recent.len(), crate::model::RECENT_MAX);
    app.do_action("settings/translation", br#"{"translation":"XYZ"}"#).await.unwrap_err();
    assert_eq!(app.data().settings.recent[0], "WEB", "a refused pick changes nothing");
}

#[tokio::test]
async fn language_is_saved_switched_and_kept_by_a_settings_save() {
    let app = test_app();
    assert_eq!(crate::i18n::current(), "en");
    let r = app.do_action("settings/language", br#"{"code":"es"}"#).await.unwrap();
    assert_eq!(app.store.load().unwrap().settings.ui_language, "es");
    assert_eq!(crate::i18n::current(), "es");
    assert_eq!(app.state_json()["lang"], "es", "the phone is told");
    assert_eq!(r["toast"], crate::i18n::tr("Language changed"), "the toast is in the new language");
    let err = app.do_action("settings/language", br#"{"code":"xx"}"#).await.unwrap_err();
    assert_eq!(err.message, crate::i18n::tr("Unknown language."));
    assert_eq!((app.data().settings.ui_language.as_str(), crate::i18n::current().as_str()), ("es", "es"), "a refused code changes nothing");
    // A Settings form opened before the change is saved: the language stays.
    app.do_action("settings", serde_json::to_string(&crate::model::Settings::default()).unwrap().as_bytes()).await.unwrap();
    assert_eq!(app.store.load().unwrap().settings.ui_language, "es");
    app.do_action("settings/language", br#"{"code":"en"}"#).await.unwrap();
    assert_eq!((crate::i18n::current().as_str(), app.state_json()["lang"].as_str()), ("en", Some("en")));
    assert_eq!(app.do_action("settings/language", br#"{"code":"en"}"#).await.unwrap()["toast"], "Language changed");
}

#[tokio::test]
async fn graphics_is_saved_for_the_next_start_and_kept_by_a_settings_save() {
    use crate::model::Graphics;
    let app = test_app();
    let r = app.do_action("settings/graphics", br#"{"graphics":"software"}"#).await.unwrap();
    assert_eq!(r["toast"], "Restart RustyPresenter to use the new graphics setting.");
    assert_eq!(app.store.load().unwrap().settings.graphics, Graphics::Software);
    assert!(app.do_action("settings/graphics", br#"{"graphics":"hologram"}"#).await.is_err());
    // A Settings form opened before the change is saved: the graphics stay.
    app.do_action("settings", serde_json::to_string(&crate::model::Settings::default()).unwrap().as_bytes()).await.unwrap();
    assert_eq!(app.store.load().unwrap().settings.graphics, Graphics::Software);
}

#[tokio::test]
async fn favourites_star_unstar_and_survive_a_settings_save() {
    let app = test_app();
    let star = |t: &str, on: bool| json!({ "translation": t, "on": on }).to_string();
    app.do_action("settings/favourite", star("ESV", true).as_bytes()).await.unwrap();
    app.do_action("settings/favourite", star("KJV", true).as_bytes()).await.unwrap();
    app.do_action("settings/favourite", star("esv", true).as_bytes()).await.unwrap_err();
    app.do_action("settings/favourite", star("ESV", true).as_bytes()).await.unwrap();
    assert_eq!(app.store.load().unwrap().settings.favourites, ["ESV", "KJV"], "once each, in the order starred");
    assert_eq!(app.do_action("settings/favourite", star("XYZ", true).as_bytes()).await.unwrap_err().message, "Unknown translation.");
    assert_eq!(app.do_action("settings/favourite", star("NWT", true).as_bytes()).await.unwrap_err().message, blocked());
    // A form opened before the star is saved: the star stays.
    let mut form = crate::model::Settings::default();
    form.lines_per_slide = 3;
    app.do_action("settings", serde_json::to_string(&form).unwrap().as_bytes()).await.unwrap();
    let s = app.data().settings.clone();
    assert_eq!((s.lines_per_slide, s.favourites.as_slice()), (3, ["ESV".to_string(), "KJV".to_string()].as_slice()));
    app.do_action("settings/favourite", star("ESV", false).as_bytes()).await.unwrap();
    app.do_action("settings/favourite", star("GONE", false).as_bytes()).await.unwrap();
    assert_eq!(app.data().settings.favourites, ["KJV"], "unstarring needs no installed Bible");
}

#[test]
fn blocked_bibles_leave_favourites_and_recent() {
    let mut d = Data::default();
    d.settings.favourites = vec!["KJV".into(), "NWT".into()];
    d.settings.recent = vec!["TPT".into(), "WEB".into()];
    bible::blocklist::strip(&mut d);
    assert_eq!((d.settings.favourites, d.settings.recent), (vec!["KJV".to_string()], vec!["WEB".to_string()]));
}

#[tokio::test]
async fn display_close_rejects_unknown_role() {
    let err = test_app().do_action("display/close", br#"{"role":"projector"}"#).await.unwrap_err();
    assert_eq!((err.status, err.message.as_str()), (400, "Unknown screen."));
}

#[tokio::test]
async fn backup_now_writes_a_backup() {
    let app = test_app();
    let v = app.do_action("backup/now", b"{}").await.unwrap();
    assert!(app.store.backups().iter().any(|b| b.name == v["name"].as_str().unwrap()));
}

#[test]
fn fifty_installed_bibles_are_listed_without_reading_their_verses() {
    // Library spec §14, lazy loading: a church with many Bibles must not pay for parsing them on every update.
    use bible::import::{self, ImportedBible};
    let seed = crate::store::test_dir();
    assert!(import::list(&seed).is_empty());
    let ids: Vec<String> = (0..50).map(|i| format!("B{i:02}")).collect();
    for id in &ids {
        let mut verses = bible::import::Verses::new();
        verses.insert((42, 3, 16), format!("{id} John 3:16"));
        import::save(&seed, &ImportedBible { abbreviation: id.clone(), name: format!("{id} Bible"), verses, ..Default::default() }).unwrap();
    }
    // A fresh data folder holding those files and their index, as after a restart.
    let root = std::env::temp_dir().join(format!("rp-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(root.join("bibles")).unwrap();
    for e in std::fs::read_dir(seed.join("bibles")).unwrap().flatten() {
        if e.path().is_file() { std::fs::copy(e.path(), root.join("bibles").join(e.file_name())).unwrap(); }
    }
    let store = Store::open(root.clone()).unwrap();
    let data = store.load().unwrap();
    let app = App::new(store, data, 0, None);
    let state = app.state_json();
    let available = app.available_translations();
    app.refresh_bible_credits();
    for id in &ids { assert!(available.contains(id), "{id} is listed from the index"); }
    assert_eq!(state["available_translations"].as_array().unwrap().len(), available.len());
    assert_eq!(import::in_memory(&root), 0, "no Bible was parsed for the snapshot, the sources or the credits");
    assert!(import::get(&root, "B07").is_some());
    assert_eq!(import::in_memory(&root), 1, "a lookup is what loads one");
}

#[test]
fn available_translations_are_the_bundled_ones() {
    let app = test_app();
    assert_eq!(app.available_translations(), ["KJV", "WEB", "BSB"]);
    assert_eq!(app.state_json()["available_translations"], json!(["KJV", "WEB", "BSB"]));
}

#[test]
fn screen_guard_marks_room_connected_until_dropped() {
    let app = test_app();
    let g = app.screen_guard("room");
    assert_eq!(app.state_json()["room_connected"], true);
    assert_eq!(app.state_json()["stage_connected"], false);
    drop(g);
    assert_eq!(app.state_json()["room_connected"], false);
    let g = app.screen_guard("stage");
    assert_eq!(app.state_json()["stage_connected"], true);
    drop(g);
    assert_eq!(app.state_json()["stage_connected"], false);
}

#[tokio::test]
async fn import_path_adds_a_picture_to_the_media_library() {
    let app = test_app();
    let png = std::env::temp_dir().join(format!("{}.png", Uuid::new_v4()));
    image::RgbaImage::new(4, 4).save(&png).unwrap();
    let Imported::Media(m) = app.import_path(&png, ImportPurpose::Media, |_| {}).await.unwrap() else { panic!() };
    assert_eq!((m.kind, m.status.clone()), (MediaKind::Image, MediaStatus::Ready));
    assert!(png.exists(), "the user's file is copied, never moved");
    assert_eq!(app.data().media.len(), 1);
    assert!(app.store.asset_path(&m.asset).exists());
}

#[tokio::test]
async fn import_path_leaves_unknown_extensions_to_the_probe() {
    let p = std::env::temp_dir().join(format!("{}.xyz", Uuid::new_v4()));
    std::fs::write(&p, b"x").unwrap();
    let e = test_app().import_path(&p, ImportPurpose::Media, |_| {}).await.unwrap_err();
    assert_eq!(e.message, crate::video::ffi::missing());
}

#[tokio::test]
async fn import_path_lyrics_imports_a_songselect_song() {
    let app = test_app();
    let p = std::env::temp_dir().join(format!("{}.txt", Uuid::new_v4()));
    std::fs::write(&p, "A Test Song\r\n\r\nVerse 1\r\nOriginal test line one\r\nOriginal test line two\r\n\r\nTest Writer\r\nCCLI Song # 12345\r\n© 2026 Test Music\r\nCCLI License # 98765").unwrap();
    let Imported::Song { id, title, duplicate_of } = app.import_path(&p, ImportPurpose::Lyrics, |_| {}).await.unwrap() else { panic!() };
    assert_eq!((title.as_str(), duplicate_of), ("A Test Song", None));
    let id = id.unwrap();
    assert_eq!(app.data().songs[0].id, id);
    // The same song again saves nothing: the operator asks what to do.
    let Imported::Song { id: second, duplicate_of, .. } = app.import_path(&p, ImportPurpose::Lyrics, |_| {}).await.unwrap() else { panic!() };
    assert_eq!((second, duplicate_of), (None, Some(id)));
    assert_eq!(app.data().songs.len(), 1);
    assert!(p.exists());
}

const SONG_V1: &str = "A Test Song\n\nVerse 1\nOriginal test line one\n\nChorus\nOriginal test chorus\n\nTest Writer\nCCLI Song # 12345\n© 2026 Test Music\nCCLI License # 98765";
const SONG_V2: &str = "A Test Song (New Key)\n\nVerse 1\nChanged test line one\n\nNew Writer\nCCLI Song # 12345\n© 2027 Test Music\nCCLI License # 98765";

fn song_import_body(text: &str, mode: &str) -> Vec<u8> { json!({ "text": text, "mode": mode }).to_string().into_bytes() }

#[tokio::test]
async fn import_ask_without_duplicate_saves() {
    let app = test_app();
    let v = app.do_action("song/import", &song_import_body(SONG_V1, "ask")).await.unwrap();
    let songs = app.data().songs.clone();
    assert_eq!(songs.len(), 1);
    assert_eq!(v, json!({ "id": songs[0].id, "title": "A Test Song", "duplicate_of": null }));
}

#[tokio::test]
async fn import_ask_with_duplicate_saves_nothing() {
    let app = test_app();
    let first = app.do_action("song/import", &song_import_body(SONG_V1, "ask")).await.unwrap();
    let mut rx = app.subscribe();
    let v = app.do_action("song/import", &song_import_body(SONG_V2, "ask")).await.unwrap();
    assert_eq!(v, json!({ "id": null, "title": "A Test Song (New Key)", "duplicate_of": first["id"] }));
    assert!(matches!(rx.try_recv(), Err(broadcast::error::TryRecvError::Empty)), "the screens were told of a change");
    let songs = app.data().songs.clone();
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].title, "A Test Song");
}

#[tokio::test]
async fn import_ask_never_matches_on_an_empty_ccli_number() {
    let app = test_app();
    let text = "No Number\n\nVerse 1\nA line";
    app.do_action("song/import", &song_import_body(text, "ask")).await.unwrap();
    let v = app.do_action("song/import", &song_import_body(text, "ask")).await.unwrap();
    assert!(v["id"].is_string());
    assert_eq!(v["duplicate_of"], Value::Null);
    assert_eq!(app.data().songs.len(), 2);
}

#[tokio::test]
async fn import_update_keeps_song_id_and_service_items() {
    let app = test_app();
    let first = app.do_action("song/import", &song_import_body(SONG_V1, "ask")).await.unwrap();
    let id: Uuid = serde_json::from_value(first["id"].clone()).unwrap();
    app.edit(|d| {
        let song = d.songs.iter_mut().find(|s| s.id == id).unwrap();
        song.arrangement = vec!["Chorus".into(), "Verse 1".into()];
        let service = d.service_mut().ok_or("no service")?;
        service.items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: id, arrangement: None }, background: None, notes: String::new(), theme: None });
        Ok(())
    }).unwrap();
    let v = app.do_action("song/import", &song_import_body(SONG_V2, "update")).await.unwrap();
    assert_eq!(v["id"], first["id"]);
    let d = app.data();
    assert_eq!(d.songs.len(), 1);
    let song = &d.songs[0];
    assert_eq!((song.id, song.title.as_str(), song.ccli.as_str()), (id, "A Test Song (New Key)", "12345"));
    assert_eq!(song.writers, "New Writer");
    assert!(song.copyright.contains("2027"));
    assert_eq!(song.arrangement, vec!["Chorus".to_string(), "Verse 1".to_string()]);
    let item_song = match d.service().unwrap().items[0].kind { ItemKind::Song { song_id, .. } => song_id, _ => panic!() };
    let shown = d.songs.iter().find(|s| s.id == item_song).unwrap();
    let settings = Settings { credits: "none".into(), ..Default::default() };
    let texts: Vec<_> = crate::slides::song_slides(shown, &settings).into_iter().map(|s| match s.content { SlideContent::Text { text, .. } => text, _ => panic!() }).collect();
    // "Chorus" is no longer a section, so the core drops that name from the order.
    assert_eq!(texts, vec!["Changed test line one"]);
}

#[tokio::test]
async fn import_update_of_a_song_deleted_meanwhile_adds_it() {
    let app = test_app();
    let v = app.do_action("song/import", &song_import_body(SONG_V1, "update")).await.unwrap();
    assert!(v["id"].is_string());
    assert_eq!(v["duplicate_of"], Value::Null);
    assert_eq!(app.data().songs.len(), 1);
}

#[tokio::test]
async fn import_keep_both_adds_second_song() {
    let app = test_app();
    let first = app.do_action("song/import", &song_import_body(SONG_V1, "ask")).await.unwrap();
    let v = app.do_action("song/import", &song_import_body(SONG_V2, "keep_both")).await.unwrap();
    assert_ne!(v["id"], first["id"]);
    assert_eq!(v["duplicate_of"], first["id"]);
    assert_eq!(app.data().songs.len(), 2);
}

#[tokio::test]
async fn song_import_rejects_an_unknown_mode() {
    let e = test_app().do_action("song/import", &song_import_body(SONG_V1, "replace")).await.unwrap_err();
    assert_eq!(e.status, 400);
}

#[tokio::test]
async fn import_path_action_imports_a_file_on_this_computer() {
    let app = test_app();
    let png = std::env::temp_dir().join(format!("{}.png", Uuid::new_v4()));
    image::RgbaImage::new(2, 2).save(&png).unwrap();
    let body = json!({ "path": png, "purpose": "media" }).to_string();
    let v = app.do_action("import/path", body.as_bytes()).await.unwrap();
    assert!(v["media"]["asset"].is_string(), "{v}");
    assert_eq!(app.data().media.len(), 1);
    let body = json!({ "path": png, "purpose": "other" }).to_string();
    assert_eq!(app.do_action("import/path", body.as_bytes()).await.unwrap_err().status, 400);
}

#[tokio::test]
async fn import_path_asset_returns_the_stored_asset() {
    let app = test_app();
    let png = std::env::temp_dir().join(format!("{}.png", Uuid::new_v4()));
    image::RgbaImage::new(2, 2).save(&png).unwrap();
    let Imported::Asset(a) = app.import_path(&png, ImportPurpose::Asset, |_| {}).await.unwrap() else { panic!() };
    assert!(a.ends_with(".png") && app.store.asset_path(&a).exists());
    assert!(app.data().media.is_empty(), "an asset is not a media library entry");
}

#[tokio::test]
async fn import_path_reports_progress_up_to_100() {
    let app = test_app();
    let png = std::env::temp_dir().join(format!("{}.png", Uuid::new_v4()));
    image::RgbaImage::new(600, 600).save(&png).unwrap();
    let seen = Arc::new(Mutex::new(Vec::<u8>::new()));
    let s = seen.clone();
    app.import_path(&png, ImportPurpose::Media, move |p| s.lock().unwrap().push(p)).await.unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.last(), Some(&100));
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "progress only goes up: {seen:?}");
}

#[tokio::test]
async fn import_path_missing_file_is_an_error() {
    let p = std::env::temp_dir().join(format!("{}.png", Uuid::new_v4()));
    assert_eq!(test_app().import_path(&p, ImportPurpose::Media, |_| {}).await.unwrap_err().status, 400);
}

#[test]
fn note_survives_job_end_until_dismissed() {
    let app = test_app();
    let id = app.job_start("Importing");
    app.job_note(id, "Imported with LibreOffice — check the slides.".into());
    app.job_end(id, None);
    {
        let jobs = app.jobs.lock().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].note.as_deref(), Some("Imported with LibreOffice — check the slides."));
        assert_eq!(jobs[0].percent, 100);
        assert!(jobs[0].done && !jobs[0].cancellable);
    }
    do_dismiss(&app, id);
    assert!(app.jobs.lock().unwrap().is_empty());
}

fn do_dismiss(app: &Arc<App>, id: Uuid) {
    tokio::runtime::Runtime::new().unwrap().block_on(app.do_action("job/dismiss", json!({ "id": id }).to_string().as_bytes())).unwrap();
}

#[test]
fn job_without_note_is_removed_on_end() {
    let app = test_app();
    let id = app.job_start("Importing");
    app.job_end(id, None);
    assert!(app.jobs.lock().unwrap().is_empty());
}

#[test]
fn cancel_token_fires_and_job_disappears() {
    let app = test_app();
    let (id, token) = app.job_start_cancellable("Importing");
    assert!(app.jobs.lock().unwrap()[0].cancellable);
    assert!(!token.is_cancelled());
    app.job_cancel(id);
    assert!(token.is_cancelled());
    assert_eq!(app.jobs.lock().unwrap().len(), 1, "the job stays until the work has stopped");
    app.job_note(id, "A note from before the cancel.".into());
    app.job_end_cancelled(id);
    assert!(app.jobs.lock().unwrap().is_empty(), "no error and no note left behind");
    assert!(app.cancels.lock().unwrap().is_empty());
}

#[test]
fn every_end_drops_the_cancel_token() {
    let app = test_app();
    let (ok, _) = app.job_start_cancellable("a");
    let (failed, _) = app.job_start_cancellable("b");
    let (noted, _) = app.job_start_cancellable("c");
    app.job_note(noted, "Note".into());
    app.job_end(ok, None);
    app.job_end(failed, Some("Nope".into()));
    app.job_end(noted, None);
    assert!(app.cancels.lock().unwrap().is_empty());
    assert!(app.jobs.lock().unwrap().iter().all(|j| !j.cancellable), "nothing left to cancel");
}

#[tokio::test]
async fn job_cancel_action_is_wired() {
    let app = test_app();
    let (id, token) = app.job_start_cancellable("Importing");
    app.do_action("job/cancel", json!({ "id": Uuid::new_v4() }).to_string().as_bytes()).await.expect("an unknown id is ignored");
    assert!(!token.is_cancelled());
    app.do_action("job/cancel", json!({ "id": id }).to_string().as_bytes()).await.unwrap();
    assert!(token.is_cancelled());
}

#[test]
fn job_label_broadcasts() {
    let app = test_app();
    let id = app.job_start("Importing");
    let mut rx = app.tx.subscribe();
    app.job_label(id, "Waiting for LibreOffice…");
    assert_eq!(app.jobs.lock().unwrap()[0].label, "Waiting for LibreOffice…");
    assert!(matches!(rx.try_recv(), Ok(Update::Data)));
}

#[test]
fn job_note_broadcasts_and_the_last_one_wins() {
    let app = test_app();
    let id = app.job_start("Importing");
    let mut rx = app.tx.subscribe();
    app.job_note(id, "First".into());
    assert!(matches!(rx.try_recv(), Ok(Update::Data)));
    app.job_note(id, "Second".into());
    assert_eq!(app.jobs.lock().unwrap()[0].note.as_deref(), Some("Second"));
}

#[test]
fn fallback_note_and_another_are_joined_fallback_first() {
    let count = "12 slides were expected but 11 were imported.";
    let fallback = fallback_note();
    let fallback = fallback.as_str();
    for notes in [[fallback, count], [count, fallback]] {
        let app = test_app();
        let id = app.job_start("Importing");
        for n in notes { app.job_note(id, n.into()); }
        assert_eq!(app.jobs.lock().unwrap()[0].note.as_deref(), Some(format!("{fallback} {count}").as_str()));
    }
    let app = test_app();
    let id = app.job_start("Importing");
    app.job_note(id, fallback_note().into());
    app.job_note(id, fallback_note().into());
    assert_eq!(app.jobs.lock().unwrap()[0].note.as_deref(), Some(fallback_note().as_str()), "not doubled");
}

#[test]
fn fallback_note_survives_later_notes_and_is_never_doubled() {
    let count = "12 slides were expected but 11 were imported.";
    let other = "Imported with LibreOffice — check the slides.";
    let fallback = fallback_note();
    let fallback = fallback.as_str();
    let note = |notes: &[&str]| {
        let app = test_app();
        let id = app.job_start("Importing");
        for n in notes { app.job_note(id, (*n).into()); }
        app.jobs.lock().unwrap()[0].note.clone().unwrap()
    };
    assert_eq!(note(&[fallback, count, other]), format!("{fallback} {other}"));
    assert_eq!(note(&[fallback, count, fallback]), format!("{fallback} {count}"));
    assert_eq!(note(&[count, fallback, other]), format!("{fallback} {other}"));
}

#[test]
fn exporting_a_service_runs_as_a_job_that_goes_away() {
    let app = test_app();
    let dest = app.store.root.join("s.rpservice");
    let id = app.data().service().unwrap().id;
    block_on(app.export_service(id, dest.clone())).unwrap();
    assert!(dest.exists());
    assert!(app.jobs.lock().unwrap().is_empty());
}

#[test]
fn importing_a_bad_service_file_leaves_the_error_on_its_job() {
    let app = test_app();
    let bad = app.store.root.join("x.rpservice");
    std::fs::write(&bad, b"no").unwrap();
    assert_eq!(block_on(app.import_service(bad)).unwrap_err(), crate::bundle::strings::not_a_service_file());
    assert_eq!(app.data().services.len(), 1);
    assert_eq!(app.jobs.lock().unwrap()[0].error.as_deref(), Some(crate::bundle::strings::not_a_service_file().as_str()));
}

#[test]
fn export_with_something_missing_makes_no_job() {
    let app = test_app();
    let dest = app.store.root.join("s.rpservice");
    let id = {
        let mut d = app.data();
        let s = d.service_mut().unwrap();
        s.items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: Uuid::new_v4(), arrangement: None }, background: None, notes: String::new(), theme: None });
        s.id
    };
    let e = block_on(app.export_service(id, dest.clone())).unwrap_err();
    assert!(matches!(e, ExportError::Missing(ref lines) if lines.len() == 1), "{e:?}");
    assert!(!dest.exists());
    assert!(app.jobs.lock().unwrap().is_empty());
}

#[test]
fn job_result_is_kept() {
    let app = test_app();
    let (id, deck) = (app.job_start("Importing"), Uuid::new_v4());
    app.job_result(id, deck);
    assert_eq!(app.jobs.lock().unwrap()[0].result, Some(deck));
}

#[test]
fn job_json_unchanged_without_new_fields() {
    let app = test_app();
    app.job_start("Importing");
    let job = serde_json::to_value(&app.jobs.lock().unwrap()[0]).unwrap();
    let mut keys: Vec<_> = job.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["error", "id", "label", "percent"]);
}

#[tokio::test]
async fn do_action_unknown_action_is_404() {
    assert_eq!(test_app().do_action("nope", b"{}").await.unwrap_err().status, 404);
}

#[tokio::test]
async fn lookup_with_also_returns_each_and_left_out() {
    let app = test_app();
    let v = app.do_action("bible/lookup", br#"{"reference":"John 3:16","translation":"KJV","also":["WEB","ESV","KJV"]}"#).await.unwrap();
    assert_eq!(v["also"][0]["translation"], "WEB");
    assert!(v["also"][0]["verses"][0]["text"].as_str().unwrap().starts_with("For God so loved"));
    assert_eq!(v["also"][0]["verses"][0]["id"], json!([42, 3, 16]));
    assert_eq!(v["left_out"], json!(["ESV"]), "no ESV key; KJV is the main one, so not an Also Bible");
}

#[tokio::test]
async fn main_equal_to_also_turns_also_off() {
    let app = test_app();
    app.do_action("settings/also", br#"{"also_show":["WEB"],"parallel_layout":"stacked"}"#).await.unwrap();
    assert_eq!(app.data().settings.parallel_layout, crate::scripture::ParallelLayout::Stacked);
    let r = app.do_action("settings/translation", br#"{"translation":"WEB"}"#).await.unwrap();
    assert_eq!(r["toast"], "WEB turned off (same as main).");
    assert!(app.data().settings.also_show.is_empty());
}

#[tokio::test]
async fn settings_also_keeps_two_and_refuses_blocked() {
    let app = test_app();
    app.do_action("settings/also", br#"{"also_show":["WEB","KJV","BSB","WEB"]}"#).await.unwrap();
    assert_eq!(app.data().settings.also_show, ["WEB", "BSB"]);
    let err = app.do_action("settings/also", br#"{"also_show":["TPT"]}"#).await.unwrap_err();
    assert_eq!(err.message, blocked());
}

#[tokio::test]
async fn add_with_unavailable_also_adds_the_rest_and_warns() {
    let app = test_app();
    app.edit(|d| { d.settings.also_show = vec!["WEB".into(), "ESV".into()]; Ok(()) }).unwrap();
    let r = app.do_action("service/add", br#"{"item":{"kind":"scripture","reference":"John 3:16","translation":"KJV","verses":[{"reference":"John 3:16","text":"For God"}]}}"#).await.unwrap();
    assert_eq!(r["toast"], "ESV isn't available right now, so only KJV · WEB was added.");
    let d = app.data();
    let item = d.service().unwrap().items.last().unwrap();
    let ItemKind::Scripture { also, .. } = &item.kind else { panic!() };
    assert_eq!(also.iter().map(|a| a.translation.as_str()).collect::<Vec<_>>(), ["WEB"]);
}

#[tokio::test]
async fn online_text_never_saved() {
    let app = test_app();
    let body = json!({ "item": { "kind": "scripture", "reference": "John 3:16", "translation": "KJV",
        "verses": [{ "reference": "John 3:16", "text": "For God so loved the world, that he gave" }],
        "also": [{ "translation": "NIV", "verses": [{ "reference": "John 3:16", "text": "NIV words from the stub" }] }] } });
    app.do_action("service/add", body.to_string().as_bytes()).await.unwrap();
    let saved = std::fs::read_to_string(app.store.data_path()).unwrap();
    assert!(!saved.contains("NIV words from the stub"), "online text stays out of data.json");
    assert!(saved.contains("For God so loved the world, that he gave"), "KJV is stored");
    assert!(saved.contains(r#""translation": "NIV""#), "the Bible itself is kept");
    let name = { let d = app.data().clone(); app.store.backup(&d).unwrap() };
    let backup = std::fs::read_to_string(app.store.root.join("backups").join(name).join("data.json")).unwrap();
    assert!(!backup.contains("NIV words from the stub"), "and out of backups");
    let d = app.data();
    let ItemKind::Scripture { also, .. } = &d.services[0].items[0].kind else { panic!() };
    assert_eq!(also[0].verses.len(), 1, "this session still shows it");
}

#[test]
fn online_text_comes_back_from_the_cache_when_the_service_opens() {
    let root = std::env::temp_dir().join(format!("rp-test-{}", Uuid::new_v4()));
    let store = Store::open(root.clone()).unwrap();
    bible::sources::put_cached(&root, "ESV", "John 3:16", "For God so loved the world, ESV");
    let mut data = Data::default();
    data.services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Scripture { reference: "John 3:16".into(), translation: "ESV".into(), verses: vec![], per_slide: 0, text_size: 0,
        also: vec![crate::scripture::AlsoShow { translation: "NIV".into(), verses: vec![] }], layout: Default::default(), breaks: Default::default() }, background: None, notes: String::new(), theme: None });
    let app = App::new(store, data, 0, None);
    let d = app.data();
    let ItemKind::Scripture { verses, also, .. } = &d.services[0].items[0].kind else { panic!() };
    assert_eq!(verses[0].text, "For God so loved the world, ESV");
    assert!(also[0].verses.is_empty(), "NIV was never fetched, so it stays empty");
    drop(d);
    assert!(app.preflight().iter().any(|l| l.ends_with("NIV isn't ready offline for John 3:16. Open this service once with internet.")), "{:?}", app.preflight());
    assert!(!app.preflight().iter().any(|l| l.contains("ESV isn't ready")));
}
