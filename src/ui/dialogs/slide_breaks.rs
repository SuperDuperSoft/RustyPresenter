//! Slide breaks (FEAT-5 §3.3, `ui/dialogs/slide-breaks.slint`): a scripture item's verses with the gap between each
//! pair. A click on a gap adds a new slide there, or keeps an automatic break together, or puts it back; the slides
//! on the right follow each click (planned by the core with the marks so far). Nothing changes until Save, which sends
//! only `breaks` and `per_slide`.
use super::{DialogKind, background::strings};
use crate::{
    scripture::{VerseBreaks, VerseId, breaks::{GapState, apply_mark, click}, strings as s},
    ui::{BreakGap, BreakSlide, BreakVerse, SlideBreaksDlg, operator::window::Operator},
};
use serde::Deserialize;
use serde_json::{Value, json};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::{Cell, RefCell}, rc::{Rc, Weak}};
use uuid::Uuid;

#[derive(Deserialize)]
struct Row { id: Option<VerseId>, number: String, text: String, also_only: bool }

#[derive(Deserialize)]
struct Gap { state: String, enabled: bool }

#[derive(Deserialize)]
struct SlideRow { label: String, text: String, too_long: bool }

#[derive(Deserialize)]
struct View { verses: Vec<Row>, gaps: Vec<Gap>, slides: Vec<SlideRow>, save: VerseBreaks }

#[derive(Deserialize)]
struct Reply { view: View, per_slide: u8, breaks: VerseBreaks }

/// What the open dialog remembers: the marks so far and the last plan.
struct State {
    id: Uuid,
    breaks: RefCell<VerseBreaks>,
    per_slide: Cell<u8>,
    ids: RefCell<Vec<Option<VerseId>>>,
    gaps: RefCell<Vec<GapState>>,
    save: RefCell<VerseBreaks>,
    split: Cell<usize>,
}

fn gap_state(name: &str) -> GapState {
    match name {
        "auto" => GapState::Auto,
        "added" => GapState::Added,
        "kept" => GapState::Kept,
        _ => GapState::None,
    }
}

fn gap_label(state: GapState) -> String {
    match state {
        GapState::Auto => s::gap_auto(),
        GapState::Added => s::gap_added(),
        GapState::Kept => s::gap_kept(),
        GapState::None => String::new(),
    }
}

/// Opens the dialog for scripture item `id`, with its saved marks.
pub fn open(op: &Rc<Operator>, id: Uuid) {
    op.open_dialog(DialogKind::SlideBreaks(id));
    let g = op.ui.global::<SlideBreaksDlg>();
    g.set_verses(ModelRc::default());
    g.set_gaps(ModelRc::default());
    g.set_slides(ModelRc::default());
    g.set_note("".into());
    g.set_too_long_chip(s::too_long_chip().into());
    g.set_none_hover(s::gap_none_hover().into());
    let state = Rc::new(State {
        id, breaks: RefCell::default(), per_slide: Cell::new(0), ids: RefCell::default(), gaps: RefCell::default(),
        save: RefCell::default(), split: Cell::new(0),
    });
    connect(op, &state);
    load(op, &state, true);
}

fn connect(op: &Rc<Operator>, state: &Rc<State>) {
    let g = op.ui.global::<SlideBreaksDlg>();
    let with = |f: fn(&Rc<Operator>, &Rc<State>)| {
        let (me, st): (Weak<Operator>, Rc<State>) = (Rc::downgrade(op), state.clone());
        move || if let Some(op) = me.upgrade() { f(&op, &st) }
    };
    g.on_reset(with(|op, st| {
        *st.breaks.borrow_mut() = VerseBreaks::default();
        load(op, st, false);
    }));
    g.on_cancel(with(|op, _| op.close_dialog()));
    g.on_save(with(save));
    let (me, st) = (Rc::downgrade(op), state.clone());
    g.on_gap_clicked(move |i| if let (Some(op), Ok(i)) = (me.upgrade(), usize::try_from(i)) { gap_clicked(&op, &st, i) });
    let (me, st) = (Rc::downgrade(op), state.clone());
    g.on_per_slide_changed(move |n| if let Some(op) = me.upgrade() {
        st.per_slide.set(u8::try_from(n.clamp(0, 4)).unwrap_or(0));
        load(&op, &st, false);
    });
}

/// Plans the passage with the marks so far. `first`: take the item's saved marks and verses per slide.
fn load(op: &Rc<Operator>, state: &Rc<State>, first: bool) {
    let mut body = json!({ "id": state.id });
    if !first {
        body["breaks"] = json!(*state.breaks.borrow());
        body["per_slide"] = json!(state.per_slide.get());
    }
    let (me, st) = (Rc::downgrade(op), state.clone());
    op.ops().call::<Reply>("scripture/breaks", body, move |r| {
        let Some(op) = me.upgrade() else { return };
        if op.dialog() != Some(DialogKind::SlideBreaks(st.id)) { return }
        match r {
            Ok(reply) => show(&op, &st, reply, first),
            Err(e) => op.toast(&e, true),
        }
    });
}

