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
    app.data().settings.record = RecordPrefs { size: Preset::P720, folder: folder.display().to_string(), sound: "none".into(), ..Default::default() };
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.toggle_recording();
    assert!(op.is_recording(), "{:?}", toasts(&op));
    assert!(op.ui.get_recording());
    assert_eq!(op.ui.get_record_time(), "0:00");
    assert_eq!(toasts(&op).last(), Some(&(record::tr::recording(), false)));
    let note = record::unfinished::path(&app.store.root);
    assert!(note.is_file(), "a note while it records, in case the app stops");
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
    assert!(!note.exists(), "ended: no note");
    assert_eq!(toasts(&op).last(), Some(&(record::tr::saved(&file.display().to_string()), false)));
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn after_the_app_stopped_while_live_it_asks_to_go_live_again() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    // No note: nothing.
    op.after_unfinished_recording();
    assert_eq!(op.ui.get_ask().row_count(), 0);
    // Stopped while recording only: the part is finished (none is here), and nothing is asked.
    let folder = std::env::temp_dir().join(format!("rp-record-gone-{}", uuid::Uuid::new_v4()));
    let file = folder.join("Recording 2026-11-16 10-30.recording.mkv");
    record::unfinished::mark(&app.store.root, &record::unfinished::Unfinished { file: file.clone(), live: false });
    op.after_unfinished_recording();
    assert_eq!(op.ui.get_ask().row_count(), 0);
    assert!(!record::unfinished::path(&app.store.root).exists(), "asked once");
    // Stopped while live: asked.
    record::unfinished::mark(&app.store.root, &record::unfinished::Unfinished { file, live: true });
    op.after_unfinished_recording();
    let ask = op.ui.get_ask().row_data(0).expect("a question");
    assert_eq!(ask.text, record::tr::was_live());
    assert_eq!((ask.ok_label.as_str(), ask.cancel_label.as_str()), (record::tr::go_live_again().as_str(), "Not now"));
    crate::ui::dialogs::ask::answer(&op, false, String::new());
    assert!(!op.is_recording());
    op.after_unfinished_recording();
    assert_eq!(op.ui.get_ask().row_count(), 0, "asked once");
}

#[test]
fn a_clip_marked_while_recording_is_saved_as_its_own_video() {
    if !has_ffmpeg() { return eprintln!("no ffmpeg here: skipped") }
    let app = test_app();
    let folder = std::env::temp_dir().join(format!("rp-record-clip-{}", uuid::Uuid::new_v4()));
    app.data().settings.record = RecordPrefs { size: Preset::P720, folder: folder.display().to_string(), sound: "none".into(), ..Default::default() };
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.toggle_clip();
    assert!(!op.ui.get_clipping(), "not recording: no clip");
    op.toggle_recording();
    let sink = room.sink.lock().unwrap().clone().expect("recording");
    let feed = |ms: u64| {
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_millis(ms) {
            let mut b = sink.buffer();
            b.fill(150);
            sink.put(b);
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    feed(1000);
    op.toggle_clip();
    assert!(op.ui.get_clipping());
    assert_eq!(toasts(&op).last(), Some(&(record::tr::clip_started(), false)));
    feed(2500);
    op.toggle_clip();
    assert!(!op.ui.get_clipping());
    assert_eq!(toasts(&op).last(), Some(&(record::tr::clip_marked(), false)));
    // A slide action can start one too: it runs to the end.
    *app.clip_request.lock().unwrap() = Some(true);
    op.take_clip_request();
    assert!(op.ui.get_clipping());
    feed(1500);
    op.toggle_recording();
    let ended = testing::wait_ended(&op);
    let file = ended.file.expect("a file");
    let clips = [record::clips::name(&file, 1), record::clips::name(&file, 2)];
    // `name` gives the next free one, so the clips made are those just before.
    let made: Vec<PathBuf> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().contains(" clip ")).collect();
    assert_eq!(made.len(), 2, "{made:?} (next free: {clips:?})");
    for p in &made { assert!(toasts(&op).contains(&(record::tr::clip_saved(&p.display().to_string()), false)), "{:?}", toasts(&op)) }
    assert!(!op.ui.get_clipping(), "the recording ended: no clip open");
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
    app.data().settings.record.stream = vec![dest(0, true), dest(2, false)];
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
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
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
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
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
    assert_eq!(prefs.stream_delay_secs, 0, "no stream delay to start with");
    g.set_stream_delay(2);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!(prefs.stream_delay_secs, 30);
    assert!(g.get_stream_lighter(), "a lighter stream for slow internet is on to start with");
    g.set_stream_lighter(false);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!(prefs.stream_lighter, Some(false));
    assert!(!g.get_stream_ndi() && !g.get_stream_ndi_words(), "nothing over NDI to start with");
    g.set_stream_ndi_words(true);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!(prefs.ndi(), crate::stream::ndi::OutPrefs { stream: false, words: true });
    assert_eq!((prefs.camera.as_str(), prefs.camera2.as_str()), ("Cam A", ""));
    g.set_stream_camera2_index(2);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!(prefs.camera2, "Cam B");
    assert_eq!((prefs.cards.hold.words.as_str(), prefs.cards.starting.picture.as_str()), ("Back soon", "C:/Pictures/welcome.png"));
    let json = serde_json::to_value(&prefs).unwrap();
    assert_eq!(json["cards"]["hold"], serde_json::json!({ "words": "Back soon" }));
    let old: RecordPrefs = serde_json::from_value(serde_json::json!({ "camera_layout": 2 })).unwrap();
    assert_eq!(old, RecordPrefs::default(), "the old camera layout is left behind");
    // The stream delay and the lighter stream show again as saved.
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!((g.get_stream_delay(), g.get_stream_lighter(), g.get_stream_ndi_words()), (2, false, true));
}

#[test]
fn settings_keep_the_streams_sound_levels_and_clean_up() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let mut prefs = RecordPrefs::default();
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!((g.get_stream_board_level(), g.get_stream_pc_level()), (0, 0));
    assert!(g.get_stream_even(), "evened out to start with");
    assert!(!g.get_stream_quiet_noise());
    assert!(!g.get_stream_tracks(), "one track to start with");
    g.set_stream_tracks(true);
    g.set_stream_board_level(8);
    g.set_stream_pc_level(-50);
    g.set_stream_even(false);
    g.set_stream_quiet_noise(true);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!(prefs.sound_fx(), record::args::SoundFx { delay_ms: 0, board_db: 8, pc_db: -20, even: false, quiet_noise: true, tracks: true, board_filters: record::args::Filters::SAFE, pc_filters: record::args::Filters::SAFE });
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!((g.get_stream_board_level(), g.get_stream_pc_level(), g.get_stream_even(), g.get_stream_quiet_noise()), (8, -20, false, true));
    // Turned on again: back to how it starts, so nothing is saved for it.
    g.set_stream_even(true);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!(prefs.stream_even, None);
}

