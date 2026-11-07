//! The service column (`renderService()`, `operator.js:123-153`): one row per item, click to select, double-click
//! to show slide 1, edit and remove on hover, and drag to reorder. The row model is one `VecModel` that is filled
//! with `set_vec` on a new snapshot and patched with `set_row_data` when only the selection or the live item moves.

use super::{view::{self, OperatorSnapshot}, window::Operator};
use crate::{
    live::Command,
    ui::{ServiceRow, ops::outcome_toast},
};
use slint::{DataTransfer, Model, ModelRc};
use std::rc::{Rc, Weak};
use uuid::Uuid;

/// What a dragged row carries: plain text `svc:<id>`, the web's `text/x-rp-item` (`operator.js:136`).
const DRAG_PREFIX: &str = "svc:";

/// The icon for an item kind; an unknown kind gets the text icon (`I[it.kind] || I.text`).
pub fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "song" => "song",
        "scripture" => "scripture",
        "video" => "video",
        "image" => "image",
        "audio" => "audio",
        "deck" => "deck",
        _ => "text",
    }
}

/// The list rows for a snapshot (`renderService()`): numbered from 1, with the selected and the live item marked.
/// Headers (FEAT-6) have no number and do not count.
pub fn rows(snap: &OperatorSnapshot, selected: Option<Uuid>, live_item: Option<Uuid>) -> Vec<ServiceRow> {
    let mut num = 0;
    snap.items.iter().map(|it| {
        let header = match &it.item.kind { crate::model::ItemKind::Header { color, .. } => Some(crate::planning::header_color(color)), _ => None };
        let blank = it.kind == "blank";
        if header.is_none() { num += 1 }
        ServiceRow {
            id: it.id.to_string().into(),
            num: if header.is_some() { 0 } else { num },
            kind: kind_icon(&it.kind).into(),
            title: it.title.as_str().into(),
            subtitle: if blank { crate::planning::strings::blank_subtitle().into() } else { it.subtitle.as_str().into() },
            selected: selected == Some(it.id),
            live: live_item == Some(it.id),
            blank,
            header: header.is_some(),
            color: header.and_then(crate::ui::dialogs::services::parse_color).unwrap_or_default(),
        }
    }).collect()
}

/// The drag payload for the row of item `id`.
fn drag_data(id: &str) -> DataTransfer {
    let mut d = DataTransfer::default();
    d.set_plain_text(format!("{DRAG_PREFIX}{id}").into());
    d
}

/// The item id a drag payload carries, or "" when it is anything else (a file, text from another program).
fn drag_id(d: &DataTransfer) -> String {
    d.plain_text().ok()
        .and_then(|t| t.strip_prefix(DRAG_PREFIX).and_then(|id| Uuid::parse_str(id).ok()))
        .map(|id| id.to_string())
        .unwrap_or_default()
}

impl Operator {
    /// Rebuilds the list from the current snapshot (a new `Data`), keeping the same model.
    pub(super) fn render_service(&self) {
        let st = self.state.borrow();
        let Some(snap) = st.snap.as_ref() else { return };
        self.service_rows.set_vec(rows(snap, st.selected, self.live_item()));
        self.ui.set_service_count(view::count_label(snap.items.len()).into());
        drop(st);
        self.render_service_theme();
    }

    /// Moves the selected and live marks without rebuilding the rows (a click or a `Live` update).
    pub(super) fn mark_service(&self) {
        let (selected, live) = (self.state.borrow().selected, self.live_item());
        let is = |id: Option<Uuid>, row: &ServiceRow| id.is_some_and(|id| row.id == id.to_string().as_str());
        for i in 0..self.service_rows.row_count() {
            let Some(mut row) = self.service_rows.row_data(i) else { continue };
            let (s, l) = (is(selected, &row), is(live, &row));
            if row.selected != s || row.live != l {
                row.selected = s;
                row.live = l;
                self.service_rows.set_row_data(i, row);
            }
        }
    }

    /// A click on a row: select it and drop the Bible arm; nothing goes live (`operator.js:134`).
    fn select_item(&self, id: Uuid) {
        {
            let mut st = self.state.borrow_mut();
            st.selected = Some(id);
            st.armed = None;
        }
        self.mark_service();
        self.render_slides();
    }

