//! Editing a song from its slides, in the Slides panel (`ui/operator/slide-editor.slint`, `src/song_edit`): the small
//! editor over the slides (double-click, the pencil, Edit words in the Live panel), the slide's menu (right-click or
//! ⋯), Change section, dragging a slide onto another to move its part, and the song's order as chips on the panel.
//! Every change goes to the app as one `song/slides` step, so Edit → Undo takes it back. The service's own order of
//! the song changes, not the song's usual one, until "Save as the usual order".
use super::window::Operator;
use crate::{
    model::{ItemKind, Settings, Song},
    sections::{SectionKind, SectionRef, arrangement},
    song_edit::{At, Change, Doc, Refused, actions::ItemOrder, order::{self, OrderOp}, strings as s, words},
    ui::{ArrAddRow, SectionPick, dialogs::song_editor::{arr_add_drag_data, arr_chips, arr_drag_data, arr_drag_index, arr_drag_label, arr_rows}},
};
use crate::shortcuts::{Action, Chord, EditKey, Keymap};
use serde_json::{Value, json};
use slint::{ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::Rc};
use uuid::Uuid;

thread_local! {
    /// The copied slide's lines (chords and all), for Paste in this song or another.
    static SLIDE_CLIP: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// `+ <part>` buttons to a row in the panel's order strip.
const ADDS_PER_ROW: usize = 6;
/// What a dragged slide carries: plain text `slide:<index>`.
const SLIDE_DRAG: &str = "slide:";

/// The slide the editor or the section chooser works on: found again in the song's words when it is saved, so a
/// change made meanwhile (a phone, Undo) is noticed instead of overwritten.
#[derive(Clone, Debug)]
struct Target {
    item_id: Uuid,
    song_id: Uuid,
    section: SectionRef,
    /// Its lines as they were when the editor opened.
    lines: Vec<String>,
    /// A new slide after this one (Add a slide after).
    adding: bool,
    /// The text box shows the chords as `[G]` marks (always for a slide of chords alone).
    chords: bool,
}

/// The editor between calls.
#[derive(Default)]
pub(crate) struct EditState {
    target: Option<Target>,
    /// The item whose slides the mouse was over (`SlideEdit.hover`): another item's slides start with none.
    hover_item: Option<Uuid>,
    /// "Edit chords too" was ticked last time: the next slide with chords opens the same way.
    chords: bool,
}

/// The selected service item when it is a song: the item, its song, the order it plays in, and the settings.
struct Selected { item_id: Uuid, song: Song, order: Vec<String>, own: bool, settings: Settings, slides: Vec<crate::model::Slide> }

fn selected(op: &Operator) -> Option<Selected> {
    let st = op.state.borrow();
    let snap = st.snap.as_ref()?;
    let row = st.selected.and_then(|id| snap.items.iter().find(|i| i.id == id))?;
    let ItemKind::Song { song_id, arrangement } = &row.item.kind else { return None };
    let song = snap.data.songs.iter().find(|s| s.id == *song_id)?.clone();
    let order = arrangement.clone().unwrap_or_else(|| song.arrangement.clone());
    Some(Selected { item_id: row.id, own: arrangement.is_some(), order, settings: snap.settings.clone(), slides: row.slides.clone(), song })
}

impl Selected {
    fn lps(&self) -> usize { self.song.lines(&self.settings) }
    fn doc(&self) -> Doc { Doc::read(&self.song.lyrics) }
    /// Slide `idx` of the item: its section and its place in the words.
    fn slide(&self, idx: usize) -> Option<(SectionRef, At)> {
        let r = self.slides.get(idx)?.section.clone()?;
        let at = self.doc().find(&self.order, &r, self.lps())?;
        Some((r, at))
    }
}

/// Sends one change; `done` says what happened in a short message.
fn send(op: &Rc<Operator>, body: Value, done: String) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("song/slides", body, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r { Ok(_) => op.toast(done, false), Err(e) => op.toast(e, true) }
    });
}

/// A change of the song's words (and what it means for its orders).
fn send_change(op: &Rc<Operator>, song_id: Uuid, c: Change, done: String) {
    send(op, json!({ "song_id": song_id, "lyrics": c.lyrics, "fixes": c.fixes }), done)
}