#[test]
fn settings_keep_the_encoder_and_bitrate() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let mut prefs = RecordPrefs::default();
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!((g.get_record_encoder(), g.get_record_bitrate()), (0, 0), "automatic, both");
    g.set_record_encoder(2);
    g.set_record_bitrate(3);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!((prefs.encoder.as_str(), prefs.bitrate_kbps), (record::args::CHOOSABLE[1], record::BITRATES[2]));
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!((g.get_record_encoder(), g.get_record_bitrate()), (2, 3));
    let json = serde_json::to_value(RecordPrefs::default()).unwrap();
    assert!(json.get("encoder").is_none() && json.get("bitrate_kbps").is_none(), "automatic: nothing saved");
}

#[test]
fn settings_keep_the_broadcast_thumbnail() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let mut prefs = RecordPrefs { stream_thumbnail: "C:/Pictures/sunday.jpg".into(), ..RecordPrefs::default() };
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!(g.get_stream_thumbnail(), "C:/Pictures/sunday.jpg");
    g.set_stream_thumbnail("".into());
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!(prefs.stream_thumbnail, "");
}

#[test]
fn settings_keep_the_scene_layout() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    use stream::compose::{Corner, CornerSize, Layout};
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let mut prefs = RecordPrefs::default();
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!((g.get_stream_corner(), g.get_stream_corner_size(), g.get_stream_words_top()), (3, 1, 0), "bottom right, medium, at the bottom");
    g.set_stream_corner(0);
    g.set_stream_corner_size(2);
    g.set_stream_words_top(1);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!(prefs.stream_layout, Layout { corner: Corner::TopLeft, corner_size: CornerSize::Large, words_top: true });
    assert_eq!(serde_json::to_value(RecordPrefs::default()).unwrap().get("stream_layout"), None, "as it starts: nothing saved");
}

#[test]
fn settings_keep_which_destinations_get_the_tall_stream() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let mut prefs = RecordPrefs::default();
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    g.set_stream_on_0(true);
    g.set_stream_on_2(true);
    g.set_stream_tall_2(true);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    let tall: Vec<(usize, bool)> = prefs.stream.iter().map(|d| ((0..4).find(|&i| stream::built_in_id(i) == d.id).unwrap(), d.tall)).collect();
    assert_eq!(tall, [(0, false), (2, true)]);
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert!(g.get_stream_tall_2() && !g.get_stream_tall_0());
    assert_eq!(serde_json::to_value(&prefs.stream[0]).unwrap().get("tall"), None, "not tall: nothing saved");
}

#[test]
fn settings_keep_each_cameras_control_address() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let mut prefs = RecordPrefs { camera_control: "192.168.1.60".into(), ..RecordPrefs::default() };
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!((g.get_stream_camera_control().as_str(), g.get_stream_camera2_control().as_str()), ("192.168.1.60", ""));
    g.set_stream_camera_control(" 192.168.1.61:1259 ".into());
    g.set_stream_camera2_control("192.168.1.62".into());
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!((prefs.camera_control.as_str(), prefs.camera2_control.as_str()), ("192.168.1.61:1259", "192.168.1.62"));
    assert_eq!(serde_json::to_value(RecordPrefs::default()).unwrap().get("camera_control"), None, "none: nothing saved");
}

