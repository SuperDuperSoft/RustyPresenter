//! The Song use report dialog (FEAT-7 §3.3, `ui/dialogs/usage.slint`): pick the dates, see each song's uses, save
//! a spreadsheet or open a page to print, mark the dates as reported, and remove single uses under Details.
use super::{DialogKind, confirm, strings as dlg};
use crate::{
    ui::{UsageDetailRow, UsageDlg, UsageReportRow, operator::{file_input::Filter, files, window::Operator}},
    usage::{Quick, ReportView, export, parse_date, strings, today},
};
use chrono::NaiveDate;
use serde_json::{Value, json};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::Cell, rc::{Rc, Weak}};

/// What the open dialog remembers between reads.
#[derive(Default)]
struct State { reported_through: Cell<Option<NaiveDate>> }

/// Opens the report with its default dates (since the last report, else the last 3 months, to today).
pub fn open(op: &Rc<Operator>) {
    op.open_dialog(DialogKind::Usage);
    let g = op.ui.global::<UsageDlg>();
    g.set_from("".into());
    g.set_to("".into());
    g.set_include_no_ccli(false);
    g.set_details_open(false);
    g.set_error("".into());
    g.set_rows(ModelRc::default());
    g.set_entries(ModelRc::default());
    let state = Rc::new(State::default());
    connect(op, &state);
    load(op, &state, true);
}

fn connect(op: &Rc<Operator>, state: &Rc<State>) {
    let g = op.ui.global::<UsageDlg>();
    let with = |f: fn(&Rc<Operator>, &Rc<State>)| {
        let (me, st): (Weak<Operator>, Rc<State>) = (Rc::downgrade(op), state.clone());
        move || if let Some(op) = me.upgrade() { f(&op, &st) }
    };
    g.on_dates_edited(with(dates_edited));
    g.on_include_toggled(with(|op, st| load(op, st, false)));
    g.on_export_csv(with(export_csv));
    g.on_open_page(with(open_page));
    g.on_mark_reported(with(mark_reported));
    let (me, st) = (Rc::downgrade(op), state.clone());
    g.on_quick(move |i| if let (Some(op), Some(q)) = (me.upgrade(), Quick::from_index(i.max(0) as usize)) { quick(&op, &st, q) });
    let (me, st) = (Rc::downgrade(op), state.clone());
    g.on_remove(move |id| if let Some(op) = me.upgrade() { remove(&op, &st, id.to_string()) });
    let me = Rc::downgrade(op);
    g.on_keep_toggled(move |on| if let Some(op) = me.upgrade() { keep(&op, on) });
}

/// The dates and the checkbox as the action wants them; empty dates take the defaults.
fn range(g: &UsageDlg) -> Value {
    json!({ "from": g.get_from().trim(), "to": g.get_to().trim(), "include_no_ccli": g.get_include_no_ccli() })
}

/// Reads the report for the dates shown. `set_dates` writes the dates the core used into the fields (first open).
fn load(op: &Rc<Operator>, state: &Rc<State>, set_dates: bool) {
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
    let body = range(&op.ui.global::<UsageDlg>());
    op.ops().call::<ReportView>("usage/report", body, move |r| {
        let (Some(op), Some(state)) = (me.upgrade(), st.upgrade()) else { return };
        if op.dialog() != Some(DialogKind::Usage) { return }
        let g = op.ui.global::<UsageDlg>();
        match r {
            Ok(v) => {
                state.reported_through.set(v.reported_through.as_deref().and_then(parse_date));
                fill(&g, &v, set_dates);
            }
            Err(e) => g.set_error(e.into()),
        }
    });
}

fn fill(g: &UsageDlg, v: &ReportView, set_dates: bool) {
    if set_dates {
        g.set_from(v.from.as_str().into());
        g.set_to(v.to.as_str().into());
    }
    g.set_error("".into());
    g.set_keep_log(v.enabled);
    g.set_has_license(!v.license.is_empty());
    g.set_license_line(if v.license.is_empty() { strings::no_license().into() } else { strings::license_line(&v.license).into() });
    g.set_has_since_last(v.reported_through.is_some());
    g.set_mark_label(strings::mark_reported(&v.to).into());
    let rows: Vec<UsageReportRow> = v.rows.iter().map(|r| UsageReportRow {
        title: r.title.as_str().into(), ccli: r.ccli.as_str().into(), shown: r.shown.to_string().into(),
        streamed: r.streamed.to_string().into(), last: export::day(r.last_used).into(),
    }).collect();
    g.set_rows(ModelRc::new(VecModel::from(rows)));
    let entries: Vec<UsageDetailRow> = v.entries.iter().map(|e| UsageDetailRow {
        id: e.id.to_string().into(), date: e.date.as_str().into(), time: e.time.as_str().into(), service: e.service.as_str().into(),
        song: e.song.as_str().into(), streamed: e.streamed,
    }).collect();
    g.set_entries(ModelRc::new(VecModel::from(entries)));
}

