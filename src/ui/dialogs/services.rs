//! The Services dialog (`operator.js:776-790`, `ui/dialogs/services.slint`, FEAT-6a §3.1): Copy last week, new
//! from a template, an empty service or a copy of this one; date, print, rename, delete and open services; save a
//! service as a template, and the templates. Also the service list's blanks and headers (FEAT-6a, §10c idea 12).
#[path = "../../strings/services.rs"]
pub mod strings;

use super::{DialogKind, ask::notice, choose, confirm, prompt, strings as dtr};
use crate::{
    app::ExportError,
    bundle,
    model::{BlankKind, DateOrder, ItemKind},
    planning::{self, strings as ptr},
    ui::{ServiceListRow, ServicePlanning, ServicesDialogState, TemplateListRow, dialogs::AddTab, operator::{file_input::Filter, files, window::Operator}},
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{Value, json};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::{Rc, Weak};
use uuid::Uuid;

/// How many services show before "Show N older services" (§3.1).
const NEWEST: usize = 8;

/// "Service 11/2/2026": the default name of a new service or a copy (`operator.js:779-780`), in the computer's
/// date order (FEAT-6 §5.2).
pub fn dated_name(date: NaiveDate, order: DateOrder) -> String { strings::dated_name(&planning::format_date(date, order)) }

fn today() -> NaiveDate { chrono::Local::now().date_naive() }

fn order(op: &Operator) -> DateOrder { planning::date_order(&op.app().data.lock().unwrap().settings) }

fn today_name(op: &Operator) -> String { dated_name(today(), order(op)) }

/// Opens the dialog. It is never redrawn by an update (`DialogKind::refreshes`); its own actions redraw it.
pub fn open(op: &Rc<Operator>) {
    op.open_dialog(DialogKind::Services);
    op.ui.global::<ServicesDialogState>().set_show_all(false);
    draw(op);
}

/// Builds the rows from the core's data, so a change just made shows before its update arrives.
fn draw(op: &Operator) {
    let g = op.ui.global::<ServicesDialogState>();
    let (rows, templates, older) = {
        let d = op.app().data.lock().unwrap();
        let order = planning::date_order(&d.settings);
        let current = d.service().map(|s| s.id);
        let all: Vec<ServiceListRow> = planning::sorted(&d.services).into_iter().map(|svc| {
            let blanks = planning::blank_count(&svc.items);
            ServiceListRow {
                id: svc.id.to_string().into(),
                name: svc.name.as_str().into(),
                count: strings::item_count(svc.items.iter().filter(|i| !matches!(i.kind, ItemKind::Header { .. })).count()).into(),
                current: current == Some(svc.id),
                date: svc.date.map_or(ptr::no_date().into(), |d| planning::format_day_date(d, order)).into(),
                dated: svc.date.is_some(),
                blanks: if blanks > 0 { ptr::blanks_chip(blanks).into() } else { "".into() },
            }
        }).collect();
        let templates: Vec<TemplateListRow> = d.templates.iter().map(|t| TemplateListRow {
            id: t.id.to_string().into(),
            name: t.name.as_str().into(),
            day: planning::day_label(t.day).into(),
            parts: ptr::template_parts(t.items.len(), planning::blank_count(&t.items)).into(),
        }).collect();
        let older = if g.get_show_all() { 0 } else { all.len().saturating_sub(NEWEST) };
        let mut rows = all;
        rows.truncate(rows.len() - older);
        (rows, templates, older)
    };
    g.set_can_delete(rows.len() + older > 1);
    g.set_rows(ModelRc::new(VecModel::from(rows)));
    g.set_older_text(if older > 0 { ptr::show_older(older).into() } else { "".into() });
    g.set_templates(ModelRc::new(VecModel::from(templates)));
    g.set_no_templates_text(ptr::no_templates().into());
    g.set_need_template_text(ptr::need_template().into());
}

/// Rebuilds the dialog if it is still the one open.
fn redraw(op: &Operator) {
    if op.dialog() == Some(DialogKind::Services) { draw(op) }
}

/// A call whose failure is an error toast; `then` runs on success.
fn call(op: &Rc<Operator>, action: &'static str, body: Value, then: impl FnOnce(&Rc<Operator>, Value) + 'static) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>(action, body, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) => then(&op, v),
            Err(e) => { op.toast(&e, true); redraw(&op) }
        }
    });
}