#[test]
fn camera_positions_go_to_the_camera_in_the_picture() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    let camera = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    camera.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let port = camera.local_addr().unwrap().port();
    // Two cameras; only the second can be moved.
    *op.record.cameras.borrow_mut() = vec!["Cam A".into(), "Cam B".into()];
    *op.record.camera_controls.borrow_mut() = vec![None, stream::ptz::Control::parse(&format!("127.0.0.1:{port}"))];
    op.record.live.set(true);
    op.show_positions();
    assert_eq!(op.ui.get_camera_positions(), 0, "the first camera in the picture has no control");
    op.ui.set_camera_active(1);
    op.show_positions();
    assert_eq!(op.ui.get_camera_positions(), i32::from(stream::ptz::POSITIONS));
    op.pick_position(2);
    let mut buf = [0u8; 64];
    let n = camera.recv(&mut buf).expect("the camera was sent to its position");
    assert_eq!(&buf[..n], [0x81, 0x01, 0x04, 0x3F, 0x02, 0x02, 0xFF], "position 3");
    // Not live: nothing.
    op.record.live.set(false);
    op.show_positions();
    assert_eq!(op.ui.get_camera_positions(), 0);
}

/// Live with cameras, as Go Live leaves the window (no recording: the choices are the window's).
fn live_with(op: &Rc<Operator>, cameras: &[&str]) {
    *op.record.cameras.borrow_mut() = cameras.iter().map(|c| c.to_string()).collect();
    *op.record.camera_controls.borrow_mut() = vec![None; cameras.len()];
    op.record.live.set(true);
    op.ui.set_camera_active(0);
    op.record.next.set((cameras.len() > 1).then_some(1));
    op.show_cameras();
}

#[test]
fn switch_and_fade_put_the_next_camera_out_and_the_one_out_becomes_next() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    let g = op.ui.camera_switch_all();
    assert!(!g.get_shown(), "not live: no Cameras box");
    live_with(&op, &["Pulpit", "Wide"]);
    assert!(g.get_shown());
    assert_eq!((g.get_live(), g.get_next()), (0, 1));
    assert_eq!((g.get_live_caption().as_str(), g.get_next_caption().as_str()), ("1 Pulpit", "2 Wide"));
    g.invoke_switch();
    assert_eq!((op.ui.get_camera_active(), g.get_live(), g.get_next()), (1, 1, 0), "the second out, the first next");
    g.invoke_fade();
    assert_eq!((op.ui.get_camera_active(), g.get_live(), g.get_next()), (0, 0, 1), "and back again");
    // The scene button's camera rows switch at once in the same way.
    op.pick_camera(1);
    assert_eq!((g.get_live(), g.get_next()), (1, 0));
    // The stream stops: the box goes.
    op.show_recording(false);
    assert!(!g.get_shown());
    g.invoke_switch();
    assert_eq!(op.ui.get_camera_active(), 1, "not live: nothing switches");
}

#[test]
fn with_more_cameras_any_but_the_one_out_can_be_chosen_next() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    let g = op.ui.camera_switch_all();
    live_with(&op, &["A", "B", "C", "D"]);
    assert_eq!(g.get_names().row_count(), 4);
    g.invoke_choose_next(3);
    assert_eq!(g.get_next(), 3);
    g.invoke_choose_next(0);
    assert_eq!(g.get_next(), 3, "the camera out cannot be next");
    g.invoke_choose_next(7);
    assert_eq!(g.get_next(), 3, "no such camera");
    g.invoke_switch();
    assert_eq!((g.get_live(), g.get_next()), (3, 0));
    // Switching to a camera that was not next leaves next as it was.
    op.pick_camera(2);
    assert_eq!((g.get_live(), g.get_next()), (2, 0));
}

#[test]
fn one_camera_shows_the_live_picture_only() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    let g = op.ui.camera_switch_all();
    live_with(&op, &["Pulpit"]);
    assert!(g.get_shown());
    assert_eq!(g.get_next(), -1, "no Next, no Switch");
    g.invoke_switch();
    assert_eq!(op.ui.get_camera_active(), 0);
}

#[test]
fn the_small_pictures_are_shown_and_no_picture_is_said() {
    use crate::record::peek::{Peek, Small};
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    let g = op.ui.camera_switch_all();
    live_with(&op, &["Pulpit", "Wide"]);
    let blue = Arc::new(Small { width: 4, height: 2, rgba: [0, 0, 200, 255].repeat(8) });
    op.show_peek(Peek { live: Some(blue.clone()), next: None, cameras: vec![] });
    assert!(g.get_live_has_picture() && !g.get_next_has_picture(), "the next camera sends nothing: No picture");
    let size = g.get_live_picture().size();
    assert_eq!((size.width, size.height), (4, 2));
    op.show_peek(Peek { live: Some(blue.clone()), next: Some(blue), cameras: vec![] });
    assert!(g.get_next_has_picture());
    op.show_peek(Peek::default());
    assert!(!g.get_live_has_picture() && !g.get_next_has_picture());
}

