//! Help extras (N35) and the menu bar's shortcuts (N28): Find my mouse brings the pointer to the middle of this
//! window and rings it; Tutorials writes short how-tos to a page and opens it in the browser; Report a problem
//! writes a text file that says what to send and where the logs are (there is no network to send it with).
use super::window::Operator;
use crate::{
    print::html::escape,
    shortcuts::{Action, Chord, ClearGroup, EditKey, Keymap, labels},
    ui::{FindMouse, MenuGroup},
};
use slint::{ComponentHandle, Keys, Model, ModelRc, VecModel};
use std::{cell::Cell, rc::{Rc, Weak}, time::Duration};

/// How long the rings stay up.
const RINGS_FOR: Duration = Duration::from_millis(1600);

thread_local! {
    /// Each Find my mouse press; a timer that belongs to an older one does nothing.
    static PRESS: Cell<u64> = const { Cell::new(0) };
}

/// Sets each menu item's shortcut from the church's keys, or none while Settings → Help waits for a new key.
pub(crate) fn render_keys(op: &Operator, map: &Keymap, groups: &[ClearGroup]) {
    let off = crate::ui::dialogs::settings_keys::capturing();
    let key = |a: Action| -> Keys {
        if off { return Keys::default() }
        map.menu_key(a).and_then(|c| Keys::from_parts(c.slint_parts().iter().map(String::as_str)).ok()).unwrap_or_default()
    };
    let g = op.ui.menu_keys_all();
    g.set_undo(key(Action::Undo));
    g.set_redo(key(Action::Redo));
    g.set_find(key(Action::Find));
    g.set_bible(key(Action::FocusBible));
    g.set_clear_all(key(Action::ClearAll));
    g.set_clear_slide(key(Action::ClearSlide));
    g.set_clear_media(key(Action::ClearMedia));
    g.set_clear_audio(key(Action::ClearAudio));
    g.set_logo(key(Action::Logo));
    g.set_clear_overlays(key(Action::ClearOverlays));
    g.set_clear_message(key(Action::ClearMessage));
    g.set_clear_timers(key(Action::ClearTimers));
    g.set_find_mouse(key(Action::FindMouse));
    g.set_paste(key(Action::PasteLink));
    let edit = |e: EditKey| -> Keys {
        if off { return Keys::default() }
        e.menu_key(map).and_then(|c| Keys::from_parts(c.slint_parts().iter().map(String::as_str)).ok()).unwrap_or_default()
    };
    g.set_delete(edit(EditKey::Delete));
    g.set_cut(edit(EditKey::Cut));
    g.set_copy(edit(EditKey::Copy));
    g.set_duplicate(edit(EditKey::Duplicate));
    g.set_move_up(edit(EditKey::MoveUp));
    g.set_move_down(edit(EditKey::MoveDown));
    g.set_new_service(edit(EditKey::NewService));
    g.set_open_file(edit(EditKey::OpenService));
    g.set_print(edit(EditKey::Print));
    g.set_settings(edit(EditKey::Settings));
    let rows: Vec<MenuGroup> = groups.iter().map(|gr| MenuGroup { name: gr.name.as_str().into(), keys: key(Action::Group(gr.id)) }).collect();
    if g.get_groups().iter().ne(rows.iter().cloned()) { g.set_groups(ModelRc::new(VecModel::from(rows))) }
    g.set_capturing(off);
    let label = |a: Action| -> slint::SharedString { map.keys(a).first().map(Chord::label).unwrap_or_default().into() };
    g.set_clear_all_label(label(Action::ClearAll));
    g.set_clear_slide_label(label(Action::ClearSlide));
    g.set_clear_media_label(label(Action::ClearMedia));
    g.set_clear_audio_label(label(Action::ClearAudio));
    g.set_logo_label(label(Action::Logo));
}

/// The menu bar again from the newest snapshot (after a key change starts or ends).
pub(crate) fn rerender_keys(op: &Operator) {
    let groups = op.state.borrow().snap.as_ref().map(|s| s.settings.keys.clear_groups.clone()).unwrap_or_default();
    render_keys(op, &op.keymap(), &groups);
}

