use super::*;
use crate::{app::test_app, ui::operator::window::testing::{open as open_window, refresh}};

#[test]
fn go_live_with_nothing_set_up_walks_through_the_helper() {
    let app = test_app();
    let op = open_window(&app);
    refresh(&op);
    assert!(!op.ui.get_can_stream(), "nothing set up: no Live button, but File → Go Live");
    op.go_live();
    assert_eq!(op.dialog(), Some(DialogKind::GoLiveHelper), "the helper, not a message");
    assert!(!op.is_recording());
    let g = op.ui.global::<GoLiveHelperDlg>();
    assert_eq!(g.get_step(), 0);
    assert!(g.get_key_help().contains("YouTube Studio"));
    // No key: it says so and stays.
    g.invoke_next();
    assert_eq!(g.get_step(), 0);
    assert!(!g.get_error().is_empty());
    g.set_key("abcd-1234".into());
    g.invoke_next();
    assert_eq!(g.get_step(), 1, "{}", g.get_error());
    assert_eq!(stream::keys::app_store().get(stream::built_in_id(0)).as_deref(), Some("abcd-1234"), "kept in the password store");
    assert!(g.get_key().is_empty() && g.get_key_kept());
    // No camera, then the app's own sound.
    g.set_camera(0);
    g.invoke_next();
    g.set_sound(1);
    g.invoke_next();
    assert_eq!(g.get_step(), 3);
    g.invoke_back();
    assert_eq!(g.get_step(), 2);
    g.invoke_next();
    g.invoke_next();
    assert_eq!(g.get_step(), 4);
    let p = app.data().settings.record.clone();
    let yt = p.stream.iter().find(|d| d.id == stream::built_in_id(0)).expect("YouTube is set up");
    assert!(yt.on);
    assert!(p.camera.is_empty());
    assert_eq!(p.sound, Sound::App.key());
    assert!(!helper::needed(&p));
    g.invoke_finish(false);
    assert_eq!(op.dialog(), None);
    refresh(&op);
    assert!(op.ui.get_can_stream(), "the Live button shows now");
}

#[test]
fn another_server_needs_its_address() {
    let app = test_app();
    let op = open_window(&app);
    refresh(&op);
    open(&op);
    let g = op.ui.global::<GoLiveHelperDlg>();
    g.set_service(3);
    g.invoke_service_changed(3);
    assert!(g.get_key_help().contains("rtmp://"));
    g.invoke_next();
    assert_eq!(g.get_step(), 0);
    g.set_url("rtmp://live.example.org/app".into());
    g.invoke_next();
    assert_eq!(g.get_step(), 1, "a server with no key");
    let p = app.data().settings.record.clone();
    let custom = p.stream.iter().find(|d| d.id == stream::built_in_id(3)).unwrap();
    assert_eq!((custom.on, custom.url.as_str()), (true, "rtmp://live.example.org/app"));
}