#[test]
fn the_next_camera_can_be_moved_before_it_goes_out() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    let camera = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    camera.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let port = camera.local_addr().unwrap().port();
    let g = op.ui.camera_switch_all();
    live_with(&op, &["Pulpit", "Wide"]);
    assert_eq!(g.get_next_positions(), 0, "the next camera cannot be moved");
    *op.record.camera_controls.borrow_mut() = vec![None, stream::ptz::Control::parse(&format!("127.0.0.1:{port}"))];
    op.show_cameras();
    assert_eq!(g.get_next_positions(), i32::from(stream::ptz::POSITIONS));
    g.invoke_move_next(4);
    let mut buf = [0u8; 64];
    let n = camera.recv(&mut buf).expect("the next camera was moved");
    assert_eq!(&buf[..n], [0x81, 0x01, 0x04, 0x3F, 0x02, 0x04, 0xFF], "position 5");
    g.invoke_switch();
    assert_eq!(g.get_next_positions(), 0, "the first camera is next now, and it cannot be moved");
}

#[test]
fn settings_keep_a_third_and_fourth_camera_with_their_control_addresses() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let mut prefs = RecordPrefs { camera: "rtsp://192.168.1.50/a".into(), camera2: "rtsp://192.168.1.51/b".into(), camera3: "ndi://PC (OBS)".into(),
        camera3_control: "192.168.1.63".into(), ..RecordPrefs::default() };
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!(g.get_stream_camera3_control(), "192.168.1.63");
    assert_eq!(g.get_stream_camera4_index(), 0);
    g.set_stream_camera4_index(g.get_stream_camera4_labels().row_count() as i32 - 1);
    g.set_stream_camera4_address(" srt://192.168.1.64:9000 ".into());
    g.set_stream_camera4_control("192.168.1.64".into());
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!((prefs.camera3.as_str(), prefs.camera4.as_str()), ("ndi://PC (OBS)", "srt://192.168.1.64:9000"));
    assert_eq!((prefs.camera3_control.as_str(), prefs.camera4_control.as_str()), ("192.168.1.63", "192.168.1.64"));
    assert_eq!(prefs.cameras().len(), 4);
    // No second camera: the third and fourth are not kept either.
    g.set_stream_camera2_index(0);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!((prefs.camera2.as_str(), prefs.camera3.as_str(), prefs.camera4.as_str()), ("", "", ""));
}

#[test]
fn settings_keep_a_network_camera_by_its_address() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
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
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    g.set_stream_speed_testing(true);
    g.set_record_size(2);
    crate::ui::dialogs::settings_record::show_speed(&g, Ok(6.0), 1);
    assert!(!g.get_stream_speed_testing());
    assert_eq!(g.get_record_size(), 1, "720p fits, 1080p does not");
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
    let l = |v: &[bool]| v.iter().map(|up| stream::send::Link { up: *up, ..Default::default() }).collect::<Vec<_>>();
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
    // With a lighter stream to go to, the skips first make it lighter (said), and full again later (said).
    let light = |skips, kbps, lightest| vec![stream::send::Link::default(), stream::send::Link { up: true, skips, kbps, lightest, ..Default::default() }];
    op.say_links(&light(3, 0, false));
    assert_eq!(toasts(&op).len(), said.len(), "a lighter stream is still to come");
    op.say_links(&light(3, 4200, false));
    assert_eq!(toasts(&op).last(), Some(&(stream::tr::lighter("Facebook", 4200), false)));
    op.say_links(&light(3, 0, false));
    assert_eq!(toasts(&op).last(), Some(&(stream::tr::full_again("Facebook"), false)));
    let said = toasts(&op);
    let slow = |skips| vec![stream::send::Link::default(), stream::send::Link { up: true, skips, lightest: true, ..Default::default() }];
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
fn the_chat_button_counts_new_messages_until_it_is_opened() {
    use stream::chat::{Message, add};
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.show_chat();
    assert_eq!((op.ui.get_chat_new(), op.ui.get_chat_lines().row_count()), (0, 0));
    let m = |name: &str, from, text: &str| Message { from, name: name.into(), text: text.into() };
    add(&op.record.chat, vec![m("Ruth", stream::Service::YouTube, "Hello from Ohio"), m("Mark", stream::Service::Facebook, "Amen")]);
    op.show_chat();
    assert_eq!(op.ui.get_chat_new(), 2);
    assert_eq!(op.ui.get_chat_lines().row_data(0).unwrap().as_str(), stream::tr::chat_line("Ruth", "YouTube", "Hello from Ohio"));
    assert_eq!(stream::tr::chat_line("Ruth", "YouTube", "Hello from Ohio"), "Ruth · YouTube: Hello from Ohio");
    op.chat_opened();
    assert_eq!(op.ui.get_chat_new(), 0, "seen");
    add(&op.record.chat, vec![m("Ann", stream::Service::Twitch, "Good morning!")]);
    op.show_chat();
    assert_eq!((op.ui.get_chat_new(), op.ui.get_chat_lines().row_count()), (1, 3));
}

