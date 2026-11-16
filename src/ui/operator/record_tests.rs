use super::*;
use crate::{
    app::test_app,
    record::{FrameSink, Preset, RecordPrefs},
    ui::{operator::window::{OperatorScreens, VideoInfo, testing::refresh}, ops::Ops, ui_test_init},
    video::{DrawMode, FrameGrabber},
};
use slint::Model;
use std::sync::{Arc, Mutex};

/// A room window that may be open, keeping the recording it was handed.
#[derive(Default)]
struct Room { open: bool, sink: Mutex<Option<Arc<FrameSink>>> }

impl OperatorScreens for Room {
    fn video_info(&self) -> VideoInfo { VideoInfo { draw_mode: DrawMode::Software, mpv: None } }
    fn preview_grabber(&self) -> Option<FrameGrabber> { None }
    fn record_to(&self, sink: Option<Arc<FrameSink>>) -> bool {
        *self.sink.lock().unwrap() = sink;
        self.open
    }
}

fn open(app: &Arc<crate::app::App>, room: &Arc<Room>) -> Rc<Operator> {
    ui_test_init();
    Operator::open(app.clone(), Ops::inline(app.clone()), Rc::new(room.clone()) as Rc<dyn OperatorScreens>).unwrap()
}

fn toasts(op: &Operator) -> Vec<(String, bool)> { op.state.borrow().toasts.clone() }

fn has_ffmpeg() -> bool { std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("ffmpeg").is_file() || d.join("ffmpeg.exe").is_file())) }

#[test]
fn with_no_room_screen_it_says_to_turn_it_on() {
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    refresh(&op);
    op.toggle_recording();
    assert!(!op.is_recording() && !op.ui.get_recording());
    assert_eq!(toasts(&op).last(), Some(&(record::tr::no_room(), true)));
}

#[test]
fn record_then_stop_saves_an_mp4_in_the_chosen_folder() {
    if !has_ffmpeg() { return eprintln!("no ffmpeg here: skipped") }
    let app = test_app();
    let folder = std::env::temp_dir().join(format!("rp-record-ui-{}", uuid::Uuid::new_v4()));
    app.data.lock().unwrap().settings.record = RecordPrefs { size: Preset::P720, folder: folder.display().to_string(), sound: "none".into(), ..Default::default() };
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.toggle_recording();
    assert!(op.is_recording(), "{:?}", toasts(&op));
    assert!(op.ui.get_recording());
    assert_eq!(op.ui.get_record_time(), "0:00");
    assert_eq!(toasts(&op).last(), Some(&(record::tr::recording(), false)));
    let sink = room.sink.lock().unwrap().clone().expect("the room was handed the recording");
    assert_eq!(sink.size, (1280, 720));
    let mut b = sink.buffer();
    b.fill(200);
    sink.put(b);
    std::thread::sleep(Duration::from_millis(1200));
    op.toggle_recording();
    assert!(!op.ui.get_recording(), "the button goes back at once");
    assert_eq!(toasts(&op).last(), Some(&(record::tr::finishing(), false)));
    let ended = testing::wait_ended(&op);
    let file = ended.file.clone().expect("a file");
    assert_eq!(file.parent(), Some(folder.as_path()));
    assert!(file.extension().is_some_and(|e| e == "mp4") && file.is_file(), "{ended:?}");
    assert!(!op.is_recording());
    assert!(room.sink.lock().unwrap().is_none(), "the room stops feeding it");
    assert_eq!(toasts(&op).last(), Some(&(record::tr::saved(&file.display().to_string()), false)));
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn file_menu_and_the_top_bar_both_record() {
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    refresh(&op);
    op.ui.invoke_menu("record".into());
    assert_eq!(toasts(&op).last(), Some(&(record::tr::no_room(), true)));
    op.ui.invoke_record();
    assert_eq!(toasts(&op).iter().filter(|t| t.0 == record::tr::no_room()).count(), 2);
}

#[test]
fn without_ffmpeg_it_offers_to_get_it_and_names_the_install_command() {
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    op.offer_ffmpeg();
    let ask = op.ui.get_ask().row_data(0).expect("a question");
    assert!(ask.text.starts_with("Recording needs FFmpeg, a free program."), "{}", ask.text);
    assert!(ask.text.ends_with(tools::ffmpeg_install_command(tools::OS)), "{}", ask.text);
    assert_eq!(ask.ok_label, "Get FFmpeg");
    assert_eq!(ask.cancel_label, "Not now");
}

fn dest(i: usize, on: bool) -> stream::Destination {
    stream::Destination { id: stream::built_in_id(i), service: stream::BUILT_IN[i].0, on, ..Default::default() }
}

#[test]
fn go_live_shows_once_a_destination_is_on_and_needs_its_key() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    assert!(!op.ui.get_can_stream(), "no destination: no Go Live button");
    app.data.lock().unwrap().settings.record.stream = vec![dest(0, true), dest(2, false)];
    refresh(&op);
    assert!(op.ui.get_can_stream());
    // On, but this computer has no key for it: nothing starts, and the operator is told which and why.
    op.go_live();
    assert!(!op.is_recording() && !op.ui.get_live());
    let said = toasts(&op);
    assert_eq!(said.last(), Some(&(stream::tr::nothing_ready(), true)));
    assert!(said.iter().any(|(t, _)| *t == stream::tr::not_ready("YouTube", &stream::tr::no_key())), "{said:?}");
}