/// Typing a date: the report follows once both dates read as dates; a whole date that doesn't read says so.
fn dates_edited(op: &Rc<Operator>, state: &Rc<State>) {
    let g = op.ui.global::<UsageDlg>();
    let (from, to) = (g.get_from(), g.get_to());
    if parse_date(&from).is_some() && parse_date(&to).is_some() { return load(op, state, false) }
    let whole = |t: &str| t.trim().len() >= 10;
    let bad = (whole(&from) && parse_date(&from).is_none()) || (whole(&to) && parse_date(&to).is_none());
    g.set_error(if bad { strings::bad_date().into() } else { "".into() });
}

fn quick(op: &Rc<Operator>, state: &Rc<State>, q: Quick) {
    let (from, to) = q.range(state.reported_through.get(), today());
    let g = op.ui.global::<UsageDlg>();
    g.set_from(export::day(from).into());
    g.set_to(export::day(to).into());
    load(op, state, false);
}

/// The dates as typed, when they read; otherwise the error is shown and nothing happens.
fn checked_dates(op: &Operator) -> Option<(NaiveDate, NaiveDate)> {
    let g = op.ui.global::<UsageDlg>();
    let (from, to) = (parse_date(&g.get_from()), parse_date(&g.get_to()));
    let dates = from.zip(to);
    match dates {
        None => g.set_error(strings::bad_date().into()),
        Some((from, to)) if from > to => { g.set_error(strings::from_after_to().into()); return None }
        Some(_) => {}
    }
    dates
}

fn export_csv(op: &Rc<Operator>, _: &Rc<State>) {
    let Some((from, to)) = checked_dates(op) else { return };
    let me = Rc::downgrade(op);
    files::pick_save(op, Filter::Csv, &export::file_name(from, to, "csv"), move |path| {
        let Some(op) = me.upgrade() else { return };
        let mut body = range(&op.ui.global::<UsageDlg>());
        body["format"] = json!("csv");
        body["path"] = json!(path);
        let me = Rc::downgrade(&op);
        op.ops().call::<Value>("usage/export", body, move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(v) => op.toast(strings::saved_to(v["path"].as_str().unwrap_or_default()), false),
                Err(e) => op.toast(e, true),
            }
        });
    });
}

/// Writes the page to the data folder's `reports/` and opens it in the browser.
fn open_page(op: &Rc<Operator>, _: &Rc<State>) {
    if checked_dates(op).is_none() { return }
    let mut body = range(&op.ui.global::<UsageDlg>());
    body["format"] = json!("html");
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("usage/export", body, move |r| {
        if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(e, true) }
    });
}

fn mark_reported(op: &Rc<Operator>, state: &Rc<State>) {
    let Some((_, to)) = checked_dates(op) else { return };
    let to = export::day(to);
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
    confirm(op, &strings::mark_confirm(&to), &strings::mark(), &dlg::cancel(), false, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        let me = Rc::downgrade(&op);
        op.ops().call::<Value>("usage/mark-reported", json!({ "through": to }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(_) => {
                    op.toast(strings::marked(&to), false);
                    if let Some(state) = st.upgrade() && op.dialog() == Some(DialogKind::Usage) { load(&op, &state, false) }
                }
                Err(e) => op.toast(e, true),
            }
        });
    });
}

fn remove(op: &Rc<Operator>, state: &Rc<State>, id: String) {
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
    confirm(op, &strings::remove_confirm(), &strings::remove(), &dlg::cancel(), true, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        let me = Rc::downgrade(&op);
        op.ops().call::<Value>("usage/remove", json!({ "id": id }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(_) => {
                    op.toast(strings::removed(), false);
                    if let Some(state) = st.upgrade() && op.dialog() == Some(DialogKind::Usage) { load(&op, &state, false) }
                }
                Err(e) => op.toast(e, true),
            }
        });
    });
}

fn keep(op: &Rc<Operator>, on: bool) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("usage/set", json!({ "enabled": on }), move |r| {
        if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(e, true) }
    });
}

/// Connects the window's `usage-report` callback, which the menu (or anything else) calls to open the report.
pub(crate) fn wire(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ui.on_usage_report(move || if let Some(op) = me.upgrade() { open(&op) });
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