/// A new order for the selected item (its own, for this service), with where each part went.
fn send_order(op: &Rc<Operator>, sel: &Selected, op_: OrderOp) {
    let Some(r) = order::apply(&sel.song.lyrics, &sel.order, &op_) else {
        if matches!(op_, OrderOp::ChorusAfterVerses | OrderOp::ChorusAtEnd) && order::chorus(&sel.song.lyrics).is_none() { op.toast(s::no_chorus(), true) }
        return;
    };
    let body = json!({ "song_id": sel.song.id, "item_id": sel.item_id, "item_order": ItemOrder::Own(r.order).to_json(), "moved": r.moved });
    send(op, body, s::order_changed())
}

impl ItemOrder {
    fn to_json(&self) -> Value {
        match self { ItemOrder::Own(o) => json!({ "own": o }), ItemOrder::Usual => json!("usual") }
    }
}

fn refused(op: &Operator, r: Refused) { op.toast(r.message(), true) }

/// The editor shows `target`.
fn show_editor(op: &Operator, sel: &Selected, at: &At, mut target: Target, hint: String) {
    let g = op.ui.slide_edit_all();
    let doc = sel.doc();
    let label = &doc.sections[at.section].label;
    let plays = doc.plays(&sel.order, at.section);
    let has_chords = target.lines.iter().any(|l| words::has_chords(l));
    let chords_only = has_chords && words::words(&target.lines).is_empty();
    let chords = chords_only || (has_chords && op.slide_edit.borrow().chords);
    let text = if target.adding { String::new() } else if chords { target.lines.join("\n") } else { words::words(&target.lines) };
    let heading = if target.adding { format!("{} · {}", label, s::new_slide()) } else { s::where_slide(label, at.index + 1, at.pages) };
    g.set_heading(heading.into());
    g.set_kind(SectionKind::from_label(label).0.id().into());
    g.set_note(if plays > 1 { s::plays_times(label, plays) } else { String::new() }.into());
    g.set_has_chords(has_chords && !target.adding);
    g.set_chords_mode(chords && !target.adding);
    g.set_chord_view(if has_chords && !target.adding { words::chord_view(&target.lines) } else { String::new() }.into());
    g.set_hint(if hint.is_empty() && chords_only { s::chords_only_note() } else if hint.is_empty() && has_chords && !chords { s::chords_kept_note() } else { hint }.into());
    g.set_adding(target.adding);
    let live = op.shown_live().and_then(|l| l.view().item.as_ref().map(|i| i.item_id)) == Some(target.item_id);
    g.set_live(live);
    g.set_start_text(text.into());
    g.set_serial(g.get_serial() + 1);
    g.set_choosing(false);
    g.set_open(true);
    target.chords = chords;
    op.slide_edit.borrow_mut().target = Some(target);
}

/// Closes the editor and the chooser.
pub(crate) fn close(op: &Operator) {
    let g = op.ui.slide_edit_all();
    g.set_open(false);
    g.set_choosing(false);
    op.slide_edit.borrow_mut().target = None;
}

/// Opens the editor on slide `idx` of the selected song (`adding`: a new slide after it).
fn open_at(op: &Operator, idx: usize, adding: bool, hint: String) {
    let Some(sel) = selected(op) else { return op.toast(s::not_a_song(), true) };
    let Some((section, at)) = sel.slide(idx) else { return refused(op, Refused::Gone) };
    let lines = sel.doc().slide_lines(&at);
    let target = Target { item_id: sel.item_id, song_id: sel.song.id, section, lines, adding, chords: false };
    show_editor(op, &sel, &at, target, hint);
}

/// The target found again in the song as it is now; None (with a message) when it changed meanwhile.
fn find_target(op: &Operator) -> Option<(Selected, Target, At)> {
    let t = op.slide_edit.borrow().target.clone()?;
    let sel = selected(op).filter(|s| s.item_id == t.item_id && s.song.id == t.song_id);
    let found = sel.and_then(|sel| { let at = sel.doc().find(&sel.order, &t.section, sel.lps())?; Some((sel, at)) });
    match found {
        Some((sel, at)) if sel.doc().slide_lines(&at) == t.lines => Some((sel, t, at)),
        _ => { refused(op, Refused::Gone); None }
    }
}

/// The text box's text as the slide's new lines: as typed when the chords show, else the words with the chords put
/// back over them.
fn new_lines(t: &Target, text: &str) -> Vec<String> {
    if t.chords || t.adding { text.split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect() } else { words::merge_words(&t.lines, text) }
}