/// New empty service (`:779`), or Copy this service (`:780`): a name prompt, then `service/new`, then close.
/// Cancel does nothing; an empty answer is still sent (the core names it).
fn new_service(op: &Rc<Operator>, copy: bool) {
    let me = Rc::downgrade(op);
    let title = if copy { strings::copy_prompt() } else { strings::new_prompt() };
    prompt(op, &title, &today_name(op), move |name| {
        let (Some(op), Some(name)) = (me.upgrade(), name) else { return };
        let copy_from = copy.then(|| op.state.borrow().snap.as_ref().and_then(|s| s.service_id)).flatten();
        let body = if copy { json!({ "name": name, "copy_from": copy_from }) } else { json!({ "name": name }) };
        call(&op, "service/new", body, |op, _| op.close_dialog());
    });
}

/// Rename (`:785`): a prompt starting from the name. A non-empty answer is sent as typed (untrimmed). The dialog
/// is rebuilt and stays open, even when the prompt was cancelled.
fn rename(op: &Rc<Operator>, id: Uuid) {
    let Some(current) = op.app().data.lock().unwrap().services.iter().find(|s| s.id == id).map(|s| s.name.clone()) else { return };
    let me = Rc::downgrade(op);
    prompt(op, &strings::rename_prompt(), &current, move |name| {
        let Some(op) = me.upgrade() else { return };
        let Some(name) = name.filter(|n| !n.is_empty()) else { return redraw(&op) };
        call(&op, "service/rename", json!({ "id": id, "name": name }), |op, _| redraw(op));
    });
}

/// The trash button (`:786`): confirm, `service/delete`, and the dialog is rebuilt and stays open.
fn delete(op: &Rc<Operator>, id: Uuid) {
    let Some(name) = op.app().data.lock().unwrap().services.iter().find(|s| s.id == id).map(|s| s.name.clone()) else { return };
    let me = Rc::downgrade(op);
    confirm(op, &strings::delete_question(&name), &dtr::ok(), &dtr::cancel(), false, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        call(&op, "service/delete", json!({ "id": id }), |op, _| redraw(op));
    });
}

/// Open (`:787`): `service/select`, then close.
fn select(op: &Rc<Operator>, id: Uuid) {
    call(op, "service/select", json!({ "id": id }), |op, _| op.close_dialog());
}

/// What `service/copy-week` and `service/from-template` answer.
#[derive(Deserialize)]
struct Made { name: String, blanks: usize }

fn made(op: &Rc<Operator>, v: Value) {
    op.close_dialog();
    if let Ok(m) = serde_json::from_value::<Made>(v) { op.toast(ptr::ready(&m.name, m.blanks), false) }
}

/// Copy last week (§3.2): which service and which date, a question, then `service/copy-week`. A service already
/// on that date is offered first.
fn copy_week(op: &Rc<Operator>) {
    let today = today();
    let found = {
        let d = op.app().data.lock().unwrap();
        let order = planning::date_order(&d.settings);
        planning::week_source(&d.services, d.service().map(|s| s.id), today).and_then(|id| d.services.iter().find(|s| s.id == id)).map(|src| {
            let date = planning::next_week_date(src.date, today);
            let existing = planning::find_on_date(&d.services, date).and_then(|id| d.services.iter().find(|s| s.id == id)).map(|s| (s.id, s.name.clone()));
            (src.id, src.name.clone(), date, planning::format_day_date(date, order), existing)
        })
    };
    let Some((source, name, date, date_text, existing)) = found else { return op.toast(ptr::no_service_to_copy(), true) };
    let ask = move |op: &Rc<Operator>| {
        let me = Rc::downgrade(op);
        choose(op, &ptr::copy_week_question(&date_text, &name), &ptr::copy(), &ptr::copy_with_blanks(), move |answer| {
            let (Some(op), Some(plain)) = (me.upgrade(), answer) else { return };
            let body = json!({ "source": source, "date": date.format("%Y-%m-%d").to_string(), "blanks": !plain });
            call(&op, "service/copy-week", body, made);
        });
    };
    match existing {
        None => ask(op),
        Some((id, existing_name)) => {
            let me = Rc::downgrade(op);
            let date_text = planning::format_day_date(date, order(op));
            choose(op, &ptr::already_on_date(&date_text, &existing_name), &ptr::open_it(), &ptr::make_another(), move |answer| {
                let Some(op) = me.upgrade() else { return };
                match answer {
                    Some(true) => select(&op, id),
                    Some(false) => ask(&op),
                    None => {}
                }
            });
        }
    }
}