#[test]
fn settings_keep_the_destinations_and_never_a_key() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = op.ui.global::<SettingsDlg>();
    let mut prefs = RecordPrefs::default();
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!(prefs, RecordPrefs::default(), "an untouched form changes nothing");
    // YouTube on, a custom server's address typed (not on yet), and a key typed for YouTube.
    crate::ui::dialogs::settings_record::wire_keys(&op);
    g.set_stream_on_0(true);
    g.set_stream_url(" rtmp://stream.example.org/live ".into());
    g.invoke_stream_key_edited(0, "SECRET-KEY".into());
    assert!(g.get_stream_saved_0() && !g.get_stream_saved_1());
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!(prefs.stream.len(), 2);
    assert_eq!((prefs.stream[0].service, prefs.stream[0].on, prefs.stream[1].url.as_str(), prefs.stream[1].on), (stream::Service::YouTube, true, "rtmp://stream.example.org/live", false));
    let json = serde_json::to_string(&prefs).unwrap();
    assert!(!json.contains("SECRET"), "{json}");
    // The key is in the store, under the row's own id; emptying the field forgets it.
    assert_eq!(stream::keys::app_store().get(stream::built_in_id(0)).as_deref(), Some("SECRET-KEY"));
    g.invoke_stream_key_edited(0, "".into());
    assert!(!g.get_stream_saved_0());
}

#[test]
fn livestream_scene_settings_save() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = op.ui.global::<SettingsDlg>();
    let mut prefs = RecordPrefs::default();
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert!(g.get_stream_auto_scenes(), "automatic scenes are on to start with");
    g.set_stream_auto_scenes(false);
    g.set_stream_presentation_camera(1);
    g.set_stream_card_words_2(" Back soon ".into());
    g.set_stream_card_picture_0("C:/Pictures/welcome.png".into());
    g.set_stream_sound_delay(5000);
    // A second camera that is the first again is no second camera.
    g.set_stream_camera_names(slint::ModelRc::new(slint::VecModel::from(vec![slint::SharedString::new(), "Cam A".into(), "Cam B".into()])));
    g.set_stream_camera_index(1);
    g.set_stream_camera2_names(slint::ModelRc::new(slint::VecModel::from(vec![slint::SharedString::new(), "Cam A".into(), "Cam B".into()])));
    g.set_stream_camera2_index(1);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!((prefs.auto_scenes, prefs.presentation_camera), (Some(false), true));
    assert_eq!(prefs.sound_delay_ms, 1000, "at most a second");
    assert_eq!((prefs.camera.as_str(), prefs.camera2.as_str()), ("Cam A", ""));
    g.set_stream_camera2_index(2);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!(prefs.camera2, "Cam B");
    assert_eq!((prefs.cards.hold.words.as_str(), prefs.cards.starting.picture.as_str()), ("Back soon", "C:/Pictures/welcome.png"));
    let json = serde_json::to_value(&prefs).unwrap();
    assert_eq!(json["cards"]["hold"], serde_json::json!({ "words": "Back soon" }));
    let old: RecordPrefs = serde_json::from_value(serde_json::json!({ "camera_layout": 2 })).unwrap();
    assert_eq!(old, RecordPrefs::default(), "the old camera layout is left behind");
}

