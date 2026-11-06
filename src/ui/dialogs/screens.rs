//! The Screens dialog (`operator.js:793-828`, `ui/dialogs/screens.slint`): show the room and the stage display on a
//! monitor, move them, turn them off or test them in a window.
#[path = "../../strings/screens.rs"]
pub mod strings;

use super::DialogKind;
use crate::{
    app::Monitor,
    ui::{MonitorChoice, ScreenCard, ScreensDialogState, operator::{view::{OperatorSnapshot, RoomStatus}, window::Operator}},
};
use serde_json::{Value, json};
use slint::{ComponentHandle, ModelRc, Timer, VecModel};
use std::{rc::{Rc, Weak}, time::Duration};

/// How long after Show on this screen the dialog is redrawn, so the new window's status shows (`operator.js:813`).
const REDRAW_AFTER_SHOW: Duration = Duration::from_millis(1500);

/// The monitor a card starts on (`operator.js:802`): the saved one if it is still connected, else the first that is
/// not the primary, else none (Show on this screen is then disabled).
pub fn default_pick(monitors: &[Monitor], saved: Option<&str>) -> Option<String> {
    saved.filter(|s| monitors.iter().any(|m| m.id == *s)).map(str::to_string)
        .or_else(|| monitors.iter().find(|m| !m.primary).map(|m| m.id.clone()))
}

/// A card's status chip, `(text, state)` (`operator.js:804, 809`): Showing once its screen has connected,
/// Starting… while its window is open but not connected yet, else Off.
fn chip(connected: bool, open: bool) -> (&'static str, &'static str) {
    if connected { (strings::SHOWING, "ok") } else if open { (strings::STARTING, "warn") } else { (strings::OFF, "") }
}

fn card(s: &OperatorSnapshot, role: &str) -> ScreenCard {
    let (title, desc, saved, connected) = if role == "room" {
        (strings::ROOM_TITLE, strings::ROOM_DESC, s.settings.displays.room.as_deref(), s.room == RoomStatus::Showing)
    } else {
        (strings::STAGE_TITLE, strings::STAGE_DESC, s.settings.displays.stage.as_deref(), s.stage_connected)
    };
    let open = s.windows.iter().any(|w| w == role);
    let (chip_text, chip_state) = chip(connected, open);
    let pick = default_pick(&s.monitors, saved).and_then(|id| s.monitors.iter().position(|m| m.id == id));
    let monitors: Vec<MonitorChoice> = s.monitors.iter().map(|m| MonitorChoice { id: m.id.as_str().into(), label: m.label.as_str().into() }).collect();
    ScreenCard {
        role: role.into(), title: title.into(), desc: desc.into(), chip: chip_text.into(), chip_state: chip_state.into(), open,
        monitors: ModelRc::new(VecModel::from(monitors)), pick: pick.map_or(-1, |i| i as i32),
    }
}

/// Opens the dialog and asks for a fresh read, so newly plugged-in monitors show; the dialog is redrawn when it
/// arrives (and after every `Data` update, `DialogKind::refreshes`).
pub fn open(op: &Rc<Operator>) {
    op.open_dialog(DialogKind::Screens);
    op.on_dialog_refresh(draw);
    draw(op);
    op.request_refresh();
}

/// Draws both cards from the newest snapshot. New card models make new cards, so a picked monitor goes back to the
/// default, as the web's redraw does.
fn draw(op: &Rc<Operator>) {
    let Some(s) = op.state.borrow().snap.clone() else { return };
    let cards = vec![card(&s, "room"), card(&s, "stage")];
    op.ui.global::<ScreensDialogState>().set_cards(ModelRc::new(VecModel::from(cards)));
}

fn title(role: &str) -> &'static str { if role == "room" { strings::ROOM_TITLE } else { strings::STAGE_TITLE } }

/// `display/open` (`operator.js:813, 815`). On a monitor, it toasts "<title> opened" and redraws 1.5 s later; a test
/// window (`monitor` None) does neither.
fn show(op: &Rc<Operator>, role: String, monitor: Option<String>) {
    let me = Rc::downgrade(op);
    let on_monitor = monitor.is_some();
    op.ops().call::<Value>("display/open", json!({ "role": role, "monitor": monitor }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) if on_monitor => {
                op.toast(strings::opened(title(&role)), false);
                let me = Rc::downgrade(&op);
                Timer::single_shot(REDRAW_AFTER_SHOW, move || if let Some(op) = me.upgrade() { op.dialog_refresh() });
            }
            Ok(_) => {}
            Err(e) => op.toast(&e, true),
        }
    });
}

/// Turn off (`operator.js:814`).
fn turn_off(op: &Rc<Operator>, role: String) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("display/close", json!({ "role": role }), move |r| {
        if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(&e, true) }
    });
}

