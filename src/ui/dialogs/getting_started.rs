//! Getting started (`ui/dialogs/getting-started.slint`): a welcome and three short steps (the screen, something to
//! show, showing it). It opens by itself once, the first time the app starts with no songs and nothing in a
//! service, and from Help → Getting started… whenever it is wanted. Every step has Skip. The screen and the "add"
//! steps open the real dialog; the guide comes back on its next step when that dialog closes.
use super::{AddTab, DialogKind};
use crate::ui::{GettingStartedDlg, operator::window::Operator};
use slint::ComponentHandle;
use std::{path::Path, rc::Rc};

/// The last step.
const LAST: i32 = 3;

/// In the data folder once the guide has opened by itself (or was not needed): it never opens by itself again.
pub const SHOWN_FILE: &str = "getting-started-shown";

pub fn title() -> String { crate::tr!("Getting started") }

/// Opens the guide on its welcome.
pub fn open(op: &Rc<Operator>) { open_at(op, 0) }

fn open_at(op: &Rc<Operator>, step: i32) {
    op.open_dialog(DialogKind::GettingStarted);
    op.ui.global::<GettingStartedDlg>().set_step(step.clamp(0, LAST));
}

/// Nothing made yet: no songs, and no service with anything in it.
fn is_new(data: &crate::model::Data) -> bool { data.songs.is_empty() && data.services.iter().all(|s| s.items.is_empty()) }

fn shown(root: &Path) -> bool { root.join(SHOWN_FILE).exists() }

fn mark_shown(root: &Path) {
    if let Err(e) = std::fs::write(root.join(SHOWN_FILE), "") { eprintln!("getting started not noted: {e}") }
}

/// A little after the window opens: the guide, once, on a computer where nothing has been made yet. A church that
/// already has songs or a service never sees it by itself. Never over another dialog or while something is on the
/// screens (it waits for the next start).
pub fn at_start(op: &Rc<Operator>) {
    let root = op.app().store.root.clone();
    if shown(&root) { return }
    if !is_new(&op.app().data()) { return mark_shown(&root) }
    if op.dialog().is_some() || op.app().on_air() { return }
    mark_shown(&root);
    open(op);
}

/// The guide again on `step`, when nothing else has opened meanwhile.
fn resume(op: &Rc<Operator>, step: i32) {
    if op.dialog().is_none() { open_at(op, step) }
}

pub(crate) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<GettingStartedDlg>();
    let me = Rc::downgrade(op);
    g.on_next(move || if let Some(op) = me.upgrade() {
        let g = op.ui.global::<GettingStartedDlg>();
        g.set_step((g.get_step() + 1).min(LAST));
    });
    let me = Rc::downgrade(op);
    g.on_back(move || if let Some(op) = me.upgrade() {
        let g = op.ui.global::<GettingStartedDlg>();
        g.set_step((g.get_step() - 1).max(0));
    });
    let me = Rc::downgrade(op);
    g.on_skip(move || if let Some(op) = me.upgrade() {
        op.close_dialog();
        op.toast(tr::skipped(), false);
    });
    let me = Rc::downgrade(op);
    g.on_finish(move || if let Some(op) = me.upgrade() { op.close_dialog() });
    let me = Rc::downgrade(op);
    g.on_screens(move || if let Some(op) = me.upgrade() { super::screens::open_then(&op, |op| resume(op, 2)) });
    let me = Rc::downgrade(op);
    g.on_add(move || if let Some(op) = me.upgrade() {
        super::add::open_add(&op, AddTab::Song);
        op.on_dialog_close(|op| resume(op, LAST));
    });
    let me = Rc::downgrade(op);
    g.on_tutorials(move || if let Some(op) = me.upgrade() { op.tutorials() });
}

mod tr {
    use crate::tr;
    pub fn skipped() -> String { tr!("Help → Getting started… shows those steps again.") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, ui::operator::window::testing::{open as open_window, refresh}};

    #[test]
    fn a_new_install_sees_the_guide_once_and_can_skip_it() {
        let app = test_app();
        let op = open_window(&app);
        refresh(&op);
        at_start(&op);
        assert_eq!(op.dialog(), Some(DialogKind::GettingStarted));
        let g = op.ui.global::<GettingStartedDlg>();
        assert_eq!(g.get_step(), 0);
        g.invoke_skip();
        assert_eq!(op.dialog(), None, "Skip closes it");
        assert!(op.toast_log().iter().any(|(t, _)| t.contains("Getting started")), "and says where it is");
        at_start(&op);
        assert_eq!(op.dialog(), None, "never by itself a second time");
        // Help → Getting started… always opens it.
        open(&op);
        assert_eq!(op.dialog(), Some(DialogKind::GettingStarted));
    }

    #[test]
    fn a_church_with_songs_never_sees_it_by_itself() {
        let app = test_app();
        app.data().songs.push(Default::default());
        let op = open_window(&app);
        refresh(&op);
        at_start(&op);
        assert_eq!(op.dialog(), None);
        assert!(shown(&app.store.root), "settled: not asked again when the library is emptied");
    }

    #[test]
    fn the_steps_open_the_real_dialogs_and_come_back() {
        let app = test_app();
        let op = open_window(&app);
        refresh(&op);
        open(&op);
        let g = op.ui.global::<GettingStartedDlg>();
        g.invoke_back();
        assert_eq!(g.get_step(), 0, "nothing before the welcome");
        g.invoke_next();
        assert_eq!(g.get_step(), 1);
        g.invoke_screens();
        assert_eq!(op.dialog(), Some(DialogKind::Screens));
        op.close_dialog();
        assert_eq!((op.dialog(), g.get_step()), (Some(DialogKind::GettingStarted), 2), "back on the next step");
        g.invoke_add();
        assert!(matches!(op.dialog(), Some(DialogKind::Add(_))));
        op.close_dialog();
        assert_eq!((op.dialog(), g.get_step()), (Some(DialogKind::GettingStarted), LAST));
        g.invoke_next();
        assert_eq!(g.get_step(), LAST, "nothing after the last step");
        g.invoke_tutorials();
        assert!(crate::report_page::testing::OPENED.lock().unwrap().iter().any(|p| p.starts_with(crate::report_page::reports_dir(&app.store.root))));
        g.invoke_finish();
        assert_eq!(op.dialog(), None);
    }
}
