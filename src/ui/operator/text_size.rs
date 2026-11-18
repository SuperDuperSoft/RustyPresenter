//! The slides header's A− / A+ / Auto for a Bible passage: they change that passage's text size
//! (`ItemKind::Scripture::text_size`) through `service/update`, and when the passage is on the screen its slide is
//! shown again so the room redraws at once. Automatic (0) follows Settings → Look's scripture size.
//!
//! A song has the same buttons and, beside them, − / + for its lines on each slide. Both are the song's own
//! (`Song::text_size`, `Song::lines_per_slide`), saved with `song/save`, so they hold every week and in every
//! service; 0 follows Settings. A size change on the live song redraws at once; a change of lines moves the words
//! between slides, so the live song keeps what it shows and offers Update screen, as after any edit.
use super::{strings as tr, window::Operator};
use crate::{
    live::{Command, SlideLayer},
    model::{ItemKind, PassageSlide, Settings, Slide, SlideContent, Song, Theme},
    render::{Fonts, passage_px},
    ui::ops::outcome_toast,
};
use std::{cell::RefCell, rc::{Rc, Weak}};

/// One press of A− or A+, in pixels on a 1080p screen.
pub const STEP: u32 = 8;
/// The range of a passage's own size (the Settings slider's).
pub const MIN: u32 = 36;
pub const MAX: u32 = 120;

thread_local! {
    /// Fonts for measuring the size a passage shows at, made on the first press (it loads the system fonts).
    static FONTS: RefCell<Option<Fonts>> = const { RefCell::new(None) };
}

/// What a press asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press { Smaller, Larger, Automatic }

/// The new `text_size` for a press, given the passage's own size (`current`, 0 = automatic), the size it falls
/// back to (`default`, Settings') and the size its text shows at now (`shown`, which is smaller when the fullest
/// slide had to shrink). `None`: the press changes nothing.
pub fn next_size(press: Press, current: u32, default: u32, shown: u32) -> Option<u32> {
    let max = if current == 0 { default } else { current };
    let to = match press {
        Press::Automatic => return (current != 0).then_some(0),
        // From what shows, so the first press always makes the text visibly smaller.
        Press::Smaller => shown.min(max).saturating_sub(STEP).max(MIN),
        // Text that already had to shrink cannot grow: it would no longer fit.
        Press::Larger if shown < max => return None,
        Press::Larger => (max + STEP).min(MAX),
    };
    (to != current).then_some(to)
}

/// The new `Song::text_size` for a press: a step from the song's own size, or from the theme's (`default`) when it
/// is automatic. A slide too full for the size still shrinks to fit when drawn.
pub fn next_song_size(press: Press, current: u32, default: u32) -> Option<u32> {
    let from = if current == 0 { default } else { current };
    let to = match press {
        Press::Automatic => 0,
        Press::Smaller => from.saturating_sub(STEP).max(MIN),
        Press::Larger => (from + STEP).min(Song::MAX_TEXT),
    };
    (to != current).then_some(to)
}

/// The new `Song::lines_per_slide` for a press, from the song's own number or Settings' (`default`).
pub fn next_lines(press: Press, current: u8, default: u8) -> Option<u8> {
    let from = if current == 0 { default.max(1) } else { current };
    let to = match press {
        Press::Automatic => 0,
        Press::Smaller => from.saturating_sub(1).max(1),
        Press::Larger => (from + 1).min(Song::MAX_LINES),
    };
    (to != current).then_some(to)
}

/// The theme's lyric size (0 = the default 84).
fn lyric_size(settings: &Settings) -> u32 { if settings.theme.size == 0 { 84 } else { settings.theme.size } }

/// Every text slide of `slides`, for measuring.
fn passage_of(slides: &[Slide]) -> Vec<PassageSlide> {
    slides.iter().filter_map(|s| match &s.content {
        SlideContent::Text { text, reference, footer, .. } => Some(PassageSlide { text: text.clone(), reference: reference.clone(), footer: footer.clone() }),
        _ => None,
    }).collect()
}

/// The size the passage's text shows at on a 1080p screen.
fn shown_px(slides: &[Slide], size: u32, theme: &Theme) -> u32 {
    let passage = passage_of(slides);
    if passage.is_empty() { return if size == 0 { theme.scripture_size } else { size } }
    FONTS.with(|f| {
        let mut f = f.borrow_mut();
        passage_px(f.get_or_insert_with(Fonts::new), &passage, size, theme, (1920, 1080)).round() as u32
    })
}