/// Connects the top bar's Screens button and the dialog's buttons. They hold a `Weak`.
pub(crate) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_screens(move || if let Some(op) = me.upgrade() { open(&op) });
    let g = op.ui.global::<ScreensDialogState>();
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_show(move |role, monitor| if let Some(op) = me.upgrade() {
        if !monitor.is_empty() { show(&op, role.into(), Some(monitor.into())) }
    });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_test_window(move |role| if let Some(op) = me.upgrade() { show(&op, role.into(), None) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_turn_off(move |role| if let Some(op) = me.upgrade() { turn_off(&op, role.into()) });
    // Refresh (`operator.js:823`): the follower re-reads everything, monitors included, and the snapshot's arrival
    // redraws the dialog. `App::monitors()` is never called on the Slint thread.
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_refresh(move || if let Some(op) = me.upgrade() { op.request_refresh() });
}

#[cfg(test)]
mod tests {
    use super::super::services::testing::*;
    use super::*;
    use crate::{app::{App, Screens, test_app}, ui::operator::window::testing::{find, find_all, refresh}};
    use slint::Model;
    use std::sync::{Arc, Mutex};

    /// A windowing layer with monitors that can change and a record of what was opened.
    #[derive(Default)]
    struct FakeDisplays { monitors: Mutex<Vec<Monitor>>, open: Mutex<Vec<String>>, calls: Mutex<Vec<(String, Option<String>)>> }

    impl Screens for FakeDisplays {
        fn open(&self, role: &str, monitor: Option<&str>) -> Result<(), String> {
            self.calls.lock().unwrap().push((role.into(), monitor.map(str::to_string)));
            let mut open = self.open.lock().unwrap();
            if !open.iter().any(|r| r == role) { open.push(role.into()); }
            Ok(())
        }
        fn close(&self, role: &str) { self.open.lock().unwrap().retain(|r| r != role); }
        fn open_roles(&self) -> Vec<String> { self.open.lock().unwrap().clone() }
        fn monitors(&self) -> Vec<Monitor> { self.monitors.lock().unwrap().clone() }
    }

    fn mon(id: &str, primary: bool) -> Monitor {
        Monitor { id: id.into(), label: format!("{id} 1920×1080"), x: 0, y: 0, width: 1920, height: 1080, primary }
    }

    fn with_displays(monitors: Vec<Monitor>) -> (Arc<App>, Arc<FakeDisplays>) {
        let app = test_app();
        let d = Arc::new(FakeDisplays { monitors: Mutex::new(monitors), ..Default::default() });
        app.set_screens(d.clone());
        (app, d)
    }

    fn open_screens(app: &Arc<App>) -> Rc<Operator> {
        let op = opened(app);
        op.ui.invoke_screens();
        settle(&op);
        assert_eq!(op.dialog(), Some(DialogKind::Screens));
        assert_eq!(op.ui.get_dialog_title(), "Screens");
        op
    }

    fn cards(op: &Operator) -> Vec<ScreenCard> { op.ui.global::<ScreensDialogState>().get_cards().iter().collect() }

    fn toasts(op: &Operator) -> Vec<String> { op.ui.get_toasts().iter().map(|t| t.text.to_string()).collect() }

    #[test]
    fn default_pick_prefers_saved_then_non_primary() {
        let ms = [mon("A", true), mon("B", false), mon("C", false)];
        assert_eq!(default_pick(&ms, Some("C")).as_deref(), Some("C"), "the saved monitor");
        assert_eq!(default_pick(&ms, Some("gone")).as_deref(), Some("B"), "saved but unplugged: first non-primary");
        assert_eq!(default_pick(&ms, None).as_deref(), Some("B"));
        assert_eq!(default_pick(&ms[..1], None), None, "only the primary");
        assert_eq!(default_pick(&ms[..1], Some("A")).as_deref(), Some("A"), "the saved primary is kept");
        assert_eq!(default_pick(&[], Some("A")), None);
    }

    #[test]
    fn one_monitor_nothing_saved_disables_show() {
        let (app, d) = with_displays(vec![mon("A", true)]);
        let op = open_screens(&app);
        assert!(cards(&op).iter().all(|c| c.pick == -1));
        let show = find(&op, "ScreenCardView::show-btn").unwrap();
        assert_eq!(show.accessible_enabled(), Some(false));
        click(&op, "ScreenCardView::show-btn", 0);
        assert!(d.calls.lock().unwrap().is_empty(), "a disabled button opens nothing");
        // Test in a window still works, on no monitor.
        click(&op, "ScreenCardView::test-btn", 0);
        assert_eq!(*d.calls.lock().unwrap(), [("room".to_string(), None)]);
        assert!(toasts(&op).is_empty(), "no toast for a test window");
    }

    #[test]
    fn one_monitor_shows_plug_in_note() {
        let (app, _d) = with_displays(vec![mon("A", true)]);
        let op = open_screens(&app);
        assert_eq!(find_all(&op, "ScreenCardView::one-screen-note").len(), 2, "on both cards");
        assert!(find(&op, "ScreenCardView::monitor").is_none());
        let (app, _d) = with_displays(vec![mon("A", true), mon("B", false)]);
        let op = open_screens(&app);
        assert!(find(&op, "ScreenCardView::one-screen-note").is_none());
        assert_eq!(find_all(&op, "ScreenCardView::monitor").len(), 4, "two monitors on each card");
    }

    #[test]
    fn show_opens_on_the_picked_monitor_and_toasts() {
        let (app, d) = with_displays(vec![mon("A", true), mon("B", false), mon("C", false)]);
        let op = open_screens(&app);
        let mons = find_all(&op, "ScreenCardView::monitor");
        assert_eq!(mons[1].accessible_checked(), Some(true), "B: the first non-primary");
        click(&op, "ScreenCardView::monitor", 2);
        assert_eq!(find_all(&op, "ScreenCardView::monitor")[2].accessible_checked(), Some(true));
        click(&op, "ScreenCardView::show-btn", 0);
        assert_eq!(*d.calls.lock().unwrap(), [("room".to_string(), Some("C".to_string()))]);
        assert_eq!(app.data.lock().unwrap().settings.displays.room.as_deref(), Some("C"), "remembered");
        assert_eq!(toasts(&op), ["Room screen opened"]);
    }

    #[test]
    fn turn_off_only_while_window_open() {
        let (app, d) = with_displays(vec![mon("A", true), mon("B", false)]);
        let op = open_screens(&app);
        assert!(find(&op, "ScreenCardView::off-btn").is_none());
        assert_eq!(find(&op, "ScreenCardView::show-btn").unwrap().accessible_label().unwrap_or_default(), "Show on this screen");
        click(&op, "ScreenCardView::show-btn", 1);
        assert_eq!(*d.calls.lock().unwrap(), [("stage".to_string(), Some("B".to_string()))]);
        refresh(&op);
        settle(&op);
        let offs = find_all(&op, "ScreenCardView::off-btn");
        assert_eq!(offs.len(), 1, "only the stage card");
        assert_eq!(find_all(&op, "ScreenCardView::show-btn")[1].accessible_label().unwrap_or_default(), "Move / reopen here");
        click(&op, "ScreenCardView::off-btn", 0);
        assert!(d.open.lock().unwrap().is_empty());
        refresh(&op);
        settle(&op);
        assert!(find(&op, "ScreenCardView::off-btn").is_none());
    }

    #[test]
    fn screens_status_chips() {
        let (app, d) = with_displays(vec![mon("A", true)]);
        let op = open_screens(&app);
        let chips = |op: &Operator| cards(op).iter().map(|c| (c.chip.to_string(), c.chip_state.to_string())).collect::<Vec<_>>();
        assert_eq!(chips(&op), [("Off".into(), "".into()), ("Off".into(), "".into())]);
        d.open.lock().unwrap().push("room".into());
        d.open.lock().unwrap().push("stage".into());
        refresh(&op);
        assert_eq!(chips(&op), [("Starting…".into(), "warn".into()), ("Starting…".into(), "warn".into())], "in `windows`");
        let _room = app.screen_guard("room");
        refresh(&op);
        assert_eq!(chips(&op), [("Showing".into(), "ok".into()), ("Starting…".into(), "warn".into())]);
        let _stage = app.screen_guard("stage");
        refresh(&op);
        assert_eq!(chips(&op)[1], ("Showing".into(), "ok".into()), "the stage uses stage_connected");
        settle(&op);
        let chip = find_all(&op, "ScreenCardView::chip");
        assert_eq!(chip[0].accessible_label().unwrap_or_default(), "Showing");
    }

    #[test]
    fn refresh_requests_a_new_read_with_monitors() {
        let (app, d) = with_displays(vec![mon("A", true)]);
        let op = open_screens(&app);
        assert!(find(&op, "ScreenCardView::monitor").is_none());
        d.monitors.lock().unwrap().push(mon("B", false));
        click(&op, "ScreensDialog::refresh-btn", 0);
        assert_eq!(op.state.borrow().snap.as_ref().unwrap().monitors.len(), 2, "a new read, monitors included");
        assert_eq!(find_all(&op, "ScreenCardView::monitor").len(), 4, "and the dialog was redrawn");
        assert!(cards(&op).iter().all(|c| c.pick == 1), "B is picked");
    }
}