/// The service's name prompt for a template, then `service/from-template`.
fn from_template(op: &Rc<Operator>, template: Uuid) {
    let Some((name, date)) = ({
        let d = op.app().data.lock().unwrap();
        let order = planning::date_order(&d.settings);
        d.templates.iter().find(|t| t.id == template).map(|t| {
            let date = planning::default_date(t.day, today());
            (format!("{} {}", t.name, planning::format_date(date, order)), date)
        })
    }) else { return };
    let me = Rc::downgrade(op);
    prompt(op, &ptr::new_from_template_prompt(), &name, move |answer| {
        let (Some(op), Some(name)) = (me.upgrade(), answer) else { return };
        call(&op, "service/from-template", json!({ "template": template, "name": name, "date": date.format("%Y-%m-%d").to_string() }), made);
    });
}

/// New from template… (§3.3): one template goes straight on; two are a choice; more are picked from their rows.
fn new_from_template(op: &Rc<Operator>) {
    let templates: Vec<(Uuid, String)> = op.app().data.lock().unwrap().templates.iter().map(|t| (t.id, t.name.clone())).collect();
    match templates.as_slice() {
        [] => {}
        [(id, _)] => from_template(op, *id),
        [(a, an), (b, bn)] => {
            let (a, b) = (*a, *b);
            let me = Rc::downgrade(op);
            choose(op, &ptr::pick_template(), an, bn, move |answer| {
                let (Some(op), Some(first)) = (me.upgrade(), answer) else { return };
                from_template(&op, if first { a } else { b });
            });
        }
        _ => notice(op, &ptr::pick_template_below()),
    }
}

/// Date… (§3.5): the date typed in the computer's order; empty clears it.
fn set_date(op: &Rc<Operator>, id: Uuid) {
    let order = order(op);
    let Some(current) = op.app().data.lock().unwrap().services.iter().find(|s| s.id == id).map(|s| s.date) else { return };
    let title = ptr::date_example(&planning::format_date(current.unwrap_or_else(today), order));
    let me = Rc::downgrade(op);
    prompt(op, &title, &current.map(|d| planning::format_date(d, order)).unwrap_or_default(), move |answer| {
        let (Some(op), Some(text)) = (me.upgrade(), answer) else { return };
        let date = if text.trim().is_empty() { None } else {
            match planning::parse_date(&text, order) {
                Some(d) => Some(d.format("%Y-%m-%d").to_string()),
                None => { op.toast(ptr::bad_date(), true); return redraw(&op) }
            }
        };
        call(&op, "service/set-date", json!({ "id": id, "date": date }), |op, _| redraw(op));
    });
}

/// Save as template (§3.3): its name without the date, then `template/from-service`.
fn save_template(op: &Rc<Operator>, id: Uuid) {
    let Some(name) = ({
        let d = op.app().data.lock().unwrap();
        let order = planning::date_order(&d.settings);
        d.services.iter().find(|s| s.id == id).map(|s| planning::template_name(s, order))
    }) else { return };
    let me = Rc::downgrade(op);
    prompt(op, &ptr::template_name_prompt(), &name, move |answer| {
        let (Some(op), Some(name)) = (me.upgrade(), answer) else { return };
        call(&op, "template/from-service", json!({ "service": id, "name": name }), |op, v| {
            op.toast(ptr::template_saved(v["name"].as_str().unwrap_or_default()), false);
            redraw(op);
        });
    });
}

fn template_name(op: &Operator, id: Uuid) -> Option<String> {
    op.app().data.lock().unwrap().templates.iter().find(|t| t.id == id).map(|t| t.name.clone())
}

fn rename_template(op: &Rc<Operator>, id: Uuid) {
    let Some(name) = template_name(op, id) else { return };
    let me = Rc::downgrade(op);
    prompt(op, &ptr::rename_template(), &name, move |answer| {
        let (Some(op), Some(name)) = (me.upgrade(), answer) else { return };
        call(&op, "template/rename", json!({ "id": id, "name": name }), |op, _| redraw(op));
    });
}

