//! The service column (`renderService()`, `operator.js:123-153`): one row per item, click to select, double-click
//! to show slide 1, edit and remove on hover, and drag to reorder. The row model is one `VecModel` that is filled
//! with `set_vec` on a new snapshot and patched with `set_row_data` when only the selection or the live item moves.

use super::{view::{self, OperatorSnapshot}, window::Operator};
use crate::{
    live::Command,
    model::passage,
    ui::{ServiceRow, ServiceSlide, ops::outcome_toast, thumbs::{self, Pixels}},
};
use slint::{DataTransfer, Image, Model, ModelRc, VecModel};
use std::collections::HashSet;
use std::rc::{Rc, Weak};
use uuid::Uuid;

/// The picture list (the EasyWorship-style layout, `Layout::service_pictures`): each item shows its first slide,
/// and an arrow unfolds the rest under it. Which items are unfolded is kept for this session.
#[derive(Default)]
pub(super) struct Pics {
    expanded: HashSet<Uuid>,
    /// Bumped whenever the pictures are asked for again, so a late one for an older list is dropped.
    generation: u64,
}

/// The size the list's pictures are drawn at: the row's 96 px picture at up to 2x.
const PIC_SIZE: (u32, u32) = (192, 108);

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
            custom_order: matches!(it.item.kind, crate::model::ItemKind::Song { arrangement: Some(_), .. }),
            thumb: Image::default(),
            expanded: false,
            slide_count: it.slides.len() as i32,
            slides: ModelRc::default(),
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
        let (snap, selected) = { let st = self.state.borrow(); (st.snap.clone(), st.selected) };
        let Some(snap) = snap else { return };
        let mut list = rows(&snap, selected, self.live_item());
        let pictures = self.ui.get_service_pictures();
        if pictures { self.unfold_rows(&snap, &mut list) }
        self.service_rows.set_vec(list);
        self.ui.set_service_count(view::count_label(snap.items.len()).into());
        self.ui.set_service_count_short(snap.items.len().to_string().into());
        self.render_service_theme();
        if pictures { self.draw_pictures(&snap) }
    }

    /// The live item and slide, if any.
    fn live_place(&self) -> Option<(Uuid, usize)> {
        self.state.borrow().live.as_ref().and_then(|l| l.view().cursor.as_ref().map(|c| (c.item_id, c.slide)))
    }

    /// The picture list: the unfolded items get their slides (the pictures arrive from `draw_pictures`).
    fn unfold_rows(&self, snap: &OperatorSnapshot, list: &mut [ServiceRow]) {
        let live = self.live_place();
        let pics = self.service_pics.borrow();
        for (row, it) in list.iter_mut().zip(&snap.items) {
            if row.header || it.slides.len() < 2 || !pics.expanded.contains(&it.id) { continue }
            row.expanded = true;
            let slides: Vec<ServiceSlide> = it.slides.iter().enumerate().map(|(i, s)| ServiceSlide {
                idx: i as i32,
                image: Image::default(),
                label: s.label.as_str().into(),
                text: super::slides::slide_text(s).replace('\n', " / ").into(),
                live: live == Some((it.id, i)),
            }).collect();
            row.slides = ModelRc::new(VecModel::from(slides));
        }
    }

    /// Asks for the list's pictures: every item's first slide, and all the slides of the unfolded ones. One that
    /// was drawn before comes back at once from the thumbnails' own cache.
    fn draw_pictures(&self, snap: &OperatorSnapshot) {
        let (generation, expanded) = {
            let mut pics = self.service_pics.borrow_mut();
            pics.generation += 1;
            (pics.generation, pics.expanded.clone())
        };
        let media: Vec<_> = snap.data.media.iter().filter(|m| m.thumb.is_some()).cloned().collect();
        for (row, it) in snap.items.iter().enumerate() {
            if it.slides.is_empty() || matches!(it.item.kind, crate::model::ItemKind::Header { .. }) { continue }
            let settings = thumbs::item_settings(&snap.data, &it.item);
            let verses = passage(&it.kind, &it.slides);
            let count = if expanded.contains(&it.id) { it.slides.len() } else { 1 };
            for (idx, slide) in it.slides.iter().take(count).enumerate() {
                let (key, job) = thumbs::thumb(slide, &it.kind, &verses, None, &settings, &media, PIC_SIZE);
                let (me, id) = (self.me.clone(), it.id);
                self.service_thumbs.request(key, job, move |px| if let Some(op) = me.upgrade() { op.picture_drawn(generation, row, id, idx, px) });
            }
        }
    }

    /// A picture for the list arrived: slide `idx` of the item in row `row`.
    fn picture_drawn(&self, generation: u64, row: usize, id: Uuid, idx: usize, px: Pixels) {
        if self.service_pics.borrow().generation != generation { return }
        let Some(mut r) = self.service_rows.row_data(row) else { return };
        if r.id != id.to_string().as_str() { return }
        let image = Image::from_rgba8(px);
        if r.expanded && let Some(mut s) = r.slides.row_data(idx) {
            s.image = image.clone();
            r.slides.set_row_data(idx, s);
        }
        if idx == 0 {
            r.thumb = image;
            self.service_rows.set_row_data(row, r);
        }
    }

    /// The arrow on a row of the picture list: unfolds the item's slides, or folds them away.
    fn toggle_expand(self: &Rc<Self>, id: Uuid) {
        {
            let mut pics = self.service_pics.borrow_mut();
            if !pics.expanded.remove(&id) { pics.expanded.insert(id); }
        }
        self.render_service();
    }

    /// A click on a slide of an unfolded item: the item is selected and that slide goes live.
    fn show_item_slide(self: &Rc<Self>, id: Uuid, slide: usize) {
        self.select_item(id);
        self.state.borrow_mut().list = super::window::ListFocus::Live;
        let me = Rc::downgrade(self);
        self.ops.cmd(Command::Show { item_id: id, slide }, None, move |o| {
            if let (Some(op), Some((msg, err))) = (me.upgrade(), outcome_toast(&o)) { op.toast(&msg, err); }
        });
    }

    /// Moves the selected and live marks without rebuilding the rows (a click or a `Live` update).
    pub(super) fn mark_service(&self) {
        let (selected, live) = (self.state.borrow().selected, self.live_item());
        let is = |id: Option<Uuid>, row: &ServiceRow| id.is_some_and(|id| row.id == id.to_string().as_str());
        for i in 0..self.service_rows.row_count() {
            let Some(mut row) = self.service_rows.row_data(i) else { continue };
            let (s, l) = (is(selected, &row), is(live, &row));
            // The picture list, unfolded: the live slide's mark moves with the screen.
            if row.expanded {
                let place = self.live_place();
                for k in 0..row.slides.row_count() {
                    let Some(mut slide) = row.slides.row_data(k) else { continue };
                    let on = l && place.is_some_and(|(_, at)| at == k);
                    if slide.live != on {
                        slide.live = on;
                        row.slides.set_row_data(k, slide);
                    }
                }
            }
            if row.selected != s || row.live != l {
                row.selected = s;
                row.live = l;
                self.service_rows.set_row_data(i, row);
            }
        }
    }

    /// A click on a row: select it and drop the Bible arm; nothing goes live (`operator.js:134`).
    pub(super) fn select_item(&self, id: Uuid) {
        {
            let mut st = self.state.borrow_mut();
            st.selected = Some(id);
            st.armed = None;
            st.list = super::window::ListFocus::Service;
        }
        self.mark_service();
        self.render_slides();
    }

    /// A double-click: show the item's first slide, if it has one (`operator.js:135`).
    fn show_first(self: &Rc<Self>, id: Uuid) {
        self.state.borrow_mut().list = super::window::ListFocus::Live;
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
    pub(super) fn service_action(self: &Rc<Self>, action: &'static str, body: serde_json::Value) {
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
    ui.on_service_toggle_expand(on(|op, id| op.toggle_expand(id)));
    let me = Rc::downgrade(op);
    ui.on_service_show_slide(move |id, slide| {
        if let (Some(op), Ok(id), Ok(slide)) = (me.upgrade(), Uuid::parse_str(&id), usize::try_from(slide)) { op.show_item_slide(id, slide) }
    });
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
            let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: t.to_string(), body: format!("{t} body"), layout: Default::default() }, background: None, notes: String::new(), theme: None };
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

    #[test]
    fn the_easyworship_layout_lists_pictures_and_unfolds_an_items_slides() {
        use crate::model::Song;
        let app = test_app();
        let song = Song { id: Uuid::new_v4(), title: "Amazing Grace".into(), lyrics: "Verse 1
Amazing grace

Verse 2
I once was lost

Verse 3
Through many dangers".into(), ..Default::default() };
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None };
        let id = item.id;
        {
            let mut d = app.data.lock().unwrap();
            d.songs.push(song);
            d.services[0].items.push(item);
        }
        add_texts(&app, &["Welcome"]);
        let op = opened(&app);
        assert!(model(&op).iter().all(|r| r.thumb.size().width == 0 && !r.expanded), "the other layouts keep the plain list");
        assert!(find(&op, "ServiceColumn::expand").is_none());

        op.dock_menu("layout:builtin:easyworship");
        let rows = model(&op);
        assert!(rows.iter().all(|r| r.thumb.size().width > 0), "every item shows its first slide");
        assert_eq!(rows.iter().map(|r| r.slide_count).collect::<Vec<_>>(), [3, 1]);
        assert_eq!(find_all(&op, "ServiceColumn::expand").len(), 1, "only an item with more slides than one has the arrow");

        // The arrow unfolds the song's slides under it, each with its picture.
        op.ui.invoke_service_toggle_expand(id.to_string().into());
        let song_row = &model(&op)[0];
        assert!(song_row.expanded);
        let slides: Vec<ServiceSlide> = song_row.slides.iter().collect();
        assert_eq!(slides.len(), 3);
        assert!(slides.iter().all(|s| s.image.size().width > 0 && !s.live));
        assert!(slides[1].text.contains("I once was lost"), "{:?}", slides[1].text);

        // A click on one of them takes it live, and the mark follows.
        op.ui.invoke_service_show_slide(id.to_string().into(), 2);
        op.apply_live(crate::app::LiveUpdate::new(op.app().live_view()));
        let cursor = app.live.lock().unwrap().cursor.clone().unwrap();
        assert_eq!((cursor.item_id, cursor.slide), (id, 2));
        let slides: Vec<ServiceSlide> = model(&op)[0].slides.iter().collect();
        assert_eq!(slides.iter().map(|s| s.live).collect::<Vec<_>>(), [false, false, true]);

        // Folded again, and back in Standard the plain list returns.
        op.ui.invoke_service_toggle_expand(id.to_string().into());
        assert!(!model(&op)[0].expanded);
        op.dock_menu("layout:builtin:standard");
        assert!(find(&op, "ServiceColumn::expand").is_none());
    }

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
    fn filter_box_hides_rows_without_the_words_and_drops_still_go_to_the_right_place() {
        let app = test_app();
        let ids = add_texts(&app, &["Welcome", "Amazing Grace", "Sermon", "Grace Alone"]);
        let op = opened(&app);
        let filter = find(&op, "ServiceColumn::service-filter").expect("the filter box under the list");
        filter.set_accessible_value("grace");
        let shown: Vec<String> = find_all(&op, "ServiceColumn::row").iter().map(|r| r.accessible_label().unwrap().into()).collect();
        assert_eq!(shown, ["Amazing Grace", "Grace Alone"]);
        // Dropping "Grace Alone" on "Amazing Grace" moves it to that row's place in the whole list (1, not 0).
        row(&op, 1).mock_drag(center(&row(&op, 0)), PointerEventButton::Left);
        assert_eq!(order(&app), [ids[0], ids[3], ids[1], ids[2]]);
        refresh(&op);
        filter.set_accessible_value("");
        assert_eq!(find_all(&op, "ServiceColumn::row").len(), 4);
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
    fn trash_under_the_list_removes_the_selected_item_without_confirm() {
        let app = test_app();
        let ids = add_texts(&app, &["A", "B"]);
        let op = opened(&app);
        // One Edit and one Remove, in the bar under the list, for the selected item (the first).
        assert_eq!(find_all(&op, "ServiceColumn::trash").len(), 1);
        assert_eq!(find_all(&op, "ServiceColumn::edit").len(), 1);
        // Nothing pops up on a row as the pointer passes over it.
        op.window().dispatch_event(WindowEvent::PointerMoved { position: center(&row(&op, 1)) });
        assert_eq!(find_all(&op, "ServiceColumn::trash").len(), 1);
        assert_eq!(find_all(&op, "ServiceColumn::edit").len(), 1);
        let trash = find(&op, "ServiceColumn::trash").expect("Remove under the list");
        assert!(trash.absolute_position().y > row(&op, 1).absolute_position().y, "under the rows, not on them");
        trash.mock_single_click(PointerEventButton::Left);
        assert_eq!(order(&app), [ids[1]]);
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
        app.data.lock().unwrap().services[0].items.insert(0, ServiceItem { id: x, kind: ItemKind::Text { title: "X".into(), body: "x".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None });
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
        op.window().set_size(slint::LogicalSize::new(1440.0, 900.0));
        slint::platform::update_timers_and_animations();
        assert!(find(&op, "ServiceColumn::empty").is_none(), "nothing before the first read");
        refresh(&op);
        assert_eq!(find(&op, "ServiceColumn::empty-title").unwrap().accessible_label().unwrap(), "Your service is empty");
        assert_eq!(
            find(&op, "ServiceColumn::empty-text").unwrap().accessible_label().unwrap(),
            "Add songs, videos and slides, type a Bible verse up top, or drag files onto this window."
        );
        assert!(find(&op, "ServiceColumn::add-btn").is_some());
        assert!(find(&op, "ServiceColumn::empty-add").is_some(), "N7: the empty list has its own big Add button");
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