#[test]
fn stopping_the_stream_says_how_it_went() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.record.live.set(true);
    *op.record.links.borrow_mut() = vec![("YouTube".into(), false), ("Facebook".into(), false), ("Twitch".into(), false)];
    let l = |v: [bool; 3]| v.iter().map(|up| stream::send::Link { up: *up, ..Default::default() }).collect::<Vec<_>>();
    op.say_links(&l([true, true, false]));
    op.say_links(&l([true, false, false]));
    op.say_links(&l([true, true, false]));
    op.say_links(&l([true, false, false]));
    op.say_links(&l([true, true, false]));
    op.say_stream_summary(4530);
    assert_eq!(toasts(&op).last(), Some(&(stream::tr::summary(4530, &[("YouTube".into(), 0, true), ("Facebook".into(), 2, true), ("Twitch".into(), 0, false)]), true)));
    assert_eq!(stream::tr::summary(4530, &[("YouTube".into(), 0, true), ("Facebook".into(), 2, true), ("Twitch".into(), 0, false)]),
        "The stream ran 1:15:30. YouTube: connected the whole time. Facebook: dropped 2 times and came back. Twitch: never connected.");
    assert_eq!(stream::tr::summary(65, &[("YouTube".into(), 1, true)]), "The stream ran 1:05. YouTube: dropped once and came back.");
}

#[test]
fn a_destination_that_never_connects_is_said_once() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.record.live.set(true);
    *op.record.links.borrow_mut() = vec![("YouTube".into(), false)];
    let before = toasts(&op).len();
    let tried = |failed| vec![stream::send::Link { failed, ..Default::default() }];
    op.say_links(&tried(NEVER_TRIES - 1));
    assert_eq!(toasts(&op).len(), before, "a try or two can fail");
    op.say_links(&tried(NEVER_TRIES));
    assert_eq!(toasts(&op).last(), Some(&(stream::tr::never_connected("YouTube"), true)));
    op.say_links(&tried(NEVER_TRIES + 4));
    assert_eq!(toasts(&op).len(), before + 1, "once");
    op.say_links(&[stream::send::Link { up: true, ..Default::default() }]);
    assert_eq!(toasts(&op).last(), Some(&(stream::tr::connected("YouTube"), false)));
}

#[test]
fn the_streaming_licence_note_is_shown_once() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.note_stream_licence();
    assert_eq!(op.state.borrow().asks, 1, "the note is up");
    assert!(app.data().settings.record.licence_noted, "and its mark is kept");
    crate::ui::dialogs::ask::answer(&op, true, String::new());
    refresh(&op);
    op.note_stream_licence();
    assert_eq!(op.state.borrow().asks, 0, "not again");
}

#[test]
fn a_camera_that_never_starts_is_said_and_said_again_when_it_comes() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.record.live.set(true);
    let before = toasts(&op).len();
    // Starting takes a few seconds: nothing said.
    for _ in 1..CAMERA_START_SECS { op.say_camera(Some(false)) }
    assert_eq!(toasts(&op).len(), before);
    // Still nothing: unplugged, or another program has it.
    op.say_camera(Some(false));
    assert_eq!(toasts(&op).last(), Some(&(stream::tr::camera_never(), true)));
    for _ in 0..30 { op.say_camera(Some(false)) }
    assert_eq!(toasts(&op).len(), before + 1, "said once");
    op.say_camera(Some(true));
    assert_eq!(toasts(&op).last(), Some(&(stream::tr::camera_back(), false)));
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

#[test]
fn the_operator_hears_when_the_computer_cannot_keep_up_and_when_it_does_again() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    let before = toasts(&op).len();
    let ms = Duration::from_millis;
    for _ in 0..30 { op.say_slow(Some(ms(40))) }
    assert_eq!(toasts(&op).len(), before, "keeping up: nothing to say");
    for _ in 0..30 { op.say_slow(Some(ms(1200))) }
    assert_eq!(toasts(&op).len(), before + 1, "said once");
    assert_eq!(toasts(&op).last(), Some(&(record::tr::too_slow(), true)));
    for _ in 0..30 { op.say_slow(Some(ms(40))) }
    assert_eq!(toasts(&op).last(), Some(&(record::tr::smooth_again(), false)));
    // Not recording: nothing, and a new recording starts afresh.
    op.say_slow(None);
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
    let show = |id| { let base = Some(app.live().version); assert!(app.command(Envelope { id: uuid::Uuid::new_v4().to_string(), base, cmd: Command::Show { item_id: id, slide: 0 } }).ok) };
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

/// The church's own scenes come after the built-in ones on the scene button, and light when chosen.
#[test]
fn the_churchs_own_scenes_are_on_the_scene_button() {
    use crate::stream::{own_scenes::OwnScene, scene::Scene};
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    let ours = OwnScene::new("Welcome".into());
    *op.record.own_scenes.borrow_mut() = vec![ours.clone()];
    op.record.live.set(true);
    let i = Scene::ALL.len() as i32;
    op.ui.invoke_pick_scene(i);
    assert_eq!(app.stream_choice.lock().unwrap().scene, Some(Scene::Own(ours.id)));
    assert_eq!(op.ui.get_scene_chosen(), i, "lit");
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
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
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
fn settings_sign_in_to_twitch_and_out_again() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let (_rt, _seen) = stand_in();
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    crate::ui::dialogs::settings_record::fill_form(&g, &RecordPrefs::default());
    crate::ui::dialogs::settings_record::wire_keys(&op);
    assert!(g.get_stream_can_sign_in_2());
    g.invoke_stream_sign_in(2);
    assert_eq!(g.get_stream_account_2(), "GraceChurch", "{}", g.get_stream_sign_in_text());
    let kept = stream::accounts::kept(stream::keys::app_store().as_ref(), stream::built_in_id(2));
    assert_eq!(kept, Some(("GraceChurch".to_string(), "u9|gracechurch|trt".to_string())));
    g.invoke_stream_sign_out(2);
    assert_eq!(g.get_stream_account_2(), "");
}