/// "Sunday", "sun", "Sundays" → Sunday; empty or "any" → any day. The words in the app's language work too.
pub fn parse_day(text: &str) -> Option<Option<chrono::Weekday>> {
    let full = text.trim().to_lowercase();
    let any = ["any".to_string(), ptr::any_typed().to_lowercase(), ptr::any_day().to_lowercase()];
    if full.is_empty() || any.iter().any(|a| !a.is_empty() && full.starts_with(a.as_str())) { return Some(None) }
    if let Some(i) = ptr::days_plural().iter().position(|d| d.to_lowercase() == full) {
        return chrono::Weekday::try_from(i as u8).ok().map(Some);
    }
    let t = full.trim_end_matches('s');
    if t.is_empty() { return Some(None) }
    if t.chars().count() < 2 { return None }
    ptr::days_long().iter().position(|d| d.to_lowercase().starts_with(t)).and_then(|i| chrono::Weekday::try_from(i as u8).ok()).map(Some)
}

fn template_day(op: &Rc<Operator>, id: Uuid) {
    let Some((name, day)) = op.app().data.lock().unwrap().templates.iter().find(|t| t.id == id).map(|t| (t.name.clone(), t.day)) else { return };
    let me = Rc::downgrade(op);
    let current = day.map_or_else(String::new, |d| ptr::days_long()[d.num_days_from_monday() as usize].clone());
    prompt(op, &ptr::template_day_question(&name), &current, move |answer| {
        let (Some(op), Some(text)) = (me.upgrade(), answer) else { return };
        let Some(day) = parse_day(&text) else { op.toast(ptr::bad_day(), true); return redraw(&op) };
        call(&op, "template/day", json!({ "id": id, "day": day }), |op, _| redraw(op));
    });
}

fn delete_template(op: &Rc<Operator>, id: Uuid) {
    let Some(name) = template_name(op, id) else { return };
    let me = Rc::downgrade(op);
    confirm(op, &ptr::delete_template_question(&name), &dtr::ok(), &dtr::cancel(), true, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        call(&op, "template/delete", json!({ "id": id }), |op, _| redraw(op));
    });
}

/// The file name a save starts from: the service's name without what Windows refuses in one.
fn file_name(name: &str) -> String {
    format!("{}.{}", crate::report_page::safe_name(name, "Service"), bundle::EXT)
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
        open_path(&op, path);
    });
}

/// Opens the service file at `path` (a pick, or File → Open recent).
pub(crate) fn open_path(op: &Rc<Operator>, path: std::path::PathBuf) {
    op.close_dialog();
    let me = Rc::downgrade(op);
    op.ops().import_service(path, move |r| {
        if let (Some(op), Ok(opened)) = (me.upgrade(), r) { notice(&op, &bundle::strings::opened_report(&opened)) }
    });
}

// ---------- blanks and headers in the service list ----------

/// A header's colour for the list.
pub fn parse_color(hex: &str) -> Option<slint::Color> {
    let h = hex.strip_prefix('#')?;
    let v = u32::from_str_radix(h, 16).ok().filter(|_| h.len() == 6)?;
    Some(slint::Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// The item `id` of the open service, from the core.
fn item(op: &Operator, id: Uuid) -> Option<crate::model::ServiceItem> {
    op.app().data.lock().unwrap().service().and_then(|s| s.items.iter().find(|i| i.id == id).cloned())
}

/// Which tab of the Add dialog fills a blank: Anything opens on Song.
pub fn add_tab(want: BlankKind) -> AddTab {
    match want {
        BlankKind::Song | BlankKind::Any => AddTab::Song,
        BlankKind::Scripture => AddTab::Scripture,
        BlankKind::Media => AddTab::Media,
        BlankKind::Deck => AddTab::Deck,
        BlankKind::Text => AddTab::Text,
    }
}

/// Choose (§3.4): the Add dialog on the blank's kind, titled "Fill in: Song"; what is added there replaces the blank.
pub fn choose_for_blank(op: &Rc<Operator>, id: Uuid) {
    let Some(ItemKind::Blank { label, want }) = item(op, id).map(|i| i.kind) else { return };
    super::add::open_add(op, add_tab(want));
    op.state.borrow_mut().fill = Some(id);
    let label = if label.trim().is_empty() { ptr::blank_label(want).to_string() } else { label };
    op.ui.set_dialog_title(ptr::fill_title(&label).into());
}

/// The Add dialog's choice for the blank being filled: `service/fill`; `done` runs as after an add.
pub(crate) fn fill_blank(op: &Rc<Operator>, blank: Uuid, item: Value, done: impl FnOnce(&Rc<Operator>, Uuid) + 'static) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("service/fill", json!({ "id": blank, "item": item }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => {
                op.state.borrow_mut().fill = None;
                op.select_added(blank);
                done(&op, blank);
            }
            // The dialog stays open (§5.7).
            Err(e) => op.toast(&e, true),
        }
    });
}

