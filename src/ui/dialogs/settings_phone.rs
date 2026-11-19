//! Settings → Remote and network → Phone camera and microphone (`ui/dialogs/settings-phone.slint`): the QR code
//! beside each secure address (it opens the phone's page and connects the phone in one scan,
//! `network::phone_link`), the live line under each, "Let phones in" when Windows Firewall keeps phones out, and the
//! big "Show the QR code" window. Redrawn every second while Settings (or the big window) is open; a code is drawn
//! again only when it changes (used by a phone, or renewed).
use super::settings_window::{self, win};
use crate::{
    network::phone_link::{self, Health, Kind},
    platform::firewall::{self, Firewall},
    ui::{PhoneLinkDlg, PhoneQrWindow, operator::window::Operator},
};
use slint::{ComponentHandle, Timer, TimerMode};
use std::{cell::RefCell, rc::Rc, sync::Mutex, time::{Duration, Instant}};

/// How often Windows Firewall is asked again (it takes a moment, so not every second).
const FIREWALL_EVERY: Duration = Duration::from_secs(15);

thread_local! {
    static TIMER: Timer = Timer::default();
    /// The links drawn as QR codes now (camera, microphone), so a code is drawn again only when it changes.
    static DRAWN: RefCell<[Option<String>; 2]> = const { RefCell::new([None, None]) };
    /// The big window, and whose code it shows.
    static BIG: RefCell<Option<(PhoneQrWindow, Kind)>> = const { RefCell::new(None) };
}

/// Windows Firewall's last answer, when it was asked, and whether it is being asked now.
static FIREWALL: Mutex<(Firewall, Option<Instant>, bool)> = Mutex::new((Firewall::Unknown, None, false));

/// Settings is opening: the codes, the lines and the buttons, then a redraw every second.
pub(crate) fn load(op: &Rc<Operator>) {
    DRAWN.with(|d| *d.borrow_mut() = [None, None]);
    let g = win(op).global::<PhoneLinkDlg>();
    g.set_firewall_note("".into());
    g.set_firewall_busy(false);
    g.set_camera_has_qr(false);
    g.set_mic_has_qr(false);
    let me = Rc::downgrade(op);
    g.on_show_big(move |mic| if let Some(op) = me.upgrade() { show_big(&op, if mic { Kind::Mic } else { Kind::Camera }) });
    let me = Rc::downgrade(op);
    g.on_let_phones_in(move || if let Some(op) = me.upgrade() { let_phones_in(&op) });
    refresh(op);
    let me = Rc::downgrade(op);
    TIMER.with(|t| t.start(TimerMode::Repeated, Duration::from_secs(1), move || {
        let Some(op) = me.upgrade() else { return TIMER.with(Timer::stop) };
        let big_open = BIG.with(|b| b.borrow().as_ref().is_some_and(|(w, _)| w.window().is_visible()));
        if settings_window::is_open(&op) || big_open { refresh(&op) } else { TIMER.with(Timer::stop) }
    }));
}

/// What Windows Firewall does with phones now: the last answer, asked again in the background when it is old.
/// Tests never ask this PC's firewall.
fn firewall_now(op: &Operator) -> Firewall {
    if op.ops().is_inline() { return Firewall::Unknown }
    let mut f = FIREWALL.lock().unwrap_or_else(|e| e.into_inner());
    if !f.2 && f.1.is_none_or(|at| at.elapsed() > FIREWALL_EVERY) {
        f.2 = true;
        let port = phone_link::secure_port(op.app());
        let _ = std::thread::Builder::new().name("firewall".into()).spawn(move || {
            let answer = firewall::check(port);
            *FIREWALL.lock().unwrap_or_else(|e| e.into_inner()) = (answer, Some(Instant::now()), false);
        });
    }
    f.0
}

fn qr_image(link: Option<&str>) -> Option<slint::Image> {
    link.and_then(phone_link::qr_svg).and_then(|svg| slint::Image::load_from_svg_data(svg.as_bytes()).ok())
}

/// The heading of the big window.
fn heading(kind: Kind) -> String {
    match kind { Kind::Camera => crate::tr!("Phone as a camera"), Kind::Mic => crate::tr!("Phone as a microphone") }
}