impl Operator {
    /// The song the selected item plays, with Settings.
    fn selected_song(&self) -> Option<(Song, Settings)> {
        let st = self.state.borrow();
        let snap = st.snap.as_ref()?;
        let row = st.selected.and_then(|id| snap.items.iter().find(|i| i.id == id))?;
        let ItemKind::Song { song_id, .. } = row.item.kind else { return None };
        Some((snap.data.songs.iter().find(|s| s.id == song_id)?.clone(), snap.settings.clone()))
    }

    /// The header's size controls for the selected item: shown for a Bible passage or a song, with "Auto" or its
    /// size; a song also shows its lines per slide.
    pub(super) fn render_text_size(&self) {
        // The song's quick Key control sits with the song's size and lines (songs folder design).
        crate::ui::song_key::render(self);
        let song = self.selected_song();
        let size = match &song {
            Some((s, _)) => Some(s.text_size),
            None => {
                let st = self.state.borrow();
                let snap = st.snap.as_ref();
                st.selected.and_then(|id| snap?.items.iter().find(|i| i.id == id))
                    .and_then(|row| match row.item.kind { ItemKind::Scripture { text_size, .. } => Some(text_size), _ => None })
            }
        };
        self.ui.set_slides_text_size_shown(size.is_some());
        self.ui.set_slides_text_size_custom(size.is_some_and(|s| s > 0));
        self.ui.set_slides_text_size(size.filter(|s| *s > 0).map(|s| s.to_string()).unwrap_or_else(|| tr::text_size_auto().into()).into());
        self.ui.set_slides_song_fit(song.is_some());
        self.ui.set_slides_lines(song.as_ref().map(|(s, set)| s.lines(set).to_string()).unwrap_or_default().into());
        self.ui.set_slides_lines_custom(song.as_ref().is_some_and(|(s, _)| s.lines_per_slide > 0));
    }

    /// Changes the selected item's song (`song/fit`: only the fields in `patch`, so quick presses never undo each
    /// other); `reshow` shows the live slide again when this item's words are on the screen, so the room redraws
    /// at once.
    fn save_song_fit(self: &Rc<Self>, patch: serde_json::Value, reshow: bool) {
        let item_id = self.state.borrow().selected;
        let again = if reshow { self.state.borrow().live.as_ref().map(|l| l.view()).and_then(|v| {
            let c = v.cursor.as_ref().filter(|c| Some(c.item_id) == item_id)?;
            matches!(v.frame.slide, Some(SlideLayer::Text { .. })).then_some((c.item_id, c.slide, v.version))
        }) } else { None };
        let me = Rc::downgrade(self);
        self.ops.call::<serde_json::Value>("song/fit", patch, move |r| {
            let Some(op) = me.upgrade() else { return };
            if let Err(e) = r { return op.toast(e, true) }
            if let Some((item_id, slide, base)) = again {
                let me = Rc::downgrade(&op);
                op.ops.cmd(Command::Show { item_id, slide }, Some(base), move |o| {
                    if let (Some(op), Some((msg, err))) = (me.upgrade(), outcome_toast(&o)) { op.toast(&msg, err) }
                });
            }
        });
    }

    /// − / + / Auto for the selected song's lines on each slide.
    pub(super) fn press_lines(self: &Rc<Self>, press: Press) {
        let Some((song, settings)) = self.selected_song() else { return };
        let Some(to) = next_lines(press, song.lines_per_slide, settings.lines_per_slide) else { return };
        self.save_song_fit(serde_json::json!({ "id": song.id, "lines_per_slide": to }), false);
    }

