//! The slides column (`renderGrid()` and `markLive()`, `operator.js:156-197`): the selected item's header, its
//! slides as thumbnails in rows of `columns()` cells, and the empty states. Thumbnails are drawn by `Thumbs` off
//! the UI thread; until one arrives its cell is black. Clicking a thumbnail takes that slide live at once (there
//! is no separate Preview step).
use super::{service::kind_icon, strings as tr, view::{self, ItemRow, OperatorSnapshot}, window::Operator};
use crate::{
    live::Command,
    model::{Background, MediaFile, Settings, Slide, SlideContent, passage},
    ui::{SlideCell, SlideJump, SlideRowCells, ops::outcome_toast, thumbs::{self, Pixels}},
};
use slint::{ComponentHandle, Image, Model, ModelRc, VecModel};
use std::rc::{Rc, Weak};
use uuid::Uuid;

/// `.grid`: `minmax(230px, 1fr)` cells, 14 px gaps, 16 px side padding (`operator.css:92`).
const MIN_CELL: f32 = 230.0;
const GAP: f32 = 14.0;
const PAD: f32 = 16.0;

/// Cells per row for a grid `width_px` wide, as CSS `repeat(auto-fill, minmax(230px, 1fr))` lays them out.
pub fn columns(width_px: f32) -> usize {
    ((width_px - 2.0 * PAD + GAP) / (MIN_CELL + GAP)).floor().max(1.0) as usize
}

/// How wide each of `cols` cells is in a grid `width_px` wide (the `1fr` share).
fn cell_width(width_px: f32, cols: usize) -> f32 {
    let cols = cols.max(1) as f32;
    ((width_px - 2.0 * PAD - GAP * (cols - 1.0)) / cols).max(1.0)
}

/// The slide indexes of each row, `cols` to a row.
pub fn chunk_rows(n: usize, cols: usize) -> Vec<Vec<usize>> {
    (0..n).collect::<Vec<_>>().chunks(cols.max(1)).map(<[usize]>::to_vec).collect()
}

/// The row slide `idx` is in.
pub fn row_of(idx: usize, cols: usize) -> usize {
    idx / cols.max(1)
}

/// A slide label's section colour name and its short form: "Verse 2" → ("verse", "V2"), "Chorus" → ("chorus", "C").
/// Anything else (a passage reference, a slide title) has no section: ("", "").
pub fn section(label: &str) -> (&'static str, String) {
    let lower = label.trim().to_lowercase();
    let (word, rest) = lower.split_once(' ').unwrap_or((lower.as_str(), ""));
    let (id, letter) = match word {
        "verse" => ("verse", "V"),
        "chorus" | "refrain" => ("chorus", "C"),
        "pre-chorus" | "prechorus" => ("pre", "P"),
        "bridge" => ("bridge", "B"),
        "tag" | "ending" | "outro" => ("end", "E"),
        _ => return ("", String::new()),
    };
    // A label like "Verse of the day" is not a section.
    if !rest.is_empty() && !rest.chars().all(|c| c.is_ascii_digit()) { return ("", String::new()) }
    (id, format!("{letter}{rest}"))
}

/// The jump buttons of a song: each section once, at its first slide, in order.
pub fn jumps(kind: &str, slides: &[Slide]) -> Vec<SlideJump> {
    if kind != "song" { return Vec::new() }
    let mut seen: Vec<&str> = Vec::new();
    slides.iter().enumerate().filter_map(|(i, s)| {
        if seen.contains(&s.label.as_str()) { return None }
        seen.push(&s.label);
        let (id, short) = section(&s.label);
        (!id.is_empty()).then(|| SlideJump { idx: i as i32, short: short.into(), section: id.into() })
    }).collect()
}

/// The header title (`operator.js:162`): the selected item's, else a hint that depends on the service being empty.
pub fn header_title(snap: &OperatorSnapshot, item: Option<&ItemRow>) -> String {
    match item {
        Some(it) => it.title.clone(),
        None if snap.items.is_empty() => tr::WELCOME.into(),
        None => tr::PICK_AN_ITEM.into(),
    }
}

