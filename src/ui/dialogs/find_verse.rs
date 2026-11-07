//! Find a verse (FEAT-5 §3.1, `ui/dialogs/find-verse.slint`): words typed in the Bible box (or here) are searched in
//! one of the Bibles stored on this computer, 250 ms after the last key. The first search of a Bible builds its index
//! in the background; the dialog shows "Getting KJV ready to search…" and asks again until it is ready. Closing the
//! dialog stops a build under way. Picking a result shows its whole chapter; click a verse, Shift+click to widen, and
//! Add puts the passage after the live item like the Bible box does.
use super::DialogKind;
use crate::{
    model::VerseText,
    scripture::{VerseId, input::{display_name, verse_id}, position::{SeqGuard, Selection, add_label}, strings as s},
    ui::{FindVerseDlg, FindVerseHit, FindVerseLine, operator::{bible, window::Operator}},
};
use serde::Deserialize;
use serde_json::{Value, json};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::{Cell, RefCell}, rc::{Rc, Weak}, time::Duration};

/// Wait after the last key before searching (spec §3.1).
const DEBOUNCE: Duration = Duration::from_millis(250);
/// How often to ask again while a Bible's index is being built.
const POLL: Duration = Duration::from_millis(200);

#[derive(Deserialize)]
struct Bible { id: String, label: String, name: String }

#[derive(Deserialize)]
struct Bibles { bibles: Vec<Bible> }

#[derive(Deserialize, Clone)]
struct Hit { id: VerseId, reference: String, snippet: String }

#[derive(Deserialize)]
struct Reply {
    state: String,
    #[serde(default)] total: usize,
    #[serde(default)] hits: Vec<Hit>,
    translation: String,
    #[serde(default)] note: Option<String>,
    #[serde(default)] label: Option<String>,
}

#[derive(Deserialize)]
struct Chapter { verses: Vec<VerseText> }

/// What the open dialog remembers.
#[derive(Default)]
struct State {
    bibles: RefCell<Vec<Bible>>,
    hits: RefCell<Vec<Hit>>,
    /// The Bible the shown results came from (the one asked for may not be searchable).
    searched: RefCell<String>,
    chapter: RefCell<Vec<VerseText>>,
    sel: Cell<Option<Selection>>,
    seq: Cell<SeqGuard>,
    debounce: Timer,
    poll: Timer,
}

thread_local! {
    static OPEN: RefCell<Option<Rc<State>>> = const { RefCell::new(None) };
}

/// Opens the dialog with `query` searched at once, in the Bible box's Bible when it is stored here.
pub fn open(op: &Rc<Operator>, query: &str) {
    op.open_dialog(DialogKind::FindVerse);
    let g = op.ui.global::<FindVerseDlg>();
    g.set_query(query.trim().into());
    g.set_header("".into());
    g.set_note("".into());
    g.set_hits(ModelRc::default());
    g.set_current(-1);
    g.set_lines(ModelRc::default());
    g.set_chapter_title("".into());
    g.set_add_label(s::add_button("").trim().into());
    g.set_can_add(false);
    let state = Rc::new(State::default());
    OPEN.with(|o| *o.borrow_mut() = Some(state.clone()));
    connect(op, &state);
    op.on_dialog_close(|op| {
        OPEN.with(|o| o.borrow_mut().take());
        op.ops().call::<Value>("bible/search-cancel", json!({}), |_| {});
    });
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(&state));
    op.ops().call::<Bibles>("bible/searchable", json!({}), move |r| {
        let (Some(op), Some(state), Ok(list)) = (me.upgrade(), st.upgrade(), r) else { return };
        if op.dialog() != Some(DialogKind::FindVerse) { return }
        let chosen = op.state.borrow().snap.as_ref().map(|s| s.translation.clone()).unwrap_or_default();
        let at = list.bibles.iter().position(|b| crate::bible::id::same_id(&b.id, &chosen)).unwrap_or(0);
        let g = op.ui.global::<FindVerseDlg>();
        let names: Vec<slint::SharedString> = list.bibles.iter().map(|b| if b.name.is_empty() || b.name == b.label { b.label.as_str().into() } else { format!("{} — {}", b.label, b.name).into() }).collect();
        g.set_bibles(ModelRc::new(VecModel::from(names)));
        g.set_bible(at as i32);
        *state.bibles.borrow_mut() = list.bibles;
        // The Bible box's Bible can't be searched: say which one is searched instead.
        let asked = if chosen.is_empty() { None } else { Some(chosen) };
        search(&op, &state, asked);
    });
}