/// A double-click on a blank chooses for it; on a header it does nothing. True when handled here.
pub(crate) fn planning_double_click(op: &Rc<Operator>, id: Uuid) -> bool {
    match item(op, id).map(|i| i.kind) {
        Some(ItemKind::Blank { .. }) => { choose_for_blank(op, id); true }
        Some(ItemKind::Header { .. }) => true,
        _ => false,
    }
}

/// Edit on a header renames it. True when `id` is a header.
pub(crate) fn edit_header(op: &Rc<Operator>, id: Uuid) -> bool {
    let Some(it) = item(op, id) else { return false };
    let ItemKind::Header { title, color } = &it.kind else { return false };
    let color = color.clone();
    let me = Rc::downgrade(op);
    prompt(op, &ptr::rename_header(), title, move |answer| {
        let (Some(op), Some(title)) = (me.upgrade(), answer.filter(|t| !t.trim().is_empty())) else { return };
        let item = crate::model::ServiceItem { kind: ItemKind::Header { title: title.trim().into(), color }, ..it };
        call(&op, "service/update", serde_json::to_value(&item).unwrap_or_default(), |_, _| {});
    });
    true
}

/// The header's colour button: the next colour.
fn recolor(op: &Rc<Operator>, id: Uuid) {
    let Some(it) = item(op, id) else { return };
    let ItemKind::Header { title, color } = &it.kind else { return };
    let kind = ItemKind::Header { title: title.clone(), color: planning::next_header_color(color).into() };
    call(op, "service/update", serde_json::to_value(crate::model::ServiceItem { kind, ..it }).unwrap_or_default(), |_, _| {});
}

/// Header: a name, then a header above the selected item (or at the end).
fn add_header(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    prompt(op, &ptr::header_prompt(), &ptr::header_default(), move |answer| {
        let (Some(op), Some(title)) = (me.upgrade(), answer.filter(|t| !t.trim().is_empty())) else { return };
        let position = {
            let sel = op.state.borrow().selected;
            sel.and_then(|id| op.app().data.lock().unwrap().service().and_then(|s| s.items.iter().position(|i| i.id == id)))
        };
        let colors = op.app().data.lock().unwrap().service().map_or(0, |s| s.items.iter().filter(|i| matches!(i.kind, ItemKind::Header { .. })).count());
        let item = json!({ "kind": "header", "title": title.trim(), "color": planning::HEADER_COLORS[colors % planning::HEADER_COLORS.len()] });
        call(&op, "service/add", json!({ "item": item, "position": position }), |_, _| {});
    });
}