/// What the grid was built from (the web's `gridKey`): a change rebuilds it, anything else only moves the live mark.
#[derive(Clone, PartialEq)]
struct GridKey { id: Uuid, kind: String, slides: Vec<Slide>, background: Option<Background>, settings: Settings, media: Vec<MediaFile> }

/// The grid between updates.
#[derive(Default)]
pub(super) struct SlidesState {
    built: Option<GridKey>,
    cols: usize,
    /// The size thumbnails are drawn at (`thumbs::thumb_size` of the cell width).
    size: (u32, u32),
    /// Bumped whenever the thumbnails are asked for again, so a late one for an older grid is dropped.
    generation: u64,
    /// One per slide; empty until drawn.
    images: Vec<Image>,
    live: Option<usize>,
    /// The live slide last scrolled into view: the grid scrolls only when it changes.
    scrolled: Option<(Uuid, usize)>,
}

impl Operator {
    /// `renderGrid()`: the header, then the grid when what it shows changed, then the live mark.
    pub(super) fn render_slides(&self) {
        let (snap, selected) = { let st = self.state.borrow(); (st.snap.clone(), st.selected) };
        let Some(snap) = snap else { return };
        let item = selected.and_then(|id| snap.items.iter().find(|i| i.id == id));
        let ui = &self.ui;
        ui.set_slides_title(header_title(&snap, item).into());
        ui.set_slides_kind(item.map_or("", |i| kind_icon(&i.kind)).into());
        ui.set_has_selection(item.is_some());
        ui.set_slide_count(item.map_or(0, |i| i.slides.len()) as i32);
        ui.set_slides_note(item.map_or_else(String::new, |i| i.notes.join("\n")).into());
        self.render_text_size();
        ui.set_slide_jumps(ModelRc::new(VecModel::from(item.map_or_else(Vec::new, |i| jumps(&i.kind, &i.slides)))));
        let key = item.map(|it| GridKey {
            id: it.id, kind: it.kind.clone(), slides: it.slides.clone(), background: it.item.background.clone(), settings: snap.settings.clone(),
            media: snap.data.media.iter().filter(|m| m.thumb.is_some()).cloned().collect(),
        });
        if self.slides.borrow().built != key {
            let other_item = self.slides.borrow().built.as_ref().map(|k| k.id) != key.as_ref().map(|k| k.id);
            if other_item { ui.invoke_slides_scroll_to_top(); }
            {
                let mut s = self.slides.borrow_mut();
                s.images = vec![Image::default(); key.as_ref().map_or(0, |k| k.slides.len())];
                s.built = key;
                s.live = None;
                s.scrolled = None;
            }
            self.measure_grid(ui.get_slides_grid_width());
            self.fill_grid();
            self.draw_thumbs();
        }
        self.mark_slides();
    }

    /// `markLive()`: marks the live slide of the selected item, and scrolls it into view when it changed.
    pub(super) fn mark_slides(&self) {
        let live = { let st = self.state.borrow(); st.live.as_ref().and_then(|l| view::live_index(l.view(), st.selected)) };
        let (old, live, scroll) = {
            let mut s = self.slides.borrow_mut();
            let Some(k) = &s.built else { return };
            let (id, live) = (k.id, live.filter(|i| *i < k.slides.len()));
            let old = std::mem::replace(&mut s.live, live);
            let target = live.map(|i| (id, i));
            let scroll = live.filter(|_| target != s.scrolled).map(|i| row_of(i, s.cols));
            s.scrolled = target;
            (old, live, scroll)
        };
        if old != live { for i in [old, live].into_iter().flatten() { self.set_cell(i) } }
        if let Some(row) = scroll { self.ui.invoke_slides_scroll_to_row(row as i32) }
    }