/// Save (or Save and show now).
fn save(op: &Rc<Operator>, text: &str, show_now: bool) {
    let Some((sel, t, at)) = find_target(op) else { return close(op) };
    close(op);
    let lines = new_lines(&t, text);
    let doc = sel.doc();
    let r = if t.adding { doc.add_after(&at, &lines) } else { doc.set_lines(&at, &lines) };
    match r {
        Ok(c) if c.lyrics == sel.song.lyrics => {}
        Ok(c) => {
            let msg = if t.adding { s::added() } else if lines.iter().all(|l| l.trim().is_empty()) { s::deleted() } else { s::saved() };
            send_change(op, sel.song.id, c, msg);
            if show_now { crate::ui::dialogs::song_editor::refresh_live(op) }
        }
        Err(e) => refused(op, e),
    }
}

/// Split here: the text as edited, then a new slide from the line the caret is on.
fn split_at(op: &Rc<Operator>, text: &str, caret: usize) {
    let Some((sel, t, at)) = find_target(op) else { return close(op) };
    close(op);
    let caret = caret.min(text.len());
    let line = text.get(..caret).map_or(0, |b| b.matches('\n').count());
    let mut lines = new_lines(&t, text);
    let chords = t.chords;
    // In the words the cursor's line counts only lines with words; lines of chords alone sit before it.
    let place = if chords { Some(line) } else { lines.iter().enumerate().filter(|(_, l)| !words::part(l).words.is_empty()).nth(line).map(|(i, _)| i) };
    match place {
        Some(i) if i > 0 && i < lines.len() => {
            lines.insert(i, String::new());
            match sel.doc().set_lines(&at, &lines) { Ok(c) => send_change(op, sel.song.id, c, s::split_done()), Err(e) => refused(op, e) }
        }
        _ => refused(op, Refused::NoSplit),
    }
}

/// The byte offset where line `line` of `text` starts.
fn line_offset(text: &str, line: usize) -> usize { text.split_inclusive('\n').take(line).map(str::len).sum() }

/// Edit the whole song…: the song editor, the cursor at the slide's first line.
fn whole_song(op: &Rc<Operator>, sel: &Selected, at: &At) {
    let caret = line_offset(&sel.song.lyrics, at.tag.unwrap_or(at.lines.start));
    crate::ui::dialogs::song_editor::open_song_editor_at(op, Some(sel.song.id), false, Some(caret), None);
}

/// The song slide the mouse is over (`SlideEdit.hover`), when the open item is a song with such a slide.
fn hovered(op: &Operator) -> Option<usize> {
    let idx = usize::try_from(op.ui.slide_edit_all().get_hover()).ok()?;
    selected(op).is_some_and(|s| s.slide(idx).is_some()).then_some(idx)
}

/// Delete, Copy, Cut or Duplicate with the mouse over a song slide: that slide, as its menu does it. False when
/// the mouse is over no song slide (the key then works on the service list, as before).
pub(crate) fn edit_key(op: &Rc<Operator>, key: EditKey) -> bool {
    let action = match key { EditKey::Delete => "delete", EditKey::Copy => "copy", EditKey::Cut => "cut", EditKey::Duplicate => "duplicate", _ => return false };
    let Some(idx) = hovered(op) else { return false };
    act(op, idx, action);
    true
}

/// Paste with the mouse over a song slide, when there is a slide or plain text to paste: it becomes a new slide
/// after that one. False: not over a slide, or nothing for a slide (a copied service item or a link is pasted
/// as before).
pub(crate) fn paste_key(op: &Rc<Operator>) -> bool {
    let Some(idx) = hovered(op).filter(|_| clip_lines().is_some()) else { return false };
    act(op, idx, "paste");
    true
}

/// The key that pastes, as its menu shows it ("Ctrl+V").
fn paste_label() -> String { Keymap::default().keys(Action::PasteLink).first().map(Chord::label).unwrap_or_default() }

/// Keeps `lines` as the copied slide, and puts its words on the system clipboard too (to paste into another program).
fn copy_slide(op: &Operator, lines: Vec<String>, done: String) {
    let text = lines.join("\n");
    SLIDE_CLIP.with(|c| *c.borrow_mut() = Some(lines));
    #[cfg(not(test))]
    crate::ui::dialogs::settings::copy(op, &text, &done);
    #[cfg(test)]
    { let _ = text; op.toast(done, false) }
}