fn show(op: &Rc<Operator>, state: &Rc<State>, reply: Reply, first: bool) {
    let g = op.ui.global::<SlideBreaksDlg>();
    let Reply { view, per_slide, breaks } = reply;
    if first {
        *state.breaks.borrow_mut() = breaks;
        state.per_slide.set(per_slide);
    }
    g.set_per_slide(i32::from(state.per_slide.get()));
    let split = view.slides.iter().filter(|s| s.too_long).count();
    // A click that just made a slide too long says so under the list (no question to answer).
    g.set_note(if !first && split > state.split.get() { s::join_too_long_note().into() } else { "".into() });
    state.split.set(split);
    let gaps: Vec<GapState> = view.gaps.iter().map(|g| gap_state(&g.state)).collect();
    let rows: Vec<BreakGap> = view.gaps.iter().zip(&gaps).map(|(gap, &st)| BreakGap {
        state: gap.state.as_str().into(), label: gap_label(st).into(), enabled: gap.enabled,
    }).collect();
    g.set_gaps(ModelRc::new(VecModel::from(rows)));
    let verses: Vec<BreakVerse> = view.verses.iter().map(|v| BreakVerse { number: v.number.as_str().into(), text: v.text.as_str().into(), also_only: v.also_only }).collect();
    g.set_verses(ModelRc::new(VecModel::from(verses)));
    let slides: Vec<BreakSlide> = view.slides.iter().map(|s| BreakSlide { label: s.label.as_str().into(), text: s.text.as_str().into(), too_long: s.too_long }).collect();
    g.set_slides(ModelRc::new(VecModel::from(slides)));
    *state.ids.borrow_mut() = view.verses.iter().map(|v| v.id).collect();
    *state.gaps.borrow_mut() = gaps;
    *state.save.borrow_mut() = view.save;
}

/// The gap before verse `i + 1`: None → a new slide; automatic → kept together; added or kept → automatic again.
fn gap_clicked(op: &Rc<Operator>, state: &Rc<State>, i: usize) {
    let Some(Some(verse)) = state.ids.borrow().get(i + 1).copied() else { return };
    let Some(&gap) = state.gaps.borrow().get(i) else { return };
    apply_mark(&mut state.breaks.borrow_mut(), verse, click(gap));
    load(op, state, false);
}

/// Sends only the changed keys, then closes with "Saved". An item removed meanwhile keeps the dialog open.
fn save(op: &Rc<Operator>, state: &Rc<State>) {
    let body = json!({ "id": state.id, "breaks": *state.save.borrow(), "per_slide": state.per_slide.get() });
    let (me, id) = (Rc::downgrade(op), state.id);
    op.ops().call::<Value>("service/update", body, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => {
                if op.dialog() == Some(DialogKind::SlideBreaks(id)) { op.close_dialog() }
                op.toast(strings::saved(), false);
            }
            Err(e) => op.toast(&e, true),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, model::{ItemKind, ServiceItem}, ui::operator::window::testing::{find, find_all, open as open_window, refresh}};
    use slint::{Model, platform::PointerEventButton};

    fn passage(app: &crate::app::App) -> Uuid {
        let (_, verses) = crate::bible::kjv().lookup("Psalm 23:1-6").unwrap();
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Scripture { reference: "Psalm 23:1-6".into(), translation: "KJV".into(), verses, per_slide: 0,
            text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() }, background: None, notes: String::new(), theme: None };
        let id = item.id;
        app.data().services[0].items.push(item);
        id
    }

    fn slide_labels(op: &Operator) -> Vec<String> { op.ui.global::<SlideBreaksDlg>().get_slides().iter().map(|s| s.label.to_string()).collect() }

    fn gaps(op: &Operator) -> Vec<String> { op.ui.global::<SlideBreaksDlg>().get_gaps().iter().map(|g| g.state.to_string()).collect() }

    #[test]
    fn a_click_adds_a_break_and_save_keeps_it() {
        let app = test_app();
        let id = passage(&app);
        let op = open_window(&app);
        op.window().set_size(slint::LogicalSize::new(1400.0, 900.0));
        refresh(&op);
        open(&op, id);
        assert_eq!(op.dialog(), Some(DialogKind::SlideBreaks(id)));
        assert_eq!(op.ui.global::<SlideBreaksDlg>().get_verses().row_count(), 6);
        let before = slide_labels(&op);
        let at = gaps(&op).iter().position(|g| g == "none").expect("a gap with no break");
        find_all(&op, "SlideBreaksDialog::gap")[at].mock_single_click(PointerEventButton::Left);
        assert_eq!(gaps(&op)[at], "added");
        let starts = format!("Psalm 23:{}", at + 2);
        assert!(slide_labels(&op).iter().any(|l| l == &starts || l.starts_with(&format!("{starts}–"))), "a slide starts at the break: {:?}", slide_labels(&op));
        assert_ne!(slide_labels(&op), before);
        find(&op, "SlideBreaksDialog::save").unwrap().mock_single_click(PointerEventButton::Left);
        assert_eq!(op.dialog(), None);
        let d = app.data();
        let ItemKind::Scripture { breaks, .. } = &d.services[0].items[0].kind else { panic!() };
        assert_eq!(breaks.before.iter().map(|v| v.verse()).collect::<Vec<_>>(), [at as u16 + 2]);
    }

    #[test]
    fn keep_together_reset_and_verses_per_slide() {
        let app = test_app();
        let id = passage(&app);
        let op = open_window(&app);
        refresh(&op);
        open(&op, id);
        let g = op.ui.global::<SlideBreaksDlg>();
        g.set_per_slide(1);
        g.invoke_per_slide_changed(1);
        assert_eq!(slide_labels(&op).len(), 6, "one verse a slide");
        g.invoke_gap_clicked(0);
        assert_eq!(gaps(&op)[0], "kept");
        assert_eq!(slide_labels(&op)[0], "Psalm 23:1–2", "kept together");
        g.invoke_reset();
        assert!(gaps(&op).iter().all(|g| g == "auto"), "{:?}", gaps(&op));
        assert_eq!(slide_labels(&op).len(), 6);
        g.invoke_cancel();
        assert_eq!(op.dialog(), None);
        let d = app.data();
        assert!(matches!(&d.services[0].items[0].kind, ItemKind::Scripture { per_slide: 0, breaks, .. } if breaks.is_empty()), "Cancel changes nothing");
    }
}
