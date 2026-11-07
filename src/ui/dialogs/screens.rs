//! Screen Configuration (`ui/dialogs/screens.slint`): the Audience and Stage lists, each with a switch, and the picked
//! screen's name, output and size. What is typed is saved shortly after the typing stops.
#[path = "../../strings/screens.rs"]
pub mod strings;

use super::DialogKind;
use crate::{
    app::Monitor,
    model::{Screen, ScreenOutput, ScreenSetup, ScreenTarget},
    platform::monitors::MonitorId,
    ui::{ScreenList, ScreenRow, ScreensDialogState, operator::{view::OperatorSnapshot, window::Operator}},
};
use serde_json::{Value, json};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, VecModel};
use std::{cell::RefCell, rc::{Rc, Weak}, time::Duration};
use uuid::Uuid;

/// How long typing must stop before it is saved: every save writes the whole library.
const SAVE_AFTER: Duration = Duration::from_millis(400);

/// What was typed and not saved yet. While it is there, a redraw leaves the fields alone.
struct Typing { id: Uuid, name: Option<String>, size: Option<(u32, u32)> }

thread_local! {
    static TYPING: RefCell<Option<Typing>> = const { RefCell::new(None) };
    static SAVE: Timer = Timer::default();
}

/// Whether `m` is the saved monitor: exactly, or the same one at a new resolution or place.
pub fn is_monitor(m: &Monitor, saved: &str) -> bool {
    m.id == saved || MonitorId::parse(&m.id).is_some_and(|id| id.matches_saved(saved) || MonitorId::parse(saved).is_some_and(|s| s.name == id.name))
}

/// The Output choices for a screen set to `current`, and the one it is on. A saved monitor that is unplugged stays
/// listed, so looking at the dialog never changes the choice.
pub fn outputs(monitors: &[Monitor], current: &ScreenTarget) -> (Vec<(String, ScreenTarget)>, usize) {
    let mut list = vec![(strings::output_none(), ScreenTarget::None), (strings::output_window(), ScreenTarget::Window)];
    list.extend(monitors.iter().map(|m| (m.label.clone(), ScreenTarget::Monitor(m.id.clone()))));
    let at = match current {
        ScreenTarget::None => Some(0),
        ScreenTarget::Window => Some(1),
        ScreenTarget::Monitor(saved) => monitors.iter().position(|m| is_monitor(m, saved)).map(|i| i + 2),
    };
    let at = at.unwrap_or_else(|| { list.push((strings::output_gone(), current.clone())); list.len() - 1 });
    (list, at)
}

/// `1.78 (16:9)`; empty for a size with no sides.
pub fn aspect(width: u32, height: u32) -> String {
    fn gcd(a: u32, b: u32) -> u32 { if b == 0 { a } else { gcd(b, a % b) } }
    if width == 0 || height == 0 { return String::new() }
    let d = gcd(width, height);
    format!("{:.2} ({}:{})", f64::from(width) / f64::from(height), width / d, height / d)
}

/// The output's size: the monitor's real one while it is connected, else as last saved.
fn real_size(s: &OperatorSnapshot, o: &ScreenOutput) -> (u32, u32) {
    let connected = match &o.target { ScreenTarget::Monitor(saved) => s.monitors.iter().find(|m| is_monitor(m, saved)), _ => None };
    connected.map_or((o.width, o.height), |m| (m.width.max(0) as u32, m.height.max(0) as u32))
}

fn monitor_gone(s: &OperatorSnapshot, target: &ScreenTarget) -> bool {
    matches!(target, ScreenTarget::Monitor(saved) if !s.monitors.iter().any(|m| is_monitor(m, saved)))
}

/// A row's chip, `(text, state)`.
fn status(s: &OperatorSnapshot, setup: &ScreenSetup, group: &str, screen: &Screen) -> (String, &'static str) {
    let target = screen.output().target;
    let open = setup.keyed(group).iter().any(|(key, x)| x.id == screen.id && s.windows.contains(key));
    if open { (strings::showing(), "ok") }
    else if target == ScreenTarget::None { (strings::no_output(), "") }
    else if monitor_gone(s, &target) { (strings::unplugged(), "warn") }
    else { (strings::off(), "") }
}

