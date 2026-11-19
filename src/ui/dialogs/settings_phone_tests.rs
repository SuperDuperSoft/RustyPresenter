//! Settings → Remote and network → Phone camera and microphone: a QR code beside each secure address that pairs in one
//! scan, a live line under each, a new code once one is used, and the big window.
use super::*;
use crate::{network::phone_link, ui::{dialogs::{SettingsTab, settings}, operator::window::testing::{open as open_window, refresh as read}}};
use std::sync::Arc;

fn opened(app: &Arc<crate::app::App>) -> Rc<Operator> {
    crate::ui::ui_test_init();
    let op = open_window(app);
    read(&op);
    op
}

fn g(op: &Operator) -> PhoneLinkDlg<'static> { win(op).global::<PhoneLinkDlg>() }

#[test]
fn each_address_has_its_one_scan_code_and_a_live_line() {
    let app = phone_link::tests::app_on_network();
    let op = opened(&app);
    settings::open(&op, SettingsTab::PhoneCamMic);
    assert_eq!(g(&op).get_camera_address(), "https://192.168.1.20:4320/camera", "the secure page, at the network address");
    assert_eq!(g(&op).get_mic_address(), "https://192.168.1.20:4320/stage-view/mic");
    assert!(g(&op).get_camera_has_qr() && g(&op).get_mic_has_qr());
    // The codes are the links with their one-time secrets.
    let camera = phone_link::link(&app, phone_link::Kind::Camera).unwrap();
    assert!(camera.starts_with("https://192.168.1.20:4320/camera#pair="));
    assert_eq!(DRAWN.with(|d| d.borrow()[0].clone()), Some(camera.clone()), "the code drawn is that link");
    // The secure server has not started in this test: said plainly, in red.
    assert_eq!(g(&op).get_camera_health(), "The secure server is starting…");
    assert!(g(&op).get_camera_problem());
    // Running, then a phone connects with the microphone page open.
    *app.net.https.lock().unwrap() = Some(4320);
    refresh(&op);
    assert_eq!(g(&op).get_mic_health(), "Waiting for a phone");
    assert!(!g(&op).get_mic_problem() && !g(&op).get_firewall_blocked());
    phone_link::saw(&app, phone_link::Kind::Mic, Some("iPhone".into()));
    refresh(&op);
    assert_eq!(g(&op).get_mic_health(), "Phone connected: iPhone");
    // A used code is replaced by a new one, drawn again.
    app.net.links.lock().unwrap()[0] = None;
    refresh(&op);
    let new = DRAWN.with(|d| d.borrow()[0].clone()).unwrap();
    assert_ne!(new, camera);
    assert!(new.starts_with("https://192.168.1.20:4320/camera#pair="));

    // "Show the QR code": the big window with the same code, address and line.
    g(&op).invoke_show_big(true);
    let (w, kind) = big_window().expect("the big window");
    assert_eq!(kind, phone_link::Kind::Mic);
    assert_eq!(w.get_heading(), "Phone as a microphone");
    assert_eq!(w.get_address(), "https://192.168.1.20:4320/stage-view/mic");
    assert_eq!(w.get_health(), "Phone connected: iPhone");
    g(&op).invoke_show_big(false);
    assert_eq!(big_window().unwrap().0.get_heading(), "Phone as a camera");
    w.invoke_close_window();
    settings::close(&op);
}

#[test]
fn off_the_network_there_is_no_code() {
    let app = crate::app::test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::PhoneCamMic);
    assert!(!g(&op).get_camera_has_qr() && !g(&op).get_mic_has_qr());
    assert_eq!(g(&op).get_camera_address(), crate::stream::phone_camera::tr::no_network());
    assert_eq!(g(&op).get_mic_health(), crate::stream::phone_camera::tr::no_network());
    assert!(g(&op).get_mic_problem());
    settings::close(&op);
}