#[test]
fn settings_keep_a_network_camera_by_its_address() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = op.ui.global::<SettingsDlg>();
    let mut prefs = RecordPrefs { camera: "rtsp://192.168.1.50/stream1".into(), ..RecordPrefs::default() };
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    let labels: Vec<String> = g.get_stream_camera_labels().iter().map(|s| s.to_string()).collect();
    assert_eq!(labels, ["No camera", "Network camera"], "an address is no camera that is not connected");
    assert_eq!(g.get_stream_camera_index(), 1);
    assert_eq!(g.get_stream_camera_address(), "rtsp://192.168.1.50/stream1");
    assert_eq!(g.get_stream_camera2_index(), 0);
    // The second camera on the network too.
    g.set_stream_camera2_index(1);
    g.set_stream_camera2_address("  srt://192.168.1.60:9000 ".into());
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!((prefs.camera.as_str(), prefs.camera2.as_str()), ("rtsp://192.168.1.50/stream1", "srt://192.168.1.60:9000"));
    // Network camera with no address, or something that is not one: no camera.
    g.set_stream_camera_address("192.168.1.50".into());
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!((prefs.camera.as_str(), prefs.camera2.as_str()), ("", ""));
}

#[test]
fn the_speed_test_says_what_fits_and_picks_the_size() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = op.ui.global::<SettingsDlg>();
    g.set_stream_speed_testing(true);
    g.set_record_size(1);
    crate::ui::dialogs::settings_record::show_speed(&g, Ok(6.0), 1);
    assert!(!g.get_stream_speed_testing());
    assert_eq!(g.get_record_size(), 0, "720p fits, 1080p does not");
    assert!(g.get_stream_speed_text().contains("720p"), "{}", g.get_stream_speed_text());
    crate::ui::dialogs::settings_record::show_speed(&g, Err("no route".into()), 1);
    assert!(g.get_stream_speed_text().contains("could not reach"), "{}", g.get_stream_speed_text());
}

#[test]
fn the_operator_hears_when_a_destination_connects_and_when_it_drops() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.record.live.set(true);
    *op.record.links.borrow_mut() = vec![("YouTube".to_string(), false), ("Facebook".to_string(), false)];
    let l = |v: &[bool]| v.iter().map(|up| stream::send::Link { up: *up, skips: 0 }).collect::<Vec<_>>();
    let before = toasts(&op).len();
    op.say_links(&l(&[false, false]));
    assert_eq!(toasts(&op).len(), before, "nothing changed: nothing said");
    op.say_links(&l(&[true, false]));
    assert_eq!(toasts(&op).last(), Some(&(stream::tr::connected("YouTube"), false)));
    op.say_links(&l(&[true, false]));
    op.say_links(&l(&[false, true]));
    let said = toasts(&op);
    assert_eq!(said[said.len() - 2..], [(stream::tr::lost("YouTube"), true), (stream::tr::connected("Facebook"), false)]);
    // The Live button counts them: one of two connected.
    assert_eq!((op.ui.get_live_up(), op.ui.get_live_total()), (1, 2));
    // Only while live.
    // An upload too slow for Facebook: said once, after a few skips.
    let slow = |skips| vec![stream::send::Link { up: false, skips: 0 }, stream::send::Link { up: true, skips }];
    op.say_links(&slow(2));
    assert_eq!(toasts(&op).len(), said.len(), "two skips are a hiccup");
    op.say_links(&slow(3));
    assert_eq!(toasts(&op).last(), Some(&(stream::tr::too_slow("Facebook"), true)));
    op.say_links(&slow(9));
    assert_eq!(toasts(&op).len(), said.len() + 1, "once");
    op.record.live.set(false);
    op.say_links(&l(&[true, true]));
    assert_eq!(toasts(&op).len(), said.len() + 1);
}

#[test]
fn the_streaming_licence_note_is_shown_once() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.note_stream_licence();
    assert_eq!(op.state.borrow().asks, 1, "the note is up");
    assert!(app.data.lock().unwrap().settings.record.licence_noted, "and its mark is kept");
    crate::ui::dialogs::ask::answer(&op, true, String::new());
    refresh(&op);
    op.note_stream_licence();
    assert_eq!(op.state.borrow().asks, 0, "not again");
}