    /// A double-click: show the item's first slide, if it has one (`operator.js:135`).
    fn show_first(self: &Rc<Self>, id: Uuid) {
        if crate::ui::dialogs::services::planning_double_click(self, id) { return }
        let has_slides = self.state.borrow().snap.as_ref()
            .and_then(|s| s.items.iter().find(|i| i.id == id))
            .is_some_and(|i| !i.slides.is_empty());
        if !has_slides { return; }
        let me = Rc::downgrade(self);
        self.ops.cmd(Command::Show { item_id: id, slide: 0 }, None, move |o| {
            if let (Some(op), Some((msg, err))) = (me.upgrade(), outcome_toast(&o)) { op.toast(&msg, err); }
        });
    }

    /// `service/remove` or `service/move`; a failure is an error toast with the core's message (`guard()`).
    fn service_action(self: &Rc<Self>, action: &'static str, body: serde_json::Value) {
        let me = Rc::downgrade(self);
        self.ops.call::<serde_json::Value>(action, body, move |r| {
            if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(&e, true); }
        });
    }

    /// The drag ended (dropped or cancelled): apply the snapshot that arrived during it, if any.
    fn finish_drag(&self) {
        let held = {
            let mut st = self.state.borrow_mut();
            st.dragging = false;
            st.held_snapshot.take()
        };
        if let Some(s) = held { self.apply_snapshot(s); }
    }
}

