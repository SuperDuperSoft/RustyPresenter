//! Settings → Video → Graphics (`video::graphics`): the choices this build can draw with, saved at once and used from
//! the next start, and the one in use now.
use super::{SettingsTab, settings::Edit};
use crate::{model::Graphics, tr, ui::{GraphicsTestRow, SettingsDlg, operator::window::Operator}, video::{bench::{self, Measured, Offer}, graphics::{self, Build}}};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, path::{Path, PathBuf}, rc::Rc, time::Duration};

/// The picker's choices; a saved one this build lacks shows as Auto, which is what is used then.
fn choices() -> Vec<Graphics> { graphics::available(Build::this()) }

pub(super) fn fill(g: &SettingsDlg, saved: Graphics) {
    let list = choices();
    let labels: Vec<slint::SharedString> = list.iter().map(|c| graphics::label(*c).into()).collect();
    g.set_graphics_labels(ModelRc::new(VecModel::from(labels)));
    g.set_graphics_index(list.iter().position(|c| *c == saved).unwrap_or(0) as i32);
    g.set_graphics_now(graphics::in_use(graphics::chosen().graphics).into());
    fill_test(g);
}

/// A choice in the picker: saved now (`settings/graphics`), used from the next start.
pub(super) fn choose(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(choice) = choices().get(i).copied() else { return };
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>("settings/graphics", serde_json::json!({ "graphics": choice }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) => {
                edit.settings.borrow_mut().graphics = choice;
                if let Some(t) = v["toast"].as_str() { op.toast(t, false) }
                op.ui.global::<SettingsDlg>().set_graphics_restart(true);
            }
            Err(e) => op.toast(e, true),
        }
    });
}

// ---- "Test which is fastest" (`video::bench`) ----

thread_local! {
    /// The last test's results while this run of the app lasts (for "Use this" and opening Settings again).
    static LAST: RefCell<Vec<Measured>> = const { RefCell::new(Vec::new()) };
}

fn data_root(op: &Operator) -> PathBuf { op.app().store.data_path().parent().map(PathBuf::from).unwrap_or_default() }

/// Shows `results` as the list, picking the best (or the first that worked).
pub(super) fn show_results(g: &SettingsDlg, results: &[Measured]) {
    let rows = bench::rows(results);
    let pick = rows.iter().position(|r| r.best).unwrap_or(0);
    g.set_graphics_test_rows(ModelRc::new(VecModel::from(rows.iter().map(|r| GraphicsTestRow {
        name: r.name.as_str().into(), bar: r.bar, word: r.word.as_str().into(), details: r.details.as_str().into(), best: r.best, works: r.works,
    }).collect::<Vec<_>>())));
    g.set_graphics_test_pick(pick as i32);
    g.set_graphics_test_status(match bench::best(results) { Some(w) => bench::done_text(w), None => bench::nothing_worked() }.into());
}

/// What a finished test leaves: saved for Automatic (`<data>/graphics-test.json`) and shown.
pub(super) fn finish(g: &SettingsDlg, root: &Path, results: Vec<Measured>) {
    bench::save(root, &bench::record(results.clone()));
    show_results(g, &results);
    g.set_graphics_testing(false);
    LAST.with(|l| *l.borrow_mut() = results);
}

/// Fills the test's part of the page: the last results of this run, if any.
pub(super) fn fill_test(g: &SettingsDlg) {
    if g.get_graphics_testing() { return }
    let last = LAST.with(|l| l.borrow().clone());
    if last.is_empty() {
        g.set_graphics_test_rows(ModelRc::default());
        g.set_graphics_test_status("".into());
    } else {
        show_results(g, &last);
    }
}

/// "Test which is fastest": each choice in a child process of this app, one at a time, on a thread of its own.
pub(super) fn start_test(op: &Rc<Operator>, _edit: &Rc<Edit>) { run_test(op) }

fn run_test(op: &Rc<Operator>) {
    let g = op.ui.global::<SettingsDlg>();
    if g.get_graphics_testing() { return }
    let exe = match std::env::current_exe() { Ok(e) => e, Err(e) => { op.toast(e.to_string(), true); return } };
    let list = bench::choices(&choices());
    let root = data_root(op);
    // The test's room goes on the projector only while the church's room window is closed.
    let app = op.app();
    let room_open = app.state_json()["room_connected"].as_bool().unwrap_or(true);
    let saved = app.data.lock().ok().and_then(|d| d.settings.screens.monitors("audience").first().map(|m| m.to_string()));
    let room = bench::room_arg(room_open, saved.as_deref());
    g.set_graphics_testing(true);
    g.set_graphics_test_rows(ModelRc::default());
    let total = bench::schedule(&list).len();
    if let Some(first) = list.first() { g.set_graphics_test_status(bench::testing(*first, 0, total).into()) }
    let ui = op.ui.as_weak();
    std::thread::spawn(move || {
        let progress = ui.clone();
        let results = bench::run_all(&bench::Exe { exe, room }, &list, &std::env::temp_dir(), bench::LIMIT, Duration::from_millis(100), |i, n, choice| {
            let text = bench::testing(choice, i, n);
            let _ = progress.upgrade_in_event_loop(move |ui| ui.global::<SettingsDlg>().set_graphics_test_status(text.into()));
        });
        let _ = ui.upgrade_in_event_loop(move |ui| finish(&ui.global::<SettingsDlg>(), &root, results));
    });
}


/// "Use this": saves the picked choice as the setting (`settings/graphics`), used from the next start.
pub(super) fn use_picked(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(m) = LAST.with(|l| l.borrow().get(i).cloned()) else { return };
    if bench::verdict(&m) == bench::Verdict::Failed { return }
    let Some(at) = choices().iter().position(|c| *c == m.graphics) else { return };
    op.ui.global::<SettingsDlg>().set_graphics_index(at as i32);
    choose(op, edit, at);
}

/// At start (once the operator is open): offers the test the first time, and again when the card or driver changed.
/// Yes opens Settings → Video and starts it.
pub fn offer_at_start(op: &Rc<Operator>) {
    let root = data_root(op);
    let why = bench::offer(bench::load(&root).as_ref(), graphics::chosen().graphics, &graphics::adapter());
    if why == Offer::No { return }
    bench::mark_offered(&root, why);
    let me = Rc::downgrade(op);
    super::ask::confirm(op, &bench::offer_text(why), &tr!("Yes, test now"), &bench::not_now(), false, move |yes| {
        let Some(op) = me.upgrade() else { return };
        if !yes { return }
        super::settings::open(&op, SettingsTab::Video);
        run_test(&op);
    });
}