/// What Paste makes a slide of: the copied slide, unless the system clipboard has other plain text since (words
/// copied from a web page or a document), which is then the new slide's lines. A link is left to Download from
/// YouTube.
fn clip_lines() -> Option<Vec<String>> {
    let clip = SLIDE_CLIP.with(|c| c.borrow().clone());
    let text = crate::ui::dialogs::settings::paste_text().map(|t| t.replace("\r\n", "\n")).filter(|t| !t.trim().is_empty());
    match (clip, text) {
        (Some(lines), None) => Some(lines),
        (Some(lines), Some(t)) if t.trim() == lines.join("\n").trim() => Some(lines),
        (_, Some(t)) if crate::ui::dialogs::add_youtube::pasted_link().is_none() => Some(t.trim_matches('\n').split('\n').map(|l| l.trim_end().to_string()).collect()),
        _ => None,
    }
}

/// Forgets the copied slide (each test starts with none).
#[cfg(test)]
pub(crate) fn forget_clip() { SLIDE_CLIP.with(|c| *c.borrow_mut() = None) }

/// A choice from a slide's menu.
fn act(op: &Rc<Operator>, idx: usize, action: &str) {
    if action == "edit" { return open_at(op, idx, false, String::new()) }
    if action == "add" { return open_at(op, idx, true, String::new()) }
    if action == "split" { return open_at(op, idx, false, s::no_split()) }
    let Some(sel) = selected(op) else { return op.toast(s::not_a_song(), true) };
    let Some((section, at)) = sel.slide(idx) else { return refused(op, Refused::Gone) };
    let doc = sel.doc();
    let song_id = sel.song.id;
    match action {
        "join" => match doc.join_next(&at, sel.lps()) { Ok(c) => send_change(op, song_id, c, s::joined()), Err(e) => refused(op, e) },
        "duplicate" => match doc.duplicate(&at) { Ok(c) => send_change(op, song_id, c, s::duplicated()), Err(e) => refused(op, e) },
        "copy" => copy_slide(op, doc.slide_lines(&at), s::copied(&paste_label())),
        "paste" => match clip_lines() {
            Some(lines) => match doc.add_after(&at, &lines) { Ok(c) => send_change(op, song_id, c, s::pasted()), Err(e) => refused(op, e) },
            None => op.toast(s::nothing_to_paste(), true),
        },
        "delete" | "cut" => {
            if action == "cut" { copy_slide(op, doc.slide_lines(&at), s::cut(&paste_label())) }
            let c = match doc.delete(&at) { Ok(c) => c, Err(e) => return refused(op, e) };
            let plays = doc.plays(&sel.order, at.section);
            if plays <= 1 { return send_change(op, song_id, c, s::deleted()) }
            let me = Rc::downgrade(op);
            let q = s::delete_repeated(&doc.sections[at.section].label, plays);
            crate::ui::dialogs::confirm(op, &q, &s::delete_yes(), &crate::ui::dialogs::add_strings::cancel(), true, move |yes| {
                if let Some(op) = me.upgrade().filter(|_| yes) { send_change(&op, song_id, c, s::deleted()) }
            });
        }
        "section" => {
            let g = op.ui.slide_edit_all();
            let picks: Vec<SectionPick> = doc.section_choices(at.section).into_iter()
                .map(|n| SectionPick { kind: SectionKind::from_label(&n).0.id().into(), name: n.into() }).collect();
            g.set_choices(ModelRc::new(VecModel::from(picks)));
            g.set_open(false);
            g.set_choosing(true);
            let lines = doc.slide_lines(&at);
            op.slide_edit.borrow_mut().target = Some(Target { item_id: sel.item_id, song_id, section, lines, adding: false, chords: false });
        }
        "earlier" | "later" => send_order(op, &sel, OrderOp::Step { occurrence: usize::from(section.occurrence), later: action == "later" }),
        "song" => whole_song(op, &sel, &at),
        _ => {}
    }
}

/// Change section: the chosen name.
fn chose(op: &Rc<Operator>, name: &str) {
    let Some((sel, _, at)) = find_target(op) else { return close(op) };
    close(op);
    match sel.doc().change_section(&at, name) {
        Ok(c) if c.lyrics == sel.song.lyrics => {}
        Ok(c) => send_change(op, sel.song.id, c, s::section_changed(name)),
        Err(e) => refused(op, e),
    }
}

