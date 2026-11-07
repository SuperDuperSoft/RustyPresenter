//! The Phone remote dialog (`operator.js:830-874`, `ui/dialogs/phone.slint`): the address, pairing a phone with a
//! QR code and a PIN that works for two minutes, and the paired phones.
#[path = "../../strings/phone.rs"]
pub mod strings;

use super::DialogKind;
use crate::ui::{PairedPhone, PhoneDialogState, operator::window::Operator};
use serde::Deserialize;
use serde_json::{Value, json};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::{Cell, RefCell}, rc::{Rc, Weak}, time::Duration};
use uuid::Uuid;

/// `pair/start`'s answer. `qr` is an SVG, or null without a network address.
#[derive(Deserialize)]
struct Pairing { pin: String, qr: Option<String>, seconds: i64 }

/// The open dialog's pairing: the countdown, whether a code is showing (then updates do not redraw) and the paired
/// phones when it started (a new one means a phone just paired, which closes pairing).
#[derive(Default)]
struct PairState { timer: Timer, left: Cell<i64>, showing: Cell<bool>, phones: RefCell<Vec<Uuid>> }

impl PairState {
    fn stop(&self) {
        self.timer.stop();
        self.showing.set(false);
    }
}

/// Opens the dialog. It is redrawn by updates only while no code is showing (`operator.js:874`); closing it stops
/// the countdown and pairing.
pub fn open(op: &Rc<Operator>) {
    op.open_dialog(DialogKind::Phone);
    let state = Rc::new(PairState::default());
    let g = op.ui.global::<PhoneDialogState>();
    let (me, st) = (Rc::downgrade(op), state.clone());
    g.on_pair(move || if let Some(op) = me.upgrade() { start(&op, &st) });
    let (me, st) = (Rc::downgrade(op), state.clone());
    g.on_remove(move |id| if let (Some(op), Ok(id)) = (me.upgrade(), id.parse::<Uuid>()) { revoke(&op, &st, id) });
    let me = Rc::downgrade(op);
    g.on_may_draw(move |id, on| if let (Some(op), Ok(id)) = (me.upgrade(), id.parse::<Uuid>()) { may_draw(&op, id, on) });
    let st = state.clone();
    op.on_dialog_refresh(move |op| {
        // A phone that pairs (by QR code or PIN) closes pairing on the computer: show it in the list right away.
        if st.showing.get() && phone_ids(op) != *st.phones.borrow() { st.stop() }
        if !st.showing.get() { draw(op) }
    });
    let st = state;
    op.on_dialog_close(move |op| {
        st.stop();
        op.ops().call::<Value>("pair/stop", json!({}), |_| {});
    });
    draw(op);
}

fn phone_ids(op: &Operator) -> Vec<Uuid> {
    op.state.borrow().snap.as_ref().map(|s| s.phones_list.iter().map(|(id, ..)| *id).collect()).unwrap_or_default()
}

/// Draws the note, the empty pairing area and the paired phones (`draw()`, `operator.js:833-843`).
fn draw(op: &Operator) {
    let Some(s) = op.state.borrow().snap.clone() else { return };
    let g = op.ui.global::<PhoneDialogState>();
    g.set_lan_url(s.lan_url.as_deref().unwrap_or_default().into());
    g.set_pin("".into());
    g.set_countdown("".into());
    g.set_has_qr(false);
    g.set_qr(slint::Image::default());
    let phones: Vec<PairedPhone> = s.phones_list.iter().map(|(id, name, may_draw)| PairedPhone { id: id.to_string().into(), name: name.as_str().into(), may_draw: *may_draw }).collect();
    g.set_phones(ModelRc::new(VecModel::from(phones)));
}

/// Pair a phone (`startPair`, `operator.js:844-857`): the QR code (which pairs on its own: it carries a one-time
/// secret), the PIN for typing instead, and a countdown ticking each second.
fn start(op: &Rc<Operator>, state: &Rc<PairState>) {
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
    op.ops().call::<Pairing>("pair/start", json!({}), move |r| {
        let (Some(op), Some(state)) = (me.upgrade(), st.upgrade()) else { return };
        let p = match r {
            Ok(p) => p,
            Err(e) => return op.toast(&e, true),
        };
        let g = op.ui.global::<PhoneDialogState>();
        let qr = p.qr.as_deref().and_then(|svg| slint::Image::load_from_svg_data(svg.as_bytes()).ok());
        g.set_has_qr(qr.is_some());
        g.set_qr(qr.unwrap_or_default());
        g.set_pin(p.pin.as_str().into());
        state.showing.set(true);
        *state.phones.borrow_mut() = phone_ids(&op);
        state.left.set(p.seconds);
        tick(&op, &state);
        let (me, st) = (Rc::downgrade(&op), Rc::downgrade(&state));
        state.timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            if let (Some(op), Some(state)) = (me.upgrade(), st.upgrade()) { tick(&op, &state) }
        });
    });
}

