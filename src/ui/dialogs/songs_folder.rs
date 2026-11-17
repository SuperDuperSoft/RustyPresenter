//! Settings → Songs → "Keep my songs in a folder" (`ui/dialogs/settings-songs-folder.slint`, `crate::songs_folder`).
//! Self-contained: `connect` (once, when the window opens) answers `SongsFolderState`'s callbacks and, once a
//! second, shows the folder's status line and its messages ("“Grace” came in from the songs folder."). `fill` puts
//! the saved choices in the rows (when Settings opens). Each choice saves at once through `songs-folder/set`.
use crate::{
    songs_folder::{self, Status},
    ui::{SongsFolderState, operator::window::Operator},
};
use serde::Deserialize;
use serde_json::{Value, json};
use slint::{ComponentHandle, TimerMode};
use std::{cell::RefCell, rc::Rc, time::{Duration, SystemTime}};

/// How often the status line and the messages are looked at.
const EVERY: Duration = Duration::from_secs(1);

thread_local! {
    /// The window's timer for the status line and the messages.
    static TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

/// What `songs-folder/set` answers: the choices as saved.
#[derive(Deserialize, Debug, Clone, PartialEq)]
struct Saved { on: bool, folder: String, chosen: bool, tidy: bool }

/// The saved choices into the rows, and the status line.
pub fn fill(op: &Operator) {
    let prefs = op.state.borrow().snap.as_ref().map(|s| s.data.settings.songs_folder.clone()).unwrap_or_default();
    show_prefs(op, &Saved { on: prefs.on, folder: prefs.dir().display().to_string(), chosen: prefs.folder.is_some(), tidy: prefs.tidy });
    show_status(op, &songs_folder::status());
}

fn show_prefs(op: &Operator, p: &Saved) {
    let g = op.ui.global::<SongsFolderState>();
    g.set_on(p.on);
    g.set_folder(p.folder.as_str().into());
    g.set_chosen(p.chosen);
    g.set_tidy(p.tidy);
}

/// The line under the choice. A status for another folder (just changed, not looked at yet) is not shown.
fn show_status(op: &Operator, s: &Status) {
    let g = op.ui.global::<SongsFolderState>();
    let current = g.get_on() && s.on && s.folder.display().to_string() == g.get_folder().as_str();
    g.set_status(if current { songs_folder::status_line(s, SystemTime::now()) } else { String::new() }.into());
    g.set_problem(current && s.problem.is_some());
}

/// Saves one choice; the rows show what was saved (or go back, with the reason, when it could not be).
fn save(op: &Rc<Operator>, body: Value) {
    let me = Rc::downgrade(op);
    op.ops().call::<Saved>("songs-folder/set", body, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(saved) => { show_prefs(&op, &saved); show_status(&op, &songs_folder::status()) }
            Err(e) => { op.toast(e, true); fill(&op) }
        }
    });
}

/// Answers the rows' callbacks and starts the status line's timer. Call once when the window opens.
pub fn connect(op: &Rc<Operator>) {
    let g = op.ui.global::<SongsFolderState>();
    let me = Rc::downgrade(op);
    g.on_set_on(move |on| if let Some(op) = me.upgrade() { save(&op, json!({ "on": on })) });
    let me = Rc::downgrade(op);
    g.on_set_tidy(move |tidy| if let Some(op) = me.upgrade() { save(&op, json!({ "tidy": tidy })) });
    let me = Rc::downgrade(op);
    g.on_usual_folder(move || if let Some(op) = me.upgrade() { save(&op, json!({ "usual": true })) });
    let me = Rc::downgrade(op);
    g.on_choose_folder(move || {
        let Some(op) = me.upgrade() else { return };
        let start = std::path::PathBuf::from(op.ui.global::<SongsFolderState>().get_folder().as_str());
        let me = Rc::downgrade(&op);
        crate::ui::operator::files::pick_folder(&op, &start, move |dir| if let Some(op) = me.upgrade() { save(&op, json!({ "folder": dir })) });
    });
    let me = Rc::downgrade(op);
    g.on_open_folder(move || {
        let Some(op) = me.upgrade() else { return };
        let dir = std::path::PathBuf::from(op.ui.global::<SongsFolderState>().get_folder().as_str());
        let opened = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|()| crate::usage::open::open_file(&dir));
        if let Err(e) = opened { op.toast(crate::tr!("Could not open the songs folder: {error}", error = e), true) }
    });
    fill(op);
    let me = Rc::downgrade(op);
    let timer = slint::Timer::default();
    timer.start(TimerMode::Repeated, EVERY, move || if let Some(op) = me.upgrade() { tick(&op) });
    TIMER.with(|t| *t.borrow_mut() = Some(timer));
}