fn list(s: &OperatorSnapshot, setup: &ScreenSetup, key: &str, title: &str) -> ScreenList {
    let group = setup.group(key).cloned().unwrap_or_default();
    let rows: Vec<ScreenRow> = group.screens.iter().map(|screen| {
        let (status_text, state) = status(s, setup, key, screen);
        let o = screen.output();
        let (w, h) = real_size(s, &o);
        let size = if o.target == ScreenTarget::None { String::new() } else { format!("{w} × {h}") };
        ScreenRow { id: screen.id.to_string().into(), name: typed_name(screen).into(), size: size.into(), status: status_text.into(), state: state.into() }
    }).collect();
    ScreenList { key: key.into(), title: title.into(), on: group.on, rows: ModelRc::new(VecModel::from(rows)) }
}

/// A screen's name, as typed if that is not saved yet.
fn typed_name(screen: &Screen) -> String {
    TYPING.with_borrow(|t| t.as_ref().filter(|t| t.id == screen.id).and_then(|t| t.name.clone())).unwrap_or_else(|| screen.name.clone())
}

/// What a screen's note says: the first audience screen is the one with sound; the screen you work on gets covered.
fn note(s: &OperatorSnapshot, setup: &ScreenSetup, group: &str, screen: &Screen) -> String {
    let keyed = setup.keyed("audience");
    if group == "audience" && let Some((_, first)) = keyed.first().filter(|_| keyed.iter().skip(1).any(|(_, x)| x.id == screen.id)) {
        return strings::mirrors(&first.name)
    }
    let main = s.monitors.iter().find(|m| m.primary);
    if let ScreenTarget::Monitor(saved) = screen.output().target && main.is_some_and(|m| is_monitor(m, &saved)) { return strings::on_main().into() }
    String::new()
}

/// Opens the dialog and asks for a fresh read, so newly plugged-in monitors show; the dialog is redrawn when it
/// arrives (and after every `Data` update, `DialogKind::refreshes`).
pub fn open(op: &Rc<Operator>) {
    op.open_dialog(DialogKind::Screens);
    op.on_dialog_refresh(draw);
    op.on_dialog_close(save_typing);
    draw(op);
    op.request_refresh();
}

/// The screen on the right: the one picked, else the first there is.
fn shown(op: &Operator, setup: &ScreenSetup) -> Option<Uuid> {
    let picked = Uuid::parse_str(&op.ui.global::<ScreensDialogState>().get_selected()).ok();
    picked.filter(|id| setup.find(*id).is_some()).or_else(|| ScreenSetup::GROUPS.iter().filter_map(|g| setup.group(g)?.screens.first()).map(|s| s.id).next())
}

/// Draws the lists and the settings from the newest snapshot.
fn draw(op: &Rc<Operator>) {
    let Some(s) = op.state.borrow().snap.clone() else { return };
    let setup = &s.settings.screens;
    let g = op.ui.global::<ScreensDialogState>();
    g.set_lists(ModelRc::new(VecModel::from(vec![list(&s, setup, "audience", &strings::audience()), list(&s, setup, "stage", &strings::stage())])));
    g.set_one_monitor(s.monitors.len() <= 1);
    let Some(id) = shown(op, setup) else { return };
    let Some((group, screen)) = setup.find(id) else { return };
    let moved = g.get_selected() != id.to_string();
    g.set_selected(id.to_string().into());
    g.set_kind(if group == "audience" { strings::audience_screen() } else { strings::stage_screen() }.into());
    let o = screen.output();
    let (choices, at) = outputs(&s.monitors, &o.target);
    let labels: Vec<SharedString> = choices.into_iter().map(|(l, _)| l.into()).collect();
    g.set_outputs(ModelRc::new(VecModel::from(labels)));
    g.set_output(at as i32);
    let typing = TYPING.with_borrow(|t| t.as_ref().is_some_and(|t| t.id == id));
    if moved || !typing {
        g.set_name(screen.name.as_str().into());
        g.set_width_text(o.width.to_string().into());
        g.set_height_text(o.height.to_string().into());
    }
    g.set_has_size(o.target != ScreenTarget::None);
    g.set_size_editable(o.target == ScreenTarget::Window);
    let (w, h) = real_size(&s, &o);
    g.set_size(format!("{w} × {h}").into());
    g.set_aspect(aspect(w, h).into());
    g.set_note(note(&s, setup, group, screen).into());
    g.set_can_remove(setup.group(group).is_some_and(|x| x.screens.len() > 1));
}