impl Operator {
    /// Help → Find my mouse (Alt+M): the pointer comes to the middle of this window, wherever it was (on the
    /// projector, say), and two rings close in on it.
    pub fn find_mouse(self: &Rc<Self>) {
        let window = self.window();
        let size = window.size().to_logical(window.scale_factor());
        let body = self.ui.get_content_height().min(size.height);
        let top = size.height - body;
        let (x, y) = (size.width / 2.0, body / 2.0);
        let moved = warp(window, x, top + y);
        let g = self.ui.global::<FindMouse>();
        g.set_x(x);
        g.set_y(y);
        g.set_on(false);
        let press = PRESS.with(|p| { p.set(p.get() + 1); p.get() });
        let me: Weak<Operator> = Rc::downgrade(self);
        // Off for a moment first, so a second press starts the rings again.
        slint::Timer::single_shot(Duration::from_millis(30), move || {
            let Some(op) = me.upgrade() else { return };
            op.ui.global::<FindMouse>().set_on(true);
            let me = Rc::downgrade(&op);
            slint::Timer::single_shot(RINGS_FOR, move || {
                if PRESS.with(Cell::get) != press { return }
                if let Some(op) = me.upgrade() { op.ui.global::<FindMouse>().set_on(false) }
            });
        });
        if !moved { self.toast(tr::cant_move_pointer(), false) }
    }

    /// Help → Tutorials: short how-tos for volunteers, with this church's keys, in the browser.
    pub(crate) fn tutorials(&self) {
        let root = self.app().store.root.clone();
        let snap = self.state.borrow().snap.clone();
        let keys = snap.as_ref().map(|s| s.settings.keys.clone()).unwrap_or_default();
        match crate::report_page::write_and_open(&root, "tutorials", &tutorials_page(&keys)) {
            Ok((_, true)) => {}
            Ok((path, false)) => self.toast(tr::page_not_opened(&path.display().to_string()), true),
            Err(e) => self.toast(e, true),
        }
    }

    /// Help → Report a problem…: a text file in the logs folder, opened in the computer's text editor.
    pub(crate) fn report_problem(&self) {
        let logs = self.app().store.root.join("logs");
        let path = logs.join(REPORT_FILE);
        let written = std::fs::create_dir_all(&logs).map_err(|e| e.to_string())
            .and_then(|()| crate::report_page::write_to(&path, &report_text(&logs)));
        match written {
            Ok(()) if crate::report_page::open(&path) => {}
            Ok(()) => self.toast(tr::page_not_opened(&path.display().to_string()), true),
            Err(e) => self.toast(e, true),
        }
    }
}

/// The Report a problem file's name, in the logs folder.
pub const REPORT_FILE: &str = "report-a-problem.txt";

/// Moves the pointer to `(x, y)` in the window (logical pixels). False where the system does not allow it (Wayland)
/// and on the testing backend.
fn warp(window: &slint::Window, x: f32, y: f32) -> bool {
    use slint::winit_030::{WinitWindowAccessor, winit::dpi::LogicalPosition};
    window.with_winit_window(|w| {
        w.focus_window();
        w.set_cursor_position(LogicalPosition::new(f64::from(x), f64::from(y))).is_ok()
    }).unwrap_or(false)
}