/// Connects the top bar's Services button, the dialog's buttons and the service list's blank and header buttons.
/// They hold a `Weak`.
pub(crate) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_services(move || if let Some(op) = me.upgrade() { open(&op) });
    let g = op.ui.global::<ServicesDialogState>();
    let plain = |f: fn(&Rc<Operator>)| {
        let me: Weak<Operator> = Rc::downgrade(op);
        move || if let Some(op) = me.upgrade() { f(&op) }
    };
    let by_id = |f: fn(&Rc<Operator>, Uuid)| {
        let me: Weak<Operator> = Rc::downgrade(op);
        move |id: slint::SharedString| if let (Some(op), Ok(id)) = (me.upgrade(), id.parse()) { f(&op, id) }
    };
    g.on_copy_week(plain(copy_week));
    g.on_new_from_template(plain(new_from_template));
    g.on_new_service(plain(|op| new_service(op, false)));
    g.on_copy_service(plain(|op| new_service(op, true)));
    g.on_save_file(plain(save_file));
    g.on_open_file(plain(open_file));
    g.on_show_older(plain(|op| { op.ui.global::<ServicesDialogState>().set_show_all(true); redraw(op) }));
    g.on_rename(by_id(rename));
    g.on_delete(by_id(delete));
    g.on_open(by_id(select));
    g.on_set_date(by_id(set_date));
    g.on_print(by_id(super::print::open));
    g.on_save_template(by_id(save_template));
    g.on_rename_template(by_id(rename_template));
    g.on_template_day(by_id(template_day));
    g.on_delete_template(by_id(delete_template));
    g.on_use_template(by_id(from_template));
    let p = op.ui.global::<ServicePlanning>();
    p.on_choose(by_id(choose_for_blank));
    p.on_recolor(by_id(recolor));
    p.on_add_header(plain(add_header));
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
    use slint::ComponentHandle;
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
        assert_eq!(asked(&op), Some((strings::new_prompt().into(), today_name(&op))));
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
        assert_eq!(dated_name(chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(), DateOrder::Mdy), "Service 1/2/2026");
        assert_eq!(dated_name(chrono::NaiveDate::from_ymd_opt(2026, 11, 22).unwrap(), DateOrder::Mdy), "Service 11/22/2026");
        assert_eq!(dated_name(chrono::NaiveDate::from_ymd_opt(2026, 11, 22).unwrap(), DateOrder::Dmy), "Service 22/11/2026");
        let app = test_app();
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Hi".into() }, background: None, notes: String::new(), theme: None };
        app.data.lock().unwrap().services[0].items.push(item);
        let (from, _) = current(&app);
        let op = open_services(&app);
        click(&op, "ServicesDialog::copy-btn", 0);
        let today = today_name(&op);
        assert_eq!(asked(&op), Some((strings::copy_prompt().into(), today.clone())));
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
        assert_eq!(asked(&op), Some((strings::rename_prompt().into(), name)));
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
        d.services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: id, arrangement: None }, background: None, notes: String::new(), theme: None });
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
        assert_eq!(app.jobs.lock().unwrap()[0].error.as_deref(), Some(crate::bundle::strings::not_a_service_file().as_str()));
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

    // ---------- FEAT-6a ----------

    /// The latest Sunday on or before `today`.
    fn sunday_before(today: NaiveDate) -> NaiveDate {
        use chrono::Datelike;
        today - chrono::Duration::days(today.weekday().num_days_from_sunday() as i64)
    }

    fn set_mdy(app: &App) { app.data.lock().unwrap().settings.date_order = Some(DateOrder::Mdy) }

    fn song_item() -> ServiceItem { ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: Uuid::new_v4(), arrangement: None }, background: None, notes: String::new(), theme: None } }

    #[test]
    fn copy_last_week_creates_dated_service_and_opens_it() {
        let app = test_app();
        set_mdy(&app);
        let last = sunday_before(today());
        let (from, _) = current(&app);
        {
            let mut d = app.data.lock().unwrap();
            d.services[0].date = Some(last);
            d.services[0].name = format!("Service {}", planning::format_date(last, DateOrder::Mdy));
            d.services[0].items.push(song_item());
        }
        let op = open_services(&app);
        assert_eq!(rows(&op)[0].date, planning::format_day_date(last, DateOrder::Mdy).as_str());
        click(&op, "ServicesDialog::copy-week-btn", 0);
        let next = last + chrono::Duration::days(7);
        let want = ptr::copy_week_question(&planning::format_day_date(next, DateOrder::Mdy), &format!("Service {}", planning::format_date(last, DateOrder::Mdy)));
        assert_eq!(asked(&op).map(|a| a.0), Some(want));
        key(&op, Key::Return);
        let (id, name) = current(&app);
        assert_ne!(id, from);
        assert_eq!(name, format!("Service {}", planning::format_date(next, DateOrder::Mdy)));
        assert_eq!(app.data.lock().unwrap().service().unwrap().date, Some(next));
        assert!(matches!(app.data.lock().unwrap().service().unwrap().items[0].kind, ItemKind::Song { .. }), "kept, no blanks");
        assert_eq!(op.dialog(), None, "the new service opens");
        assert!(op.toast_log().iter().any(|(t, _)| *t == ptr::ready(&name, 0)), "{:?}", op.toast_log());
    }

    #[test]
    fn copy_last_week_blanks_choice_makes_blanks() {
        let app = test_app();
        app.data.lock().unwrap().services[0].items.push(song_item());
        let op = open_services(&app);
        click(&op, "ServicesDialog::copy-week-btn", 0);
        click(&op, "AskOverlay::other", 0);
        let d = app.data.lock().unwrap();
        assert_eq!(d.services.len(), 2);
        assert!(matches!(d.service().unwrap().items[0].kind, ItemKind::Blank { .. }));
        drop(d);
        assert!(op.toast_log().iter().any(|(t, _)| t.ends_with("fill in 1 blank")), "{:?}", op.toast_log());
    }

    #[test]
    fn copy_last_week_existing_date_offers_open() {
        let app = test_app();
        let last = sunday_before(today());
        app.data.lock().unwrap().services[0].date = Some(last);
        let there = add_service(&app, "Next week");
        app.data.lock().unwrap().services[1].date = Some(last + chrono::Duration::days(7));
        let op = open_services(&app);
        click(&op, "ServicesDialog::copy-week-btn", 0);
        assert!(asked(&op).unwrap().0.starts_with("There is already a service for"), "{:?}", asked(&op));
        key(&op, Key::Return);
        assert_eq!(current(&app).0, there, "Open it");
        assert_eq!(app.data.lock().unwrap().services.len(), 2);
    }

    #[test]
    fn services_sorted_by_date_undated_last_and_older_collapse() {
        let app = test_app();
        let base = NaiveDate::from_ymd_opt(2026, 1, 4).unwrap();
        for w in 0..10 {
            let id = add_service(&app, &format!("Week {w}"));
            app.data.lock().unwrap().services.iter_mut().find(|s| s.id == id).unwrap().date = Some(base + chrono::Duration::days(7 * w));
        }
        let op = open_services(&app);
        let r = rows(&op);
        assert_eq!(r.len(), 8);
        assert_eq!(r[0].name, "Week 9", "newest first");
        assert_eq!(op.ui.global::<ServicesDialogState>().get_older_text(), "Show 3 older services");
        click(&op, "ServicesDialog::older-btn", 0);
        let r = rows(&op);
        assert_eq!(r.len(), 11);
        assert_eq!((r[10].name.as_str(), r[10].date.as_str(), r[10].dated), ("Sunday Service", ptr::no_date().as_str(), false), "undated last");
        assert!(find(&op, "ServicesDialog::older-btn").is_none());
    }

    #[test]
    fn set_date_prompt_sets_and_clears() {
        let app = test_app();
        set_mdy(&app);
        let (id, _) = current(&app);
        let op = open_services(&app);
        click(&op, "ServicesDialog::date-btn", 0);
        typed(&op, "11/8/2026");
        key(&op, Key::Return);
        assert_eq!(app.data.lock().unwrap().services[0].date, NaiveDate::from_ymd_opt(2026, 11, 8));
        assert_eq!(rows(&op)[0].date, "Sun 11/8/2026");
        click(&op, "ServicesDialog::date-btn", 0);
        assert_eq!(asked(&op).map(|a| a.1), Some("11/8/2026".into()));
        typed(&op, "soon");
        key(&op, Key::Return);
        assert!(op.toast_log().iter().any(|(t, e)| *t == ptr::bad_date() && *e));
        click(&op, "ServicesDialog::date-btn", 0);
        key(&op, Key::Backspace);
        key(&op, Key::Return);
        assert_eq!(app.data.lock().unwrap().services.iter().find(|s| s.id == id).unwrap().date, None);
    }

    #[test]
    fn save_as_template_defaults_songs_to_blank_and_makes_services() {
        let app = test_app();
        app.data.lock().unwrap().services[0].items = vec![
            ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Hi".into() }, background: None, notes: String::new(), theme: None },
            song_item(),
        ];
        let op = open_services(&app);
        assert!(find(&op, "ServicesDialog::no-templates").is_some());
        click(&op, "ServicesDialog::template-btn", 0);
        assert_eq!(asked(&op), Some((ptr::template_name_prompt().into(), "Sunday Service".into())));
        key(&op, Key::Return);
        {
            let d = app.data.lock().unwrap();
            assert_eq!(d.templates.len(), 1);
            assert_eq!(planning::blank_count(&d.templates[0].items), 1, "the song became a blank");
        }
        assert_eq!(op.ui.global::<ServicesDialogState>().get_templates().row_count(), 1, "the list shows it");
        click(&op, "ServicesDialog::from-template-btn", 0);
        assert!(asked(&op).unwrap().1.starts_with("Sunday Service "), "{:?}", asked(&op));
        key(&op, Key::Return);
        let d = app.data.lock().unwrap();
        assert_eq!(d.services.len(), 2);
        let s = d.service().unwrap();
        assert_eq!(s.template, Some(d.templates[0].id));
        use chrono::Datelike;
        assert_eq!(s.date.map(|d| d.weekday()), Some(chrono::Weekday::Sun));
        assert!(matches!(s.items[1].kind, ItemKind::Blank { .. }));
    }

    #[test]
    fn parse_day_reads_day_names() {
        assert_eq!(parse_day("Sunday"), Some(Some(chrono::Weekday::Sun)));
        assert_eq!(parse_day("sundays"), Some(Some(chrono::Weekday::Sun)));
        assert_eq!(parse_day("Wed"), Some(Some(chrono::Weekday::Wed)));
        assert_eq!(parse_day("th"), Some(Some(chrono::Weekday::Thu)));
        assert_eq!(parse_day(""), Some(None));
        assert_eq!(parse_day("Any day"), Some(None));
        assert_eq!(parse_day("t"), None);
        assert_eq!(parse_day("someday"), None);
    }

    #[test]
    fn parse_day_reads_the_translated_words() {
        let po = "msgid \"\"\nmsgstr \"\"\n\"Language: xx\\n\"\n\n\
                  msgctxt \"typed answer: no fixed day\"\nmsgid \"any\"\nmsgstr \"cualquier\"\n\n\
                  msgid \"Sunday\"\nmsgstr \"Domingo\"\n\nmsgid \"Sundays\"\nmsgstr \"Domingos\"\n";
        crate::i18n::testing::with_catalog(po, || {
            assert_eq!(parse_day("Cualquier día"), Some(None));
            assert_eq!(parse_day("any"), Some(None));
            assert_eq!(parse_day("domingos"), Some(Some(chrono::Weekday::Sun)));
            assert_eq!(parse_day("Dom"), Some(Some(chrono::Weekday::Sun)));
        });
    }

    #[test]
    fn blank_row_shows_choose_button_and_fill_mode_replaces_blank() {
        let app = test_app();
        let blank = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Blank { label: "Song".into(), want: crate::model::BlankKind::Song }, background: None, notes: "upbeat".into(), theme: None };
        let bid = blank.id;
        app.data.lock().unwrap().services[0].items.push(blank);
        let song = crate::ui::dialogs::add::tests::song(&app, "Amazing Grace", "", "", "Verse 1\nAmazing grace");
        let op = opened(&app);
        let row = op.ui.get_service_rows().row_data(0).unwrap();
        assert!(row.blank && !row.header);
        assert_eq!((row.title.as_str(), row.subtitle.as_str()), ("Song (blank)", ptr::blank_subtitle().as_str()));
        click(&op, "ServiceColumn::choose-btn", 0);
        assert_eq!(op.dialog(), Some(DialogKind::Add(AddTab::Song)));
        assert_eq!(op.ui.get_dialog_title(), "Fill in: Song");
        crate::ui::dialogs::add::tests::click(&op, "AddListRow::add-btn");
        let d = app.data.lock().unwrap();
        let it = &d.services[0].items;
        assert_eq!(it.len(), 1, "replaced, not added");
        assert_eq!(it[0].id, bid);
        assert!(matches!(it[0].kind, ItemKind::Song { song_id, .. } if song_id == song));
        assert_eq!(it[0].notes, "upbeat");
        drop(d);
        assert_eq!(op.state.borrow().fill, None);
    }

    #[test]
    fn enter_on_blank_says_choose_first() {
        let app = test_app();
        let blank = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Blank { label: "Song".into(), want: crate::model::BlankKind::Song }, background: None, notes: String::new(), theme: None };
        app.data.lock().unwrap().services[0].items.push(blank);
        let op = opened(&app);
        key(&op, Key::Return);
        assert!(op.toast_log().iter().any(|(t, _)| *t == ptr::blank_choose_first()), "{:?}", op.toast_log());
    }

    #[test]
    fn headers_are_added_renamed_and_coloured() {
        let app = test_app();
        app.data.lock().unwrap().services[0].items.push(song_item());
        let op = opened(&app);
        click(&op, "ServiceColumn::header-btn", 0);
        assert_eq!(asked(&op), Some((ptr::header_prompt().into(), ptr::header_default().into())));
        key(&op, Key::Return);
        refresh(&op);
        let first = |app: &App| app.data.lock().unwrap().services[0].items[0].clone();
        let h = first(&app);
        assert!(matches!(&h.kind, ItemKind::Header { title, color } if title == "Worship" && color == planning::HEADER_COLORS[0]), "above the selected song: {:?}", h.kind);
        let row = op.ui.get_service_rows().row_data(0).unwrap();
        assert!(row.header && row.num == 0);
        assert_eq!(op.ui.get_service_rows().row_data(1).unwrap().num, 1, "headers are not numbered");
        recolor(&op, h.id);
        assert!(matches!(first(&app).kind, ItemKind::Header { color, .. } if color == planning::HEADER_COLORS[1]));
        assert!(edit_header(&op, h.id));
        typed(&op, "Message");
        key(&op, Key::Return);
        assert!(matches!(first(&app).kind, ItemKind::Header { title, .. } if title == "Message"));
    }
}
