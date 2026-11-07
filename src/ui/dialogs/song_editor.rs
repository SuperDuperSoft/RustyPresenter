//! The song editor (`openSongEditor()`, `operator.js:647-710`): a song's fields, its order with a button per section,
//! and a preview of its slides made by the core's own `slides::song_slides` 250 ms after the last edit. It is never
//! redrawn by an update, so a phone or a conversion never wipes what the user typed.
use super::{DialogKind, add_strings as tr, confirm};
use crate::{
    model::Song,
    sections::arrangement,
    slides::{parse_sections, song_slides},
    ui::{
        ArrChip, ArrChipRow, PreviewSlide, SectionChipRow, SongEditorState,
        operator::{add_item::add_item_then, window::Operator},
        thumbs::{Thumbs, thumb, thumb_size},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use slint::{ComponentHandle, Image, Model, ModelRc, SharedString, Timer, VecModel};
use std::{cell::Cell, rc::{Rc, Weak}, time::Duration};
use uuid::Uuid;

/// How long the preview waits after the last keystroke (`operator.js:686`).
pub const PREVIEW_AFTER: Duration = Duration::from_millis(250);
/// The width a preview picture is drawn for, in logical pixels (half the wide dialog's right column).
const PREVIEW_W: f32 = 256.0;
/// `+ <label>` buttons per row under Order (the web wraps them; Slint has no wrapping layout).
const CHIPS_PER_ROW: usize = 4;
/// Order chips per row.
const ARR_PER_ROW: usize = 8;
/// What a dragged order chip carries: plain text `arr:<index>`.
const ARR_DRAG: &str = "arr:";

/// The open editor: which song, what Save does, the preview's timer and pictures.
struct EditorCtx {
    op: Weak<Operator>,
    song_id: Option<Uuid>,
    /// The library title when it opened, for "Delete “T” from the library?".
    saved_title: String,
    add_after: bool,
    timer: Timer,
    thumbs: Thumbs,
    previews: Rc<VecModel<PreviewSlide>>,
    /// Bumped on each preview, so a picture for an older one is dropped.
    generation: Rc<Cell<u64>>,
}

/// `openSongEditor(song, {addAfter})`: `song_id` is a library song to edit, or `None` for a new one. A song that is
/// no longer in the library opens as a new one.
pub fn open_song_editor(op: &Rc<Operator>, song_id: Option<Uuid>, add_after: bool) { open_song_editor_at(op, song_id, add_after, None) }

/// Edit words on the live song (FEAT-4 §3.4): the editor with the caret at the live section's first lyric line,
/// found from the live slide's occurrence in the order that built it (service item `item_id`'s own, if it has one).
pub fn edit_live_words(op: &Rc<Operator>, item_id: Uuid, song_id: Uuid, occurrence: Option<u16>) {
    let caret = occurrence.and_then(|o| {
        let st = op.state.borrow();
        let snap = st.snap.as_ref()?;
        let song = snap.data.songs.iter().find(|s| s.id == song_id)?;
        let order = snap.items.iter().find(|i| i.id == item_id).map_or(song.arrangement.as_slice(), |i| crate::slides::item_order(&i.item, song));
        crate::sections::arrangement::caret_for_occurrence(&song.lyrics, order, o)
    });
    open_song_editor_at(op, Some(song_id), false, Some(caret.unwrap_or(0)));
}

/// The song on screen now, if any.
fn live_song(op: &Operator) -> Option<Uuid> { op.shown_live().and_then(|l| l.view().item.as_ref()?.song_id) }

/// `caret`: where the lyrics' caret goes (a byte offset), for Edit words.
fn open_song_editor_at(op: &Rc<Operator>, song_id: Option<Uuid>, add_after: bool, caret: Option<usize>) {
    let song = song_id.and_then(|id| op.state.borrow().snap.as_ref()?.data.songs.iter().find(|s| s.id == id).cloned());
    let song_id = song.as_ref().map(|s| s.id);
    op.open_dialog(DialogKind::SongEditor { song_id, add_after });
    let s = song.clone().unwrap_or_default();
    let st = op.ui.global::<SongEditorState>();
    st.set_title(s.title.as_str().into());
    st.set_writers(s.writers.as_str().into());
    st.set_ccli(s.ccli.as_str().into());
    st.set_copyright(s.copyright.as_str().into());
    st.set_artist(s.artist.as_str().into());
    st.set_publisher(s.publisher.as_str().into());
    st.set_year(s.year.as_str().into());
    st.set_lyrics(s.lyrics.as_str().into());
    let labels: Vec<SharedString> = std::iter::once(crate::sections::strings::label_this_slide())
        .chain(label_names(op)).map(SharedString::from).collect();
    st.set_labels(ModelRc::new(VecModel::from(labels)));
    st.set_order(s.arrangement.join(", ").into());
    st.set_existing(song.is_some());
    st.set_add_after(add_after);
    st.set_focus_lyrics(song.is_some());
    st.set_caret(caret.map_or(-1, |c| i32::try_from(c).unwrap_or(0)));
    st.set_live_song(song_id.is_some() && song_id == live_song(op));
    st.set_arr_selected(-1);
    let thumbs = if op.ops().is_inline() { Thumbs::inline(op.app().store.root.clone()) } else { Thumbs::new(op.app().store.root.clone()) };
    let ctx = Rc::new(EditorCtx {
        op: Rc::downgrade(op), song_id, saved_title: s.title, add_after, timer: Timer::default(), thumbs,
        previews: Rc::default(), generation: Rc::default(),
    });
    st.set_previews(ModelRc::from(ctx.previews.clone()));
    wire(op, &ctx);
    draw(op, &ctx);
    // A preview still waiting when the editor closes is not drawn.
    let c = Rc::downgrade(&ctx);
    op.on_dialog_close(move |_| if let Some(c) = c.upgrade() { c.timer.stop() });
}

/// The slide labels Settings → Sections lists.
fn label_names(op: &Operator) -> Vec<String> {
    let look = op.state.borrow().snap.as_ref().map(|s| s.settings.sections.clone()).unwrap_or_default();
    look.labels().into_iter().map(|l| l.name).collect()
}

/// The byte offset `caret` moved back to a character boundary of `text`.
fn at_char(text: &str, caret: usize) -> usize {
    let mut c = caret.min(text.len());
    while !text.is_char_boundary(c) { c -= 1 }
    c
}

/// Insert slide break (N34): the words before the caret end a slide and the rest start the next one (one empty line
/// between them). Gives the new text and the caret, at the start of the new slide. Nothing before the caret: no change.
pub fn insert_slide_break(text: &str, caret: usize) -> (String, usize) {
    let caret = at_char(text, caret);
    let space = [' ', '\t', '\r', '\n'];
    let before = text[..caret].trim_end_matches(space);
    if before.is_empty() { return (text.to_string(), caret) }
    let after = text[caret..].trim_start_matches(space);
    (format!("{before}\n\n{after}"), before.len() + 2)
}

/// Label this slide (N19): `{name}` on top of the slide the caret is in, in place of a label already there. On a
/// section name or an empty line, the slide after it. Gives the new text and the caret, just after the label.
pub fn label_slide(text: &str, caret: usize, name: &str) -> (String, usize) {
    let caret = at_char(text, caret);
    // Each line's start and its text (no line break).
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut at = 0;
    for l in text.split_inclusive('\n') { lines.push((at, l.trim_end_matches(['\r', '\n']))); at += l.len() }
    // The empty line after a last line break (or of an empty text).
    if text.is_empty() || text.ends_with('\n') { lines.push((text.len(), "")) }
    let breaks = |l: &str| l.trim().is_empty() || crate::slides::section_label(l).is_some();
    let here = lines.iter().rposition(|(start, _)| *start <= caret).unwrap_or(0);
    let mut first = here;
    if lines.get(here).is_some_and(|(_, l)| breaks(l)) {
        first = here + 1;
        while lines.get(first).is_some_and(|(_, l)| l.trim().is_empty()) { first += 1 }
    } else {
        while first > 0 && !breaks(lines[first - 1].1) { first -= 1 }
    }
    let mark = format!("{{{}}}", name.trim());
    match lines.get(first) {
        Some((start, l)) if crate::sections::label_line(l).is_some() => {
            (format!("{}{mark}{}", &text[..*start], &text[start + l.len()..]), start + mark.len())
        }
        Some((start, _)) => (format!("{}{mark}\n{}", &text[..*start], &text[*start..]), start + mark.len()),
        None => {
            let gap = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
            (format!("{text}{gap}{mark}"), text.len() + gap.len() + mark.len())
        }
    }
}

/// Order as typed: split on commas, trimmed, empties dropped (`operator.js:699`).
pub fn parse_order(order: &str) -> Vec<String> {
    order.split(',').map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect()
}

/// The Order names that match no section of the lyrics (the core leaves them out silently), each once.
pub fn order_warnings(lyrics: &str, order: &str) -> Vec<String> {
    let sections = parse_sections(lyrics);
    let mut out: Vec<String> = Vec::new();
    for name in parse_order(order) {
        let known = sections.iter().any(|s| s.label.eq_ignore_ascii_case(&name));
        if !known && !out.iter().any(|o| o.eq_ignore_ascii_case(&name)) { out.push(name) }
    }
    out
}

/// The song as the fields hold it now.
fn song_from_fields(st: &SongEditorState, id: Option<Uuid>, trim: bool) -> Song {
    let f = |v: SharedString| if trim { v.trim().to_string() } else { v.to_string() };
    Song {
        id: id.unwrap_or_else(Uuid::nil), title: f(st.get_title()), writers: f(st.get_writers()), ccli: f(st.get_ccli()),
        copyright: f(st.get_copyright()), lyrics: st.get_lyrics().to_string(), arrangement: parse_order(&st.get_order()),
        artist: f(st.get_artist()), publisher: f(st.get_publisher()), year: f(st.get_year()),
    }
}

/// `draw()` (`operator.js:667-684`): the section buttons, the Order warning and the preview, from the core's slides.
fn draw(op: &Operator, ctx: &EditorCtx) {
    let st = op.ui.global::<SongEditorState>();
    let song = song_from_fields(&st, ctx.song_id, false);
    let mut labels: Vec<SharedString> = Vec::new();
    for s in parse_sections(&song.lyrics) {
        if !labels.iter().any(|l| l == s.label.as_str()) { labels.push(s.label.into()) }
    }
    let chips: Vec<SectionChipRow> = labels.chunks(CHIPS_PER_ROW)
        .map(|c| SectionChipRow { labels: ModelRc::new(VecModel::from(c.to_vec())) }).collect();
    st.set_chips(ModelRc::new(VecModel::from(chips)));
    let warnings = order_warnings(&song.lyrics, &st.get_order());
    st.set_warning(if warnings.is_empty() { String::new() } else { tr::not_in_lyrics(&warnings) }.into());
    let (arr, _, written) = arrangement::chips(&song.lyrics, &song.arrangement);
    let arr = arr_chips(arr);
    st.set_arr_count(arr.len() as i32);
    st.set_arr_rows(arr_rows(arr));
    st.set_arr_written(written);

    let Some(settings) = op.state.borrow().snap.as_ref().map(|s| s.settings.clone()) else { return };
    let slides = song_slides(&song, &settings);
    let generation = ctx.generation.get() + 1;
    ctx.generation.set(generation);
    ctx.thumbs.cancel_pending();
    // A slide's label (N19) shows after its section: "Verse 1 · Instrumental".
    let caption = |s: &crate::model::Slide| match &s.tag { Some(t) => format!("{} · {t}", s.label), None => s.label.clone() };
    ctx.previews.set_vec(slides.iter().map(|s| PreviewSlide { image: Image::default(), label: caption(s).into() }).collect::<Vec<_>>());
    let size = thumb_size(PREVIEW_W, op.window().scale_factor());
    let media = op.state.borrow().snap.as_ref().map(|s| s.data.media.clone()).unwrap_or_default();
    for (i, slide) in slides.iter().enumerate() {
        let (key, job) = thumb(slide, "song", &[], None, &settings, &media, size);
        let (previews, me) = (ctx.previews.clone(), ctx.op.clone());
        let ctx_gen = ctx.generation.clone();
        ctx.thumbs.request(key, job, move |px| {
            if ctx_gen.get() != generation || me.upgrade().is_none() { return }
            if let Some(mut p) = previews.row_data(i) {
                p.image = Image::from_rgba8(px);
                previews.set_row_data(i, p);
            }
        });
    }
}

/// Redraws 250 ms after the last edit.
fn later(ctx: &Rc<EditorCtx>) {
    let c = Rc::downgrade(ctx);
    ctx.timer.start(slint::TimerMode::SingleShot, PREVIEW_AFTER, move || {
        if let Some(ctx) = c.upgrade() && let Some(op) = ctx.op.upgrade() { draw(&op, &ctx) }
    });
}

/// `+ <label>`: the label goes on the end of Order, and the preview follows at once.
fn add_to_order(op: &Operator, ctx: &EditorCtx, label: &str) {
    let st = op.ui.global::<SongEditorState>();
    let mut order = parse_order(&st.get_order());
    order.push(label.to_string());
    st.set_order(order.join(", ").into());
    draw(op, ctx);
}

/// The order's chips for an `ArrangementStrip` (song editor and Item edit).
pub(crate) fn arr_chips(chips: Vec<arrangement::ArrChipData>) -> Vec<ArrChip> {
    chips.into_iter().enumerate().map(|(index, c)| ArrChip {
        index: index as i32, label: c.label.into(), short: c.short.into(), kind: c.kind.id().into(), missing: c.missing, faded: c.faded,
    }).collect()
}

pub(crate) fn arr_rows(chips: Vec<ArrChip>) -> ModelRc<ArrChipRow> {
    ModelRc::new(VecModel::from(chips.chunks(ARR_PER_ROW).map(|c| ArrChipRow { chips: ModelRc::new(VecModel::from(c.to_vec())) }).collect::<Vec<_>>()))
}

/// A change to the order through its chips (FEAT-4 §3.5). An empty order (the written one) is spelled out first.
fn change_order(op: &Operator, ctx: &EditorCtx, f: impl FnOnce(&mut Vec<String>)) {
    let st = op.ui.global::<SongEditorState>();
    let mut order = parse_order(&st.get_order());
    arrangement::ensure_real(&mut order, &st.get_lyrics());
    f(&mut order);
    st.set_order(order.join(", ").into());
    draw(op, ctx);
}

pub(crate) fn arr_drag_data(index: i32) -> slint::DataTransfer {
    let mut d = slint::DataTransfer::default();
    d.set_plain_text(format!("{ARR_DRAG}{index}").into());
    d
}

pub(crate) fn arr_drag_index(d: &slint::DataTransfer) -> i32 {
    d.plain_text().ok().and_then(|t| t.strip_prefix(ARR_DRAG).and_then(|i| i.parse().ok())).unwrap_or(-1)
}

#[derive(Deserialize)]
struct Saved { id: Uuid }

/// Save (`operator.js:697-704`): every field trimmed except the lyrics, `song/save`, then add it to the service
/// (Save and add to service) or the toast "Song saved", and close. A refusal is an error toast and the editor stays.
/// `show_now` (Save and show now, FEAT-4): once saved, Update screen for the live item.
fn save(op: &Rc<Operator>, ctx: &EditorCtx, show_now: bool) {
    let song = song_from_fields(&op.ui.global::<SongEditorState>(), ctx.song_id, true);
    let add_after = ctx.add_after;
    let me = Rc::downgrade(op);
    op.ops().call::<Saved>("song/save", serde_json::to_value(&song).unwrap_or_default(), move |r| {
        let Some(op) = me.upgrade() else { return };
        let Saved { id } = match r { Ok(s) => s, Err(e) => return op.toast(e, true) };
        if show_now {
            refresh_live(&op);
            op.close_dialog();
        } else if add_after {
            add_item_then(&op, json!({ "kind": "song", "song_id": id }), false, |op, _| op.close_dialog());
        } else {
            op.toast(tr::song_saved(), false);
            op.close_dialog();
        }
    });
}

/// Update screen: the live item as the library has it now, at the same place. With the text cleared, a toast says
/// why nothing moved.
pub fn refresh_live(op: &Rc<Operator>) {
    let Some((item_id, cleared)) = op.shown_live().and_then(|l| Some((l.view().cursor.as_ref()?.item_id, l.view().frame.slide.is_none()))) else { return };
    let me = Rc::downgrade(op);
    op.ops().cmd(crate::live::Command::Refresh { item_id }, None, move |o| {
        let Some(op) = me.upgrade() else { return };
        if o.ok && cleared { op.toast(crate::sections::strings::saved_text_cleared(), false) }
        else if let Some((msg, err)) = crate::ui::ops::outcome_toast(&o) { op.toast(msg, err) }
    });
}

/// Delete song (`operator.js:706`): asks first; `song/delete` refuses a song still in a service.
fn delete(op: &Rc<Operator>, ctx: &EditorCtx) {
    let Some(id) = ctx.song_id else { return };
    let me = Rc::downgrade(op);
    confirm(op, &tr::delete_from_library(&ctx.saved_title), &tr::ok(), &tr::cancel(), true, move |yes| {
        let Some(op) = me.upgrade().filter(|_| yes) else { return };
        let me = Rc::downgrade(&op);
        op.ops().call::<Value>("song/delete", json!({ "id": id }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r { Ok(_) => op.close_dialog(), Err(e) => op.toast(e, true) }
        });
    });
}

/// A button's change to the lyrics at the caret; the preview follows at once. Gives the caret's new place.
fn edit_lyrics(ctx: &EditorCtx, caret: i32, f: impl FnOnce(&str, usize) -> (String, usize)) -> i32 {
    let Some(op) = ctx.op.upgrade() else { return caret };
    let st = op.ui.global::<SongEditorState>();
    let (text, at) = f(&st.get_lyrics(), usize::try_from(caret).unwrap_or(0));
    st.set_lyrics(text.into());
    draw(&op, ctx);
    i32::try_from(at).unwrap_or(caret)
}

/// Points `SongEditorState`'s callbacks at this editor. Set on each open.
fn wire(op: &Rc<Operator>, ctx: &Rc<EditorCtx>) {
    let st = op.ui.global::<SongEditorState>();
    let c = ctx.clone();
    st.on_edited(move || later(&c));
    let c = ctx.clone();
    st.on_chip(move |label| if let Some(op) = c.op.upgrade() { add_to_order(&op, &c, &label) });
    let c = ctx.clone();
    st.on_arr_move(move |from, to| if let (Some(op), Ok(from), Ok(to)) = (c.op.upgrade(), usize::try_from(from), usize::try_from(to)) {
        change_order(&op, &c, |v| arrangement::move_entry(v, from, to))
    });
    let c = ctx.clone();
    st.on_arr_remove(move |i| if let (Some(op), Ok(i)) = (c.op.upgrade(), usize::try_from(i)) { change_order(&op, &c, |v| arrangement::remove(v, i)) });
    let c = ctx.clone();
    st.on_arr_reset(move || if let Some(op) = c.op.upgrade() {
        op.ui.global::<SongEditorState>().set_order(SharedString::new());
        draw(&op, &c);
    });
    st.on_arr_drag_data(arr_drag_data);
    st.on_arr_drag_index(|d| arr_drag_index(&d));
    let c = ctx.clone();
    st.on_slide_break(move |caret| edit_lyrics(&c, caret, insert_slide_break));
    let c = ctx.clone();
    st.on_label_slide(move |caret, i| {
        let Some(name) = c.op.upgrade().and_then(|op| label_names(&op).into_iter().nth(usize::try_from(i).unwrap_or(usize::MAX))) else { return caret };
        edit_lyrics(&c, caret, |t, at| label_slide(t, at, &name))
    });
    let c = ctx.clone();
    st.on_save(move || if let Some(op) = c.op.upgrade() { save(&op, &c, false) });
    let c = ctx.clone();
    st.on_save_and_show(move || if let Some(op) = c.op.upgrade() { save(&op, &c, true) });
    let c = ctx.clone();
    st.on_delete(move || if let Some(op) = c.op.upgrade() { delete(&op, &c) });
    let c = ctx.clone();
    st.on_cancel(move || if let Some(op) = c.op.upgrade() { op.close_dialog() });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        model::{ItemKind, ServiceItem},
        ui::{
            dialogs::{AddTab, add::{open_add, tests::*}},
            operator::window::testing::{find, find_all, refresh},
        },
    };
    use i_slint_backend_testing::mock_elapsed_time;
    use slint::platform::{Key, PointerEventButton};

    fn st(op: &Operator) -> SongEditorState<'_> { op.ui.global::<SongEditorState>() }

    fn labels(op: &Operator) -> Vec<String> { st(op).get_previews().iter().map(|p| p.label.to_string()).collect() }

    fn chip_labels(op: &Operator) -> Vec<String> {
        st(op).get_chips().iter().flat_map(|r| r.labels.iter().map(|l| l.to_string()).collect::<Vec<_>>()).collect()
    }

    fn library(app: &App) -> Vec<Song> { app.data.lock().unwrap().songs.clone() }

    fn question(op: &Operator) -> Option<String> { op.ui.get_ask().row_data(0).map(|a| a.text.to_string()) }

    #[test]
    fn song_editor_preview_uses_core_slides() {
        let app = test_app();
        let op = opened(&app);
        open_song_editor(&op, None, false);
        settle(&op);
        assert_eq!(op.ui.get_dialog_title(), "New song");
        assert!(op.ui.get_dialog_wide());
        assert!(labels(&op).is_empty());
        assert!(find(&op, "SongEditor::empty").is_some(), "the empty preview text");
        st(&op).set_lyrics("Verse 1\na\nb\nc".into());
        st(&op).invoke_edited();
        mock_elapsed_time(PREVIEW_AFTER - Duration::from_millis(50));
        assert!(labels(&op).is_empty(), "not before 250 ms");
        mock_elapsed_time(Duration::from_millis(60));
        assert_eq!(labels(&op), ["Verse 1 (1/2)", "Verse 1 (2/2)"], "2 lines per slide, by the core");
        let previews = find_all(&op, "SongEditor::preview");
        assert_eq!(previews.len(), 2);
        assert_eq!(previews[1].accessible_label().unwrap_or_default(), "Verse 1 (2/2)");
        assert!(st(&op).get_previews().iter().all(|p| p.image.size().width > 0), "each slide is drawn");
    }

    fn arr(op: &Operator) -> Vec<(String, bool, bool)> {
        st(op).get_arr_rows().iter().flat_map(|r| r.chips.iter().map(|c| (c.short.to_string(), c.missing, c.faded)).collect::<Vec<_>>()).collect()
    }

    #[test]
    fn order_chips_drag_remove_and_reset() {
        let app = test_app();
        let id = song(&app, "Grace", "", "", "Verse 1\na\n\nChorus\nb\n\nVerse 2\nc");
        let op = opened(&app);
        open_song_editor(&op, Some(id), false);
        settle(&op);
        assert_eq!(arr(&op), [("V1".into(), false, true), ("C".into(), false, true), ("V2".into(), false, true)], "the written order, faded");
        assert!(st(&op).get_arr_written());
        st(&op).invoke_arr_move(1, 3);
        assert_eq!(st(&op).get_order(), "Verse 1, Verse 2, Chorus", "the first drag spells out the written order");
        assert_eq!(labels(&op), ["Verse 1", "Verse 2", "Chorus"], "the preview follows");
        st(&op).invoke_arr_move(2, 0);
        assert_eq!(st(&op).get_order(), "Chorus, Verse 1, Verse 2");
        st(&op).invoke_arr_remove(1);
        assert_eq!(st(&op).get_order(), "Chorus, Verse 2");
        st(&op).set_order("Chorus, Coda".into());
        st(&op).invoke_edited();
        mock_elapsed_time(PREVIEW_AFTER + Duration::from_millis(10));
        assert_eq!(arr(&op), [("C".into(), false, false), ("E".into(), true, false)], "Coda is not in the lyrics");
        st(&op).invoke_arr_reset();
        assert_eq!(st(&op).get_order(), "");
        assert!(st(&op).get_arr_written());
    }

    fn key(op: &Operator, k: Key) {
        let text: SharedString = char::from(k).into();
        op.window().dispatch_event(slint::platform::WindowEvent::KeyPressed { text: text.clone() });
        op.window().dispatch_event(slint::platform::WindowEvent::KeyReleased { text });
    }

    fn ctrl(op: &Operator, k: Key) {
        let c: SharedString = char::from(Key::Control).into();
        op.window().dispatch_event(slint::platform::WindowEvent::KeyPressed { text: c.clone() });
        key(op, k);
        op.window().dispatch_event(slint::platform::WindowEvent::KeyReleased { text: c });
    }

    /// Clicks order chip `i` (selects it and gives the strip the keyboard).
    fn click_chip(op: &Operator, i: usize) {
        find_all(op, "ArrangementStrip::arr-ta")[i].mock_single_click(PointerEventButton::Left);
    }

    #[test]
    fn ctrl_arrow_moves_selected_chip() {
        let app = test_app();
        let id = song(&app, "Grace", "", "", "Verse 1\na\n\nChorus\nb\n\nVerse 2\nc");
        let op = opened(&app);
        open_song_editor(&op, Some(id), false);
        settle(&op);
        click_chip(&op, 0);
        assert_eq!(st(&op).get_arr_selected(), 0);
        ctrl(&op, Key::RightArrow);
        assert_eq!(st(&op).get_order(), "Chorus, Verse 1, Verse 2", "the first move spells out the written order");
        assert_eq!(st(&op).get_arr_selected(), 1, "the chip stays chosen");
        ctrl(&op, Key::RightArrow);
        assert_eq!(st(&op).get_order(), "Chorus, Verse 2, Verse 1");
        ctrl(&op, Key::RightArrow);
        assert_eq!(st(&op).get_order(), "Chorus, Verse 2, Verse 1", "the last one stays last");
        key(&op, Key::LeftArrow);
        assert_eq!(st(&op).get_arr_selected(), 1, "an arrow alone chooses another chip");
        ctrl(&op, Key::LeftArrow);
        assert_eq!(st(&op).get_order(), "Verse 2, Chorus, Verse 1");
    }

    #[test]
    fn delete_key_removes_selected_chip() {
        let app = test_app();
        let id = song(&app, "Grace", "", "", "Verse 1\na\n\nChorus\nb");
        let op = opened(&app);
        open_song_editor(&op, Some(id), false);
        settle(&op);
        st(&op).set_order("Verse 1, Chorus, Chorus".into());
        st(&op).invoke_edited();
        mock_elapsed_time(PREVIEW_AFTER + Duration::from_millis(10));
        click_chip(&op, 2);
        key(&op, Key::Delete);
        assert_eq!(st(&op).get_order(), "Verse 1, Chorus");
        assert_eq!(st(&op).get_arr_selected(), 1, "the one before is chosen");
        key(&op, Key::Backspace);
        assert_eq!(st(&op).get_order(), "Verse 1");
        assert_eq!(op.dialog(), Some(DialogKind::SongEditor { song_id: Some(id), add_after: false }), "the dialog stays open");
    }

    #[test]
    fn insert_slide_break_splits_at_the_caret() {
        let t = "Verse 1\nline one\nline two";
        let at = t.find("line two").unwrap();
        assert_eq!(insert_slide_break(t, at), ("Verse 1\nline one\n\nline two".into(), at + 1));
        let mid = t.find("one").unwrap();
        assert_eq!(insert_slide_break(t, mid).0, "Verse 1\nline\n\none\nline two", "mid-line: the line splits");
        let gap = "Verse 1\na\n\nb";
        assert_eq!(insert_slide_break(gap, gap.find('b').unwrap()).0, gap, "one empty line, never two");
        assert_eq!(insert_slide_break("abc", 0), ("abc".into(), 0), "nothing before the caret");
        assert_eq!(insert_slide_break("é", 1), ("é".into(), 0), "a caret inside a letter moves back");
    }

    #[test]
    fn label_slide_puts_the_label_on_top_of_the_caret_slide() {
        let t = "Verse 1\na\nb\n\nc\nd";
        let (out, at) = label_slide(t, t.find('d').unwrap(), "Repeat");
        assert_eq!(out, "Verse 1\na\nb\n\n{Repeat}\nc\nd");
        assert_eq!(&out[at - "{Repeat}".len()..at], "{Repeat}");
        assert_eq!(label_slide(&out, out.find('d').unwrap(), "Blank").0, "Verse 1\na\nb\n\n{Blank}\nc\nd", "replaces the label there");
        assert_eq!(label_slide(t, 2, "Instrumental").0, "Verse 1\n{Instrumental}\na\nb\n\nc\nd", "on the section name: its first slide");
        assert_eq!(label_slide("Verse 1\na\n", 10, "Blank").0, "Verse 1\na\n{Blank}", "on the empty end: a new slide");
    }

    #[test]
    fn buttons_change_the_lyrics_and_the_preview_follows_at_once() {
        let app = test_app();
        let op = opened(&app);
        open_song_editor(&op, None, false);
        settle(&op);
        let options: Vec<String> = st(&op).get_labels().iter().map(|l| l.to_string()).collect();
        assert_eq!(options, ["Label this slide…", "Blank", "Instrumental", "Repeat", "Spoken Word"]);
        st(&op).set_lyrics("Verse 1\na\nb".into());
        st(&op).invoke_edited();
        mock_elapsed_time(PREVIEW_AFTER + Duration::from_millis(10));
        assert_eq!(labels(&op), ["Verse 1"]);
        let caret = st(&op).invoke_slide_break(10);
        assert_eq!(st(&op).get_lyrics(), "Verse 1\na\n\nb");
        assert_eq!(caret, 11);
        assert_eq!(labels(&op), ["Verse 1", "Verse 1"], "two slides, with no wait");
        st(&op).invoke_label_slide(caret, 1);
        assert_eq!(st(&op).get_lyrics(), "Verse 1\na\n\n{Instrumental}\nb");
        assert_eq!(labels(&op), ["Verse 1", "Verse 1 · Instrumental"], "the preview names the label");
        assert!(find(&op, "SongEditor::slide-break").is_some());
    }

    #[test]
    fn artist_publisher_and_year_are_saved() {
        let app = test_app();
        let op = opened(&app);
        open_song_editor(&op, None, false);
        settle(&op);
        st(&op).set_title("Grace".into());
        st(&op).set_artist(" Chris Tomlin ".into());
        st(&op).set_publisher("sixsteps".into());
        st(&op).set_year("2006".into());
        click(&op, "SongEditor::save");
        let s = &library(&app)[0];
        assert_eq!((s.artist.as_str(), s.publisher.as_str(), s.year.as_str()), ("Chris Tomlin", "sixsteps", "2006"));
        refresh(&op);
        open_song_editor(&op, Some(s.id), false);
        assert_eq!(st(&op).get_artist(), "Chris Tomlin");
        assert_eq!(st(&op).get_year(), "2006");
    }

    #[test]
    fn order_chip_drag_payload_round_trips() {
        assert_eq!(arr_drag_index(&arr_drag_data(3)), 3);
        let mut other = slint::DataTransfer::default();
        other.set_plain_text("svc:1".into());
        assert_eq!(arr_drag_index(&other), -1);
    }

    #[test]
    fn section_chips_append_to_order() {
        let app = test_app();
        let id = song(&app, "Grace", "", "", "Verse 1\na\n\nChorus\nb\n\nVerse 2\nc\n\nChorus\nb");
        let op = opened(&app);
        open_song_editor(&op, Some(id), false);
        settle(&op);
        assert_eq!(chip_labels(&op), ["Verse 1", "Chorus", "Verse 2"], "each section once");
        st(&op).invoke_chip("Chorus".into());
        assert_eq!(st(&op).get_order(), "Chorus");
        assert_eq!(labels(&op), ["Chorus"], "the preview follows at once");
        settle(&op);
        find_all(&op, "SongEditor::chip")[0].mock_single_click(PointerEventButton::Left);
        assert_eq!(st(&op).get_order(), "Chorus, Verse 1");
        assert_eq!(labels(&op), ["Chorus", "Verse 1"]);
    }

    #[test]
    fn order_warnings_lists_unknown_names() {
        let lyrics = "Verse 1\na\n\nChorus\nb";
        assert_eq!(order_warnings(lyrics, "verse 1, Bridge, chorus, bridge,, Tag "), ["Bridge", "Tag"]);
        assert!(order_warnings(lyrics, "").is_empty());
        assert_eq!(parse_order(" a , ,b,"), ["a", "b"]);
        let app = test_app();
        let op = opened(&app);
        open_song_editor(&op, None, false);
        st(&op).set_lyrics(lyrics.into());
        st(&op).set_order("Chorus, Bridge".into());
        st(&op).invoke_edited();
        mock_elapsed_time(PREVIEW_AFTER);
        assert_eq!(st(&op).get_warning(), "Not in the lyrics: Bridge");
        assert_eq!(labels(&op), ["Chorus"], "the core leaves the unknown name out");
    }

    #[test]
    fn save_new_song_with_add_after_adds_to_service() {
        let app = test_app();
        let op = opened(&app);
        open_add(&op, AddTab::Song);
        settle(&op);
        click(&op, "AddSongTab::new-song");
        // The editor replaced the Add dialog; its title asks for the keyboard again 30 ms later.
        mock_elapsed_time(Duration::from_millis(40));
        settle(&op);
        assert_eq!(op.dialog(), Some(DialogKind::SongEditor { song_id: None, add_after: true }));
        assert!(find(&op, "SongEditor::delete").is_none(), "a new song cannot be deleted");
        // The title has the keyboard.
        type_text(&op, "  Amazing Grace ");
        st(&op).set_writers(" John Newton ".into());
        st(&op).set_lyrics("Verse 1\nAmazing grace \n".into());
        st(&op).set_order(" Verse 1 ,".into());
        click(&op, "SongEditor::save");
        let songs = library(&app);
        assert_eq!(songs.len(), 1);
        let s = &songs[0];
        assert_eq!((s.title.as_str(), s.writers.as_str(), s.lyrics.as_str()), ("Amazing Grace", "John Newton", "Verse 1\nAmazing grace \n"));
        assert_eq!(s.arrangement, ["Verse 1"]);
        assert!(matches!(items(&app)[..], [ItemKind::Song { song_id, .. }] if song_id == s.id));
        assert_eq!(last_toast(&op), Some(("Added to the service".into(), false)));
        assert_eq!(op.dialog(), None);

        // Edit from the Add dialog saves without adding.
        refresh(&op);
        open_add(&op, AddTab::Song);
        settle(&op);
        click(&op, "AddListRow::edit-btn");
        assert_eq!(op.dialog(), Some(DialogKind::SongEditor { song_id: Some(s.id), add_after: false }));
        assert_eq!(op.ui.get_dialog_title(), "Edit song");
        assert_eq!(st(&op).get_writers(), "John Newton");
        st(&op).set_ccli("4768151".into());
        click(&op, "SongEditor::save");
        assert_eq!(last_toast(&op), Some(("Song saved".into(), false)));
        assert_eq!(library(&app)[0].ccli, "4768151");
        assert_eq!(items(&app).len(), 1);
        assert_eq!(op.dialog(), None);
    }

    #[test]
    fn blank_title_shows_core_error_and_stays_open() {
        let app = test_app();
        let op = opened(&app);
        open_song_editor(&op, None, false);
        settle(&op);
        st(&op).set_title("   ".into());
        click(&op, "SongEditor::save");
        assert_eq!(last_toast(&op), Some(("Give the song a title.".into(), true)));
        assert_eq!(op.dialog(), Some(DialogKind::SongEditor { song_id: None, add_after: false }));
        assert!(library(&app).is_empty());
    }

    #[test]
    fn delete_song_in_service_shows_core_error() {
        let app = test_app();
        let used = song(&app, "Used", "", "", "Verse 1\na");
        let spare = song(&app, "Spare", "", "", "Verse 1\nb");
        app.data.lock().unwrap().services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: used, arrangement: None }, background: None, notes: String::new(), theme: None });
        let op = opened(&app);
        open_song_editor(&op, Some(used), false);
        settle(&op);
        click(&op, "SongEditor::delete");
        assert_eq!(question(&op).as_deref(), Some("Delete “Used” from the library?"));
        press(&op, char::from(Key::Return));
        settle(&op);
        assert_eq!(last_toast(&op), Some(("This song is in a service. Remove it from the service first.".into(), true)));
        assert_eq!(op.dialog(), Some(DialogKind::SongEditor { song_id: Some(used), add_after: false }));

        open_song_editor(&op, Some(spare), false);
        settle(&op);
        click(&op, "SongEditor::delete");
        press(&op, char::from(Key::Escape));
        settle(&op);
        assert_eq!(library(&app).len(), 2, "Cancel deletes nothing");
        assert!(op.dialog().is_some(), "and the editor stays");
        click(&op, "SongEditor::delete");
        press(&op, char::from(Key::Return));
        settle(&op);
        assert_eq!(library(&app).iter().map(|s| s.id).collect::<Vec<_>>(), [used]);
        assert_eq!(op.dialog(), None);
    }

    #[test]
    fn esc_closes_editor_without_saving() {
        let app = test_app();
        let id = song(&app, "Grace", "", "", "Verse 1\na");
        let op = opened(&app);
        open_song_editor(&op, Some(id), false);
        settle(&op);
        // The lyrics have the keyboard when editing.
        type_text(&op, "typed ");
        settle(&op);
        assert!(st(&op).get_lyrics().contains("typed "), "{}", st(&op).get_lyrics());
        press(&op, char::from(Key::Escape));
        settle(&op);
        assert_eq!(op.dialog(), None);
        assert_eq!(library(&app)[0].lyrics, "Verse 1\na", "nothing saved");
    }

    #[test]
    fn song_editor_keeps_typed_text_across_a_data_update() {
        let app = test_app();
        let id = song(&app, "Grace", "", "", "Verse 1\na");
        let op = opened(&app);
        open_song_editor(&op, Some(id), false);
        settle(&op);
        type_text(&op, "New ");
        st(&op).set_title("Grace (typed)".into());
        let typed = st(&op).get_lyrics();
        assert_ne!(typed, "Verse 1\na");
        // A phone adds a song: a `Data` update arrives while the user types.
        song(&app, "From the phone", "", "", "");
        refresh(&op);
        settle(&op);
        assert_eq!(st(&op).get_lyrics(), typed);
        assert_eq!(st(&op).get_title(), "Grace (typed)");
        assert_eq!(op.dialog(), Some(DialogKind::SongEditor { song_id: Some(id), add_after: false }));
    }
}