/// One second of the countdown: "Code works for N more seconds", then "Code expired — …" (`operator.js:856`).
fn tick(op: &Operator, state: &PairState) {
    let left = state.left.get();
    let text = if left > 0 { strings::code_works_for(left) } else { strings::code_expired() };
    op.ui.global::<PhoneDialogState>().set_countdown(text.into());
    state.left.set(left - 1);
    if left - 1 < -1 { state.timer.stop() }
}

/// Remove (`operator.js:842`), with no question: `phone/revoke`, then a fresh read. The web's `draw()` clears the
/// pairing area too, so a code on screen goes away.
fn revoke(op: &Rc<Operator>, state: &Rc<PairState>, id: Uuid) {
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
    op.ops().call::<Value>("phone/revoke", json!({ "id": id }), move |r| {
        let Some(op) = me.upgrade() else { return };
        if let Err(e) = r { return op.toast(&e, true) }
        if let Some(state) = st.upgrade() { state.stop() }
        if op.dialog() == Some(DialogKind::Phone) { draw(&op) }
        op.request_refresh();
    });
}

/// "May draw on slides" for one phone, saved at once; the list redraws from the next read.
fn may_draw(op: &Rc<Operator>, id: Uuid, on: bool) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("phone/may_draw", json!({ "id": id, "on": on }), move |r| {
        let Some(op) = me.upgrade() else { return };
        if let Err(e) = r { op.toast(&e, true) }
        op.request_refresh();
    });
}

/// Connects the top bar's Phone button. The dialog's own buttons are connected each time it opens.
pub(crate) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_phone(move || if let Some(op) = me.upgrade() { open(&op) });
}

#[cfg(test)]
mod tests {
    use super::super::services::testing::*;
    use super::*;
    use crate::{app::{App, Phone, test_app}, store::Store, ui::operator::window::testing::{find, find_all, refresh}};
    use slint::Model;
    use std::sync::Arc;

    fn app_with_lan() -> Arc<App> {
        let root = std::env::temp_dir().join(format!("rp-test-{}", Uuid::new_v4()));
        let store = Store::open(root).unwrap();
        let data = store.load().unwrap();
        App::new(store, data, 0, Some("http://192.168.1.20:7878/remote".into()))
    }

    fn open_phone(app: &Arc<App>) -> Rc<Operator> {
        let op = opened(app);
        op.ui.invoke_phone();
        settle(&op);
        assert_eq!(op.dialog(), Some(DialogKind::Phone));
        op
    }

    fn label(op: &Operator, id: &str) -> String { find(op, id).and_then(|e| e.accessible_label()).unwrap_or_default().to_string() }

    #[test]
    fn phone_dialog_title_and_address_text() {
        let app = app_with_lan();
        let op = open_phone(&app);
        assert_eq!(op.ui.get_dialog_title(), "Phone remote");
        assert_eq!(label(&op, "PhoneDialog::note"), "Control the slides from any phone on the church Wi-Fi. The phone must be on the same network as this computer.");
        assert_eq!(label(&op, "PhoneDialog::address"), "http://192.168.1.20:7878/remote");
        assert!(find(&op, "PhoneDialog::none-yet").is_some(), "None yet.");
        assert!(find(&op, "PhoneDialog::pin").is_none(), "no code until Pair a phone");

        let app = test_app();
        let op = open_phone(&app);
        assert_eq!(label(&op, "PhoneDialog::note"), "Control the slides from any phone on the church Wi-Fi. The phone must be on the same network as this computer. (No network found — connect this computer to Wi-Fi.)");
        assert!(find(&op, "PhoneDialog::address").is_none());
    }

    #[test]
    fn pair_shows_six_digit_pin_and_qr() {
        let app = app_with_lan();
        let op = open_phone(&app);
        click(&op, "PhoneDialog::pair-btn", 0);
        let pin = label(&op, "PhoneDialog::pin");
        assert_eq!(pin.len(), 6, "{pin:?}");
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(app.pairing.lock().unwrap().as_ref().map(|p| p.pin.clone()), Some(pin));
        let g = op.ui.global::<PhoneDialogState>();
        assert!(g.get_has_qr(), "the SVG loaded");
        assert!(g.get_qr().size().width > 0);
        assert_eq!(find(&op, "PhoneDialog::qr").and_then(|e| e.accessible_value()).unwrap_or_default(), "qr");
        assert_eq!(label(&op, "PhoneDialog::countdown"), "Code works for 120 more seconds");
        assert_eq!(label(&op, "PhoneDialog::scan"), "Scan this with your phone's camera. That's it.");
        assert_eq!(label(&op, "PhoneDialog::or-type"), "Or open http://192.168.1.20:7878/remote and type");
        // An update while the code shows does not redraw (it would wipe the code).
        refresh(&op);
        settle(&op);
        assert!(find(&op, "PhoneDialog::pin").is_some());
    }