#[test]
fn settings_make_next_sundays_youtube_broadcast_ahead() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let (_rt, _seen) = stand_in();
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let mut prefs = RecordPrefs::default();
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    crate::ui::dialogs::settings_record::wire_keys(&op);
    stream::accounts::keep(stream::keys::app_store().as_ref(), stream::built_in_id(0), "Grace Church", "rt").unwrap();
    crate::ui::dialogs::settings_record::fill_accounts(&g);
    assert_eq!(g.get_stream_scheduled_link(), "");
    g.invoke_stream_schedule();
    assert!(g.get_stream_scheduled_link().starts_with("https://youtu.be/"), "{}", g.get_stream_sign_in_text());
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert!(!prefs.stream_scheduled.is_empty() && prefs.stream_scheduled_link == g.get_stream_scheduled_link().as_str());
    // Kept through Save and opened again.
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!(g.get_stream_scheduled_link().as_str(), prefs.stream_scheduled_link);
}

#[test]
fn settings_keep_the_broadcast_title_and_who_can_watch() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
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
    app.data().settings.record.stream = vec![dest(0, true)];
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
    app.data().settings.record = RecordPrefs {
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

#[test]
fn settings_keep_the_words_alone_file() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let mut prefs = RecordPrefs::default();
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert!(!g.get_stream_words_file(), "off to start with");
    assert_eq!(serde_json::to_value(&prefs).unwrap().get("words_file"), None, "off: nothing saved");
    g.set_stream_words_file(true);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert!(prefs.words_file);
    g.set_stream_words_file(false);
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert!(g.get_stream_words_file(), "shown again as saved");
}

/// Settings, Media, Livestream: the sound: the setup puts in the input it finds and its filters, the other inputs,
/// each sound's filters and Listen are kept, and a change by hand makes the setup Custom.
#[test]
fn settings_keep_the_streams_sound_setup_other_inputs_filters_and_listen() {
    use crate::record::args::{Filters, Input, Strength};
    use crate::stream::sound_setup::{Mode, setup};
    use crate::ui::{SettingsDlg, dialogs::settings_sound};
    use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let mut prefs = RecordPrefs { sound: "input:Line In".into(), ..RecordPrefs::default() };
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!((g.get_stream_sound_mode(), g.get_stream_input2_index(), g.get_stream_board_filters().limit), (0, 0, true), "custom, no second input, the limiter on");
    // The inputs, as Settings lists them once FFmpeg has answered.
    let inputs = ["Microphone Array (Realtek(R) Audio)", "Microphone (Yeti Stereo Microphone)", "Line In"];
    let strings = |v: Vec<String>| ModelRc::new(VecModel::from(v.into_iter().map(SharedString::from).collect::<Vec<_>>()));
    let mut names = vec!["none".to_string(), "app".into()];
    names.extend(inputs.iter().map(|i| format!("input:{i}")));
    g.set_record_sound_labels(strings(names.clone()));
    g.set_record_sound_names(strings(names));
    g.set_record_sound_index(4);
    settings_sound::fill_inputs_found(&g, &inputs.map(|i| Input { name: i.into(), label: i.into() }));
    let yeti = (0..g.get_stream_input_names().row_count()).find(|r| g.get_stream_input_names().row_data(*r).unwrap() == inputs[1]).unwrap();
    g.set_stream_input2_index(yeti as i32);
    g.set_stream_input2_level(6);
    g.set_stream_pc_filters(settings_sound::to_ui(Filters { gate: Strength::Strong, ..Filters::SAFE }));
    settings_sound::fill_listen(&g, &[crate::video::ffi::AudioDevice { name: "wasapi/{h}".into(), label: "Headphones".into() }], "");
    g.set_stream_listen_index(2);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!((prefs.stream_input2.as_str(), prefs.stream_input2_db, prefs.stream_listen.as_str()), (inputs[1], 6, "wasapi/{h}"));
    assert_eq!(prefs.filters().pc.gate, Strength::Strong);
    assert_eq!(prefs.stream_sound_mode, Mode::Custom);
    // "USB microphone": the Yeti becomes the input, no second one, its filters.
    settings_sound::choose_mode(&g, Mode::UsbMic.index());
    assert_eq!(g.get_record_sound_index(), 3);
    assert_eq!(g.get_stream_input2_index(), 0);
    assert!(g.get_stream_quiet_noise());
    assert_eq!(settings_sound::from_ui(&g.get_stream_board_filters()), setup(Mode::UsbMic).unwrap().board);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!((prefs.stream_sound_mode, prefs.sound.as_str(), prefs.stream_input2.as_str()), (Mode::UsbMic, "input:Microphone (Yeti Stereo Microphone)", ""));
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!(g.get_stream_sound_mode(), Mode::UsbMic.index());
    // Changed by hand: Custom.
    let mut f = g.get_stream_board_filters();
    f.high = 9;
    g.set_stream_board_filters(f);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!((prefs.stream_sound_mode, prefs.filters().board.high_db), (Mode::Custom, 9));
}

