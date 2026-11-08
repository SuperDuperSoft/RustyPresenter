//! The slides column (`renderGrid()` and `markLive()`, `operator.js:156-197`): the selected item's header, its
//! slides as thumbnails in rows of `columns()` cells, and the empty states. Thumbnails are drawn by `Thumbs` off
//! the UI thread; until one arrives its cell is black. Clicking a thumbnail takes that slide live at once (there
//! is no separate Preview step).
use super::{service::kind_icon, strings as tr, view::{self, ItemRow, OperatorSnapshot}, window::Operator};
use crate::{app::LiveView, sections::{SectionKind, occurrence_views, strings as song}};
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
pub fn columns(width_px: f32) -> usize { columns_for(width_px, MIN_CELL) }

/// `columns` with cells at least `min_cell` wide (the size slider, N8).
pub fn columns_for(width_px: f32, min_cell: f32) -> usize {
    ((width_px - 2.0 * PAD + GAP) / (min_cell + GAP)).floor().max(1.0) as usize
}

/// The size slider's ends, in pixels of the smallest picture width (`thumb-size` in `slides.slint`).
pub const SIZE_RANGE: (u16, u16) = (140, 420);

/// The picture in a Table row is this wide (`TableRow` in `slides.slint`).
const TABLE_THUMB: f32 = 96.0;

/// N37: how the selected item's slides are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SlideView {
    /// Pictures in rows, as many to a row as fit.
    #[default]
    Grid,
    /// One slide to a row with its words in big type, for a small laptop screen.
    Easy,
    /// One line per slide: number, small picture, part and words.
    Table,
}

impl SlideView {
    pub fn index(self) -> i32 { match self { SlideView::Grid => 0, SlideView::Easy => 1, SlideView::Table => 2 } }
    pub fn at(i: i32) -> Option<SlideView> { Some(match i { 0 => SlideView::Grid, 1 => SlideView::Easy, 2 => SlideView::Table, _ => return None }) }
}

/// What a slide says, for the Easy and Table views: its words on one line each, or what kind of file it shows.
pub fn slide_text(slide: &Slide) -> String {
    match &slide.content {
        SlideContent::Text { text, .. } => text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n"),
        SlideContent::Image { .. } | SlideContent::Still { .. } => super::strings::media_picture(),
        SlideContent::Video { .. } => super::strings::media_video(),
        SlideContent::Audio { .. } => super::strings::media_audio(),
        SlideContent::Design { design, .. } => design.words(),
    }
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

/// A song slide's caption colour: its section kind (`SectionKind::id`); "" for other slides and unlabelled parts.
pub fn section(slide: &Slide) -> &'static str {
    slide.section.as_ref().filter(|r| r.kind != SectionKind::Other).map_or("", |r| r.kind.id())
}

/// Settings → Sections' colours into `Theme` (the captions, the section bar and the order chips).
pub fn apply_section_colors(t: &crate::ui::Theme) {
    let c = |k: SectionKind| crate::ui::dialogs::services::parse_color(&k.color()).unwrap_or_default();
    let now = [t.get_sec_verse(), t.get_sec_chorus(), t.get_sec_pre(), t.get_sec_bridge(), t.get_sec_end(), t.get_sec_tag(), t.get_sec_intro(), t.get_sec_interlude()];
    let want = [c(SectionKind::Verse), c(SectionKind::Chorus), c(SectionKind::PreChorus), c(SectionKind::Bridge), c(SectionKind::Ending), c(SectionKind::Tag),
        c(SectionKind::Intro), c(SectionKind::Interlude)];
    if now == want { return }
    let [verse, chorus, pre, bridge, end, tag, intro, interlude] = want;
    t.set_sec_verse(verse);
    t.set_sec_chorus(chorus);
    t.set_sec_pre(pre);
    t.set_sec_bridge(bridge);
    t.set_sec_end(end);
    t.set_sec_tag(tag);
    t.set_sec_intro(intro);
    t.set_sec_interlude(interlude);
}

/// A slide label's colour on its thumbnail (N19): as Settings → Sections lists it, grey for a label not there.
pub fn tag_color(look: &crate::sections::SectionLook, name: &str) -> slint::Color {
    let hex = look.label_color(name).unwrap_or_else(|| "#8d95a8".into());
    crate::ui::dialogs::services::parse_color(&hex).unwrap_or_default()
}

/// One chip of the section bar, as a click needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct BarChip { pub first_slide: usize, pub occurrence: u16, pub kind: SectionKind }