/// Runs a change, and tells the operator when it fails.
fn run(op: &Rc<Operator>, action: &'static str, body: Value) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>(action, body, move |r| if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(&e, true) });
}

/// Saves what was typed.
fn save_typing(op: &Rc<Operator>) {
    SAVE.with(Timer::stop);
    let Some(t) = TYPING.take() else { return };
    run(op, "screens/update", json!({ "id": t.id, "name": t.name, "size": t.size }));
}

/// Notes what was typed, and saves it once the typing stops.
fn typed(op: &Rc<Operator>, change: impl FnOnce(&mut Typing)) {
    let Ok(id) = Uuid::parse_str(&op.ui.global::<ScreensDialogState>().get_selected()) else { return };
    TYPING.with_borrow_mut(|t| {
        let t = t.get_or_insert(Typing { id, name: None, size: None });
        change(t);
    });
    let me = Rc::downgrade(op);
    SAVE.with(|timer| timer.start(slint::TimerMode::SingleShot, SAVE_AFTER, move || if let Some(op) = me.upgrade() { save_typing(&op) }));
}

fn select(op: &Rc<Operator>, id: &str) {
    save_typing(op);
    let g = op.ui.global::<ScreensDialogState>();
    if g.get_selected() == id { return }
    g.set_selected(id.into());
    // `draw` tells a new pick by the selected id, which is already this one: fill the fields here.
    if let Some(screen) = op.state.borrow().snap.as_ref().and_then(|s| s.settings.screens.find(Uuid::parse_str(id).ok()?).map(|(_, x)| x.clone())) {
        let o = screen.output();
        g.set_name(screen.name.as_str().into());
        g.set_width_text(o.width.to_string().into());
        g.set_height_text(o.height.to_string().into());
    }
    draw(op);
}

fn pick_output(op: &Rc<Operator>, at: usize) {
    save_typing(op);
    let g = op.ui.global::<ScreensDialogState>();
    let Some(s) = op.state.borrow().snap.clone() else { return };
    let Some(id) = shown(op, &s.settings.screens) else { return };
    let Some((_, screen)) = s.settings.screens.find(id) else { return };
    let Some((_, target)) = outputs(&s.monitors, &screen.output().target).0.into_iter().nth(at) else { return };
    g.set_selected(id.to_string().into());
    run(op, "screens/update", json!({ "id": id, "target": target }));
}

fn add(op: &Rc<Operator>, group: String) {
    save_typing(op);
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("screens/add", json!({ "group": group }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) => {
                let id = v["id"].as_str().unwrap_or_default().to_string();
                op.on_next_read(move |op| select(op, &id));
            }
            Err(e) => op.toast(&e, true),
        }
    });
}

fn remove(op: &Rc<Operator>) {
    save_typing(op);
    let Some(s) = op.state.borrow().snap.clone() else { return };
    let Some(id) = shown(op, &s.settings.screens) else { return };
    let Some((_, screen)) = s.settings.screens.find(id) else { return };
    let me = Rc::downgrade(op);
    super::confirm(op, &strings::remove_question(&screen.name), &strings::remove(), &super::strings::cancel(), true, move |yes| {
        if let (true, Some(op)) = (yes, me.upgrade()) { run(&op, "screens/remove", json!({ "id": id })) }
    });
}