fn connect(op: &Rc<Operator>, state: &Rc<State>) {
    let g = op.ui.global::<FindVerseDlg>();
    let with = |f: fn(&Rc<Operator>, &Rc<State>)| {
        let (me, st): (Weak<Operator>, Weak<State>) = (Rc::downgrade(op), Rc::downgrade(state));
        move || if let (Some(op), Some(st)) = (me.upgrade(), st.upgrade()) { f(&op, &st) }
    };
    g.on_edited(with(|op, st| {
        let (me, s2) = (Rc::downgrade(op), Rc::downgrade(st));
        st.debounce.start(TimerMode::SingleShot, DEBOUNCE, move || if let (Some(op), Some(st)) = (me.upgrade(), s2.upgrade()) { search(&op, &st, None) });
    }));
    g.on_search_now(with(|op, st| { st.debounce.stop(); search(op, st, None) }));
    g.on_add(with(add));
    let again = |op: &Rc<Operator>, st: &Rc<State>| search(op, st, None);
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
    g.on_bible_picked(move |_| if let (Some(op), Some(st)) = (me.upgrade(), st.upgrade()) { again(&op, &st) });
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
    g.on_scope_picked(move |_| if let (Some(op), Some(st)) = (me.upgrade(), st.upgrade()) { again(&op, &st) });
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
    g.on_order_picked(move |_| if let (Some(op), Some(st)) = (me.upgrade(), st.upgrade()) { again(&op, &st) });
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
    g.on_hit_picked(move |i| if let (Some(op), Some(st), Ok(i)) = (me.upgrade(), st.upgrade(), usize::try_from(i)) { pick_hit(&op, &st, i) });
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
    g.on_line_clicked(move |i, shift| if let (Some(op), Some(st), Ok(i)) = (me.upgrade(), st.upgrade(), usize::try_from(i)) { click_line(&op, &st, i, shift) });
}

/// Searches what is typed now. `asked` names a Bible to try instead of the picked one (on open).
fn search(op: &Rc<Operator>, state: &Rc<State>, asked: Option<String>) {
    state.poll.stop();
    let g = op.ui.global::<FindVerseDlg>();
    let query = g.get_query().to_string();
    let mut seq = state.seq.get();
    let n = seq.next();
    state.seq.set(seq);
    if query.trim().is_empty() {
        g.set_header("".into());
        g.set_hits(ModelRc::default());
        state.hits.borrow_mut().clear();
        return;
    }
    let picked = state.bibles.borrow().get(usize::try_from(g.get_bible()).unwrap_or(0)).map(|b| b.id.clone()).unwrap_or_else(|| "KJV".into());
    let translation = asked.clone().unwrap_or(picked);
    let scope = match g.get_scope() { 1 => "ot", 2 => "nt", _ => "all" };
    let order = if g.get_order() == 1 { "bible" } else { "best" };
    let body = json!({ "query": query, "translation": translation, "scope": scope, "order": order });
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
    op.ops().call::<Reply>("bible/search", body, move |r| {
        let (Some(op), Some(state)) = (me.upgrade(), st.upgrade()) else { return };
        if op.dialog() != Some(DialogKind::FindVerse) || !state.seq.get().is_current(n) { return }
        let g = op.ui.global::<FindVerseDlg>();
        match r {
            Ok(reply) => show(&op, &state, reply, asked),
            Err(e) => {
                g.set_header(e.into());
                g.set_hits(ModelRc::default());
                state.hits.borrow_mut().clear();
            }
        }
    });
}

