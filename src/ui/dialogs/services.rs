//! The Services dialog (`operator.js:776-790`, `ui/dialogs/services.slint`): make an empty service or a copy of
//! this one, rename and delete services, and open another one.
#[path = "../../strings/services.rs"]
pub mod strings;

use super::{DialogKind, ask::notice, confirm, prompt, strings as dtr};
use crate::{
    app::ExportError,
    bundle,
    ui::{ServiceListRow, ServicesDialogState, operator::{file_input::Filter, files, window::Operator}},
};
use serde_json::{Value, json};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::{Rc, Weak};
use uuid::Uuid;

/// "Service 11/2/2026": the default name of a new service or a copy (`operator.js:779-780`). The web's
/// `toLocaleDateString()` gives this on the en-US church PCs.
pub fn dated_name(date: chrono::NaiveDate) -> String { strings::dated_name(&date.format("%-m/%-d/%Y").to_string()) }

fn today_name() -> String { dated_name(chrono::Local::now().date_naive()) }

/// Opens the dialog. It is never redrawn by an update (`DialogKind::refreshes`); Rename and Delete rebuild it.
pub fn open(op: &Rc<Operator>) {
    op.open_dialog(DialogKind::Services);
    draw(op, |_| {});
}

/// Builds the rows from the newest snapshot. `patch` applies a change the core has made but whose update has not
/// arrived yet, so a rebuild after Rename or Delete shows it.
fn draw(op: &Operator, patch: impl FnOnce(&mut Vec<ServiceListRow>)) {
    let mut rows: Vec<ServiceListRow> = op.state.borrow().snap.as_ref().map(|s| s.data.services.iter().map(|svc| ServiceListRow {
        id: svc.id.to_string().into(),
        name: svc.name.as_str().into(),
        count: strings::item_count(svc.items.len()).into(),
        current: s.service_id == Some(svc.id),
    }).collect()).unwrap_or_default();
    patch(&mut rows);
    let g = op.ui.global::<ServicesDialogState>();
    g.set_can_delete(rows.len() > 1);
    g.set_rows(ModelRc::new(VecModel::from(rows)));
}

/// Rebuilds the dialog if it is still the one open.
fn redraw(op: &Operator, patch: impl FnOnce(&mut Vec<ServiceListRow>)) {
    if op.dialog() == Some(DialogKind::Services) { draw(op, patch) }
}

/// New empty service (`:779`), or Copy this service (`:780`): a name prompt, then `service/new`, then close.
/// Cancel does nothing; an empty answer is still sent (the core names it).
fn new_service(op: &Rc<Operator>, copy: bool) {
    let me = Rc::downgrade(op);
    let title = if copy { strings::COPY_PROMPT } else { strings::NEW_PROMPT };
    prompt(op, title, &today_name(), move |name| {
        let (Some(op), Some(name)) = (me.upgrade(), name) else { return };
        let copy_from = copy.then(|| op.state.borrow().snap.as_ref().and_then(|s| s.service_id)).flatten();
        let body = if copy { json!({ "name": name, "copy_from": copy_from }) } else { json!({ "name": name }) };
        let me = Rc::downgrade(&op);
        op.ops().call::<Value>("service/new", body, move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(_) => op.close_dialog(),
                Err(e) => op.toast(&e, true),
            }
        });
    });
}

/// Rename (`:785`): a prompt starting from the name. A non-empty answer is sent as typed (untrimmed). The dialog
/// is rebuilt and stays open, even when the prompt was cancelled.
fn rename(op: &Rc<Operator>, id: Uuid) {
    let Some(current) = op.state.borrow().snap.as_ref().and_then(|s| s.data.services.iter().find(|s| s.id == id).map(|s| s.name.clone())) else { return };
    let me = Rc::downgrade(op);
    prompt(op, strings::RENAME_PROMPT, &current, move |name| {
        let Some(op) = me.upgrade() else { return };
        let Some(name) = name.filter(|n| !n.is_empty()) else { return redraw(&op, |_| {}) };
        let me = Rc::downgrade(&op);
        op.ops().call::<Value>("service/rename", json!({ "id": id, "name": name }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(_) => redraw(&op, |rows| {
                    let key = id.to_string();
                    if let Some(row) = rows.iter_mut().find(|r| r.id == key.as_str()) { row.name = name.as_str().into(); }
                }),
                Err(e) => op.toast(&e, true),
            }
        });
    });
}