#[test]
fn the_operator_hears_when_the_camera_drops_out_and_comes_back() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.record.live.set(true);
    let before = toasts(&op).len();
    // Starting: not seen yet, nothing said; then seen.
    op.say_camera(Some(false));
    op.say_camera(Some(true));
    assert_eq!(toasts(&op).len(), before);
    op.say_camera(Some(false));
    assert_eq!(toasts(&op).last(), Some(&(stream::tr::camera_lost(), true)));
    op.say_camera(Some(false));
    op.say_camera(Some(true));
    assert_eq!(toasts(&op).last(), Some(&(stream::tr::camera_back(), false)));
    assert_eq!(toasts(&op).len(), before + 2);
    // No camera: nothing.
    op.say_camera(None);
    assert_eq!(toasts(&op).len(), before + 2);
}

#[test]
fn the_operator_hears_when_the_sound_input_goes_silent_and_comes_back() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    *op.record.input.borrow_mut() = "Line In (USB Audio)".into();
    let before = toasts(&op).len();
    op.say_silence(Some(false));
    assert_eq!(toasts(&op).len(), before, "sound coming: nothing to say");
    op.say_silence(Some(true));
    assert_eq!(toasts(&op).last(), Some(&(record::tr::no_sound("Line In (USB Audio)"), true)));
    op.say_silence(Some(true));
    assert_eq!(toasts(&op).len(), before + 1, "said once");
    op.say_silence(Some(false));
    assert_eq!(toasts(&op).last(), Some(&(record::tr::sound_back("Line In (USB Audio)"), false)));
    // No sound input: nothing.
    op.say_silence(None);
    assert_eq!(toasts(&op).len(), before + 2);
}

/// A scene chosen with the scene button holds for the live item and lights; another item taken live gives the
/// choice back to Automatic.
#[test]
fn a_scene_click_lights_and_holds_until_the_next_item() {
    use crate::live::{Command, Envelope};
    use crate::stream::scene::{Choice, Scene};
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    let add = |title: &str| -> uuid::Uuid {
        let item = serde_json::json!({ "kind": "text", "title": title, "body": "Words" });
        let r = tokio::runtime::Runtime::new().unwrap().block_on(app.do_action("service/add", serde_json::json!({ "item": item }).to_string().as_bytes())).unwrap();
        serde_json::from_value(r["id"].clone()).unwrap()
    };
    let (a, b) = (add("Welcome"), add("Sermon"));
    let show = |id| { let base = Some(app.live.lock().unwrap().version); assert!(app.command(Envelope { id: uuid::Uuid::new_v4().to_string(), base, cmd: Command::Show { item_id: id, slide: 0 } }).ok) };
    show(a);
    op.record.live.set(true);
    op.ui.invoke_pick_scene(0);
    assert_eq!(*app.stream_choice.lock().unwrap(), Choice { scene: Some(Scene::Camera), item: Some(a) });
    assert_eq!(op.ui.get_scene_chosen(), 0, "Camera lit");
    show(b);
    op.show_scene();
    assert_eq!(op.ui.get_scene_chosen(), -1, "Automatic again with the next item");
    op.ui.invoke_pick_scene(6);
    op.ui.invoke_pick_scene(-1);
    assert_eq!(app.stream_choice.lock().unwrap().scene, None, "Automatic chosen by hand");
    assert_eq!(op.ui.get_scene_chosen(), -1);
}

#[test]
fn hold_and_mute_do_nothing_unless_live() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.hold_stream();
    op.mute_stream();
    assert!(!op.ui.get_live_held() && !op.ui.get_live_muted());
}

/// A stand-in for Google and Facebook, kept running by the runtime returned with it.
fn stand_in() -> (tokio::runtime::Runtime, Arc<Mutex<stream::accounts::testing::Seen>>) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let seen = Arc::new(Mutex::new(stream::accounts::testing::Seen::default()));
    let base = rt.block_on(stream::accounts::testing::serve(seen.clone()));
    stream::accounts::testing::use_server(&base);
    (rt, seen)
}