/// The text of Report a problem: what to write, where the logs are, and this copy's version and system.
pub fn report_text(logs: &std::path::Path) -> String {
    let lines = [
        tr::report_title(),
        String::new(),
        tr::report_what(),
        tr::report_steps(),
        String::new(),
        tr::report_logs(),
        format!("    {}", logs.display()),
        String::new(),
        tr::report_send(),
        String::new(),
        format!("RustyPresenter {} — {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH),
        String::new(),
        tr::report_write_here(),
        String::new(),
    ];
    // Windows' Notepad reads either; CRLF keeps older ones happy.
    let nl = if cfg!(windows) { "\r\n" } else { "\n" };
    lines.join(nl)
}

/// The Tutorials page: a few short how-tos, the keys as this church set them.
pub fn tutorials_page(keys: &crate::shortcuts::KeySettings) -> String {
    let map = Keymap::new(keys);
    let k = |a: Action| { let l = labels(map.keys(a)); if l.is_empty() { tr::no_key() } else { l } };
    let mut clears: Vec<(String, String)> = [Action::ClearAll, Action::ClearSlide, Action::ClearMedia, Action::ClearAudio, Action::Logo,
        Action::ClearOverlays, Action::ClearMessage, Action::ClearTimers].into_iter()
        .map(|a| (k(a), crate::ui::dialogs::settings_keys::what(a, &keys.clear_groups))).collect();
    clears.extend(keys.clear_groups.iter().map(|g| (k(Action::Group(g.id)), crate::shortcuts::strings::clear_group(&g.name))));
    let sections: Vec<(String, Vec<String>)> = vec![
        (tr::t_sunday(), vec![tr::t_sunday_1(), tr::t_sunday_2(), tr::t_sunday_3()]),
        (tr::t_show(), vec![tr::t_show_1(), tr::t_show_2(&k(Action::Next), &k(Action::Back)), tr::t_show_3()]),
        (tr::t_bible(), vec![tr::t_bible_1(&k(Action::FocusBible)), tr::t_bible_2()]),
        (tr::t_songs(), vec![tr::t_songs_1(), tr::t_songs_2()]),
        (tr::t_help(), vec![tr::t_help_1(&k(Action::FindMouse)), tr::t_help_2(), tr::t_help_3()]),
    ];
    let mut html = format!("<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{t}</title>\n<style>{STYLE}</style></head><body>\n<h1>{t}</h1>\n",
        t = escape(&tr::tutorials_title()));
    for (i, (title, steps)) in sections.iter().enumerate() {
        html.push_str(&format!("<h2>{}</h2>\n<ol>\n", escape(title)));
        for s in steps { html.push_str(&format!("<li>{}</li>\n", escape(s))) }
        html.push_str("</ol>\n");
        // The clear keys go after "Put things on the screen".
        if i == 1 {
            html.push_str(&format!("<h2>{}</h2>\n<table>\n", escape(&tr::t_clear())));
            for (key, what) in &clears { html.push_str(&format!("<tr><td><kbd>{}</kbd></td><td>{}</td></tr>\n", escape(key), escape(what))) }
            html.push_str(&format!("</table>\n<p>{}</p>\n", escape(&tr::t_clear_change())));
        }
    }
    html.push_str("</body></html>\n");
    html
}

const STYLE: &str = "body{font-family:system-ui,sans-serif;max-width:46em;margin:2em auto;padding:0 1em;line-height:1.5;color:#222}\
h1{font-size:1.6em}h2{font-size:1.2em;margin-top:1.6em}kbd{background:#eee;border:1px solid #ccc;border-radius:4px;padding:1px 6px;white-space:nowrap}\
td{padding:3px 12px 3px 0;vertical-align:top}";

/// Connects View → a clear group. It holds a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_clear_group(move |i| {
        let Some(op) = me.upgrade() else { return };
        let id = op.state.borrow().snap.as_ref().and_then(|s| s.settings.keys.clear_groups.get(usize::try_from(i).ok()?).map(|g| g.id));
        if let Some(id) = id { op.clear_group(id) }
    });
    render_keys(op, &op.keymap(), &crate::shortcuts::KeySettings::default().clear_groups);
}

mod tr {
    use crate::tr;
    pub fn cant_move_pointer() -> String { tr!("This computer does not let RustyPresenter move the mouse pointer. Look for it on each screen.") }
    pub fn page_not_opened(path: &str) -> String { tr!("Could not open the page. It is at {path}", path = path) }
    pub fn no_key() -> String { tr!("(no key)") }
    pub fn report_title() -> String { tr!("Report a problem with RustyPresenter") }
    pub fn report_what() -> String { tr!("Please write below what happened and what you expected instead.") }
    pub fn report_steps() -> String { tr!("If you can, list the steps that make it happen again.") }
    pub fn report_logs() -> String { tr!("The app's log files are in this folder (Help → Open logs folder):") }
    pub fn report_send() -> String { tr!("Save this file, then send it with the newest log files to the person who looks after RustyPresenter for your church, on a USB stick or by email from another computer.") }
    pub fn report_write_here() -> String { tr!("What happened:") }
    pub fn tutorials_title() -> String { tr!("RustyPresenter: quick tutorials") }
    pub fn t_sunday() -> String { tr!("Get ready for Sunday") }
    pub fn t_sunday_1() -> String { tr!("File → New service… makes this week's service. File → Services… opens an older one or copies last week.") }
    pub fn t_sunday_2() -> String { tr!("File → Add to service adds songs, Bible passages, videos, slides and announcements. Drag items to change their order.") }
    pub fn t_sunday_3() -> String { tr!("Help → Check the service finds missing files and empty items before the service starts.") }
    pub fn t_show() -> String { tr!("Put things on the screen") }
    pub fn t_show_1() -> String { tr!("Click a slide: it goes on the screen at once. The red outline shows what the room sees.") }
    pub fn t_show_2(next: &str, back: &str) -> String { tr!("Next slide: {next}. Back: {back}.", next = next, back = back) }
    pub fn t_show_3() -> String { tr!("Window → Screens… picks which monitor is the audience screen and which is the stage screen.") }
    pub fn t_clear() -> String { tr!("Clear the screen") }
    pub fn t_clear_change() -> String { tr!("Settings → Keys changes these keys and makes your own clear groups.") }
    pub fn t_bible() -> String { tr!("Bible passages") }
    pub fn t_bible_1(key: &str) -> String { tr!("{key} puts the cursor in the Bible box. Type a reference such as John 3:16-18 and press Enter.", key = key) }
    pub fn t_bible_2() -> String { tr!("Edit → Find a verse… finds a passage by its words.") }
    pub fn t_songs() -> String { tr!("Songs") }
    pub fn t_songs_1() -> String { tr!("Edit → Quick search… finds a song in the library. Double-click a song to edit its words.") }
    pub fn t_songs_2() -> String { tr!("While a song is on the screen, C jumps to the chorus, B to the bridge and 1–9 to that verse.") }
    pub fn t_help() -> String { tr!("When something goes wrong") }
    pub fn t_help_1(key: &str) -> String { tr!("Lost the mouse on the projector? {key} brings it back to this window.", key = key) }
    pub fn t_help_2() -> String { tr!("The audience screen shows the wrong thing: press the clear keys above, then show the slide again.") }
    pub fn t_help_3() -> String { tr!("Help → Report a problem… writes a note to fill in and says where the log files are.") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, shortcuts::{Chord, KeySettings}, ui::operator::window::testing::{open, refresh}};