/// Draws the codes and lines as they are now.
pub(crate) fn refresh(op: &Rc<Operator>) {
    let app = op.app();
    let fw = firewall_now(op);
    let port = phone_link::secure_port(app);
    let g = win(op).global::<PhoneLinkDlg>();
    let mut blocked = false;
    for kind in Kind::ALL {
        let address = phone_link::address(app, kind).unwrap_or_else(crate::stream::phone_camera::tr::no_network);
        let link = phone_link::link(app, kind);
        let i = kind as usize;
        let changed = DRAWN.with(|d| d.borrow()[i] != link);
        let health = phone_link::health(app, kind, fw);
        blocked |= matches!(health, Health::Firewall { .. });
        let (text, problem) = (health.text(kind, port), health.problem());
        let image = changed.then(|| qr_image(link.as_deref()));
        if changed { DRAWN.with(|d| d.borrow_mut()[i] = link.clone()) }
        match kind {
            Kind::Camera => {
                g.set_camera_address(address.as_str().into());
                if let Some(img) = &image { g.set_camera_has_qr(img.is_some()); g.set_camera_qr(img.clone().unwrap_or_default()) }
                g.set_camera_health(text.as_str().into());
                g.set_camera_problem(problem);
            }
            Kind::Mic => {
                g.set_mic_address(address.as_str().into());
                if let Some(img) = &image { g.set_mic_has_qr(img.is_some()); g.set_mic_qr(img.clone().unwrap_or_default()) }
                g.set_mic_health(text.as_str().into());
                g.set_mic_problem(problem);
            }
        }
        BIG.with(|b| if let Some((w, k)) = b.borrow().as_ref() && *k == kind {
            w.set_address(address.as_str().into());
            if let Some(Some(img)) = &image { w.set_qr(img.clone()) }
            w.set_health(text.as_str().into());
            w.set_problem(problem);
        });
    }
    g.set_firewall_blocked(blocked);
}

/// "Show the QR code": the big window of `kind`'s code (one window, showing the code last asked for).
fn show_big(op: &Rc<Operator>, kind: Kind) {
    let made = BIG.with(|b| {
        let mut b = b.borrow_mut();
        if b.is_none() {
            let Ok(w) = PhoneQrWindow::new() else { return false };
            let weak = w.as_weak();
            w.on_close_window(move || if let Some(w) = weak.upgrade() { let _ = w.hide(); });
            *b = Some((w, kind));
        }
        if let Some((w, k)) = b.as_mut() {
            *k = kind;
            w.set_heading(heading(kind).into());
            w.set_qr(qr_image(phone_link::link(op.app(), kind).as_deref()).unwrap_or_default());
        }
        true
    });
    if !made { return }
    refresh(op);
    BIG.with(|b| if let Some((w, _)) = b.borrow().as_ref() && let Err(e) = w.show() { op.toast(e.to_string(), true) });
}

/// "Let phones in": Windows asks for an administrator's OK, then the rule is added (`platform::firewall`).
fn let_phones_in(op: &Rc<Operator>) {
    let g = win(op).global::<PhoneLinkDlg>();
    g.set_firewall_busy(true);
    g.set_firewall_note(crate::tr!("Waiting for Windows…").into());
    let ui = win(op).as_weak();
    let _ = std::thread::Builder::new().name("firewall-let-in".into()).spawn(move || {
        let done = firewall::let_phones_in();
        // Ask again at once.
        FIREWALL.lock().unwrap_or_else(|e| e.into_inner()).1 = None;
        let _ = ui.upgrade_in_event_loop(move |ui| {
            let g = ui.global::<PhoneLinkDlg>();
            g.set_firewall_busy(false);
            g.set_firewall_note(match done { Ok(()) => crate::tr!("Done: phones can now reach RustyPresenter through Windows Firewall."), Err(e) => e }.into());
        });
    });
}

/// The big window, for tests.
#[cfg(test)]
pub(crate) fn big_window() -> Option<(PhoneQrWindow, Kind)> { BIG.with(|b| b.borrow().as_ref().map(|(w, k)| (w.clone_strong(), *k))) }

#[cfg(test)]
#[path = "settings_phone_tests.rs"]
mod tests;