#[test]
fn settings_sign_in_to_facebook_and_out_again() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let (_rt, _seen) = stand_in();
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = op.ui.global::<SettingsDlg>();
    crate::ui::dialogs::settings_record::fill_form(&g, &RecordPrefs::default());
    crate::ui::dialogs::settings_record::wire_keys(&op);
    assert!(g.get_stream_can_sign_in_0() && g.get_stream_can_sign_in_1(), "this copy has the app's ids");
    assert_eq!(g.get_stream_account_1(), "");
    g.invoke_stream_sign_in(1);
    assert!(!g.get_stream_signing_in());
    assert_eq!(g.get_stream_account_1(), "Grace Page", "{}", g.get_stream_sign_in_text());
    assert_eq!(g.get_stream_sign_in_text().as_str(), stream::accounts::tr::signed_in("Grace Page"));
    let kept = stream::accounts::kept(stream::keys::app_store().as_ref(), stream::built_in_id(1));
    assert_eq!(kept, Some(("Grace Page".to_string(), "p1|pt".to_string())), "kept in the password store, not the settings");
    g.invoke_stream_sign_out(1);
    assert_eq!(g.get_stream_account_1(), "");
    assert_eq!(stream::accounts::kept(stream::keys::app_store().as_ref(), stream::built_in_id(1)), None);
}

#[test]
fn settings_keep_the_broadcast_title_and_who_can_watch() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = op.ui.global::<SettingsDlg>();
    let mut prefs = RecordPrefs::default();
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    g.set_stream_title(" Morning worship {date} ".into());
    g.set_stream_description("Welcome!".into());
    g.set_stream_privacy(1);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!((prefs.stream_title.as_str(), prefs.stream_description.as_str()), ("Morning worship {date}", "Welcome!"));
    assert_eq!(prefs.stream_privacy, stream::accounts::Privacy::Unlisted);
}

#[test]
fn go_live_signed_in_says_when_the_broadcast_cannot_be_made() {
    let (_rt, _seen) = stand_in();
    let app = test_app();
    app.data.lock().unwrap().settings.record.stream = vec![dest(0, true)];
    // A sign-in Google no longer accepts (taken back, or run out).
    stream::accounts::keep(stream::keys::app_store().as_ref(), stream::built_in_id(0), "Grace Church", "revoked").unwrap();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.go_live();
    assert!(!op.is_recording() && !op.ui.get_live());
    let said = toasts(&op);
    assert!(said.iter().any(|(t, err)| *err && t.starts_with("YouTube: the broadcast could not be made") && t.contains("expired or revoked")), "{said:?}");
    assert!(!said.iter().any(|(t, _)| *t == stream::tr::not_ready("YouTube", &stream::tr::no_key())), "signed in, no key is needed: {said:?}");
}

#[test]
fn go_live_signed_in_makes_the_broadcast_streams_to_it_and_gives_the_replay() {
    if !has_ffmpeg() { return eprintln!("no ffmpeg here: skipped") }
    let (_rt, seen) = stand_in();
    // Nothing listens here: the stream is tried and fails, which is enough to see where it goes.
    seen.lock().unwrap().ingest = "rtmp://127.0.0.1:9/live2".into();
    let app = test_app();
    let folder = std::env::temp_dir().join(format!("rp-signin-ui-{}", uuid::Uuid::new_v4()));
    app.data.lock().unwrap().settings.record = RecordPrefs {
        size: Preset::P720, folder: folder.display().to_string(), sound: "none".into(), stream: vec![dest(0, true)],
        stream_title: "Service {date}".into(), ..Default::default()
    };
    stream::accounts::keep(stream::keys::app_store().as_ref(), stream::built_in_id(0), "Grace Church", "rt").unwrap();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.go_live();
    assert!(op.is_recording() && op.ui.get_live(), "{:?}", toasts(&op));
    assert_eq!(*op.record.links.borrow(), vec![("YouTube".to_string(), false)]);
    let title = seen.lock().unwrap().broadcast["snippet"]["title"].as_str().unwrap_or_default().to_string();
    assert_eq!(title, stream::accounts::title_for("Service {date}", chrono::Local::now().date_naive(), stream::Service::YouTube));
    assert_eq!(op.record.broadcasts.borrow().first().map(|(_, b)| b.address.clone()), Some("rtmp://127.0.0.1:9/live2/new-key".to_string()));
    op.go_live();
    assert!(toasts(&op).contains(&(stream::accounts::tr::replay("YouTube", "https://youtu.be/b1"), false)), "{:?}", toasts(&op));
    assert!(op.record.broadcasts.borrow().is_empty());
    let ended = testing::wait_ended(&op);
    drop(ended);
    let _ = std::fs::remove_dir_all(&folder);
}