    /// Works out the columns and the thumbnail size for a grid `width` wide; says whether each changed.
    fn measure_grid(&self, width: f32) -> (bool, bool) {
        let cols = columns(width);
        let size = thumbs::thumb_size(cell_width(width, cols), self.ui.window().scale_factor());
        let mut s = self.slides.borrow_mut();
        let changed = (s.cols != cols, s.size != size);
        (s.cols, s.size) = (cols, size);
        changed
    }

    /// The grid was resized: new rows when the column count changed, new thumbnails when their size did.
    fn grid_resized(&self, width: f32) {
        let (cols, size) = self.measure_grid(width);
        if self.slides.borrow().built.is_none() { return }
        if cols {
            self.fill_grid();
            self.slides.borrow_mut().scrolled = None;
            self.mark_slides();
        }
        if size { self.draw_thumbs() }
    }

    fn cell(s: &SlidesState, k: &GridKey, idx: usize) -> SlideCell {
        let slide = &k.slides[idx];
        SlideCell {
            idx: idx as i32,
            label: slide.label.as_str().into(),
            image: s.images.get(idx).cloned().unwrap_or_default(),
            live: s.live == Some(idx),
            play: matches!(slide.content, SlideContent::Video { .. } | SlideContent::Audio { .. }),
            // Only songs are coloured: a passage or slide set has no sections.
            section: if k.kind == "song" { section(&slide.label).0 } else { "" }.into(),
        }
    }

    /// Rebuilds the rows from the slides and the thumbnails drawn so far.
    fn fill_grid(&self) {
        let s = self.slides.borrow();
        self.ui.set_slide_columns(s.cols as i32);
        let Some(k) = &s.built else { return self.slide_rows.set_vec(Vec::new()) };
        let rows: Vec<SlideRowCells> = chunk_rows(k.slides.len(), s.cols).into_iter().map(|r| SlideRowCells {
            cells: ModelRc::new(VecModel::from(r.into_iter().map(|i| Self::cell(&s, k, i)).collect::<Vec<_>>())),
        }).collect();
        self.slide_rows.set_vec(rows);
    }

    /// Redraws one cell in place (its thumbnail arrived, or the live mark moved).
    fn set_cell(&self, idx: usize) {
        let s = self.slides.borrow();
        let Some(k) = &s.built else { return };
        if idx >= k.slides.len() { return }
        let cell = Self::cell(&s, k, idx);
        let cols = s.cols.max(1);
        if let Some(row) = self.slide_rows.row_data(row_of(idx, cols)) { row.cells.set_row_data(idx % cols, cell) }
    }

    /// Asks for every slide's thumbnail at the current size; requests for an older grid are dropped.
    fn draw_thumbs(&self) {
        let (generation, jobs) = {
            let mut s = self.slides.borrow_mut();
            s.generation += 1;
            let Some(k) = &s.built else { return };
            let passage = passage(&k.kind, &k.slides);
            let jobs: Vec<_> = k.slides.iter().enumerate()
                .map(|(i, slide)| (i, thumbs::thumb(slide, &k.kind, &passage, k.background.as_ref(), &k.settings, &k.media, s.size)))
                .collect();
            (s.generation, jobs)
        };
        self.thumbs.cancel_pending();
        for (idx, (key, job)) in jobs {
            let me = self.me.clone();
            self.thumbs.request(key, job, move |px| if let Some(op) = me.upgrade() { op.thumb_drawn(generation, idx, px) });
        }
    }

    fn thumb_drawn(&self, generation: u64, idx: usize, px: Pixels) {
        {
            let mut s = self.slides.borrow_mut();
            if s.generation != generation || idx >= s.images.len() { return }
            s.images[idx] = Image::from_rgba8(px);
        }
        self.set_cell(idx);
    }

