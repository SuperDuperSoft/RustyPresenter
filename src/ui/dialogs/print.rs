//! Print or save (FEAT-6b §3.6, `ui/dialogs/print.slint`): the order of service and the lyrics as a web page. Print…
//! opens it in the web browser, whose print window prints it or saves it as a PDF; Save web page… writes it where
//! the operator chooses. The choices are remembered (`Settings.print`, saved by `print/write`).
use super::{DialogKind, ask::notice};
use crate::{
    model::{Paper, PrintSettings, PrintWhat},
    print::strings as tr,
    report_page,
    ui::{PrintDialogState, operator::{file_input::Filter, files, window::Operator}},
};
use serde::Deserialize;
use serde_json::json;
use slint::ComponentHandle;
use std::{path::PathBuf, rc::{Rc, Weak}};
use uuid::Uuid;

pub use tr::TITLE;

/// Opens the dialog for `service` with the choices last used.
pub fn open(op: &Rc<Operator>, service: Uuid) {
    let (mut options, paper) = op.state.borrow().snap.as_ref()
        .map(|s| (s.data.settings.print.clone(), crate::planning::paper(&s.data.settings)))
        .unwrap_or_else(|| (PrintSettings::default(), crate::planning::system_defaults().1));
    options.paper = Some(paper);
    op.open_dialog(DialogKind::Print(service));
    let g = op.ui.global::<PrintDialogState>();
    g.set_what(match options.what { PrintWhat::Order => 0, PrintWhat::Lyrics => 1, PrintWhat::Both => 2 });
    g.set_notes(options.notes);
    g.set_scripture(options.scripture);
    g.set_song_per_page(options.song_per_page);
    g.set_paper(if paper == Paper::A4 { 1 } else { 0 });
    g.set_busy(false);
}

/// The choices on screen.
pub fn options(op: &Operator) -> PrintSettings {
    let g = op.ui.global::<PrintDialogState>();
    PrintSettings {
        what: match g.get_what() { 0 => PrintWhat::Order, 1 => PrintWhat::Lyrics, _ => PrintWhat::Both },
        notes: g.get_notes(),
        scripture: g.get_scripture(),
        song_per_page: g.get_song_per_page(),
        paper: Some(if g.get_paper() == 1 { Paper::A4 } else { Paper::Letter }),
    }
}

#[derive(Deserialize)]
struct Written { path: PathBuf, opened: bool }

fn service(op: &Operator) -> Option<Uuid> { match op.dialog() { Some(DialogKind::Print(id)) => Some(id), _ => None } }

/// `print/write`, with `path` for Save web page….
fn write(op: &Rc<Operator>, path: Option<PathBuf>) {
    let Some(id) = service(op) else { return };
    let mut body = serde_json::to_value(options(op)).unwrap_or_default();
    body["service"] = json!(id);
    body["path"] = json!(path);
    op.ui.global::<PrintDialogState>().set_busy(true);
    let me = Rc::downgrade(op);
    op.ops().call::<Written>("print/write", body, move |r| {
        let Some(op) = me.upgrade() else { return };
        op.ui.global::<PrintDialogState>().set_busy(false);
        match r {
            Ok(w) if path.is_some() => op.toast(tr::saved(&w.path.file_name().unwrap_or_default().to_string_lossy()), false),
            Ok(w) if w.opened => op.toast(tr::OPENED, false),
            Ok(w) => notice(&op, &tr::not_opened(&w.path.display().to_string())),
            Err(e) => op.toast(&e, true),
        }
    });
}

/// Save web page…: "<service name> – order and lyrics.html" to start with.
fn save(op: &Rc<Operator>) {
    let Some(id) = service(op) else { return };
    let name = op.state.borrow().snap.as_ref().and_then(|s| s.data.services.iter().find(|x| x.id == id).map(|x| x.name.clone())).unwrap_or_default();
    let file = format!("{}.html", report_page::safe_name(&tr::file_stem(&name, options(op).what), "Service"));
    let me = Rc::downgrade(op);
    files::pick_save(op, Filter::WebPage, &file, move |dest| if let Some(op) = me.upgrade() { write(&op, Some(dest)) });
}

pub(crate) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<PrintDialogState>();
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_print(move || if let Some(op) = me.upgrade() { write(&op, None) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_save(move || if let Some(op) = me.upgrade() { save(&op) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_close(move || if let Some(op) = me.upgrade() { op.close_dialog() });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, ui::dialogs::services::testing::*};

    #[test]
    fn print_dialog_remembers_options() {
        let app = test_app();
        let id = app.data.lock().unwrap().services[0].id;
        let op = opened(&app);
        open(&op, id);
        settle(&op);
        assert_eq!(op.ui.get_dialog_title(), "Print or save");
        click(&op, "PrintDialog::notes", 0);
        op.ui.global::<PrintDialogState>().set_what(0);
        op.ui.global::<PrintDialogState>().set_paper(1);
        click(&op, "PrintDialog::print-btn", 0);
        let p = app.data.lock().unwrap().settings.print.clone();
        assert_eq!(p, PrintSettings { what: PrintWhat::Order, notes: false, scripture: true, song_per_page: false, paper: Some(Paper::A4) });
        assert!(op.toast_log().iter().any(|(t, err)| t == tr::OPENED && !err), "{:?}", op.toast_log());
        // Opened again later, it starts from those.
        op.close_dialog();
        crate::ui::operator::window::testing::refresh(&op);
        open(&op, id);
        assert_eq!(options(&op), p);
    }

    #[test]
    fn print_save_html_writes_file() {
        let app = test_app();
        let id = app.data.lock().unwrap().services[0].id;
        let op = opened(&app);
        open(&op, id);
        let dest = app.store.root.join("sheet.html");
        crate::ui::operator::files::testing::will_pick(vec![dest.clone()]);
        click(&op, "PrintDialog::save-btn", 0);
        let page = std::fs::read_to_string(&dest).unwrap();
        assert!(page.contains("Sunday Service"));
        assert!(op.toast_log().iter().any(|(t, _)| t == "Saved sheet.html"), "{:?}", op.toast_log());
    }
}