/// The section bar (FEAT-4 §3.1): one chip per section as the song plays. For the live item they come from the
/// live snapshot, the live one marked, with the jump key in the tooltip while section keys are on; for any other
/// item, from its slides as built now.
pub fn jumps(slides: &[Slide], live: Option<&LiveView>, keys_on: bool) -> (Vec<SlideJump>, Vec<BarChip>) {
    let views = match live { Some(v) => v.sections.clone(), None => occurrence_views(slides, None) };
    views.iter().map(|o| {
        let tooltip = match (live, o.key) {
            (Some(_), Some(k)) if keys_on => song::chip_key_hint(&o.label, k),
            (Some(_), _) => o.label.clone(),
            (None, _) => song::chip_show_hint().into(),
        };
        let jump = SlideJump { idx: o.first_slide as i32, short: o.short.as_str().into(), section: o.kind.id().into(), live: o.live, tooltip: tooltip.into() };
        (jump, BarChip { first_slide: o.first_slide, occurrence: o.occurrence, kind: o.kind })
    }).unzip()
}

/// The header title (`operator.js:162`): the selected item's, else a hint that depends on the service being empty.
pub fn header_title(snap: &OperatorSnapshot, item: Option<&ItemRow>) -> String {
    match item {
        Some(it) => it.title.clone(),
        None if snap.items.is_empty() => tr::welcome().into(),
        None => tr::pick_an_item().into(),
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
    /// The section bar's chips, and the live item and version when it shows the live song.
    bar: Vec<BarChip>,
    bar_live: Option<(Uuid, u64)>,
    /// N37 and N8: the view, and the size slider (0 until moved: `MIN_CELL`). Kept for this session only, like Zoom.
    view: SlideView,
    min_cell: u16,
    /// N15: each slide's action badge (how many, and their names).
    acts: Vec<(i32, String)>,
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
        ui.set_slides_custom_order(item.is_some_and(|i| matches!(i.item.kind, crate::model::ItemKind::Song { arrangement: Some(_), .. })));
        self.render_text_size();
        super::bible_extras::render(self);
        let key = item.map(|it| GridKey {
            id: it.id, kind: it.kind.clone(), slides: it.slides.clone(), background: None, settings: thumbs::item_settings(&snap.data, &it.item),
            media: snap.data.media.iter().filter(|m| m.thumb.is_some()).cloned().collect(),
        });
        let acts = super::action_palette::badges(self, key.as_ref().map_or(0, |k| k.slides.len()));
        let acts_changed = { let mut s = self.slides.borrow_mut(); let changed = s.acts != acts; s.acts = acts; changed };
        super::action_palette::render(self);
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
        } else if acts_changed {
            // In place, so the grid keeps its rows.
            let n = self.slides.borrow().built.as_ref().map_or(0, |k| k.slides.len());
            for i in 0..n { self.set_cell(i) }
        }
        self.mark_slides();
    }

    /// `markLive()`: marks the live slide of the selected item, and scrolls it into view when it changed.
    pub(super) fn mark_slides(&self) {
        self.render_section_bar();
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

    /// The section bar of the selected song, live or not.
    fn render_section_bar(&self) {
        let (chips, bar, bar_live) = {
            let st = self.state.borrow();
            let item = st.selected.and_then(|id| st.snap.as_ref()?.items.iter().find(|i| i.id == id));
            let keys_on = st.snap.as_ref().is_none_or(|s| s.settings.section_keys);
            let live = st.live.as_ref().map(|l| l.view()).filter(|v| v.cursor.as_ref().is_some_and(|c| Some(c.item_id) == st.selected));
            match item {
                Some(it) => {
                    let (chips, bar) = jumps(&it.slides, live, keys_on);
                    (chips, bar, live.map(|v| (it.id, v.version)))
                }
                None => Default::default(),
            }
        };
        let mut s = self.slides.borrow_mut();
        if s.bar == bar && s.bar_live == bar_live && self.ui.get_slide_jumps().row_count() == chips.len()
            && self.ui.get_slide_jumps().iter().eq(chips.iter().cloned()) { return }
        (s.bar, s.bar_live) = (bar, bar_live);
        drop(s);
        self.ui.set_slide_jumps(ModelRc::new(VecModel::from(chips)));
    }

    /// A section bar chip: on the live song it jumps there (the slides Next walks); else it takes that part live.
    fn pick_section(self: &Rc<Self>, i: usize) {
        let (chip, bar_live) = { let s = self.slides.borrow(); (s.bar.get(i).cloned(), s.bar_live) };
        let Some(chip) = chip else { return };
        let Some((item_id, version)) = bar_live else { return self.show_slide(chip.first_slide) };
        self.state.borrow_mut().armed = None;
        let me = Rc::downgrade(self);
        let cmd = Command::Jump { item_id, kind: chip.kind, number: None, occurrence: Some(chip.occurrence) };
        self.ops.cmd(cmd, Some(version), move |o| {
            let Some(op) = me.upgrade() else { return };
            if o.error.as_deref().is_some_and(|e| e.starts_with("stale")) { return op.toast(song::screen_changed(), true) }
            if let Some((msg, err)) = outcome_toast(&o) { op.toast(&msg, err) }
        });
    }

    /// Works out the columns and the thumbnail size for a grid `width` wide; says whether each changed.
    fn measure_grid(&self, width: f32) -> (bool, bool) {
        let (view, min_cell) = { let s = self.slides.borrow(); (s.view, s.min_cell) };
        let min_cell = if min_cell == 0 { MIN_CELL } else { f32::from(min_cell) };
        // Easy and Table show one slide to a row; Table's pictures are small.
        let (cols, cell) = match view {
            SlideView::Grid => { let cols = columns_for(width, min_cell); (cols, cell_width(width, cols)) }
            SlideView::Easy | SlideView::Table => (1, TABLE_THUMB),
        };
        let size = thumbs::thumb_size(cell, self.ui.window().scale_factor());
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

    /// The view buttons, View → Show slides as, or the size slider: the rows are laid out again.
    pub(super) fn set_slide_view(&self, view: SlideView, min_cell: Option<u16>) {
        {
            let mut s = self.slides.borrow_mut();
            s.view = view;
            if let Some(px) = min_cell { s.min_cell = px.clamp(SIZE_RANGE.0, SIZE_RANGE.1) }
        }
        self.ui.set_slides_view(view.index());
        if let Some(px) = min_cell { self.ui.set_slides_thumb_size(i32::from(px.clamp(SIZE_RANGE.0, SIZE_RANGE.1))) }
        // The rows are always laid out again: each view has its own row height.
        let (_, size) = self.measure_grid(self.ui.get_slides_grid_width());
        if self.slides.borrow().built.is_none() { return }
        self.fill_grid();
        self.slides.borrow_mut().scrolled = None;
        self.mark_slides();
        if size { self.draw_thumbs() }
    }

    /// The view the slides are shown in, for tests and the menu.
    pub fn slide_view(&self) -> SlideView { self.slides.borrow().view }

    fn cell(s: &SlidesState, k: &GridKey, idx: usize) -> SlideCell {
        let slide = &k.slides[idx];
        SlideCell {
            idx: idx as i32,
            label: slide.label.as_str().into(),
            image: s.images.get(idx).cloned().unwrap_or_default(),
            live: s.live == Some(idx),
            play: matches!(slide.content, SlideContent::Video { .. } | SlideContent::Audio { .. }),
            // Only songs are coloured: a passage or slide set has no sections.
            section: section(slide).into(),
            tag: slide.tag.as_deref().unwrap_or("").into(),
            tag_color: slide.tag.as_deref().map_or_else(slint::Color::default, |t| tag_color(&k.settings.sections, t)),
            text: slide_text(slide).into(),
            actions: s.acts.get(idx).map_or(0, |a| a.0),
            actions_tip: s.acts.get(idx).map(|a| a.1.as_str()).unwrap_or("").into(),
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
        // N29: armed for the stage only, the click goes there instead.
        if super::stage_only::take_click(self, item_id, idx) { return }
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
    ui.on_slides_pick_section(move |i| if let (Some(op), Ok(i)) = (me.upgrade(), usize::try_from(i)) { op.pick_section(i) });
    let me = Rc::downgrade(op);
    ui.on_slides_grid_resized(move |w| if let Some(op) = me.upgrade() { op.grid_resized(w) });
    let me = Rc::downgrade(op);
    ui.on_slides_view_picked(move |i| if let (Some(op), Some(v)) = (me.upgrade(), SlideView::at(i)) { op.set_slide_view(v, None) });
    let me = Rc::downgrade(op);
    ui.on_slides_thumb_size_changed(move |px| if let (Some(op), Ok(px)) = (me.upgrade(), u16::try_from(px)) {
        let view = op.slide_view();
        op.set_slide_view(view, Some(px));
    });
}

#[cfg(test)]
mod section_tests {
    use super::*;
    use crate::sections::refs::kit;

    #[test]
    fn captions_take_the_section_kind() {
        let s = kit::song("Verse 2\na\nPre-Chorus\nb\nTag\nc", &[], 4);
        assert_eq!(s.iter().map(section).collect::<Vec<_>>(), ["verse", "pre_chorus", "tag"]);
        assert_eq!(section(&kit::song("a\n\nb", &[], 4)[0]), "", "an unlabelled part has no colour");
        assert_eq!(section(&kit::plain(&["John 3:16"])[0]), "");
    }

    #[test]
    fn bar_chips_follow_the_played_order_for_songs_only() {
        let slides = kit::song("Verse 1\na\nChorus\nx\nVerse 2\nb", &["Verse 1", "Chorus", "Verse 2", "Chorus"], 4);
        let (chips, bar) = jumps(&slides, None, true);
        let got: Vec<(i32, String)> = chips.iter().map(|j| (j.idx, j.short.to_string())).collect();
        assert_eq!(got, vec![(0, "V1".into()), (1, "C".into()), (2, "V2".into()), (3, "C".into())]);
        assert_eq!(bar[3].occurrence, 3);
        assert!(chips.iter().all(|c| !c.live && c.tooltip == "Click to show this part"), "no key hints off the live song");
        assert!(jumps(&kit::plain(&["a"]), None, true).0.is_empty());
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
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: body.into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
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
        let kind = ItemKind::Scripture { reference: "John 3:16-17".into(), translation: "KJV".into(), verses: vs(2), per_slide: 0, text_size: 0, also, layout: Default::default(), breaks: Default::default() };
        let item = ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None };
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
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: Uuid::new_v4(), arrangement: None }, background: None, notes: String::new(), theme: None };
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
    fn easy_and_table_views_list_one_slide_per_row_with_its_words() {
        let app = test_app();
        let id = add_text(&app, "A", "One\nline two\n\nTwo\n\nThree\n\nFour");
        let op = opened(&app);
        assert!(op.ui.get_slide_columns() > 1);
        assert_eq!(find_all(&op, "SlidesPanel::thumb").len(), 4);
        assert!(find(&op, "SlidesPanel::slides-view").is_some(), "the view buttons show under the slides");
        op.ui.invoke_slides_view_picked(1);
        assert_eq!((op.slide_view(), op.ui.get_slides_view(), op.ui.get_slide_columns()), (SlideView::Easy, 1, 1));
        assert_eq!(op.ui.get_slide_rows().row_count(), 4);
        assert_eq!(cells(&op)[0].text, "One\nline two");
        assert_eq!(find_all(&op, "SlidesPanel::easy-row").len(), 4);
        assert!(find(&op, "SlidesPanel::thumb").is_none());
        assert!(find(&op, "SlidesPanel::thumb-size").is_none(), "the size slider is for the grid");
        // A click takes the slide live, and the live mark shows in this view too.
        find_all(&op, "SlidesPanel::easy-row").swap_remove(2).mock_single_click(PointerEventButton::Left);
        op.apply_live(LiveUpdate::new(app.live_view()));
        assert_eq!(app.live_view().cursor.map(|c| (c.item_id, c.slide)), Some((id, 2)));
        assert!(cells(&op)[2].live);
        op.ui.invoke_slides_view_picked(2);
        assert_eq!(find_all(&op, "SlidesPanel::table-row").len(), 4);
        assert!(cells(&op).iter().all(|c| c.image.size().width > 0), "Table rows have their small pictures");
        op.ui.invoke_slides_view_picked(0);
        assert_eq!(find_all(&op, "SlidesPanel::thumb").len(), 4);
    }

    #[test]
    fn size_slider_makes_grid_pictures_bigger_or_smaller() {
        let app = test_app();
        add_text(&app, "A", "1\n\n2\n\n3\n\n4\n\n5\n\n6");
        let op = opened(&app);
        let normal = op.ui.get_slide_columns();
        op.ui.invoke_slides_thumb_size_changed(420);
        let big = op.ui.get_slide_columns();
        op.ui.invoke_slides_thumb_size_changed(140);
        let small = op.ui.get_slide_columns();
        assert!(big < normal && normal < small, "{big} < {normal} < {small}");
        assert_eq!(op.ui.get_slide_rows().row_count(), chunk_rows(6, small as usize).len());
        op.ui.invoke_slides_thumb_size_changed(5000);
        assert_eq!(op.ui.get_slides_thumb_size(), 420, "kept inside the slider's ends");
        assert_eq!(columns_for(1000.0, 140.0), 6);
    }

    #[test]
    fn slide_text_says_what_a_file_slide_is() {
        let pic = Slide::new("Slide 1", SlideContent::Image { asset: "a.png".into(), transition: None });
        assert_eq!(slide_text(&pic), "Picture");
        let text = Slide::new("x", SlideContent::Text { text: " a \n\n b".into(), footer: None, reference: None, size: 0, parallel: None });
        assert_eq!(slide_text(&text), "a\nb");
    }

    #[test]
    fn video_cell_has_play_overlay() {
        let app = test_app();
        let media = crate::model::MediaFile { id: Uuid::new_v4(), name: "clip.mp4".into(), kind: crate::model::MediaKind::Video, asset: "clip.mp4".into(), status: crate::model::MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Media { media_id: media.id, autoplay: true, end: Default::default(), looped: false }, background: None, notes: String::new(), theme: None };
        { let mut d = app.data.lock().unwrap(); d.media.push(media); d.services[0].items.push(item); }
        let op = opened(&app);
        assert!(cells(&op)[0].play);
    }
}