/// A slide dropped on another: its part plays where the other's part plays.
fn dropped(op: &Rc<Operator>, from: usize, to: usize) {
    let Some(sel) = selected(op) else { return };
    let occ = |i: usize| sel.slides.get(i).and_then(|s| s.section.as_ref()).map(|r| usize::from(r.occurrence));
    let (Some(a), Some(b)) = (occ(from), occ(to)) else { return };
    if a == b { return }
    let labels = sel.doc().labels();
    let mut entries = sel.order.clone();
    arrangement::ensure_real(&mut entries, &sel.song.lyrics);
    let (Some(fa), Some(fb)) = (order::entry_of(&entries, &labels, a), order::entry_of(&entries, &labels, b)) else { return };
    send_order(op, &sel, OrderOp::Move { from: fa, to: fb });
}

/// Edit words in the Live panel: the live song's item in the Slides panel, and the editor on the live slide.
pub(crate) fn edit_live(op: &Rc<Operator>) {
    let Some((item_id, slide)) = op.shown_live().and_then(|l| Some((l.view().item.as_ref()?.item_id, l.view().cursor.as_ref()?.slide))) else { return };
    op.select_item(item_id);
    open_at(op, slide, false, String::new());
}

/// The order strip on the panel and whether it shows; the editor closes when its song is no longer selected.
pub(crate) fn render(op: &Operator) {
    let g = op.ui.slide_edit_all();
    let sel = selected(op);
    // Another item's slides: the mouse is over none of them yet (it clicked in the service list), so the editing
    // keys go to the list until it moves onto a slide.
    let item = op.state.borrow().selected;
    if std::mem::replace(&mut op.slide_edit.borrow_mut().hover_item, item) != item { g.set_hover(-1) }
    let label = |k: EditKey| k.chords().first().map(Chord::label).unwrap_or_default();
    g.set_key_copy(label(EditKey::Copy).into());
    g.set_key_cut(label(EditKey::Cut).into());
    g.set_key_paste(paste_label().into());
    g.set_key_duplicate(label(EditKey::Duplicate).into());
    g.set_key_delete(label(EditKey::Delete).into());
    let still = op.slide_edit.borrow().target.as_ref().is_some_and(|t| sel.as_ref().is_some_and(|s| s.item_id == t.item_id));
    if !still && (g.get_open() || g.get_choosing()) { close(op) }
    let Some(sel) = sel else { return g.set_order_shown(false) };
    let (chips, adds, written) = arrangement::chips(&sel.song.lyrics, &sel.order);
    g.set_order_count(chips.len() as i32);
    g.set_order_rows(arr_rows(arr_chips(chips)));
    g.set_order_written(written);
    let adds: Vec<ArrAddRow> = adds.chunks(ADDS_PER_ROW).map(|c| ArrAddRow {
        labels: ModelRc::new(VecModel::from(c.iter().map(|l| SharedString::from(l.as_str())).collect::<Vec<_>>())),
    }).collect();
    g.set_order_adds(ModelRc::new(VecModel::from(adds)));
    g.set_order_own(sel.own);
    g.set_has_chorus(order::chorus(&sel.song.lyrics).is_some());
    g.set_order_shown(true);
}

fn drag_data(text: String) -> slint::DataTransfer {
    let mut d = slint::DataTransfer::default();
    d.set_plain_text(text.into());
    d
}

