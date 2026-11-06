//! The slides header's A− / A+ / Auto for a Bible passage: they change that passage's text size
//! (`ItemKind::Scripture::text_size`) through `service/update`, and when the passage is on the screen its slide is
//! shown again so the room redraws at once. Automatic (0) follows Settings → Look's scripture size.
use super::{strings as tr, window::Operator};
use crate::{
    live::{Command, SlideLayer},
    model::{ItemKind, PassageSlide, Slide, SlideContent, Theme},
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

/// Every text slide of `slides`, for measuring.
fn passage_of(slides: &[Slide]) -> Vec<PassageSlide> {
    slides.iter().filter_map(|s| match &s.content {
        SlideContent::Text { text, reference, .. } => Some(PassageSlide { text: text.clone(), reference: reference.clone() }),
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
    /// The header's size controls for the selected item: shown for a Bible passage, with "Auto" or its size.
    pub(super) fn render_text_size(&self) {
        let size = {
            let st = self.state.borrow();
            let snap = st.snap.as_ref();
            st.selected.and_then(|id| snap?.items.iter().find(|i| i.id == id))
                .and_then(|row| match row.item.kind { ItemKind::Scripture { text_size, .. } => Some(text_size), _ => None })
        };
        self.ui.set_slides_text_size_shown(size.is_some());
        self.ui.set_slides_text_size_custom(size.is_some_and(|s| s > 0));
        self.ui.set_slides_text_size(size.filter(|s| *s > 0).map(|s| s.to_string()).unwrap_or_else(|| tr::TEXT_SIZE_AUTO.into()).into());
    }

    /// A−, A+ or Auto on the selected passage.
    pub(super) fn press_text_size(self: &Rc<Self>, press: Press) {
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
            if press == Press::Larger { self.toast(tr::TEXT_AS_BIG_AS_FITS, false) }
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
        let item = ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new() };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        item.id
    }

    fn psalm(app: &App) -> Uuid {
        let verses = (1..=2).map(|i| VerseText { reference: format!("Psalm 23:{i}"), text: "The LORD is my shepherd.".into() }).collect();
        push(app, ItemKind::Scripture { reference: "Psalm 23:1–2".into(), translation: "KJV".into(), verses, per_slide: 1, text_size: 0, abbr: String::new() })
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
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
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