    /// A−, A+ or Auto on the selected passage.
    pub(super) fn press_text_size(self: &Rc<Self>, press: Press) {
        if let Some((song, settings)) = self.selected_song() {
            let Some(to) = next_song_size(press, song.text_size, lyric_size(&settings)) else { return };
            return self.save_song_fit(serde_json::json!({ "id": song.id, "text_size": to }), true);
        }
        let (row, theme) = {
            let st = self.state.borrow();
            let Some(snap) = st.snap.as_ref() else { return };
            let Some(row) = st.selected.and_then(|id| snap.items.iter().find(|i| i.id == id)) else { return };
            (row.clone(), snap.settings.theme.clone())
        };
        let ItemKind::Scripture { text_size, .. } = row.item.kind else { return };
        let default = if theme.scripture_size == 0 { 64 } else { theme.scripture_size };
        let shown = if press == Press::Automatic { 0 } else { shown_px(&row.slides, text_size, &theme) };
        let Some(to) = next_size(press, text_size, default, shown) else {
            if press == Press::Larger { self.toast(tr::text_as_big_as_fits(), false) }
            return;
        };
        let mut item = row.item.clone();
        if let ItemKind::Scripture { text_size, .. } = &mut item.kind { *text_size = to }
        // Shown again only when this passage's text is on the screen now (not after Clear, say).
        let reshow = self.state.borrow().live.as_ref().map(|l| l.view()).and_then(|v| {
            let c = v.cursor.as_ref().filter(|c| c.item_id == row.id)?;
            matches!(v.frame.slide, Some(SlideLayer::Text { .. })).then_some((c.slide, v.version))
        });
        let me = Rc::downgrade(self);
        self.ops.call::<serde_json::Value>("service/update", serde_json::to_value(&item).unwrap_or_default(), move |r| {
            let Some(op) = me.upgrade() else { return };
            if let Err(e) = r { return op.toast(e, true) }
            if let Some((slide, base)) = reshow {
                let me = Rc::downgrade(&op);
                // Against the version seen at the press: when the operator moved on meanwhile, nothing is redrawn.
                op.ops.cmd(Command::Show { item_id: row.id, slide }, Some(base), move |o| {
                    if let (Some(op), Some((msg, err))) = (me.upgrade(), outcome_toast(&o)) { op.toast(&msg, err) }
                });
            }
        });
    }
}