#[test]
fn settings_keep_instagram_x_and_tiktok_with_their_servers_and_each_ones_own_quality() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    // Facebook has a quality of its own (More settings) while it is off.
    let own = stream::Quality { size: Some(Preset::P720), kbps: 2500, cap: None };
    let mut prefs = RecordPrefs { stream: vec![stream::Destination { id: stream::built_in_id(1), service: stream::Service::Facebook, quality: own, ..Default::default() }], ..RecordPrefs::default() };
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    g.set_stream_on_4(true);
    g.set_stream_url_4(" rtmps://edgetee-upload.example:443/rtmp/ ".into());
    g.set_stream_on_6(true);
    g.set_stream_tall_6(true);
    g.set_stream_url_6("rtmp://push.example/game/".into());
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    let rows: Vec<(stream::Service, bool, bool, String)> = prefs.stream.iter().map(|d| (d.service, d.on, d.tall, d.url.clone())).collect();
    assert_eq!(rows, [
        (stream::Service::Facebook, false, false, String::new()),
        (stream::Service::Instagram, true, false, "rtmps://edgetee-upload.example:443/rtmp/".into()),
        (stream::Service::TikTok, true, true, "rtmp://push.example/game/".into()),
    ]);
    assert_eq!(prefs.stream[0].quality, own, "kept though the easy page does not show it");
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert!(g.get_stream_on_4() && g.get_stream_on_6() && g.get_stream_tall_6() && !g.get_stream_on_5());
    assert_eq!(g.get_stream_url_6(), "rtmp://push.example/game/");
}

#[test]
fn after_a_stream_the_report_says_more_when_something_went_wrong_and_is_kept_in_the_logs() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.record.live.set(true);
    *op.record.links.borrow_mut() = vec![("YouTube".into(), false)];
    op.say_links(&[stream::send::Link { up: true, ..Default::default() }]);
    // Twelve seconds in which the computer was behind.
    for _ in 0..12 { op.say_slow(Some(Duration::from_millis(900))) }
    op.say_stream_summary(600);
    let t = toasts(&op);
    assert_eq!(t[t.len() - 2], (stream::tr::summary(600, &[("YouTube".into(), 0, true)]), false));
    assert!(t[t.len() - 1].0.contains("12 seconds") && t[t.len() - 1].1, "{:?}", t.last());
    let log = std::fs::read_to_string(stream::report::log_file(&app.store.root.join("logs"))).unwrap();
    assert!(log.contains("stream of 10:00 (600 s)") && log.contains("  YouTube: connected, lost 0 times"), "{log}");
}

#[test]
fn x264_too_slow_with_the_encoder_left_to_the_app_tries_the_graphics_card_next_time() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    *op.record.encoder.borrow_mut() = "libx264".into();
    op.record.encoder_auto.set(true);
    for _ in 0..30 { op.say_slow(Some(Duration::from_millis(1200))) }
    assert_eq!(toasts(&op).last(), Some(&(format!("{} {}", record::tr::too_slow(), record::tr::too_slow_next_time()), true)));
    assert!(app.data().settings.record.x264_too_slow, "kept for next time");
    // Chosen by hand: left as it is.
    let app2 = test_app();
    let op2 = open(&app2, &room);
    *op2.record.encoder.borrow_mut() = "libx264".into();
    op2.record.encoder_auto.set(false);
    for _ in 0..30 { op2.say_slow(Some(Duration::from_millis(1200))) }
    assert_eq!(toasts(&op2).last(), Some(&(record::tr::too_slow(), true)));
    assert!(!app2.data().settings.record.x264_too_slow);
}

#[test]
fn the_save_last_button_shows_only_while_recording_with_minutes_to_keep() {
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    op.record.replay_minutes.set(5);
    op.show_recording(true);
    assert_eq!(op.ui.get_replay_minutes(), 5);
    op.show_recording(false);
    assert_eq!(op.ui.get_replay_minutes(), 0);
    // Not recording: pressing it does nothing.
    let before = toasts(&op).len();
    op.save_last_minutes();
    assert_eq!(toasts(&op).len(), before);
}

/// What a stand-in VISCA camera gets within a second, in order.
fn received(camera: &std::net::UdpSocket, n: usize) -> Vec<Vec<u8>> {
    let mut buf = [0u8; 64];
    (0..n).map(|_| { let k = camera.recv(&mut buf).expect("a packet for the camera"); buf[..k].to_vec() }).collect()
}