    /// A click on a thumbnail: drop the Bible arm and take that slide live (`operator.js:182`).
    fn show_slide(self: &Rc<Self>, idx: usize) {
        let item = {
            let mut st = self.state.borrow_mut();
            st.armed = None;
            st.selected
        };
        let Some(item_id) = item else { return };
        let me = Rc::downgrade(self);
        self.ops.cmd(Command::Show { item_id, slide: idx }, None, move |o| {
            if let (Some(op), Some((msg, err))) = (me.upgrade(), outcome_toast(&o)) { op.toast(&msg, err); }
        });
    }
}

/// Connects the column's callbacks. Each holds a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let ui = &op.ui;
    ui.set_slide_rows(ModelRc::from(op.slide_rows.clone()));
    let me: Weak<Operator> = Rc::downgrade(op);
    ui.on_slides_show(move |idx| {
        if let (Some(op), Ok(idx)) = (me.upgrade(), usize::try_from(idx)) { op.show_slide(idx) }
    });
    // Edit opens Item edit (Task 18).
    let me = Rc::downgrade(op);
    ui.on_slides_edit(move || {
        let Some(op) = me.upgrade() else { return };
        let selected = op.state.borrow().selected;
        if let Some(id) = selected { crate::ui::dialogs::item_edit::open(&op, id) }
    });
    let me = Rc::downgrade(op);
    ui.on_slides_grid_resized(move |w| if let Some(op) = me.upgrade() { op.grid_resized(w) });
}

#[cfg(test)]
mod section_tests {
    use super::*;

    #[test]
    fn sections_and_short_forms() {
        assert_eq!(section("Verse 2"), ("verse", "V2".to_string()));
        assert_eq!(section("Chorus"), ("chorus", "C".to_string()));
        assert_eq!(section("Pre-Chorus 1"), ("pre", "P1".to_string()));
        assert_eq!(section("Verse of the day"), ("", String::new()));
        assert_eq!(section("John 3:16"), ("", String::new()));
    }

    #[test]
    fn jumps_list_each_section_once_for_songs_only() {
        let s = |l: &str| Slide { label: l.into(), content: SlideContent::Text { text: String::new(), footer: None, reference: None, size: 0, parallel: None }, notes: String::new() };
        let slides = [s("Verse 1"), s("Verse 1"), s("Chorus"), s("Verse 2"), s("Chorus")];
        let got: Vec<(i32, String)> = jumps("song", &slides).into_iter().map(|j| (j.idx, j.short.to_string())).collect();
        assert_eq!(got, vec![(0, "V1".to_string()), (2, "C".to_string()), (3, "V2".to_string())]);
        assert!(jumps("scripture", &slides).is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, LiveUpdate, test_app},
        model::{ItemKind, ServiceItem},
        ui::operator::window::testing::{find, find_all, open, refresh},
    };
    use i_slint_backend_testing::ElementHandle;
    use slint::platform::PointerEventButton;
    use std::sync::Arc;