/// Connects the column's callbacks. Each holds a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let ui = &op.ui;
    ui.set_service_rows(ModelRc::from(op.service_rows.clone()));
    let on = |f: fn(&Rc<Operator>, Uuid)| {
        let me: Weak<Operator> = Rc::downgrade(op);
        move |id: slint::SharedString| {
            if let (Some(op), Ok(id)) = (me.upgrade(), Uuid::parse_str(&id)) { f(&op, id) }
        }
    };
    ui.on_service_select(on(|op, id| op.select_item(id)));
    ui.on_service_show_first(on(|op, id| op.show_first(id)));
    ui.on_service_remove(on(|op, id| op.service_action("service/remove", serde_json::json!({ "id": id }))));
    // Edit opens Item edit (Task 18) and Add opens the Add dialog on the Song tab (Task 16).
    ui.on_service_edit(on(|op, id| if !crate::ui::dialogs::services::edit_header(op, id) { crate::ui::dialogs::item_edit::open(op, id) }));
    let me = Rc::downgrade(op);
    ui.on_service_move(move |id, to| {
        let (Some(op), Ok(id), Ok(to)) = (me.upgrade(), Uuid::parse_str(&id), usize::try_from(to)) else { return };
        op.service_action("service/move", serde_json::json!({ "id": id, "to": to }));
    });
    let me = Rc::downgrade(op);
    ui.on_service_drag_started(move || if let Some(op) = me.upgrade() { op.state.borrow_mut().dragging = true; });
    let me = Rc::downgrade(op);
    ui.on_service_drag_finished(move || if let Some(op) = me.upgrade() { op.finish_drag(); });
    ui.on_service_drag_data(|id| drag_data(&id));
    ui.on_service_drag_id(|d| drag_id(&d).into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        model::{ItemKind, ServiceItem},
        ui::operator::window::testing::{find, find_all, open, refresh},
    };
    use i_slint_backend_testing::ElementHandle;
    use slint::{LogicalPosition, platform::{PointerEventButton, WindowEvent}};
    use std::sync::Arc;

    /// Announcements, one slide each, appended to the current service; returns their ids.
    fn add_texts(app: &App, titles: &[&str]) -> Vec<Uuid> {
        let mut d = app.data.lock().unwrap();
        titles.iter().map(|t| {
            let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: t.to_string(), body: format!("{t} body") }, background: None, notes: String::new(), theme: None };
            d.services[0].items.push(item.clone());
            item.id
        }).collect()
    }

    fn order(app: &App) -> Vec<Uuid> { app.data.lock().unwrap().services[0].items.iter().map(|i| i.id).collect() }

    fn model(op: &Operator) -> Vec<ServiceRow> { op.ui.get_service_rows().iter().collect() }

    /// A window big enough for every row, with the service already read.
    fn opened(app: &Arc<App>) -> std::rc::Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op
    }

    fn center(e: &ElementHandle) -> LogicalPosition {
        let (p, s) = (e.absolute_position(), e.size());
        LogicalPosition::new(p.x + s.width / 2.0, p.y + s.height / 2.0)
    }

    fn row(op: &Operator, i: usize) -> ElementHandle { find_all(op, "ServiceColumn::row").swap_remove(i) }

    /// Presses on `from` and moves far enough to start a drag, without dropping.
    fn start_drag(op: &Operator, from: LogicalPosition) {
        let w = op.window();
        w.dispatch_event(WindowEvent::PointerMoved { position: from });
        w.dispatch_event(WindowEvent::PointerPressed { position: from, button: PointerEventButton::Left });
        for dy in [4.0, 8.0, 12.0, 16.0] {
            i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(10));
            w.dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(from.x, from.y + dy) });
        }
    }

    /// Moves the drag to `to` and lets go.
    fn drop_at(op: &Operator, to: LogicalPosition) {
        let w = op.window();
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(10));
        w.dispatch_event(WindowEvent::PointerMoved { position: to });
        w.dispatch_event(WindowEvent::PointerReleased { position: to, button: PointerEventButton::Left });
    }

    #[test]
    fn rows_mark_selected_and_live() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B", "C"]);
        let snap = OperatorSnapshot::read(&app);
        let r = rows(&snap, Some(ids[1]), Some(ids[2]));
        assert_eq!(r.len(), 3);
        assert_eq!(r.iter().map(|r| r.num).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(r[0].id, ids[0].to_string());
        assert_eq!(r[0].kind, "text");
        assert_eq!(r[0].title, "A");
        assert_eq!(r[0].subtitle, "Announcement · 1 slide");
        assert_eq!(r.iter().map(|r| (r.selected, r.live)).collect::<Vec<_>>(), [(false, false), (true, false), (false, true)]);
        // An unknown kind draws the text icon (`I[it.kind] || I.text`).
        assert_eq!(kind_icon("mystery"), "text");
        assert_eq!(kind_icon("scripture"), "scripture");
    }

    #[test]
    fn window_rows_follow_snapshot_and_count() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B"]);
        let op = opened(&app);
        let m = model(&op);
        assert_eq!(m.iter().map(|r| r.title.to_string()).collect::<Vec<_>>(), ["A", "B"]);
        assert!(m[0].selected, "the first item is selected when nothing was");
        assert_eq!(op.ui.get_service_count(), "2 items");
        assert_eq!(find_all(&op, "ServiceColumn::row").len(), 2);
        // Showing B marks it live without a new snapshot.
        op.ops().cmd(crate::live::Command::Show { item_id: ids[1], slide: 0 }, None, |_| {});
        op.apply_live(crate::app::LiveUpdate::new(app.live_view()));
        assert!(model(&op)[1].live);
        assert!(!model(&op)[0].live);
    }

    #[test]
    fn drag_payload_round_trips_and_rejects_others() {
        let id = Uuid::new_v4().to_string();
        assert_eq!(drag_id(&drag_data(&id)), id);
        let mut other = DataTransfer::default();
        other.set_plain_text("John 3:16".into());
        assert_eq!(drag_id(&other), "");
        other.set_plain_text("svc:not-an-id".into());
        assert_eq!(drag_id(&other), "");
        assert_eq!(drag_id(&DataTransfer::default()), "");
    }

    #[test]
    fn drop_on_row_sends_its_index() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B", "C", "D"]);
        let op = opened(&app);
        // Drag A (row 0) onto row 2: `service/move {id: A, to: 2}`.
        row(&op, 0).mock_drag(center(&row(&op, 2)), PointerEventButton::Left);
        assert_eq!(order(&app), [ids[1], ids[2], ids[0], ids[3]]);
        refresh(&op);
        // Drag D (row 3) onto row 0.
        row(&op, 3).mock_drag(center(&row(&op, 0)), PointerEventButton::Left);
        assert_eq!(order(&app), [ids[3], ids[1], ids[2], ids[0]]);
        refresh(&op);
        // Dropping a row on itself does nothing.
        let r = center(&row(&op, 1));
        row(&op, 1).mock_drag(LogicalPosition::new(r.x + 20.0, r.y + 5.0), PointerEventButton::Left);
        assert_eq!(order(&app), [ids[3], ids[1], ids[2], ids[0]]);
        assert!(!op.state.borrow().dragging, "the drag finished");
    }

    #[test]
    fn click_selects_without_going_live() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B"]);
        let op = opened(&app);
        let before = app.live_view().version;
        op.state.borrow_mut().armed = Some(ids[0]);
        row(&op, 1).mock_single_click(PointerEventButton::Left);
        assert_eq!(op.state.borrow().selected, Some(ids[1]));
        assert_eq!(op.state.borrow().armed, None, "a click clears the Bible arm");
        assert!(model(&op)[1].selected && !model(&op)[0].selected);
        assert_eq!(app.live_view().version, before, "nothing went live");
    }

    #[test]
    fn double_click_shows_slide_zero() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B"]);
        let op = opened(&app);
        let r = row(&op, 1);
        r.mock_single_click(PointerEventButton::Left);
        r.mock_single_click(PointerEventButton::Left);
        let v = app.live_view();
        let c = v.cursor.expect("something is live");
        assert_eq!((c.item_id, c.slide), (ids[1], 0));
    }

    #[test]
    fn trash_removes_without_confirm() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B"]);
        let op = opened(&app);
        assert!(find(&op, "ServiceColumn::trash").is_none(), "the row actions show on hover only");
        op.window().dispatch_event(WindowEvent::PointerMoved { position: center(&row(&op, 0)) });
        let trash = find(&op, "ServiceColumn::trash").expect("trash on the hovered row");
        assert_eq!(find_all(&op, "ServiceColumn::trash").len(), 1);
        assert!(find(&op, "ServiceColumn::edit").is_some());
        trash.mock_single_click(PointerEventButton::Left);
        assert_eq!(order(&app), [ids[1]]);
        assert_eq!(op.state.borrow().selected, Some(ids[0]), "the click on trash did not select the row");
    }

    #[test]
    fn data_update_mid_drag_is_held_until_drag_finished() {
        let app = test_app();
        add_texts(&app, &["A", "B"]);
        let op = opened(&app);
        let at = center(&row(&op, 0));
        start_drag(&op, at);
        assert!(op.state.borrow().dragging, "the drag started");
        add_texts(&app, &["C"]);
        refresh(&op);
        assert_eq!(model(&op).len(), 2, "no rebuild while a row is dragged");
        assert_eq!(op.ui.get_service_count(), "2 items");
        // Let go over the same row: nothing moves, and the held snapshot is applied.
        drop_at(&op, LogicalPosition::new(at.x, at.y + 16.0));
        assert!(!op.state.borrow().dragging);
        assert_eq!(model(&op).len(), 3);
        assert_eq!(op.ui.get_service_count(), "3 items");
    }

    #[test]
    fn drop_uses_dragged_id_after_rows_rebuilt() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B", "C", "D"]);
        let op = opened(&app);
        let target = center(&row(&op, 3));
        start_drag(&op, center(&row(&op, 1)));
        // Another client puts X first while B is in the air.
        let x = Uuid::new_v4();
        app.data.lock().unwrap().services[0].items.insert(0, ServiceItem { id: x, kind: ItemKind::Text { title: "X".into(), body: "x".into() }, background: None, notes: String::new(), theme: None });
        refresh(&op);
        drop_at(&op, target);
        // `service/move {id: B, to: 3}` on [X, A, B, C, D]: B is the item that moved.
        assert_eq!(order(&app), [x, ids[0], ids[2], ids[1], ids[3]]);
        assert_eq!(model(&op).len(), 5, "the held snapshot was applied at the end of the drag");
    }

    #[test]
    fn empty_state_text() {
        let app = test_app();
        let op = open(&app);
        assert!(find(&op, "ServiceColumn::empty").is_none(), "nothing before the first read");
        refresh(&op);
        assert_eq!(find(&op, "ServiceColumn::empty-title").unwrap().accessible_label().unwrap(), "Your service is empty");
        assert_eq!(
            find(&op, "ServiceColumn::empty-text").unwrap().accessible_label().unwrap(),
            "Click “Add to service” below, type a Bible verse up top, or drag files onto this window."
        );
        assert!(find(&op, "ServiceColumn::add-btn").is_some());
        assert_eq!(op.ui.get_service_count(), "");
        add_texts(&app, &["A"]);
        refresh(&op);
        assert!(find(&op, "ServiceColumn::empty").is_none());
    }

    #[test]
    fn removing_the_selected_row_falls_back() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B"]);
        let op = opened(&app);
        row(&op, 1).mock_single_click(PointerEventButton::Left);
        app.data.lock().unwrap().services[0].items.remove(1);
        refresh(&op);
        assert_eq!(op.state.borrow().selected, Some(ids[0]));
        assert!(model(&op)[0].selected);
    }
}
