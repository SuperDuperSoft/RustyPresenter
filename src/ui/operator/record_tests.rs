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