    /// An announcement with one slide per block of `body`; returns its id.
    fn add_text(app: &App, title: &str, body: &str) -> Uuid {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: body.into() }, background: None, notes: String::new() };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        item.id
    }

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op
    }

    fn cells(op: &Operator) -> Vec<SlideCell> { op.ui.get_slide_rows().iter().flat_map(|r| r.cells.iter().collect::<Vec<_>>()).collect() }

    fn thumb(op: &Operator, i: usize) -> ElementHandle { find_all(op, "SlidesPanel::thumb").swap_remove(i) }

    fn show(op: &Operator, item_id: Uuid, slide: usize) {
        op.ops().cmd(Command::Show { item_id, slide }, None, |_| {});
        op.apply_live(LiveUpdate::new(op.app().live_view()));
    }

    #[test]
    fn passage_note_under_the_grid_names_missing_verses() {
        let app = test_app();
        let vs = |n: u16| (16..16 + n).map(|v| crate::model::VerseText::new(format!("John 3:{v}"), "words")).collect::<Vec<_>>();
        let also = vec![crate::scripture::AlsoShow { translation: "WEB".into(), verses: vs(1) }];
        let kind = ItemKind::Scripture { reference: "John 3:16-17".into(), translation: "KJV".into(), verses: vs(2), per_slide: 0, text_size: 0, also, layout: Default::default() };
        let item = ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new() };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        let song = add_text(&app, "A", "a");
        let op = opened(&app);
        op.ui.invoke_service_select(item.id.to_string().into());
        assert_eq!(op.ui.get_slides_note(), "WEB doesn't include John 3:17.");
        assert!(find(&op, "SlidesPanel::slides-note").is_some());
        op.ui.invoke_service_select(song.to_string().into());
        assert_eq!(op.ui.get_slides_note(), "", "other items have no note");
        assert!(find(&op, "SlidesPanel::slides-note").is_none());
    }

    #[test]
    fn columns_follow_css_auto_fill() {
        assert_eq!(columns(558.0), 2);
        assert_eq!(columns(230.0), 1);
        assert_eq!(columns(1000.0), 4);
        assert_eq!(columns(0.0), 1, "never fewer than one");
        // 2 × 230 + 14 gap + 32 padding is the first width with two columns.
        assert_eq!(columns(505.0), 1);
        assert_eq!(columns(506.0), 2);
        assert_eq!(cell_width(558.0, 2), 256.0);
    }

    #[test]
    fn chunk_rows_and_row_of() {
        assert_eq!(chunk_rows(5, 2), vec![vec![0, 1], vec![2, 3], vec![4]]);
        assert_eq!(chunk_rows(0, 3), Vec::<Vec<usize>>::new());
        assert_eq!(chunk_rows(3, 4), vec![vec![0, 1, 2]]);
        assert_eq!((row_of(0, 3), row_of(2, 3), row_of(3, 3), row_of(7, 3)), (0, 0, 1, 2));
    }

    #[test]
    fn click_thumb_takes_slide_live() {
        let app = test_app();
        let id = add_text(&app, "Welcome", "One\n\nTwo\n\nThree");
        let op = opened(&app);
        op.state.borrow_mut().armed = Some(id);
        assert_eq!(find_all(&op, "SlidesPanel::thumb").len(), 3);
        thumb(&op, 2).mock_single_click(PointerEventButton::Left);
        let c = app.live_view().cursor.expect("something is live");
        assert_eq!((c.item_id, c.slide), (id, 2));
        assert_eq!(op.state.borrow().armed, None, "a thumbnail click clears the Bible arm");
    }

    #[test]
    fn live_badge_only_on_cursor_slide_of_selected_item() {
        let app = test_app();
        let a = add_text(&app, "A", "a1\n\na2\n\na3");
        let b = add_text(&app, "B", "b1\n\nb2");
        let op = opened(&app);
        assert_eq!(op.state.borrow().selected, Some(a));
        show(&op, b, 1);
        assert!(cells(&op).iter().all(|c| !c.live), "the live slide is on another item");
        assert!(find(&op, "Thumb::badge").is_none());
        show(&op, a, 1);
        assert_eq!(cells(&op).iter().map(|c| c.live).collect::<Vec<_>>(), [false, true, false]);
        assert_eq!(find_all(&op, "Thumb::badge").len(), 1);
        // Clearing the screen leaves the cursor but takes the mark off (nothing on air).
        op.ops().cmd(Command::ClearAll, None, |_| {});
        op.apply_live(LiveUpdate::new(app.live_view()));
        assert!(cells(&op).iter().all(|c| !c.live));
    }

    #[test]
    fn welcome_shown_when_service_empty() {
        let app = test_app();
        let op = open(&app);
        assert!(find(&op, "SlidesPanel::welcome").is_none(), "nothing before the first read");
        refresh(&op);
        assert!(find(&op, "SlidesPanel::welcome").is_some());
        add_text(&app, "A", "a");
        refresh(&op);
        assert!(find(&op, "SlidesPanel::welcome").is_none());
        assert!(find(&op, "SlidesPanel::choose").is_none(), "the first item is selected");
    }

    #[test]
    fn header_title_pick_an_item_or_welcome() {
        let app = test_app();
        let op = open(&app);
        assert_eq!(op.ui.get_slides_title(), "Nothing selected", "before the first read");
        refresh(&op);
        assert_eq!(op.ui.get_slides_title(), "Welcome");
        assert_eq!(find(&op, "SlidesPanel::slides-title").unwrap().accessible_label().unwrap(), "Welcome");
        add_text(&app, "Announcements", "a");
        let snap = OperatorSnapshot::read(&app);
        assert_eq!(header_title(&snap, None), "Pick an item on the left");
        assert_eq!(header_title(&snap, snap.items.first()), "Announcements");
        refresh(&op);
        assert_eq!(op.ui.get_slides_title(), "Announcements");
        assert_eq!(op.ui.get_slides_kind(), "text");
    }

    #[test]
    fn edit_button_only_with_selection() {
        let app = test_app();
        let op = opened(&app);
        assert!(find(&op, "SlidesPanel::slides-edit").is_none());
        add_text(&app, "A", "a");
        refresh(&op);
        assert!(find(&op, "SlidesPanel::slides-edit").is_some());
    }

    #[test]
    fn empty_states_text() {
        let app = test_app();
        // A song that is not in the library has no slides.
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: Uuid::new_v4() }, background: None, notes: String::new() };
        app.data.lock().unwrap().services[0].items.push(item);
        let op = opened(&app);
        assert_eq!(find(&op, "Empty::empty-title").unwrap().accessible_label().unwrap(), "Nothing to show");
        assert_eq!(find(&op, "Empty::empty-text").unwrap().accessible_label().unwrap(), "This item has no slides yet. Click Edit to add some.");
        op.state.borrow_mut().selected = None;
        op.render_slides();
        assert!(find(&op, "SlidesPanel::nothing").is_none());
        assert_eq!(find(&op, "Empty::empty-text").unwrap().accessible_label().unwrap(), "Choose something from the service list.");
        assert_eq!(op.ui.get_slides_title(), "Pick an item on the left");
    }

    #[test]
    fn thumbnails_arrive_and_rows_follow_width() {
        let app = test_app();
        add_text(&app, "A", "1\n\n2\n\n3\n\n4\n\n5");
        let op = opened(&app);
        let cols = columns(op.ui.get_slides_grid_width());
        assert!(cols >= 2, "a 1400 px window fits more than one column, got {cols}");
        let rows: Vec<usize> = op.ui.get_slide_rows().iter().map(|r| r.cells.row_count()).collect();
        assert_eq!(rows, chunk_rows(5, cols).iter().map(Vec::len).collect::<Vec<_>>());
        assert!(cells(&op).iter().all(|c| c.image.size().width > 0), "inline thumbnails are drawn at once");
        // Narrower: fewer columns, and the drawn thumbnails are kept.
        op.window().set_size(slint::LogicalSize::new(1024.0, 900.0));
        slint::platform::update_timers_and_animations();
        let narrow = columns(op.ui.get_slides_grid_width());
        assert!(narrow < cols);
        assert_eq!(op.ui.get_slide_rows().row_count(), chunk_rows(5, narrow).len());
        assert!(cells(&op).iter().all(|c| c.image.size().width > 0));
    }

    #[test]
    fn video_cell_has_play_overlay() {
        let app = test_app();
        let media = crate::model::MediaFile { id: Uuid::new_v4(), name: "clip.mp4".into(), kind: crate::model::MediaKind::Video, asset: "clip.mp4".into(), status: crate::model::MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Media { media_id: media.id, autoplay: true, end: Default::default(), looped: false }, background: None, notes: String::new() };
        { let mut d = app.data.lock().unwrap(); d.media.push(media); d.services[0].items.push(item); }
        let op = opened(&app);
        assert!(cells(&op)[0].play);
    }
}