/// The trash button (`:786`): confirm, `service/delete`, and the dialog is rebuilt and stays open.
fn delete(op: &Rc<Operator>, id: Uuid) {
    let Some(name) = op.state.borrow().snap.as_ref().and_then(|s| s.data.services.iter().find(|s| s.id == id).map(|s| s.name.clone())) else { return };
    let me = Rc::downgrade(op);
    confirm(op, &strings::delete_question(&name), dtr::OK, dtr::CANCEL, false, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        let me = Rc::downgrade(&op);
        op.ops().call::<Value>("service/delete", json!({ "id": id }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                // As the core does: the open service, if deleted, becomes the first one.
                Ok(_) => redraw(&op, |rows| {
                    let key = id.to_string();
                    let was_current = rows.iter().any(|r| r.id == key.as_str() && r.current);
                    rows.retain(|r| r.id != key.as_str());
                    if was_current && let Some(first) = rows.first_mut() { first.current = true; }
                }),
                Err(e) => op.toast(&e, true),
            }
        });
    });
}

/// Open (`:787`): `service/select`, then close.
fn select(op: &Rc<Operator>, id: Uuid) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("service/select", json!({ "id": id }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => op.close_dialog(),
            Err(e) => op.toast(&e, true),
        }
    });
}

/// The file name a save starts from: the service's name without what Windows refuses in one.
fn file_name(name: &str) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c)).collect();
    let clean = clean.trim().trim_matches('.').trim();
    format!("{}.{}", if clean.is_empty() { "Service" } else { clean }, bundle::EXT)
}

/// Save to file: the open service and everything it needs, for another computer (spec §11). The dialog closes so
/// the job card shows, which can take minutes with video in the service.
fn save_file(op: &Rc<Operator>) {
    let Some((id, name)) = op.state.borrow().snap.as_ref().and_then(|s| s.service_id.and_then(|id| s.data.services.iter().find(|x| x.id == id)).map(|x| (x.id, x.name.clone()))) else { return };
    let me = Rc::downgrade(op);
    files::pick_save(op, Filter::Services, &file_name(&name), move |dest| {
        let Some(op) = me.upgrade() else { return };
        op.close_dialog();
        let me = Rc::downgrade(&op);
        op.ops().export_service(id, dest, move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(()) => op.toast(bundle::strings::saved(&name), false),
                Err(ExportError::Missing(lines)) => notice(&op, &bundle::strings::missing_report(&lines)),
                // The job card shows the error, and a cancel needs no message.
                Err(ExportError::Failed(_)) => {}
            }
        });
    });
}

/// Open from file: adds the service, which becomes the open one, and says what came in. A bad file's error is on
/// its job card.
fn open_file(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    files::pick(op, Filter::Services, false, move |picked| {
        let (Some(op), Some(path)) = (me.upgrade(), picked.into_iter().next()) else { return };
        op.close_dialog();
        let me = Rc::downgrade(&op);
        op.ops().import_service(path, move |r| {
            if let (Some(op), Ok(opened)) = (me.upgrade(), r) { notice(&op, &bundle::strings::opened_report(&opened)) }
        });
    });
}