/// Connects the editor's, the menu's and the order strip's callbacks. Once, when the window opens.
pub(crate) fn wire(op: &Rc<Operator>) {
    let g = op.ui.slide_edit_all();
    macro_rules! on {
        ($name:ident, |$op:ident $(, $a:ident : $t:ty)*| $body:expr) => {{
            let me = Rc::downgrade(op);
            g.$name(move |$($a),*| { if let Some($op) = me.upgrade() { $(let $a: $t = $a.into();)* $body } });
        }};
    }
    on!(on_act, |op, i: i32, a: SharedString| if let Ok(i) = usize::try_from(i) { act(&op, i, &a) });
    on!(on_save, |op, text: SharedString, show: bool| save(&op, &text, show));
    on!(on_cancel, |op| close(&op));
    on!(on_split_at, |op, text: SharedString, caret: i32| split_at(&op, &text, usize::try_from(caret).unwrap_or(0)));
    on!(on_whole_song, |op| {
        let t = op.slide_edit.borrow().target.clone();
        close(&op);
        if let (Some(t), Some(sel)) = (t, selected(&op)) && let Some(at) = sel.doc().find(&sel.order, &t.section, sel.lps()) { whole_song(&op, &sel, &at) }
    });
    on!(on_chose, |op, name: SharedString| chose(&op, &name));
    on!(on_choose_cancel, |op| close(&op));
    on!(on_dropped, |op, from: i32, to: i32| if let (Ok(f), Ok(t)) = (usize::try_from(from), usize::try_from(to)) { dropped(&op, f, t) });
    let me = Rc::downgrade(op);
    g.on_chords_toggled(move |on, text| {
        let Some(op) = me.upgrade() else { return text };
        let Some(t) = op.slide_edit.borrow().target.clone() else { return text };
        let was = t.chords;
        let lines = new_lines(&t, &text);
        {
            let mut st = op.slide_edit.borrow_mut();
            st.chords = on;
            if let Some(t) = st.target.as_mut() { t.chords = on }
        }
        let g = op.ui.slide_edit_all();
        g.set_chords_mode(on);
        if on == was { return text }
        if on { lines.join("\n").into() } else { words::words(&lines).into() }
    });
    g.on_drag_data(|i| drag_data(format!("{SLIDE_DRAG}{i}")));
    g.on_drag_index(|d| d.plain_text().ok().and_then(|t| t.strip_prefix(SLIDE_DRAG).and_then(|i| i.parse().ok())).unwrap_or(-1));
    // The order strip.
    g.on_order_drag_data(arr_drag_data);
    g.on_order_drag_index(|d| arr_drag_index(&d));
    g.on_order_add_drag_data(arr_add_drag_data);
    g.on_order_drag_label(|d| arr_drag_label(&d));
    macro_rules! order_op {
        ($name:ident, |$($a:ident : $t:ty),*| $make:expr) => {{
            let me = Rc::downgrade(op);
            g.$name(move |$($a),*| {
                let Some(op) = me.upgrade() else { return };
                let Some(sel) = selected(&op) else { return };
                $(let Ok($a) = <$t>::try_from($a) else { return };)*
                send_order(&op, &sel, $make)
            });
        }};
    }
    order_op!(on_order_move, |from: usize, to: usize| OrderOp::Move { from, to });
    order_op!(on_order_remove, |at: usize| OrderOp::Remove { at });
    order_op!(on_order_duplicate, |at: usize| OrderOp::Duplicate { at });
    order_op!(on_order_pattern, |which: usize| if which == 0 { OrderOp::ChorusAfterVerses } else { OrderOp::ChorusAtEnd });
    let me = Rc::downgrade(op);
    g.on_order_add(move |label| {
        let (Some(op), Some(sel)) = (me.upgrade(), me.upgrade().and_then(|o| selected(&o))) else { return };
        let at = { let mut v = sel.order.clone(); arrangement::ensure_real(&mut v, &sel.song.lyrics); v.len() };
        send_order(&op, &sel, OrderOp::Insert { label: label.to_string(), at })
    });
    let me = Rc::downgrade(op);
    g.on_order_insert(move |label, at| {
        let (Some(op), Ok(at)) = (me.upgrade(), usize::try_from(at)) else { return };
        let Some(sel) = selected(&op) else { return };
        send_order(&op, &sel, OrderOp::Insert { label: label.to_string(), at })
    });
    on!(on_order_reset, |op| if let Some(sel) = selected(&op) {
        send(&op, json!({ "song_id": sel.song.id, "item_id": sel.item_id, "item_order": ItemOrder::Own(vec![]).to_json() }), s::order_changed())
    });
    on!(on_order_usual, |op| if let Some(sel) = selected(&op) {
        send(&op, json!({ "song_id": sel.song.id, "song_order": sel.order, "item_id": sel.item_id, "item_order": ItemOrder::Usual.to_json() }), s::order_usual())
    });
    on!(on_order_back, |op| if let Some(sel) = selected(&op) {
        send(&op, json!({ "song_id": sel.song.id, "item_id": sel.item_id, "item_order": ItemOrder::Usual.to_json() }), s::order_back())
    });
}

#[cfg(test)]
#[path = "slide_edit_tests.rs"]
mod tests;