/// Once a second: the status line, and a message for each thing the folder did.
fn tick(op: &Operator) {
    show_status(op, &songs_folder::status());
    for (text, problem) in songs_folder::take_notes() { op.toast(text, problem) }
}

/// The choices as Settings shows them, for tests.
#[cfg(test)]
fn shown(op: &Operator) -> (bool, String, bool, bool) {
    let g = op.ui.global::<SongsFolderState>();
    (g.get_on(), g.get_folder().to_string(), g.get_chosen(), g.get_tidy())
}

#[cfg(test)]
fn usual() -> String { songs_folder::Prefs::default().dir().display().to_string() }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, ui::{dialogs::add::tests::{opened, settle}, operator::files::testing::will_pick}};

    #[test]
    fn the_rows_save_each_choice_at_once() {
        let app = test_app();
        let op = opened(&app);
        connect(&op);
        assert_eq!(shown(&op), (false, usual(), false, false), "off, in Documents → RustyPresenter → Songs");
        let g = op.ui.global::<SongsFolderState>();
        g.invoke_set_on(true);
        settle(&op);
        assert!(app.data.lock().unwrap().settings.songs_folder.on);
        assert!(shown(&op).0);
        // Another folder, then back to the usual one.
        let dir = std::env::temp_dir().join(format!("rp-test-chosen-{}", uuid::Uuid::new_v4()));
        will_pick(vec![dir.clone()]);
        g.invoke_choose_folder();
        settle(&op);
        assert_eq!(app.data.lock().unwrap().settings.songs_folder.folder.as_deref(), Some(dir.as_path()));
        assert_eq!(shown(&op), (true, dir.display().to_string(), true, false));
        g.invoke_set_tidy(true);
        settle(&op);
        assert!(app.data.lock().unwrap().settings.songs_folder.tidy);
        g.invoke_usual_folder();
        settle(&op);
        assert_eq!(app.data.lock().unwrap().settings.songs_folder.folder, None);
        assert_eq!(shown(&op), (true, usual(), false, true));
        // The Settings window's Save keeps what these rows saved.
        let mut form = app.data.lock().unwrap().settings.clone();
        form.songs_folder = songs_folder::Prefs::default();
        tokio::runtime::Runtime::new().unwrap().block_on(app.do_action("settings", serde_json::to_string(&form).unwrap().as_bytes())).unwrap();
        assert!(app.data.lock().unwrap().settings.songs_folder.on, "an older form does not turn it off");
    }

    #[test]
    fn the_status_line_shows_for_the_folder_shown() {
        let app = test_app();
        let op = opened(&app);
        connect(&op);
        let g = op.ui.global::<SongsFolderState>();
        g.set_on(true);
        g.set_folder("C:\\Songs".into());
        show_status(&op, &Status { on: true, folder: "C:\\Songs".into(), songs: 12, ..Default::default() });
        assert_eq!(g.get_status(), "12 songs in the folder");
        assert!(!g.get_problem());
        show_status(&op, &Status { on: true, folder: "C:\\Songs".into(), problem: Some("The songs folder cannot be found. Is the drive connected?".into()), ..Default::default() });
        assert!(g.get_problem() && g.get_status().starts_with("The songs folder cannot be found"));
        show_status(&op, &Status { on: true, folder: "D:\\Other".into(), songs: 3, ..Default::default() });
        assert_eq!(g.get_status(), "", "not the folder shown yet");
    }
}