/// Connects A−, A+ and Auto. Each holds a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_slides_text_smaller(move || if let Some(op) = me.upgrade() { op.press_text_size(Press::Smaller) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_slides_text_larger(move || if let Some(op) = me.upgrade() { op.press_text_size(Press::Larger) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_slides_text_auto(move || if let Some(op) = me.upgrade() { op.press_text_size(Press::Automatic) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_slides_lines_fewer(move || if let Some(op) = me.upgrade() { op.press_lines(Press::Smaller) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_slides_lines_more(move || if let Some(op) = me.upgrade() { op.press_lines(Press::Larger) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_slides_lines_auto(move || if let Some(op) = me.upgrade() { op.press_lines(Press::Automatic) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, LiveUpdate, test_app},
        model::{ServiceItem, VerseText},
        ui::operator::window::testing::{find, open, refresh},
    };
    use std::sync::Arc;
    use uuid::Uuid;

    fn push(app: &App, kind: ItemKind) -> Uuid {
        let item = ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        item.id
    }

    fn psalm(app: &App) -> Uuid {
        let verses = (1..=2).map(|i| VerseText::new(format!("Psalm 23:{i}"), "The LORD is my shepherd.")).collect();
        push(app, ItemKind::Scripture { reference: "Psalm 23:1–2".into(), translation: "KJV".into(), verses, per_slide: 1, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() })
    }

    fn size_of(app: &App, id: Uuid) -> u32 {
        let d = app.data.lock().unwrap();
        match d.services[0].items.iter().find(|i| i.id == id).map(|i| &i.kind) { Some(ItemKind::Scripture { text_size, .. }) => *text_size, k => panic!("{k:?}") }
    }

    fn opened(app: &Arc<App>, select: Uuid) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op.ui.invoke_service_select(select.to_string().into());
        op
    }

    fn live(op: &Operator) { op.apply_live(LiveUpdate::new(op.app().live_view())); }

    #[test]
    fn a_minus_and_a_plus_change_the_stored_size_and_auto_clears_it() {
        let app = test_app();
        let id = psalm(&app);
        let op = opened(&app, id);
        assert!(op.ui.get_slides_text_size_shown());
        assert_eq!(op.ui.get_slides_text_size(), "Auto");
        assert!(find(&op, "SlidesPanel::text-larger").is_some());
        op.ui.invoke_slides_text_larger();
        assert_eq!(size_of(&app, id), 72);
        refresh(&op);
        assert_eq!((op.ui.get_slides_text_size().as_str(), op.ui.get_slides_text_size_custom()), ("72", true));
        op.ui.invoke_slides_text_smaller();
        refresh(&op);
        op.ui.invoke_slides_text_smaller();
        assert_eq!(size_of(&app, id), 56);
        refresh(&op);
        op.ui.invoke_slides_text_auto();
        assert_eq!(size_of(&app, id), 0, "Automatic again");
        refresh(&op);
        assert_eq!(op.ui.get_slides_text_size(), "Auto");
    }

    #[test]
    fn the_live_passage_redraws_at_once() {
        let app = test_app();
        let id = psalm(&app);
        let op = opened(&app, id);
        op.ops().cmd(Command::Show { item_id: id, slide: 1 }, None, |o| assert!(o.ok, "{o:?}"));
        live(&op);
        op.ui.invoke_slides_text_larger();
        let v = app.live_view();
        assert_eq!(v.cursor.map(|c| c.slide), Some(1), "the same slide stays on");
        assert!(matches!(v.frame.slide, Some(SlideLayer::Text { size: 72, .. })), "{:?}", v.frame.slide);
    }

    #[test]
    fn a_cleared_passage_is_not_shown_again() {
        let app = test_app();
        let id = psalm(&app);
        let op = opened(&app, id);
        op.ops().cmd(Command::Show { item_id: id, slide: 0 }, None, |o| assert!(o.ok));
        op.ops().cmd(Command::ClearSlide, None, |o| assert!(o.ok));
        live(&op);
        op.ui.invoke_slides_text_larger();
        assert_eq!(size_of(&app, id), 72);
        assert_eq!(app.live_view().frame.slide, None);
    }

    #[test]
    fn other_items_have_no_size_buttons() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into(), layout: Default::default() });
        let op = opened(&app, id);
        assert!(!op.ui.get_slides_text_size_shown());
        assert!(find(&op, "SlidesPanel::text-larger").is_none());
    }

    #[test]
    fn smaller_steps_down_from_what_shows() {
        assert_eq!(next_size(Press::Smaller, 0, 64, 64), Some(56));
        assert_eq!(next_size(Press::Smaller, 0, 64, 44), Some(36), "a shrunk passage gets visibly smaller");
        assert_eq!(next_size(Press::Smaller, 80, 64, 80), Some(72));
        assert_eq!(next_size(Press::Smaller, 36, 64, 36), None, "the smallest stays");
    }

    #[test]
    fn larger_steps_up_unless_the_text_already_shrank() {
        assert_eq!(next_size(Press::Larger, 0, 64, 64), Some(72));
        assert_eq!(next_size(Press::Larger, 116, 64, 116), Some(120));
        assert_eq!(next_size(Press::Larger, 120, 64, 120), None);
        assert_eq!(next_size(Press::Larger, 0, 64, 50), None, "it would not fit any bigger");
    }

    #[test]
    fn automatic_clears_the_passage_size() {
        assert_eq!(next_size(Press::Automatic, 88, 64, 88), Some(0));
        assert_eq!(next_size(Press::Automatic, 0, 64, 64), None);
    }
}