#[test]
fn the_next_camera_is_moved_by_hand_and_its_shot_saved() {
    use stream::ptz::{Move, move_command, save_command};
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    let camera = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    camera.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let control = stream::ptz::Control::parse(&format!("127.0.0.1:{}", camera.local_addr().unwrap().port()));
    let g = op.ui.camera_switch_all();
    live_with(&op, &["Pulpit", "Wide"]);
    *op.record.camera_controls.borrow_mut() = vec![None, control.clone()];
    op.show_cameras();
    assert_eq!(g.get_move_caption(), "Move 2 Wide");
    // Held, let go; zoom in, let go; saved as position 3.
    g.invoke_drive(3);
    g.invoke_drive(0);
    g.invoke_zoom(1);
    g.invoke_zoom(0);
    g.invoke_save_position(2);
    assert_eq!(received(&camera, 5), [move_command(Move::Left), move_command(Move::Stop), move_command(Move::ZoomIn), move_command(Move::ZoomStop), save_command(3)]);
    assert!(toasts(&op).iter().any(|(t, _)| t.contains("position 3")));
    // Held while the camera goes out (its controls go away): it is told to stop.
    g.invoke_drive(1);
    g.invoke_switch();
    assert_eq!(received(&camera, 3), [move_command(Move::Up), move_command(Move::Stop), move_command(Move::ZoomStop)]);
    assert_eq!(g.get_next_positions(), 0, "the first camera is next now, and it cannot be moved");

    // One camera: the one going out is moved.
    live_with(&op, &["Wide"]);
    *op.record.camera_controls.borrow_mut() = vec![control];
    op.show_cameras();
    assert_eq!(g.get_next_positions(), i32::from(stream::ptz::POSITIONS));
    g.invoke_zoom(-1);
    assert_eq!(received(&camera, 1), [move_command(Move::ZoomOut)]);
    g.invoke_zoom(0);
    assert_eq!(received(&camera, 1), [move_command(Move::ZoomStop)]);
}

#[test]
fn every_camera_is_shown_small_and_a_click_makes_it_next() {
    use record::peek::{Peek, Small};
    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    let g = op.ui.camera_switch_all();
    live_with(&op, &["Pulpit", "Wide", "Band"]);
    let small = Some(Arc::new(Small { width: 2, height: 1, rgba: vec![9; 8] }));
    op.show_peek(Peek { cameras: vec![small.clone(), None, small], ..Default::default() });
    let tiles: Vec<bool> = g.get_tiles().iter().map(|t| t.has_picture).collect();
    assert_eq!(tiles, [true, false, true], "the second camera sends nothing");
    g.invoke_choose_next(2);
    assert_eq!(g.get_next(), 2);
    g.invoke_choose_next(0);
    assert_eq!(g.get_next(), 2, "the one going out cannot be next");
}

#[test]
fn camera_keys_work_only_while_live_and_have_no_keys_until_set() {
    use crate::shortcuts::{Action, Chord, KeySettings, Keymap};
    let map = Keymap::default();
    for a in [Action::CameraNext(1), Action::CameraNext(4), Action::CameraSwitch, Action::CameraFade] {
        assert!(map.keys(a).is_empty(), "{a:?} has no key by default");
        assert_eq!(Action::from_id(&a.id()), Some(a));
    }
    let mut keys = KeySettings::default();
    keys.assign(Action::CameraNext(3), Chord::new("3", true, false, false));
    keys.assign(Action::CameraSwitch, Chord::new("Enter", true, false, false));
    let map = Keymap::new(&keys);
    assert_eq!(map.action_for(&Chord::new("3", true, false, false)), Some(Action::CameraNext(3)));
    assert_eq!(map.action_for(&Chord::new("Enter", false, false, false)), Some(Action::Take), "Enter alone still shows the selected item");

    let app = test_app();
    let room = Arc::new(Room { open: true, ..Default::default() });
    let op = open(&app, &room);
    refresh(&op);
    assert!(!op.camera_key(Some(2), false), "not live: the key is left alone");
    live_with(&op, &["Pulpit", "Wide", "Band"]);
    assert!(op.camera_key(Some(3), false));
    assert_eq!(op.record.next.get(), Some(2));
    assert!(op.camera_key(None, false));
    assert_eq!((op.ui.get_camera_active(), op.record.next.get()), (2, Some(0)), "switched; the one that was out is next");
}

#[test]
fn settings_keep_what_a_lost_camera_shows_and_say_how_to_use_a_phone() {
    use crate::ui::SettingsDlg;
    use slint::ComponentHandle;
    use stream::camera_lost::CameraLost;
    let app = test_app();
    let room = Arc::new(Room::default());
    let op = open(&app, &room);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let mut prefs = RecordPrefs { camera: "phone://Balcony".into(), ..RecordPrefs::default() };
    crate::ui::dialogs::settings_record::fill_form(&g, &prefs);
    assert_eq!(g.get_stream_camera_lost(), 0, "the audience screen, as before");
    assert!(g.get_stream_camera_labels().iter().any(|l| l.contains("Phone camera: Balcony")), "a saved phone camera stays listed");
    g.set_stream_camera_lost(1);
    crate::ui::dialogs::settings_record::read_form(&g, &mut prefs);
    assert_eq!((prefs.camera_lost, prefs.camera.as_str()), (CameraLost::Logo, "phone://Balcony"));
    let saved = serde_json::to_value(&prefs).unwrap();
    assert_eq!(saved["camera_lost"], "logo");
    assert!(serde_json::to_value(RecordPrefs::default()).unwrap().get("camera_lost").is_none(), "the default is not saved");
    assert!(!crate::network::phone_camera::settings_help(&app).is_empty());
}