fn show(op: &Rc<Operator>, state: &Rc<State>, reply: Reply, asked: Option<String>) {
    let g = op.ui.global::<FindVerseDlg>();
    g.set_note(reply.note.clone().unwrap_or_default().into());
    // The drop-down follows the Bible the core searched.
    if let Some(i) = state.bibles.borrow().iter().position(|b| crate::bible::id::same_id(&b.id, &reply.translation)) { g.set_bible(i as i32) }
    let label = state.bibles.borrow().iter().find(|b| crate::bible::id::same_id(&b.id, &reply.translation)).map(|b| b.label.clone())
        .or(reply.label.clone()).unwrap_or_else(|| reply.translation.clone());
    if reply.state != "ready" {
        g.set_header(s::index_building(&label).into());
        let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
        state.poll.start(TimerMode::SingleShot, POLL, move || if let (Some(op), Some(st)) = (me.upgrade(), st.upgrade()) { search(&op, &st, asked.clone()) });
        return;
    }
    let header = match reply.total {
        0 if reply.hits.is_empty() => s::no_hits(&label),
        t if t > reply.hits.len() => s::too_many_hits(reply.hits.len(), t),
        t => s::found(t, &label),
    };
    g.set_header(header.into());
    let rows: Vec<FindVerseHit> = reply.hits.iter().map(|h| FindVerseHit { reference: h.reference.as_str().into(), snippet: h.snippet.as_str().into() }).collect();
    g.set_hits(ModelRc::new(VecModel::from(rows)));
    g.set_current(-1);
    *state.searched.borrow_mut() = reply.translation;
    *state.hits.borrow_mut() = reply.hits;
    state.chapter.borrow_mut().clear();
    state.sel.set(None);
    g.set_lines(ModelRc::default());
    g.set_chapter_title("".into());
    g.set_can_add(false);
}

/// Shows the whole chapter of result `i`, with its verse picked.
fn pick_hit(op: &Rc<Operator>, state: &Rc<State>, i: usize) {
    let Some(hit) = state.hits.borrow().get(i).cloned() else { return };
    let g = op.ui.global::<FindVerseDlg>();
    g.set_current(i as i32);
    let body = json!({ "translation": state.searched.borrow().clone(), "book": hit.id.book(), "chapter": hit.id.chapter() });
    let (me, st) = (Rc::downgrade(op), Rc::downgrade(state));
    op.ops().call::<Chapter>("bible/chapter", body, move |r| {
        let (Some(op), Some(state)) = (me.upgrade(), st.upgrade()) else { return };
        if op.dialog() != Some(DialogKind::FindVerse) || op.ui.global::<FindVerseDlg>().get_current() != i as i32 { return }
        match r {
            Ok(c) => {
                let at = c.verses.iter().position(|v| verse_id(v) == Some(hit.id)).unwrap_or(0);
                *state.chapter.borrow_mut() = c.verses;
                state.sel.set(Some(Selection::at(at)));
                op.ui.global::<FindVerseDlg>().set_chapter_title(format!("{} {}", display_name(hit.id.book()), hit.id.chapter()).to_uppercase().into());
                draw_lines(&op, &state);
            }
            Err(e) => op.toast(&e, true),
        }
    });
}

/// A click picks one verse; Shift+click widens the pick to it (within the chapter).
fn click_line(op: &Rc<Operator>, state: &Rc<State>, i: usize, shift: bool) {
    let len = state.chapter.borrow().len();
    if i >= len { return }
    let sel = match state.sel.get() {
        Some(mut s) if shift => { s.shift_to(i, len); s }
        _ => Selection::at(i),
    };
    state.sel.set(Some(sel));
    draw_lines(op, state);
}

fn draw_lines(op: &Rc<Operator>, state: &Rc<State>) {
    let g = op.ui.global::<FindVerseDlg>();
    let chapter = state.chapter.borrow();
    let (a, b) = state.sel.get().map_or((usize::MAX, 0), |s| s.range());
    let lines: Vec<FindVerseLine> = chapter.iter().enumerate().map(|(i, v)| FindVerseLine {
        number: verse_id(v).map(|id| id.verse().to_string()).unwrap_or_default().into(),
        text: v.text.as_str().into(),
        picked: i >= a && i <= b,
    }).collect();
    g.set_lines(ModelRc::new(VecModel::from(lines)));
    match state.sel.get() {
        Some(sel) if !chapter.is_empty() => {
            g.set_add_label(add_label(&chapter, &sel).into());
            g.set_can_add(true);
        }
        _ => g.set_can_add(false),
    }
}

/// The picked verses as a reference the Bible readers accept ("Psalm 46:10-11").
fn picked_reference(chapter: &[VerseText], sel: Selection) -> Option<String> {
    let (a, b) = sel.range();
    let (first, last) = (verse_id(chapter.get(a)?)?, verse_id(chapter.get(b)?)?);
    let name = display_name(first.book());
    Some(if first == last { format!("{name} {}:{}", first.chapter(), first.verse()) } else { format!("{name} {}:{}-{}", first.chapter(), first.verse(), last.verse()) })
}