    #[test]
    fn without_a_network_the_pin_steps_stay() {
        let app = test_app();
        let op = open_phone(&app);
        click(&op, "PhoneDialog::pair-btn", 0);
        assert_eq!(label(&op, "PhoneDialog::pin").len(), 6);
        assert!(!op.ui.global::<PhoneDialogState>().get_has_qr());
        assert!(find(&op, "PhoneDialog::scan").is_none());
        assert!(find(&op, "PhoneDialog::or-type").is_none());
    }

    #[test]
    fn a_phone_that_pairs_shows_in_the_list_right_away() {
        let app = app_with_lan();
        let op = open_phone(&app);
        click(&op, "PhoneDialog::pair-btn", 0);
        assert!(find(&op, "PhoneDialog::none-yet").is_some());
        // What a successful /remote/pair does: close pairing, add the phone, broadcast.
        *app.pairing.lock().unwrap() = None;
        app.phones.lock().unwrap().push(Phone { id: Uuid::new_v4(), name: "iPhone".into(), hash: String::new(), last_seen: 0, may_draw: true });
        refresh(&op);
        settle(&op);
        assert_eq!(label(&op, "PhoneDialog::phone-row"), "iPhone");
        assert!(find(&op, "PhoneDialog::pin").is_none(), "the used code goes away");
        i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(3));
        assert!(find(&op, "PhoneDialog::pin").is_none(), "and the countdown stopped");
    }

    #[test]
    fn closing_phone_dialog_stops_pairing() {
        let app = app_with_lan();
        let op = open_phone(&app);
        click(&op, "PhoneDialog::pair-btn", 0);
        assert!(app.pairing.lock().unwrap().is_some());
        click(&op, "DialogShell::close-btn", 0);
        assert_eq!(op.dialog(), None);
        assert!(app.pairing.lock().unwrap().is_none(), "pair/stop ran");
    }

    #[test]
    fn countdown_expires_with_web_text() {
        let app = app_with_lan();
        let op = open_phone(&app);
        click(&op, "PhoneDialog::pair-btn", 0);
        let g = op.ui.global::<PhoneDialogState>();
        i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(1));
        assert_eq!(g.get_countdown(), "Code works for 119 more seconds");
        for _ in 1..119 { i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(1)) }
        assert_eq!(g.get_countdown(), "Code works for 1 more seconds", "as the web words it");
        i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(1));
        assert_eq!(g.get_countdown(), "Code expired — click Pair a phone again.");
        for _ in 0..5 { i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(1)) }
        assert_eq!(g.get_countdown(), "Code expired — click Pair a phone again.");
    }

    #[test]
    fn remove_revokes_without_asking_and_redraws() {
        let app = app_with_lan();
        let phone = Phone { id: Uuid::new_v4(), name: "Pastor's phone".into(), hash: String::new(), last_seen: 0, may_draw: true };
        app.phones.lock().unwrap().push(phone.clone());
        let op = open_phone(&app);
        assert_eq!(find_all(&op, "PhoneDialog::phone-row").len(), 1);
        assert_eq!(label(&op, "PhoneDialog::phone-row"), "Pastor's phone");
        click(&op, "PhoneDialog::pair-btn", 0);
        click(&op, "PhoneDialog::remove-btn", 0);
        assert_eq!(op.ui.get_ask().row_count(), 0, "no question");
        assert!(app.phones.lock().unwrap().is_empty());
        assert!(find(&op, "PhoneDialog::phone-row").is_none(), "redrawn from a fresh read");
        assert!(find(&op, "PhoneDialog::none-yet").is_some());
        assert!(find(&op, "PhoneDialog::pin").is_none(), "the pairing area is cleared, as the web's draw() does");
    }

    #[test]
    fn each_phone_has_a_may_draw_switch_on_by_default() {
        let app = app_with_lan();
        let phone = Phone { id: Uuid::new_v4(), name: "Pastor's iPad".into(), hash: String::new(), last_seen: 0, may_draw: true };
        app.phones.lock().unwrap().push(phone.clone());
        let op = open_phone(&app);
        let check = find(&op, "PhoneDialog::may-draw").expect("a switch per phone");
        assert_eq!(check.accessible_label().unwrap_or_default(), "May draw on slides");
        assert_eq!(check.accessible_checked(), Some(true));
        check.invoke_accessible_default_action();
        settle(&op);
        assert!(!app.phones.lock().unwrap()[0].may_draw, "saved at once");
        refresh(&op);
        settle(&op);
        assert_eq!(find(&op, "PhoneDialog::may-draw").unwrap().accessible_checked(), Some(false));
    }
}