/// Connects the top bar's Services button and the dialog's buttons. They hold a `Weak`.
pub(crate) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_services(move || if let Some(op) = me.upgrade() { open(&op) });
    let g = op.ui.global::<ServicesDialogState>();
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_new_service(move || if let Some(op) = me.upgrade() { new_service(&op, false) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_copy_service(move || if let Some(op) = me.upgrade() { new_service(&op, true) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_save_file(move || if let Some(op) = me.upgrade() { save_file(&op) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_open_file(move || if let Some(op) = me.upgrade() { open_file(&op) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_rename(move |id| if let (Some(op), Ok(id)) = (me.upgrade(), id.parse()) { rename(&op, id) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_delete(move |id| if let (Some(op), Ok(id)) = (me.upgrade(), id.parse()) { delete(&op, id) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_open(move |id| if let (Some(op), Ok(id)) = (me.upgrade(), id.parse()) { select(&op, id) });
}

/// What the three dialogs' tests share.
#[cfg(test)]
pub(crate) mod testing {
    use crate::{app::{App, LiveUpdate}, ui::operator::window::{Operator, testing::{find, open, refresh}}};
    use slint::{SharedString, platform::{Key, PointerEventButton, WindowEvent}};
    use std::{rc::Rc, sync::Arc};

    pub fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op.apply_live(LiveUpdate::new(op.app().live_view()));
        op
    }

    /// Runs what a frame would: change handlers, timers, and making the overlays.
    pub fn settle(op: &Operator) {
        slint::platform::update_timers_and_animations();
        let _ = find(op, "DialogShell::close-btn");
        let _ = find(op, "AskOverlay::ok");
        slint::platform::update_timers_and_animations();
    }

    pub fn press(op: &Operator, text: impl Into<SharedString>) {
        let text = text.into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text });
        settle(op);
    }

    pub fn key(op: &Operator, k: Key) { press(op, char::from(k)) }

    pub fn typed(op: &Operator, text: &str) { for c in text.chars() { press(op, c) } }

    /// Clicks the `n`th element with this id.
    pub fn click(op: &Operator, id: &str, n: usize) {
        settle(op);
        let all = crate::ui::operator::window::testing::find_all(op, id);
        all.get(n).unwrap_or_else(|| panic!("no {id} #{n} (found {})", all.len())).mock_single_click(PointerEventButton::Left);
        settle(op);
    }

    /// The question on top, if any: (its text, the prompt's starting value).
    pub fn asked(op: &Operator) -> Option<(String, String)> {
        slint::Model::row_data(&op.ui.get_ask(), 0).map(|a| (a.text.to_string(), a.value.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::{app::{App, test_app}, model::{ItemKind, Service, ServiceItem}, ui::operator::window::testing::{find, find_all, refresh}};
    use slint::{Model, platform::Key};
    use std::sync::Arc;

    fn rows(op: &Operator) -> Vec<ServiceListRow> { op.ui.global::<ServicesDialogState>().get_rows().iter().collect() }

    /// A second service, not open; returns its id.
    fn add_service(app: &App, name: &str) -> Uuid {
        let s = Service::new(name);
        let id = s.id;
        app.data.lock().unwrap().services.push(s);
        id
    }

    fn open_services(app: &Arc<App>) -> Rc<Operator> {
        let op = opened(app);
        op.ui.invoke_services();
        settle(&op);
        assert_eq!(op.dialog(), Some(DialogKind::Services));
        assert_eq!(op.ui.get_dialog_title(), "Services");
        op
    }

    fn current(app: &App) -> (Uuid, String) {
        let d = app.data.lock().unwrap();
        let s = d.service().unwrap();
        (s.id, s.name.clone())
    }

    #[test]
    fn new_service_prompt_creates_and_selects() {
        let app = test_app();
        let op = open_services(&app);
        let before = app.data.lock().unwrap().services.len();
        click(&op, "ServicesDialog::new-btn", 0);
        assert_eq!(asked(&op), Some((strings::NEW_PROMPT.into(), today_name())));
        key(&op, Key::Escape);
        assert_eq!(app.data.lock().unwrap().services.len(), before, "Cancel does nothing");
        assert_eq!(op.dialog(), Some(DialogKind::Services));

        click(&op, "ServicesDialog::new-btn", 0);
        typed(&op, "Easter");
        key(&op, Key::Return);
        assert_eq!(app.data.lock().unwrap().services.len(), before + 1);
        let (_, name) = current(&app);
        assert_eq!(name, "Easter", "the new service is the open one");
        assert!(app.data.lock().unwrap().service().unwrap().items.is_empty());
        assert_eq!(op.dialog(), None, "the dialog closes");
    }

    #[test]
    fn copy_service_prompt_defaults_to_dated_name() {
        assert_eq!(dated_name(chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap()), "Service 1/2/2026");
        assert_eq!(dated_name(chrono::NaiveDate::from_ymd_opt(2026, 11, 22).unwrap()), "Service 11/22/2026");
        let app = test_app();
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Hi".into() }, background: None, notes: String::new(), theme: None };
        app.data.lock().unwrap().services[0].items.push(item);
        let (from, _) = current(&app);
        let op = open_services(&app);
        click(&op, "ServicesDialog::copy-btn", 0);
        let today = dated_name(chrono::Local::now().date_naive());
        assert_eq!(asked(&op), Some((strings::COPY_PROMPT.into(), today.clone())));
        key(&op, Key::Return);
        let (id, name) = current(&app);
        assert_ne!(id, from);
        assert_eq!(name, today);
        assert_eq!(app.data.lock().unwrap().service().unwrap().items.len(), 1, "the items were copied");
        assert_eq!(op.dialog(), None);
    }

    #[test]
    fn rename_service_sends_untrimmed_name() {
        let app = test_app();
        let (id, name) = current(&app);
        let op = open_services(&app);
        click(&op, "ServicesDialog::rename-btn", 0);
        assert_eq!(asked(&op), Some((strings::RENAME_PROMPT.into(), name)));
        typed(&op, " Easter ");
        key(&op, Key::Return);
        assert_eq!(current(&app), (id, " Easter ".into()), "sent as typed");
        assert_eq!(op.dialog(), Some(DialogKind::Services), "the dialog stays open");
        assert_eq!(rows(&op)[0].name, " Easter ", "and is rebuilt with the new name");
    }

    #[test]
    fn cancelled_rename_keeps_services_open() {
        let app = test_app();
        let (id, name) = current(&app);
        let op = open_services(&app);
        click(&op, "ServicesDialog::rename-btn", 0);
        typed(&op, "Other");
        key(&op, Key::Escape);
        assert_eq!(current(&app), (id, name.clone()));
        assert_eq!(op.dialog(), Some(DialogKind::Services));
        assert!(op.ui.get_dialog_open());
        // An empty answer renames nothing either (`if (name)`).
        click(&op, "ServicesDialog::rename-btn", 0);
        key(&op, Key::Backspace);
        key(&op, Key::Return);
        assert_eq!(current(&app), (id, name));
        assert_eq!(op.dialog(), Some(DialogKind::Services));
    }

    #[test]
    fn delete_service_keeps_services_open() {
        let app = test_app();
        let other = add_service(&app, "Evening");
        let op = open_services(&app);
        assert_eq!(rows(&op).len(), 2);
        click(&op, "ServicesDialog::delete-btn", 1);
        assert_eq!(asked(&op).map(|a| a.0), Some("Delete the service “Evening”? (Songs and media stay in the library.)".into()));
        key(&op, Key::Escape);
        assert_eq!(app.data.lock().unwrap().services.len(), 2, "Cancel deletes nothing");
        click(&op, "ServicesDialog::delete-btn", 1);
        key(&op, Key::Return);
        assert!(!app.data.lock().unwrap().services.iter().any(|s| s.id == other));
        assert_eq!(op.dialog(), Some(DialogKind::Services), "the dialog stays open");
        assert_eq!(rows(&op).len(), 1, "rebuilt without it");
        assert!(find(&op, "ServicesDialog::delete-btn").is_none(), "one service left: no trash");
    }

    #[test]
    fn delete_open_service_marks_the_first_left_open() {
        let app = test_app();
        let (open_id, _) = current(&app);
        let other = add_service(&app, "Evening");
        let op = open_services(&app);
        assert!(rows(&op)[0].current);
        click(&op, "ServicesDialog::delete-btn", 0);
        key(&op, Key::Return);
        assert!(!app.data.lock().unwrap().services.iter().any(|s| s.id == open_id));
        assert_eq!(current(&app).0, other, "the core opened the first one left");
        // No update redraws this dialog: the patch alone must mark the row.
        let rows = rows(&op);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].current, "the service left is marked open");
        assert!(find(&op, "ServicesDialog::open-chip").is_some(), "it has the Open chip");
        assert!(find(&op, "ServicesDialog::open-btn").is_none(), "and no Open button");
    }

    /// A service saved on this computer, then opened on it as if it were another: the second service is a copy.
    fn saved_file(app: &Arc<App>, op: &Rc<Operator>) -> std::path::PathBuf {
        let dest = app.store.root.join("Sunday.rpservice");
        settle(op);
        crate::ui::operator::files::testing::will_pick(vec![dest.clone()]);
        click(op, "ServicesDialog::save-file-btn", 0);
        dest
    }

    #[test]
    fn save_to_file_writes_the_open_service() {
        let app = test_app();
        app.data.lock().unwrap().services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Hi".into() }, background: None, notes: String::new(), theme: None });
        let op = open_services(&app);
        let dest = saved_file(&app, &op);
        assert!(dest.exists());
        assert!(op.toast_log().iter().any(|(t, err)| t.contains("Sunday Service") && !err), "{:?}", op.toast_log());
        assert_eq!(op.dialog(), None, "the dialog closes so the job card shows");
    }

    #[test]
    fn save_to_file_cancelled_does_nothing() {
        let app = test_app();
        let op = open_services(&app);
        click(&op, "ServicesDialog::save-file-btn", 0);
        assert!(op.toast_log().is_empty());
        assert_eq!(op.dialog(), Some(DialogKind::Services));
        assert!(!app.store.root.join("Sunday.rpservice").exists());
    }

    #[test]
    fn save_with_a_missing_file_says_what_is_missing() {
        let app = test_app();
        let mut d = app.data.lock().unwrap();
        let id = Uuid::new_v4();
        d.services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: id }, background: None, notes: String::new(), theme: None });
        drop(d);
        let op = open_services(&app);
        let dest = saved_file(&app, &op);
        assert!(!dest.exists());
        let ask = op.ui.get_ask().row_data(0).expect("the report is up");
        assert!(ask.text.contains("song"), "{}", ask.text);
    }

    #[test]
    fn open_file_adds_the_service_opens_it_and_reports() {
        let app = test_app();
        let op = open_services(&app);
        let dest = saved_file(&app, &op);
        op.ui.invoke_services();
        crate::ui::operator::files::testing::will_pick(vec![dest]);
        click(&op, "ServicesDialog::open-file-btn", 0);
        assert_eq!(app.data.lock().unwrap().services.len(), 2);
        assert_eq!(current(&app).1, "Sunday Service (2)");
        assert_eq!(op.dialog(), None, "the dialog closes on the new service");
        let ask = op.ui.get_ask().row_data(0).expect("the report is up");
        assert!(ask.text.contains("Sunday Service (2)"), "{}", ask.text);
    }

    #[test]
    fn open_file_that_is_wrong_shows_why() {
        let app = test_app();
        let bad = app.store.root.join("x.rpservice");
        std::fs::write(&bad, b"no").unwrap();
        let op = open_services(&app);
        crate::ui::operator::files::testing::will_pick(vec![bad]);
        click(&op, "ServicesDialog::open-file-btn", 0);
        assert_eq!(app.jobs.lock().unwrap()[0].error.as_deref(), Some(crate::bundle::strings::NOT_A_SERVICE_FILE));
        assert_eq!(op.ui.get_ask().row_count(), 0, "no report");
        assert_eq!(app.data.lock().unwrap().services.len(), 1);
    }

    #[test]
    fn the_four_buttons_fit_inside_the_dialog() {
        let app = test_app();
        let op = open_services(&app);
        for w in [1024.0, 1366.0] {
            op.window().set_size(slint::LogicalSize::new(w, 768.0));
            settle(&op);
            let e = find(&op, "ServicesDialog::open-file-btn").unwrap();
            let close = find(&op, "DialogShell::close-btn").unwrap();
            let (right, edge) = (e.absolute_position().x + e.size().width, close.absolute_position().x + close.size().width);
            eprintln!("width {w}: buttons end at {right}, dialog at {edge}");
            assert!(right <= edge, "at {w}: {right} is past {edge}");
        }
    }

    #[test]
    fn file_names_are_safe_on_windows() {
        assert_eq!(file_name("Easter: 9/10 AM?"), "Easter 910 AM.rpservice");
        assert_eq!(file_name("   "), "Service.rpservice");
        assert_eq!(file_name("..."), "Service.rpservice");
    }

    #[test]
    fn delete_last_service_is_hidden() {
        let app = test_app();
        let op = open_services(&app);
        assert_eq!(rows(&op).len(), 1);
        assert!(find(&op, "ServicesDialog::rename-btn").is_some());
        assert!(find(&op, "ServicesDialog::delete-btn").is_none());
        assert!(find(&op, "ServicesDialog::open-btn").is_none(), "the open service has no Open button");
        assert!(find(&op, "ServicesDialog::open-chip").is_some(), "it has the Open chip");
        assert_eq!(rows(&op)[0].count, "0 items");
        add_service(&app, "Evening");
        refresh(&op);
        op.ui.invoke_services();
        settle(&op);
        assert_eq!(find_all(&op, "ServicesDialog::delete-btn").len(), 2);
    }

    #[test]
    fn open_other_service_selects_it() {
        let app = test_app();
        let other = add_service(&app, "Evening");
        let op = open_services(&app);
        let r = rows(&op);
        assert!(r[0].current && !r[1].current);
        assert_eq!(find_all(&op, "ServicesDialog::open-btn").len(), 1);
        click(&op, "ServicesDialog::open-btn", 0);
        assert_eq!(current(&app).0, other);
        assert_eq!(op.dialog(), None, "the dialog closes");
    }
}