#[cfg(test)]
mod song_tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        model::ServiceItem,
        ui::operator::window::testing::{find, open, refresh},
    };
    use std::sync::Arc;
    use uuid::Uuid;

    /// A four-line verse in the library and in the service; gives (song, item).
    fn grace(app: &App) -> (Uuid, Uuid) {
        let song = Song { id: Uuid::new_v4(), title: "Grace".into(), lyrics: "Verse 1\na\nb\nc\nd".into(), ..Default::default() };
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None };
        let mut d = app.data.lock().unwrap();
        d.settings.lines_per_slide = 2;
        d.settings.credits = "none".into();
        d.songs.push(song.clone());
        d.services[0].items.push(item.clone());
        (song.id, item.id)
    }

    fn song(app: &App, id: Uuid) -> Song { app.data.lock().unwrap().songs.iter().find(|s| s.id == id).cloned().unwrap() }

    fn slide_count(op: &Operator, item: Uuid) -> usize {
        op.state.borrow().snap.as_ref().unwrap().items.iter().find(|i| i.id == item).unwrap().slides.len()
    }

    fn opened(app: &Arc<App>, select: Uuid) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op.ui.invoke_service_select(select.to_string().into());
        op
    }

    #[test]
    fn steps_stay_in_range_and_auto_is_zero() {
        assert_eq!(next_lines(Press::Larger, 0, 2), Some(3), "from Settings' number");
        assert_eq!(next_lines(Press::Smaller, 0, 2), Some(1));
        assert_eq!(next_lines(Press::Smaller, 1, 2), None, "never under one line");
        assert_eq!(next_lines(Press::Larger, Song::MAX_LINES, 2), None);
        assert_eq!(next_lines(Press::Automatic, 3, 2), Some(0));
        assert_eq!(next_lines(Press::Automatic, 0, 2), None);
        assert_eq!(next_song_size(Press::Larger, 0, 84), Some(92));
        assert_eq!(next_song_size(Press::Smaller, 92, 84), Some(84));
        assert_eq!(next_song_size(Press::Smaller, MIN, 84), None);
        assert_eq!(next_song_size(Press::Larger, Song::MAX_TEXT, 84), None);
        assert_eq!(next_song_size(Press::Automatic, 92, 84), Some(0));
    }

    #[test]
    fn a_song_in_the_service_gets_lines_and_size_buttons_that_are_kept_with_the_song() {
        let app = test_app();
        let (song_id, item) = grace(&app);
        let op = opened(&app, item);
        assert!(op.ui.get_slides_song_fit() && op.ui.get_slides_text_size_shown());
        assert_eq!((op.ui.get_slides_lines().as_str(), op.ui.get_slides_lines_custom()), ("2", false));
        assert!(find(&op, "SlidesPanel::lines-more").is_some() && find(&op, "SlidesPanel::song-text-larger").is_some());
        assert!(find(&op, "SlidesPanel::text-larger").is_none(), "a song's size buttons are on the tools row only");
        assert_eq!(slide_count(&op, item), 2, "four lines, two on a slide");

        op.ui.invoke_slides_lines_more();
        refresh(&op);
        op.ui.invoke_slides_lines_more();
        assert_eq!(song(&app, song_id).lines_per_slide, 4);
        refresh(&op);
        assert_eq!((op.ui.get_slides_lines().as_str(), op.ui.get_slides_lines_custom()), ("4", true));
        assert_eq!(slide_count(&op, item), 1, "the song already in the service is split again");

        op.ui.invoke_slides_lines_auto();
        refresh(&op);
        assert_eq!(song(&app, song_id).lines_per_slide, 0);
        assert_eq!(slide_count(&op, item), 2);

        op.ui.invoke_slides_text_larger();
        assert_eq!(song(&app, song_id).text_size, 92);
        refresh(&op);
        assert_eq!((op.ui.get_slides_text_size().as_str(), op.ui.get_slides_text_size_custom()), ("92", true));
        let sized = op.state.borrow().snap.as_ref().unwrap().items.iter().find(|i| i.id == item).unwrap().slides.iter()
            .all(|s| matches!(s.content, SlideContent::Text { size: 92, .. }));
        assert!(sized, "every slide of the song carries its size");
        op.ui.invoke_slides_text_auto();
        assert_eq!(song(&app, song_id).text_size, 0);
    }

    #[test]
    fn two_quick_presses_keep_both_changes() {
        let app = test_app();
        let (song_id, item) = grace(&app);
        let op = opened(&app, item);
        op.ui.invoke_slides_lines_more();
        op.ui.invoke_slides_text_smaller();
        let s = song(&app, song_id);
        assert_eq!((s.lines_per_slide, s.text_size), (3, 76), "no refresh between the presses");
        assert_eq!(s.lyrics, "Verse 1\na\nb\nc\nd", "the words are not touched");
    }

    #[test]
    fn the_live_song_redraws_at_its_new_size() {
        let app = test_app();
        let (_, item) = grace(&app);
        let op = opened(&app, item);
        op.ops().cmd(Command::Show { item_id: item, slide: 1 }, None, |o| assert!(o.ok, "{o:?}"));
        op.apply_live(crate::app::LiveUpdate::new(op.app().live_view()));
        op.ui.invoke_slides_text_smaller();
        let v = app.live_view();
        assert_eq!(v.cursor.map(|c| c.slide), Some(1), "the same slide stays on");
        assert!(matches!(v.frame.slide, Some(SlideLayer::Text { size: 76, .. })), "{:?}", v.frame.slide);
    }
}