    #[test]
    fn the_report_names_the_logs_folder_and_the_version() {
        let text = report_text(std::path::Path::new("/data/logs"));
        assert!(text.contains("/data/logs"));
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
        assert!(text.starts_with("Report a problem with RustyPresenter"));
    }

    #[test]
    fn tutorials_list_this_churchs_keys() {
        let mut keys = KeySettings::default();
        keys.set(Action::ClearSlide, vec![Chord::parse("F10").unwrap()]);
        let page = tutorials_page(&keys);
        assert!(page.contains("<kbd>F10</kbd></td><td>Clear text"), "{page}");
        assert!(page.contains("<kbd>F9</kbd></td><td>Clear group: Everything but the background"));
        assert!(page.contains("Next slide: →  ↓  Space  Page Down."));
        assert!(!page.contains("<script"));
    }

    #[test]
    fn tutorials_and_report_open_local_files() {
        let app = test_app();
        let op = open(&app);
        op.tutorials();
        op.report_problem();
        let opened = crate::report_page::testing::OPENED.lock().unwrap().clone();
        let report = app.store.root.join("logs").join(REPORT_FILE);
        assert!(opened.contains(&report), "{opened:?}");
        assert!(std::fs::read_to_string(&report).unwrap().contains(&app.store.root.join("logs").display().to_string()));
        assert!(opened.iter().any(|p| p.starts_with(crate::report_page::reports_dir(&app.store.root)) && p.to_string_lossy().contains("tutorials")));
    }

    #[test]
    fn find_my_mouse_rings_the_middle_of_the_window() {
        let app = test_app();
        let op = open(&app);
        op.window().set_size(slint::LogicalSize::new(1400.0, 900.0));
        op.find_mouse();
        let g = op.ui.global::<FindMouse>();
        assert_eq!(g.get_x(), 700.0);
        assert!(!g.get_on(), "off for a moment first");
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(40));
        slint::platform::update_timers_and_animations();
        assert!(g.get_on());
        assert!(op.toast_log().iter().any(|(t, _)| t.starts_with("This computer does not let")), "the testing backend cannot move the pointer");
        i_slint_backend_testing::mock_elapsed_time(RINGS_FOR + Duration::from_millis(20));
        slint::platform::update_timers_and_animations();
        assert!(!g.get_on(), "the rings go away");
    }

    #[test]
    fn the_menu_shows_the_churchs_keys() {
        let app = test_app();
        let mut keys = KeySettings::default();
        keys.set(Action::ClearSlide, vec![Chord::parse("X").unwrap()]);
        keys.set(Action::Logo, vec![Chord::parse("Ctrl+L").unwrap()]);
        app.data.lock().unwrap().settings.keys = keys;
        let op = open(&app);
        refresh(&op);
        let g = op.ui.menu_keys_all();
        assert_eq!(g.get_clear_slide().to_string(), "", "a typing key is never a menu shortcut");
        assert_eq!(g.get_logo(), Keys::from_parts(["Control", "l"]).unwrap());
        assert_eq!(g.get_clear_slide_label(), "X", "the clear button's hint shows the new key");
        assert_eq!(g.get_logo_label(), "Ctrl + L");
        assert_eq!(g.get_clear_all(), Keys::from_parts(["F1"]).unwrap());
        assert_eq!(g.get_groups().row_count(), 1);
        assert_eq!(g.get_groups().row_data(0).unwrap().keys, Keys::from_parts(["F9"]).unwrap());
    }
}