/// Connects the top bar's Screens button and the dialog. They hold a `Weak`.
pub(crate) fn wire(op: &Rc<Operator>) {
    let on = |f: Rc<dyn Fn(&Rc<Operator>)>| -> Box<dyn Fn()> {
        let me: Weak<Operator> = Rc::downgrade(op);
        Box::new(move || if let Some(op) = me.upgrade() { f(&op) })
    };
    op.ui.on_screens(on(Rc::new(open)));
    let g = op.ui.global::<ScreensDialogState>();
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_power(move |group, on| if let Some(op) = me.upgrade() { run(&op, "screens/power", json!({ "group": group.as_str(), "on": on })) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_add(move |group| if let Some(op) = me.upgrade() { add(&op, group.into()) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_select(move |id| if let Some(op) = me.upgrade() { select(&op, &id) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_rename(move |name| if let Some(op) = me.upgrade() {
        typed(&op, |t| t.name = Some(name.to_string()));
        draw(&op);
    });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_pick_output(move |at| if let Some(op) = me.upgrade() { pick_output(&op, at.max(0) as usize) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_resize(move || if let Some(op) = me.upgrade() {
        let g = op.ui.global::<ScreensDialogState>();
        // Half-typed numbers wait: 19 on the way to 1920 is not a size to save.
        if let (Ok(w @ 320..=7680), Ok(h @ 240..=4320)) = (g.get_width_text().parse::<u32>(), g.get_height_text().parse::<u32>()) {
            typed(&op, |t| t.size = Some((w, h)));
        }
    });
    g.on_remove(on(Rc::new(remove)));
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_identify(move || if let Some(op) = me.upgrade() { run(&op, "screens/identify", json!({})) });
    // Refresh: the follower re-reads everything, monitors included, and the snapshot's arrival redraws the dialog.
    // `App::monitors()` is never called on the Slint thread.
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
    struct FakeDisplays {
        monitors: Mutex<Vec<Monitor>>,
        open: Mutex<Vec<String>>,
        calls: Mutex<Vec<(String, Option<String>)>>,
        sized: Mutex<Vec<(String, (u32, u32))>>,
        marks: Mutex<Vec<(String, String, String)>>,
    }

    impl Screens for FakeDisplays {
        fn open(&self, role: &str, monitor: Option<&str>) -> Result<(), String> {
            self.calls.lock().unwrap().push((role.into(), monitor.map(str::to_string)));
            let mut open = self.open.lock().unwrap();
            if !open.iter().any(|r| r == role) { open.push(role.into()); }
            Ok(())
        }
        fn open_window(&self, role: &str, size: (u32, u32)) -> Result<(), String> {
            self.sized.lock().unwrap().push((role.into(), size));
            self.open(role, None)
        }
        fn close(&self, role: &str) { self.open.lock().unwrap().retain(|r| r != role); }
        fn open_roles(&self) -> Vec<String> { self.open.lock().unwrap().clone() }
        fn monitors(&self) -> Vec<Monitor> { self.monitors.lock().unwrap().clone() }
        fn identify(&self, marks: Vec<(String, String, String)>) { *self.marks.lock().unwrap() = marks; }
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
        assert_eq!(op.ui.get_dialog_title(), "Screen Configuration");
        op
    }

    fn state(op: &Operator) -> ScreensDialogState<'_> { op.ui.global::<ScreensDialogState>() }

    fn rows(op: &Operator, list: usize) -> Vec<ScreenRow> {
        state(op).get_lists().row_data(list).unwrap().rows.iter().collect()
    }

    /// Runs an action at once (the test `Ops` are inline).
    fn act(op: &Operator, action: &'static str, body: Value) { op.ops().call::<Value>(action, body, |r| { r.unwrap(); }); }

    fn update(op: &Rc<Operator>, body: Value) {
        act(op, "screens/update", body);
        refresh(op);
        settle(op);
    }

    fn toasts(op: &Operator) -> Vec<String> { op.toast_log().into_iter().map(|t| t.0).collect() }

    #[test]
    fn opens_with_audience_and_stage_lists() {
        let (app, _d) = with_displays(vec![mon("A", true)]);
        let op = open_screens(&app);
        let lists: Vec<ScreenList> = state(&op).get_lists().iter().collect();
        assert_eq!(lists.iter().map(|l| l.title.to_string()).collect::<Vec<_>>(), ["Audience", "Stage"]);
        assert!(lists.iter().all(|l| !l.on));
        assert_eq!(rows(&op, 0)[0].name, "Audience");
        assert_eq!(rows(&op, 1)[0].name, "Stage Display");
        assert_eq!(state(&op).get_name(), "Audience", "the first screen is picked");
        assert_eq!(state(&op).get_output(), 0, "and goes nowhere yet");
        assert!(!state(&op).get_can_remove(), "a list keeps one screen");
        assert!(find(&op, "ScreensDialog::one-screen-note").is_some(), "only one monitor");
    }

    #[test]
    fn the_switch_turns_on_and_takes_the_free_monitor() {
        let (app, d) = with_displays(vec![mon("A", true), mon("B", false)]);
        let op = open_screens(&app);
        click(&op, "ScreenListView::power", 0);
        refresh(&op);
        settle(&op);
        assert_eq!(*d.calls.lock().unwrap(), [("room".to_string(), Some("B".to_string()))]);
        assert!(app.data.lock().unwrap().settings.screens.audience.on, "remembered");
        assert_eq!(rows(&op, 0)[0].status, "Showing");
        assert_eq!(rows(&op, 0)[0].size, "1920 × 1080");
        click(&op, "ScreenListView::power", 0);
        refresh(&op);
        settle(&op);
        assert!(d.open.lock().unwrap().is_empty(), "off closes it");
        assert_eq!(rows(&op, 0)[0].status, "Off");
    }

    #[test]
    fn the_switch_needs_a_screen_to_show_on() {
        let (app, d) = with_displays(vec![mon("A", true)]);
        let op = open_screens(&app);
        click(&op, "ScreenListView::power", 1);
        refresh(&op);
        settle(&op);
        assert!(d.calls.lock().unwrap().is_empty());
        assert!(!app.data.lock().unwrap().settings.screens.stage.on);
        assert_eq!(toasts(&op), ["Pick an output for this screen first."]);
    }

    #[test]
    fn output_choice_is_saved_and_moves_the_open_window() {
        let (app, d) = with_displays(vec![mon("A", true), mon("B", false), mon("C", false)]);
        let op = open_screens(&app);
        let id = rows(&op, 0)[0].id.to_string();
        state(&op).invoke_pick_output(3); // None, window, A, B
        refresh(&op);
        settle(&op);
        assert_eq!(state(&op).get_output(), 3);
        assert!(d.calls.lock().unwrap().is_empty(), "the switch is off, so nothing opens");
        act(&op, "screens/power", json!({ "group": "audience", "on": true }));
        d.calls.lock().unwrap().clear();
        act(&op, "screens/update", json!({ "id": id, "name": "Projector" }));
        assert!(d.calls.lock().unwrap().is_empty(), "a new name does not blink the room");
        update(&op, json!({ "id": id, "target": { "kind": "monitor", "monitor": "C" } }));
        assert_eq!(*d.calls.lock().unwrap(), [("room".to_string(), Some("C".to_string()))]);
    }

    #[test]
    fn a_monitor_shows_one_screen() {
        let (app, _d) = with_displays(vec![mon("A", true), mon("B", false)]);
        let op = open_screens(&app);
        let (first, second) = (rows(&op, 0)[0].id.to_string(), rows(&op, 1)[0].id.to_string());
        update(&op, json!({ "id": first, "target": { "kind": "monitor", "monitor": "B" } }));
        update(&op, json!({ "id": second, "target": { "kind": "monitor", "monitor": "B" } }));
        let s = app.data.lock().unwrap().settings.screens.clone();
        assert_eq!(s.monitors("audience"), Vec::<&str>::new());
        assert_eq!(s.monitors("stage"), ["B"]);
    }

    #[test]
    fn extra_audience_screens_get_their_own_window() {
        let (app, d) = with_displays(vec![mon("A", true), mon("B", false), mon("C", false)]);
        let op = open_screens(&app);
        click(&op, "ScreenListView::add-btn", 0);
        settle(&op);
        refresh(&op);
        settle(&op);
        let rows0 = rows(&op, 0);
        assert_eq!(rows0.iter().map(|r| r.name.to_string()).collect::<Vec<_>>(), ["Audience", "Audience 2"]);
        assert_eq!(state(&op).get_selected(), rows0[1].id, "the new screen is picked");
        assert!(state(&op).get_can_remove());
        update(&op, json!({ "id": rows0[0].id.as_str(), "target": { "kind": "monitor", "monitor": "B" } }));
        update(&op, json!({ "id": rows0[1].id.as_str(), "target": { "kind": "monitor", "monitor": "C" } }));
        assert!(state(&op).get_note().contains("Sound only plays from Audience"));
        act(&op, "screens/power", json!({ "group": "audience", "on": true }));
        let extra = format!("room:{}", rows0[1].id);
        assert_eq!(*d.calls.lock().unwrap(), [("room".to_string(), Some("B".to_string())), (extra.clone(), Some("C".to_string()))]);
        refresh(&op);
        settle(&op);
        assert_eq!(rows(&op, 0)[1].status, "Showing");
        // Removing the first makes the other one the room, which has the sound.
        act(&op, "screens/remove", json!({ "id": rows0[0].id.as_str() }));
        assert_eq!(*d.open.lock().unwrap(), ["room"]);
        assert_eq!(d.calls.lock().unwrap().last().unwrap(), &("room".to_string(), Some("C".to_string())));
    }

    #[test]
    fn remove_asks_first() {
        let (app, _d) = with_displays(vec![mon("A", true)]);
        let op = open_screens(&app);
        click(&op, "ScreenListView::add-btn", 1);
        settle(&op);
        refresh(&op);
        settle(&op);
        click(&op, "ScreensDialog::remove-btn", 0);
        assert_eq!(asked(&op).unwrap().0, "Remove “Stage Display 2”?");
        assert_eq!(app.data.lock().unwrap().settings.screens.stage.screens.len(), 2, "not yet");
        click(&op, "AskOverlay::ok", 0);
        assert_eq!(app.data.lock().unwrap().settings.screens.stage.screens.len(), 1);
    }

    #[test]
    fn typing_a_name_saves_when_the_dialog_closes() {
        let (app, _d) = with_displays(vec![mon("A", true)]);
        let op = open_screens(&app);
        state(&op).invoke_rename("Lobby TV".into());
        assert_eq!(rows(&op, 0)[0].name, "Lobby TV", "the list follows the typing");
        assert_eq!(app.data.lock().unwrap().settings.screens.audience.screens[0].name, "Audience", "not saved on each key");
        op.close_dialog();
        settle(&op);
        assert_eq!(app.data.lock().unwrap().settings.screens.audience.screens[0].name, "Lobby TV");
    }

    #[test]
    fn a_window_opens_at_its_size_and_half_typed_numbers_wait() {
        let (app, d) = with_displays(vec![mon("A", true), mon("B", false)]);
        let op = open_screens(&app);
        state(&op).invoke_pick_output(1);
        refresh(&op);
        settle(&op);
        assert!(state(&op).get_size_editable(), "a window's size can be typed");
        state(&op).set_width_text("19".into());
        state(&op).invoke_resize();
        assert!(TYPING.with_borrow(Option::is_none), "19 is on its way to something");
        state(&op).set_width_text("1280".into());
        state(&op).set_height_text("720".into());
        state(&op).invoke_resize();
        op.close_dialog();
        settle(&op);
        act(&op, "screens/power", json!({ "group": "audience", "on": true }));
        assert_eq!(*d.sized.lock().unwrap(), [("room".to_string(), (1280, 720))]);
        let op = open_screens(&app);
        assert_eq!(state(&op).get_aspect(), "1.78 (16:9)");
        state(&op).invoke_pick_output(3);
        refresh(&op);
        settle(&op);
        assert!(!state(&op).get_size_editable(), "a monitor's size is the monitor's");
        assert_eq!(state(&op).get_size(), "1920 × 1080");
    }

    #[test]
    fn an_unplugged_monitor_stays_chosen() {
        let (app, _d) = with_displays(vec![mon("A", true)]);
        app.data.lock().unwrap().settings.screens.assign("audience", ScreenTarget::Monitor("gone".into()));
        let op = open_screens(&app);
        assert_eq!(rows(&op, 0)[0].status, "Unplugged");
        assert_eq!(state(&op).get_output(), 3, "None, window, A, then the missing one");
        assert_eq!(state(&op).get_outputs().row_data(3).unwrap(), "Not connected right now");
    }

    #[test]
    fn a_connected_monitor_shows_its_real_size() {
        let (app, _d) = with_displays(vec![mon("A", true), Monitor { width: 1280, height: 720, ..mon("B", false) }]);
        app.data.lock().unwrap().settings.screens.assign("audience", ScreenTarget::Monitor("B".into())); // saved as 1920 × 1080
        let op = open_screens(&app);
        assert_eq!(rows(&op, 0)[0].size, "1280 × 720");
        assert_eq!(state(&op).get_size(), "1280 × 720");
    }

    #[test]
    fn identify_marks_every_monitor() {
        let (app, d) = with_displays(vec![mon("A", true), mon("B", false), mon("C", false)]);
        let op = open_screens(&app);
        let id = rows(&op, 1)[0].id.to_string();
        update(&op, json!({ "id": id, "target": { "kind": "monitor", "monitor": "C" } }));
        click(&op, "ScreensDialog::identify-btn", 0);
        assert_eq!(*d.marks.lock().unwrap(), [
            ("A".to_string(), "1".to_string(), "This computer".to_string()),
            ("B".to_string(), "2".to_string(), "Not used".to_string()),
            ("C".to_string(), "3".to_string(), "Stage Display".to_string()),
        ]);
    }

    #[test]
    fn refresh_requests_a_new_read_with_monitors() {
        let (app, d) = with_displays(vec![mon("A", true)]);
        let op = open_screens(&app);
        assert_eq!(state(&op).get_outputs().row_count(), 3);
        d.monitors.lock().unwrap().push(mon("B", false));
        click(&op, "ScreensDialog::refresh-btn", 0);
        assert_eq!(op.state.borrow().snap.as_ref().unwrap().monitors.len(), 2, "a new read, monitors included");
        assert_eq!(state(&op).get_outputs().row_count(), 4, "and the dialog was redrawn");
        assert!(find_all(&op, "ScreensDialog::one-screen-note").is_empty());
    }

    #[test]
    fn output_list_and_aspect() {
        let ms = [mon("A", true)];
        let (list, at) = outputs(&ms, &ScreenTarget::Window);
        assert_eq!((list.len(), at), (3, 1));
        let (list, at) = outputs(&ms, &ScreenTarget::Monitor("A".into()));
        assert_eq!((list.len(), at), (3, 2));
        assert_eq!(aspect(1920, 1080), "1.78 (16:9)");
        assert_eq!(aspect(1280, 800), "1.60 (8:5)");
        assert_eq!(aspect(0, 1080), "");
    }

    #[test]
    fn a_monitor_at_a_new_resolution_is_the_same_monitor() {
        let now = Monitor { id: r"\\.\DISPLAY2|1280×720|1920,0".into(), ..mon("x", false) };
        assert!(is_monitor(&now, r"\\.\DISPLAY2|1920×1080|1920,0"), "same name");
        assert!(is_monitor(&now, r"\\.\DISPLAY2"), "an older bare name");
        assert!(!is_monitor(&now, r"\\.\DISPLAY3|1280×720|1920,0"));
    }
}