/// Adds the picked verses in the dialog's Bible, after the live item, and closes the dialog.
fn add(op: &Rc<Operator>, state: &Rc<State>) {
    let Some(reference) = state.sel.get().and_then(|sel| picked_reference(&state.chapter.borrow(), sel)) else { return };
    let translation = state.searched.borrow().clone();
    let me = Rc::downgrade(op);
    op.ops().call::<bible::Found>("bible/lookup", json!({ "reference": reference, "translation": translation }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(found) => {
                if op.dialog() == Some(DialogKind::FindVerse) { op.close_dialog() }
                bible::add(&op, found);
            }
            Err(e) => op.toast(&e, true),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, model::ItemKind, ui::operator::window::testing::{find, find_all, open as open_window, refresh}};
    use slint::{Model, platform::PointerEventButton};

    /// Runs Slint timers and the core until `done`, at most about 60 s (the first search of a Bible builds its
    /// index).
    fn wait(op: &Rc<Operator>, done: impl Fn(&Rc<Operator>) -> bool) {
        for _ in 0..1200 {
            slint::platform::update_timers_and_animations();
            refresh(op);
            if done(op) { return }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("timed out; header: {}", op.ui.global::<FindVerseDlg>().get_header());
    }

    fn hits(op: &Operator) -> Vec<String> { op.ui.global::<FindVerseDlg>().get_hits().iter().map(|h| h.reference.to_string()).collect() }

    fn opened() -> (std::sync::Arc<crate::app::App>, Rc<Operator>) {
        let app = test_app();
        let op = open_window(&app);
        op.window().set_size(slint::LogicalSize::new(1400.0, 900.0));
        refresh(&op);
        (app, op)
    }

    #[test]
    fn words_in_the_bible_box_open_find_a_verse_with_results() {
        let (app, op) = opened();
        bible::submit(&op, "be still and know".into());
        assert_eq!(op.dialog(), Some(DialogKind::FindVerse));
        assert!(app.data.lock().unwrap().services[0].items.is_empty(), "nothing is added");
        wait(&op, |op| !hits(op).is_empty());
        assert_eq!(hits(&op)[0], "Psalm 46:10");
        assert!(op.ui.global::<FindVerseDlg>().get_header().ends_with("found in KJV"));
    }

    #[test]
    fn pick_widen_and_add_puts_the_passage_in_the_service() {
        let (app, op) = opened();
        open(&op, "be still and know");
        wait(&op, |op| !hits(op).is_empty());
        find_all(&op, "FindVerseDialog::hit")[0].mock_single_click(PointerEventButton::Left);
        wait(&op, |op| op.ui.global::<FindVerseDlg>().get_can_add());
        let g = op.ui.global::<FindVerseDlg>();
        assert_eq!(g.get_add_label(), "Add Psalm 46:10");
        g.invoke_line_clicked(10, true);
        assert_eq!(g.get_add_label(), "Add Psalm 46:10–11");
        find(&op, "FindVerseDialog::add").unwrap().mock_single_click(PointerEventButton::Left);
        wait(&op, |op| op.dialog().is_none());
        let d = app.data.lock().unwrap();
        match &d.services[0].items[0].kind {
            ItemKind::Scripture { reference, translation, verses, .. } => {
                assert_eq!((reference.as_str(), translation.as_str(), verses.len()), ("Psalm 46:10-11", "KJV", 2));
            }
            k => panic!("{k:?}"),
        }
    }

    #[test]
    fn a_bible_box_bible_that_cannot_be_searched_searches_kjv_and_says_so() {
        let (_app, op) = opened();
        bible::pick_id(&op, "NIV".into());
        open(&op, "jesus wept");
        wait(&op, |op| !hits(op).is_empty());
        let g = op.ui.global::<FindVerseDlg>();
        assert_eq!(g.get_note(), "Search isn't available for NIV, so this searches KJV.");
        assert_eq!(hits(&op)[0], "John 11:35");
        assert_eq!(g.get_bible(), 0, "the drop-down shows KJV");
    }

    #[test]
    fn closing_the_dialog_stops_the_search() {
        let (app, op) = opened();
        open(&op, "love");
        let g = op.ui.global::<FindVerseDlg>();
        wait(&op, |op| op.ui.global::<FindVerseDlg>().get_bibles().row_count() > 1);
        g.set_bible(1);
        g.invoke_bible_picked(1);
        op.close_dialog();
        wait(&op, |_| app.search.get().is_some_and(|s| !s.is_building("WEB")));
    }
}
