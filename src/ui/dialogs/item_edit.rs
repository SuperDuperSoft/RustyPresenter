//! Item edit (`openItemEdit`, `operator.js:720-796`; `ui/dialogs/item-edit.slint`). It works on a clone of the
//! service item and is never redrawn by `dialog_refresh`, so a phone or a conversion never wipes what was typed.
//! Save renames the media or slides when the name changed, then sends `service/update`; when that fails (the item
//! is gone, say) the dialog stays open with the core's message. A passage whose Bible was changed in its picker is
//! looked up again in that Bible first (Bible library spec §7.2); a failed lookup also keeps the dialog open. The
//! "Also show" pickers choose up to two more Bibles for this passage only (Nov 5 spec §3); a newly picked one is
//! looked up on Save with the rest, and one that can't be is left out with a toast. A song gets "Order for this
//! service" (FEAT-4 §3.5): untick "Use the song's usual order" to give this service its own order.
use super::{DialogKind, background::{self, Picker, strings}, song_editor::{arr_chips, arr_drag_data, arr_drag_index, arr_rows}};
use crate::{
    model::{ItemKind, MediaStatus, ServiceItem, Song, VerseText},
    scripture::{AlsoShow, MAX_ALSO, ParallelLayout, strings as bible_tr},
    sections::{arrangement, strings as song_tr},
    ui::{ArrAddRow, ItemEdit, operator::{bible, view, window::Operator}},
};
use serde::Deserialize;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::{Rc, Weak}};
use uuid::Uuid;

/// The library entry the name field renames, with the name it had when the dialog opened.
enum Name { Media { id: Uuid, was: String }, Deck { id: Uuid, was: String } }

struct Edit {
    item: ServiceItem,
    name: Option<Name>,
    picker: Option<Picker>,
    /// Scripture: the Bible its picker chose (the item's own until a pick).
    bible: Option<String>,
    /// Scripture: the Also Bibles' ids, in order, and which "Also show" picker is open.
    also: Vec<String>,
    also_slot: usize,
    /// Song: the library song and the order the strip shows (this service's own while "usual" is unticked).
    order: Option<(Song, Vec<String>)>,
    /// What each line of the Theme box picks (`None`: the service's).
    themes: Vec<Option<crate::theme::ThemeId>>,
}

/// `+ <label>` buttons per row under the order strip.
const ADDS_PER_ROW: usize = 4;

thread_local! {
    /// The open Item edit. The callbacks hold it weakly, so closing the dialog lets it (and the picker's thumbnail
    /// worker) go without touching the callbacks, which may be running at that moment.
    static OPEN: RefCell<Option<Rc<RefCell<Edit>>>> = const { RefCell::new(None) };
}

/// "When the video ends", in the order of the segments.
use crate::media_prefs::ENDS;

/// Opens Item edit for service item `id` (the row's Edit button or the slides header's). Nothing happens when the
/// item is not in the newest snapshot.
pub fn open(op: &Rc<Operator>, id: Uuid) {
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let Some(row) = snap.items.iter().find(|i| i.id == id) else { return };
    let item = row.item.clone();
    let g = op.ui.global::<ItemEdit>();
    g.set_kind(row.kind.as_str().into());
    g.set_song_exists(false);
    g.set_per_slide(0);
    g.set_size_mode(0);
    g.set_text_size(snap.data.settings.theme.scripture_size as i32);
    g.set_title("".into());
    g.set_body("".into());
    g.set_autoplay(true);
    g.set_end_index(0);
    g.set_looped(false);
    g.set_has_name(false);
    g.set_name("".into());
    g.set_not_ready(false);
    g.set_translation("".into());
    g.set_translation_changed(false);
    g.set_picker_search("".into());
    g.set_picker_rows(ModelRc::default());
    g.set_usual_order(true);
    g.set_order_selected(-1);
    // FEAT-2 auto-advance (spec §3.5).
    let adv = snap.data.automation.advance.get(&id).copied();
    g.set_advance_shown(matches!(row.kind.as_str(), "song" | "scripture" | "text" | "deck"));
    g.set_advance_allowed(row.slides.len() >= 2);
    g.set_advance_on(adv.is_some());
    g.set_advance_secs(adv.map_or(crate::automation::advance::DEFAULT_SECS, |a| a.secs).to_string().into());
    g.set_advance_loop(adv.is_none_or(|a| a.loop_item));
    let mut name = None;
    let mut order = None;
    let bible = match &item.kind { ItemKind::Scripture { translation, .. } => Some(translation.clone()), _ => None };
    let also = match &item.kind { ItemKind::Scripture { also, .. } => also.iter().map(|a| a.translation.clone()).collect(), _ => vec![] };
    g.set_also_search("".into());
    g.set_also_rows(ModelRc::default());
    g.set_also_layout(i32::from(matches!(item.kind, ItemKind::Scripture { layout: ParallelLayout::Stacked, .. })));
    match &item.kind {
        ItemKind::Song { song_id, arrangement } => if let Some(song) = snap.data.songs.iter().find(|s| s.id == *song_id) {
            g.set_song_exists(true);
            g.set_usual_order(arrangement.is_none());
            order = Some((song.clone(), arrangement.clone().unwrap_or_else(|| song.arrangement.clone())));
        },
        ItemKind::Scripture { per_slide, text_size, .. } => {
            g.set_per_slide(i32::from(*per_slide));
            if *text_size > 0 {
                g.set_size_mode(1);
                g.set_text_size(*text_size as i32);
            }
        }
        ItemKind::Text { title, body } => {
            g.set_title(title.as_str().into());
            g.set_body(body.as_str().into());
        }
        ItemKind::Media { media_id, autoplay, end, looped } => {
            g.set_autoplay(*autoplay);
            g.set_end_index(ENDS.iter().position(|e| e == end).unwrap_or(0) as i32);
            g.set_looped(*looped);
            if let Some(m) = snap.data.media.iter().find(|m| m.id == *media_id) {
                g.set_has_name(true);
                g.set_name(m.name.as_str().into());
                name = Some(Name::Media { id: m.id, was: m.name.clone() });
                let chip = match &m.status {
                    MediaStatus::Failed { reason } => Some((strings::failed(), "bad", reason.clone())),
                    _ => None,
                };
                if let Some((text, state, reason)) = chip {
                    g.set_not_ready(true);
                    g.set_status_text(text.into());
                    g.set_status_state(state.into());
                    g.set_status_reason(reason.into());
                }
            }
        }
        ItemKind::Deck { deck_id } => if let Some(d) = snap.data.decks.iter().find(|d| d.id == *deck_id) {
            g.set_has_name(true);
            g.set_name(d.title.as_str().into());
            name = Some(Name::Deck { id: d.id, was: d.title.clone() });
        },
        // FEAT-6: a blank keeps its notes and background here; a header is renamed from the service list.
        ItemKind::Blank { .. } | ItemKind::Header { .. } => {}
    }
    let has_background = matches!(item.kind, ItemKind::Song { .. } | ItemKind::Scripture { .. } | ItemKind::Text { .. });
    g.set_has_background(has_background);
    g.set_palette(ModelRc::new(VecModel::from(background::palette())));
    g.set_notes(item.notes.as_str().into());
    let picker = has_background.then(|| Picker::new(op, item.background.clone(), snap.data.media.clone(), true));
    let themes = if has_background { theme_choices(&snap.data, &item) } else { vec![] };
    g.set_theme_options(ModelRc::new(VecModel::from(themes.iter().map(|(l, _)| l.as_str().into()).collect::<Vec<slint::SharedString>>())));
    g.set_theme_index(themes.iter().position(|(_, t)| *t == item.theme).unwrap_or(0) as i32);
    let themes = themes.into_iter().map(|(_, t)| t).collect();
    let edit = Rc::new(RefCell::new(Edit { item, name, picker, bible, also, also_slot: 0, order, themes }));

    op.open_dialog(DialogKind::ItemEdit(id));
    draw_choices(op, &edit);
    draw_order(op, &edit);
    show_bible(op, &edit);
    show_also(op, &edit);
    wire(op, &edit);
    OPEN.with(|o| *o.borrow_mut() = Some(edit));
    op.on_dialog_close(|op| {
        OPEN.with(|o| o.borrow_mut().take());
        op.ui.global::<ItemEdit>().set_choices(ModelRc::default());
    });
}

fn wire(op: &Rc<Operator>, edit: &Rc<RefCell<Edit>>) {
    let g = op.ui.global::<ItemEdit>();
    let with = |f: fn(&Rc<Operator>, &Rc<RefCell<Edit>>)| {
        let (me, edit): (Weak<Operator>, _) = (Rc::downgrade(op), Rc::downgrade(edit));
        move || if let (Some(op), Some(edit)) = (me.upgrade(), edit.upgrade()) { f(&op, &edit) }
    };
    g.on_save(with(save));
    g.on_choose_breaks(with(|op, edit| save_then(op, edit, true)));
    g.on_remove(with(remove));
    g.on_edit_song(with(edit_song));
    g.on_make_usual(with(make_usual));
    g.on_order_reset(with(|op, edit| change_order(op, edit, Vec::clear)));
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_usual_order_toggled(move |on| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) {
        if let Some((song, order)) = e.borrow_mut().order.as_mut() {
            // Ticked: the song's usual order; unticked: a copy of it (or of the written order) to change.
            *order = song.arrangement.clone();
            if !on { arrangement::ensure_real(order, &song.lyrics) }
        }
        op.ui.global::<ItemEdit>().set_order_selected(-1);
        draw_order(&op, &e);
    });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_order_move(move |from, to| if let (Some(op), Some(e), Ok(from), Ok(to)) = (me.upgrade(), e.upgrade(), usize::try_from(from), usize::try_from(to)) {
        change_order(&op, &e, |v| arrangement::move_entry(v, from, to))
    });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_order_remove(move |i| if let (Some(op), Some(e), Ok(i)) = (me.upgrade(), e.upgrade(), usize::try_from(i)) { change_order(&op, &e, |v| arrangement::remove(v, i)) });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_order_add(move |label| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) { change_order(&op, &e, |v| v.push(label.into())) });
    g.on_order_drag_data(arr_drag_data);
    g.on_order_drag_index(|d| arr_drag_index(&d));
    g.on_upload(with(|op, edit| {
        let (me, edit) = (op.me.clone(), Rc::downgrade(edit));
        background::upload(op, move |files| {
            let (Some(op), Some(edit)) = (me.upgrade(), edit.upgrade()) else { return };
            if let Some(p) = edit.borrow_mut().picker.as_mut() { p.add_media(files) }
            draw_choices(&op, &edit);
        });
    }));
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_pick_background(move |i| {
        let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) else { return };
        let picked = usize::try_from(i).ok().zip(e.borrow_mut().picker.as_mut()).is_some_and(|(i, p)| p.pick(i));
        if picked { draw_choices(&op, &e) }
    });
    g.on_picker_opening(with(|op, edit| show_bible(op, edit)));
    g.on_picker_accept(with(|op, edit| if let Some(id) = bible::first_picked(&op.ui.global::<ItemEdit>().get_picker_rows()) { pick_bible(op, edit, id) }));
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_picker_search_changed(move |_| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) { show_bible(&op, &e) });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_pick_bible(move |id| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) { pick_bible(&op, &e, id.into()) });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_toggle_favourite(move |id| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) {
        bible::toggle_favourite(&op, id.into());
        show_bible(&op, &e);
        show_also(&op, &e);
    });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_also_opening(move |slot| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) {
        e.borrow_mut().also_slot = usize::try_from(slot).unwrap_or(0);
        show_also(&op, &e);
    });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_also_search_changed(move |_| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) { show_also(&op, &e) });
    g.on_also_accept(with(|op, edit| if let Some(id) = bible::first_picked(&op.ui.global::<ItemEdit>().get_also_rows()) { pick_also(op, edit, id) }));
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_also_pick(move |id| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) { pick_also(&op, &e, id.into()) });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_also_remove(move |i| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) {
        if let Ok(i) = usize::try_from(i) && i < e.borrow().also.len() { e.borrow_mut().also.remove(i); }
        show_also(&op, &e);
    });
    let me = Rc::downgrade(op);
    g.on_find_more_bibles(move |q| if let Some(op) = me.upgrade() { super::settings::open_bible_search(&op, &q) });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_custom_color(move |c| if let (Some(op), Some(e)) = (me.upgrade(), e.upgrade()) { set_color(&op, &e, background::to_hex(c)) });
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    g.on_custom_hex(move |t| if let (Some(op), Some(e), Some(hex)) = (me.upgrade(), e.upgrade(), background::parse_hex(&t)) { set_color(&op, &e, hex) });
}

fn set_color(op: &Operator, edit: &Rc<RefCell<Edit>>, hex: String) {
    if let Some(p) = edit.borrow_mut().picker.as_mut() { p.set_color(hex) }
    draw_choices(op, edit);
}

/// The Bible chip, the note under it and the picker's lines (keeping its search). A Bible no longer installed
/// shows as its id.
fn show_bible(op: &Operator, edit: &Rc<RefCell<Edit>>) {
    let e = edit.borrow();
    let (Some(chosen), ItemKind::Scripture { translation, .. }) = (e.bible.as_ref(), &e.item.kind) else { return };
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let g = op.ui.global::<ItemEdit>();
    g.set_translation(view::bible_labels(&snap).get(chosen).cloned().unwrap_or_else(|| chosen.clone()).into());
    g.set_translation_changed(!crate::bible::id::same_id(chosen, translation));
    let (rows, note) = bible::picker_rows(&snap, &g.get_picker_search(), chosen);
    g.set_picker_rows(rows);
    g.set_picker_note(note);
}

/// A row of the picker: the Bible Save looks the passage up in. An Also Bible that is now the main one is turned
/// off (Nov 5 spec §3).
fn pick_bible(op: &Operator, edit: &Rc<RefCell<Edit>>, id: String) {
    let known = op.state.borrow().snap.as_ref().is_some_and(|s| s.translations.contains(&id));
    if !known || edit.borrow().bible.is_none() { return }
    let was_also = {
        let mut e = edit.borrow_mut();
        let n = e.also.len();
        e.also.retain(|t| !crate::bible::id::same_id(t, &id));
        e.bible = Some(id.clone());
        e.also.len() != n
    };
    if was_also { op.toast(bible_tr::also_turned_off(&label_of(op, &id)), false) }
    show_bible(op, edit);
    show_also(op, edit);
}

/// What a Bible's chip says ("RV1909 · spa"), else its id.
fn label_of(op: &Operator, id: &str) -> String {
    op.state.borrow().snap.as_ref().and_then(|s| view::bible_labels(s).get(id).cloned()).unwrap_or_else(|| id.to_string())
}

/// The "Also show" chips (a second one once the first is set), the layout switch, and the open picker's lines.
fn show_also(op: &Operator, edit: &Rc<RefCell<Edit>>) {
    let e = edit.borrow();
    if e.bible.is_none() { return }
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let g = op.ui.global::<ItemEdit>();
    let labels = view::bible_labels(&snap);
    let shown: Vec<slint::SharedString> = e.also.iter().map(|id| labels.get(id).cloned().unwrap_or_else(|| id.clone()).into()).collect();
    g.set_also_labels(ModelRc::new(VecModel::from(shown)));
    let chosen = e.also.get(e.also_slot).map_or("", String::as_str);
    let (rows, note) = bible::picker_rows(&snap, &g.get_also_search(), chosen);
    g.set_also_rows(rows);
    g.set_also_note(note);
}

/// A row of an "Also show" picker: that slot's Bible for this passage. The main Bible is turned off there, as in
/// Settings; a Bible picked twice keeps its first place.
fn pick_also(op: &Operator, edit: &Rc<RefCell<Edit>>, id: String) {
    let known = op.state.borrow().snap.as_ref().is_some_and(|s| s.translations.contains(&id));
    let Some(main) = edit.borrow().bible.clone() else { return };
    if !known { return }
    if crate::bible::id::same_id(&id, &main) {
        op.toast(bible_tr::also_turned_off(&label_of(op, &id)), false);
    } else {
        let mut e = edit.borrow_mut();
        let slot = e.also_slot.min(e.also.len());
        if slot < e.also.len() { e.also[slot] = id } else { e.also.push(id) }
        crate::scripture::also::tidy_ids(&main, &mut e.also, &|_| false);
        e.also.truncate(MAX_ALSO);
    }
    show_also(op, edit);
}

/// `bible/lookup`'s answer, with the Also Bibles found and the ids left out.
#[derive(Deserialize)]
struct Found {
    label: String,
    verses: Vec<VerseText>,
    translation: String,
    #[serde(default)]
    also: Vec<AlsoShow>,
    #[serde(default)]
    left_out: Vec<String>,
}

/// The order strip: the chips, `+ <label>` buttons and whether it shows the written order.
fn draw_order(op: &Operator, edit: &Rc<RefCell<Edit>>) {
    let e = edit.borrow();
    let Some((song, order)) = &e.order else { return };
    let g = op.ui.global::<ItemEdit>();
    let (chips, adds, written) = arrangement::chips(&song.lyrics, order);
    g.set_order_count(chips.len() as i32);
    g.set_order_rows(arr_rows(arr_chips(chips)));
    g.set_order_written(written);
    let adds: Vec<ArrAddRow> = adds.chunks(ADDS_PER_ROW).map(|c| ArrAddRow {
        labels: ModelRc::new(VecModel::from(c.iter().map(|l| SharedString::from(l.as_str())).collect::<Vec<_>>())),
    }).collect();
    g.set_order_adds(ModelRc::new(VecModel::from(adds)));
}

/// A change to this service's order through the strip. An empty order (the written one) is spelled out first.
fn change_order(op: &Operator, edit: &Rc<RefCell<Edit>>, f: impl FnOnce(&mut Vec<String>)) {
    if op.ui.global::<ItemEdit>().get_usual_order() { return }
    if let Some((song, order)) = edit.borrow_mut().order.as_mut() {
        arrangement::ensure_real(order, &song.lyrics);
        f(order);
    }
    draw_order(op, edit);
}

/// **Make this the song's usual order**: the strip becomes the song's order (`song/save`), and this service goes back
/// to the usual order (`service/update` with `arrangement: null`).
fn make_usual(op: &Rc<Operator>, edit: &Rc<RefCell<Edit>>) {
    let Some((mut song, order)) = edit.borrow().order.clone() else { return };
    let id = edit.borrow().item.id;
    song.arrangement = order;
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    let saved = song.clone();
    op.ops().call::<serde_json::Value>("song/save", serde_json::to_value(&song).unwrap_or_default(), move |r| {
        let Some(op) = me.upgrade() else { return };
        if let Err(err) = r { return op.toast(err, true) }
        let me = Rc::downgrade(&op);
        op.ops().call::<serde_json::Value>("service/update", serde_json::json!({ "id": id, "arrangement": null }), move |r| {
            let Some(op) = me.upgrade() else { return };
            if let Err(err) = r { return op.toast(err, true) }
            if let Some(edit) = e.upgrade() {
                let mut ed = edit.borrow_mut();
                if let ItemKind::Song { arrangement, .. } = &mut ed.item.kind { *arrangement = None }
                ed.order = Some((saved.clone(), saved.arrangement.clone()));
                drop(ed);
                op.ui.global::<ItemEdit>().set_usual_order(true);
                draw_order(&op, &edit);
            }
            op.toast(song_tr::saved_as_usual(), false);
        });
    });
}

/// Redraws the background tiles (a pick, an upload, or a thumbnail that arrived).
fn draw_choices(op: &Operator, edit: &Rc<RefCell<Edit>>) {
    let (me, e) = (op.me.clone(), Rc::downgrade(edit));
    let redraw = move || if let (Some(op), Some(edit)) = (me.upgrade(), e.upgrade()) { draw_choices(&op, &edit) };
    let rows = edit.borrow().picker.as_ref().map(|p| p.rows(redraw)).unwrap_or_default();
    op.ui.global::<ItemEdit>().set_choices(ModelRc::new(VecModel::from(rows)));
}

/// The clone with what the dialog changed.
fn collect(edit: &Edit, g: &ItemEdit) -> ServiceItem {
    let mut item = edit.item.clone();
    match &mut item.kind {
        ItemKind::Scripture { per_slide, text_size, also, layout, .. } => {
            *per_slide = u8::try_from(g.get_per_slide()).unwrap_or(0);
            *text_size = if g.get_size_mode() == 1 { g.get_text_size().clamp(36, 120) as u32 } else { 0 };
            // The Also Bibles in their new order; a kept one keeps its verses, a new one is looked up on Save.
            let old = std::mem::take(also);
            *also = edit.also.iter().map(|id| old.iter().find(|a| a.translation == *id).cloned()
                .unwrap_or_else(|| AlsoShow { translation: id.clone(), verses: vec![] })).collect();
            *layout = if g.get_also_layout() == 1 { ParallelLayout::Stacked } else { ParallelLayout::SideBySide };
        }
        ItemKind::Text { title, body } => {
            *title = g.get_title().into();
            *body = g.get_body().into();
        }
        ItemKind::Media { autoplay, end, looped, .. } => match g.get_kind().as_str() {
            "video" => {
                *autoplay = g.get_autoplay();
                *end = usize::try_from(g.get_end_index()).ok().and_then(|i| ENDS.get(i).copied()).unwrap_or(*end);
            }
            "audio" => *looped = g.get_looped(),
            _ => {}
        },
        ItemKind::Song { arrangement, .. } => if let Some((_, order)) = &edit.order {
            *arrangement = (!g.get_usual_order()).then(|| order.clone());
        },
        ItemKind::Deck { .. } | ItemKind::Blank { .. } | ItemKind::Header { .. } => {}
    }
    if let Some(p) = &edit.picker { item.background = p.value.clone() }
    if let Some(t) = usize::try_from(g.get_theme_index()).ok().and_then(|i| edit.themes.get(i)) { item.theme = *t }
    item.notes = g.get_notes().into();
    item
}

/// Songs, scripture and announcements draw text, so they take a theme.
fn item_has_theme(item: &ServiceItem) -> bool { matches!(item.kind, ItemKind::Song { .. } | ItemKind::Scripture { .. } | ItemKind::Text { .. }) }

/// The Theme box (FEAT-1 §3.3): "Same as service (<its theme>)", then every theme. An item's theme that is gone shows
/// as the service's.
pub fn theme_choices(data: &crate::model::Data, item: &ServiceItem) -> Vec<(String, Option<crate::theme::ThemeId>)> {
    use crate::theme::{self, strings as ts};
    let service = theme::service_of(data, item.id).and_then(|(s, _)| s.theme).and_then(|id| theme::find(data, id)).unwrap_or_else(|| theme::church_default(data));
    let mut out = vec![(ts::same_as_service(&service.name), None)];
    out.extend(data.catalog().rows().into_iter().map(|r| (r.name, Some(r.id))));
    out
}

/// `parts.saveName`: a rename when the name changed and is not blank, sent trimmed.
fn rename(name: &Name, typed: &str) -> Option<(&'static str, serde_json::Value)> {
    let trimmed = typed.trim();
    match name {
        Name::Media { id, was } if !trimmed.is_empty() && typed != was => Some(("media/rename", serde_json::json!({ "id": id, "name": trimmed }))),
        Name::Deck { id, was } if !trimmed.is_empty() && typed != was => Some(("deck/rename", serde_json::json!({ "id": id, "title": trimmed }))),
        _ => None,
    }
}

/// Save: look the passage up again if its Bible changed, rename if needed, then `service/update`, then close and
/// toast "Saved". Any failure is an error toast with the core's message, and the dialog stays open.
fn save(op: &Rc<Operator>, edit: &Rc<RefCell<Edit>>) { save_then(op, edit, false) }

/// Save; `breaks`: then open Slide breaks for the item (Choose slide breaks…, FEAT-5 §3.3).
fn save_then(op: &Rc<Operator>, edit: &Rc<RefCell<Edit>>, breaks: bool) {
    let g = op.ui.global::<ItemEdit>();
    if g.get_advance_shown() && !save_advance(op, edit.borrow().item.id) { return }
    let (item, rename, chosen, was) = {
        let e = edit.borrow();
        (collect(&e, &g), e.name.as_ref().and_then(|n| rename(n, &g.get_name())), e.bible.clone(), e.item.clone())
    };
    let Some(lookup) = relookup(&was, &item, chosen.as_deref()) else { return rename_then_update(op, item, rename, None, breaks) };
    let me = Rc::downgrade(op);
    op.ops().call::<Found>("bible/lookup", lookup, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(found) => {
                let left = left_out_toast(&op, &found);
                rename_then_update(&op, with_passage(item, found), rename, left, breaks);
            }
            Err(e) => op.toast(e, true),
        }
    });
}

/// Saves "Advance slides automatically" (FEAT-2). False, with a toast, when the seconds are not 2–600.
fn save_advance(op: &Rc<Operator>, item: Uuid) -> bool {
    let g = op.ui.global::<ItemEdit>();
    let on = g.get_advance_on() && g.get_advance_allowed();
    let secs = match g.get_advance_secs().trim().parse::<u16>() {
        Ok(s) if crate::automation::advance::AutoAdvance::validate(s).is_ok() => s,
        _ if !on => 0,
        _ => { op.toast(crate::automation::strings::advance_range(), true); return false }
    };
    let body = serde_json::json!({ "item": item, "secs": on.then_some(secs), "loop_item": g.get_advance_loop() });
    let me = Rc::downgrade(op);
    op.ops().call::<serde_json::Value>("automation/advance", body, move |r| if let (Err(e), Some(op)) = (r, me.upgrade()) { op.toast(e, true) });
    true
}

/// The rename (if any), then `service/update` with `item`, then "Saved" and `note` (an Also Bible left out).
fn rename_then_update(op: &Rc<Operator>, item: ServiceItem, rename: Option<(&'static str, serde_json::Value)>, note: Option<String>, breaks: bool) {
    let id = item.id;
    let mut body = serde_json::to_value(&item).unwrap_or_default();
    // `service/update` keeps a key that is not sent: send the usual order as an explicit null.
    if matches!(item.kind, ItemKind::Song { arrangement: None, .. }) { body["arrangement"] = serde_json::Value::Null }
    // Sent even when it is none, so "Same as service" clears a theme (the save merges by key).
    if item_has_theme(&item) { body["theme"] = serde_json::to_value(item.theme).unwrap_or_default() }
    let update = move |op: &Rc<Operator>| {
        let me = Rc::downgrade(op);
        op.ops().call::<serde_json::Value>("service/update", body, move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(_) => {
                    if op.dialog() == Some(DialogKind::ItemEdit(id)) { op.close_dialog() }
                    op.toast(strings::saved(), false);
                    if let Some(n) = note { op.toast(n, true) }
                    if breaks { super::slide_breaks::open(&op, id) }
                }
                Err(e) => op.toast(e, true),
            }
        });
    };
    match rename {
        None => update(op),
        Some((action, body)) => {
            let me = Rc::downgrade(op);
            op.ops().call::<serde_json::Value>(action, body, move |r| {
                let Some(op) = me.upgrade() else { return };
                match r {
                    Ok(_) => update(&op),
                    Err(e) => op.toast(e, true),
                }
            });
        }
    }
}

/// The lookup Save needs first: the passage's reference in the Bible the picker chose and in every Also Bible, when
/// the main Bible changed or an Also Bible is new (`was` is the item as the dialog opened, `item` as it is saved).
fn relookup(was: &ServiceItem, item: &ServiceItem, chosen: Option<&str>) -> Option<serde_json::Value> {
    let (ItemKind::Scripture { reference, translation, .. }, ItemKind::Scripture { also, .. }, Some(chosen)) = (&was.kind, &item.kind, chosen) else { return None };
    let had = |id: &str| matches!(&was.kind, ItemKind::Scripture { also, .. } if also.iter().any(|a| a.translation == id));
    let new_also = also.iter().any(|a| !had(&a.translation));
    if crate::bible::id::same_id(chosen, translation) && !new_also { return None }
    let mut body = serde_json::json!({ "reference": reference, "translation": chosen });
    if !also.is_empty() { body["also"] = serde_json::json!(also.iter().map(|a| &a.translation).collect::<Vec<_>>()) }
    Some(body)
}

/// `item` with the passage as the lookup found it (its label, the Bible's id, the verses and the Also Bibles found).
fn with_passage(mut item: ServiceItem, found: Found) -> ServiceItem {
    if let ItemKind::Scripture { reference, translation, verses, also, .. } = &mut item.kind {
        *reference = found.label;
        *translation = found.translation;
        *verses = found.verses;
        *also = found.also;
    }
    item
}

/// "NIV isn't available right now, so only KJV · WEB was added." when the lookup left an Also Bible out.
fn left_out_toast(op: &Operator, found: &Found) -> Option<String> {
    if found.left_out.is_empty() { return None }
    let kept: Vec<String> = std::iter::once(&found.translation).chain(found.also.iter().map(|a| &a.translation)).map(|t| label_of(op, t)).collect();
    let left: Vec<String> = found.left_out.iter().map(|t| label_of(op, t)).collect();
    Some(bible_tr::also_left_out(&left.join(" · "), &kept.join(" · ")))
}

/// **Remove from service** (no confirm, as on the web), then close.
fn remove(op: &Rc<Operator>, edit: &Rc<RefCell<Edit>>) {
    let id = edit.borrow().item.id;
    let me = Rc::downgrade(op);
    op.ops().call::<serde_json::Value>("service/remove", serde_json::json!({ "id": id }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => if op.dialog() == Some(DialogKind::ItemEdit(id)) { op.close_dialog() },
            Err(e) => op.toast(e, true),
        }
    });
}

/// **Edit song** swaps the dialog for the song editor (`openSongEditor(song)`).
fn edit_song(op: &Rc<Operator>, edit: &Rc<RefCell<Edit>>) {
    let ItemKind::Song { song_id, .. } = edit.borrow().item.kind else { return };
    super::song_editor::open_song_editor(op, Some(song_id), false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        model::{Background, EndAction, MediaFile, MediaKind, Song},
        ui::operator::window::testing::{find, find_all, open as open_window, refresh},
    };
    use std::sync::Arc;

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open_window(app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op
    }

    fn push(app: &App, kind: ItemKind) -> Uuid {
        let item = ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        item.id
    }

    fn item(app: &App, id: Uuid) -> Option<ServiceItem> {
        app.data.lock().unwrap().services[0].items.iter().find(|i| i.id == id).cloned()
    }

    fn media(app: &App, name: &str, kind: MediaKind) -> Uuid {
        let m = MediaFile { id: Uuid::new_v4(), name: name.into(), kind, asset: format!("{}.bin", Uuid::new_v4()), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
        app.data.lock().unwrap().media.push(m.clone());
        m.id
    }

    fn settle() { slint::platform::update_timers_and_animations(); }

    fn click(op: &Operator, id: &str) {
        settle();
        find(op, id).unwrap_or_else(|| panic!("{id} not found")).invoke_accessible_default_action();
        settle();
    }

    fn press(op: &Operator, id: &str) {
        settle();
        find(op, id).unwrap_or_else(|| panic!("{id} not found")).mock_single_click(slint::platform::PointerEventButton::Left);
        settle();
    }

    fn type_into(op: &Operator, id: &str, text: &str) {
        settle();
        find(op, id).unwrap_or_else(|| panic!("{id} not found")).set_accessible_value(text);
        settle();
    }

    fn toasts(op: &Operator) -> Vec<(String, bool)> {
        use slint::Model;
        op.ui.get_toasts().iter().map(|t| (t.text.to_string(), t.err)).collect()
    }

    fn g(op: &Operator) -> ItemEdit<'_> { op.ui.global::<ItemEdit>() }

    #[test]
    fn edit_buttons_open_item_edit() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
        let op = opened(&app);
        op.ui.invoke_service_edit(id.to_string().into());
        assert_eq!(op.dialog(), Some(DialogKind::ItemEdit(id)));
        assert_eq!(op.ui.get_dialog_title(), "Welcome");
        assert_eq!(g(&op).get_title(), "Welcome");
        op.close_dialog();
        op.ui.invoke_service_select(id.to_string().into());
        op.ui.invoke_slides_edit();
        assert_eq!(op.dialog(), Some(DialogKind::ItemEdit(id)), "the slides header's Edit opens the selected item");
    }

    /// A song with the usual order Verse 1, Chorus, Verse 2 in the service; returns (song id, item id).
    fn song_item(app: &App, own: Option<&[&str]>) -> (Uuid, Uuid) {
        let song = Song { id: Uuid::new_v4(), title: "Grace".into(), lyrics: "Verse 1\na\nChorus\nb\nVerse 2\nc".into(),
            arrangement: vec!["Verse 1".into(), "Chorus".into(), "Verse 2".into()], ..Default::default() };
        app.data.lock().unwrap().songs.push(song.clone());
        let arrangement = own.map(|o| o.iter().map(|s| s.to_string()).collect());
        (song.id, push(app, ItemKind::Song { song_id: song.id, arrangement }))
    }

    fn own_order(app: &App, id: Uuid) -> Option<Vec<String>> {
        match item(app, id).unwrap().kind { ItemKind::Song { arrangement, .. } => arrangement, _ => panic!() }
    }

    fn usual_order(app: &App, song: Uuid) -> Vec<String> { app.data.lock().unwrap().songs.iter().find(|s| s.id == song).unwrap().arrangement.clone() }

    fn strip(op: &Operator) -> Vec<String> {
        use slint::Model;
        g(op).get_order_rows().iter().flat_map(|r| r.chips.iter().map(|c| c.short.to_string()).collect::<Vec<_>>()).collect()
    }

    fn untick_usual(op: &Operator) {
        g(op).set_usual_order(false);
        g(op).invoke_usual_order_toggled(false);
    }

    #[test]
    fn override_saved_on_item_only() {
        let app = test_app();
        let (song, id) = song_item(&app, None);
        let op = opened(&app);
        open(&op, id);
        assert!(g(&op).get_usual_order());
        assert_eq!(strip(&op), ["V1", "C", "V2"], "the usual order, dimmed");
        g(&op).invoke_order_move(0, 1);
        assert_eq!(strip(&op), ["V1", "C", "V2"], "nothing moves while the usual order is ticked");
        untick_usual(&op);
        g(&op).invoke_order_move(2, 0);
        g(&op).invoke_order_add("Chorus".into());
        assert_eq!(strip(&op), ["V2", "V1", "C", "C"]);
        press(&op, "ItemEditDialog::save");
        assert_eq!(own_order(&app, id), Some(vec!["Verse 2".into(), "Verse 1".into(), "Chorus".into(), "Chorus".into()]));
        assert_eq!(usual_order(&app, song), ["Verse 1", "Chorus", "Verse 2"], "the song keeps its usual order");
        refresh(&op);
        let rows: Vec<_> = { use slint::Model; op.ui.get_service_rows().iter().collect() };
        assert!(rows[0].custom_order, "the service row shows Custom order");
        assert!(find(&op, "ServiceColumn::custom-order").is_some());
        op.ui.invoke_service_select(id.to_string().into());
        assert!(op.ui.get_slides_custom_order(), "the slides header says so too");
    }

    #[test]
    fn use_usual_order_clears_override() {
        let app = test_app();
        let (_, id) = song_item(&app, Some(&["Chorus"]));
        let op = opened(&app);
        open(&op, id);
        assert!(!g(&op).get_usual_order());
        assert_eq!(strip(&op), ["C"]);
        g(&op).set_usual_order(true);
        g(&op).invoke_usual_order_toggled(true);
        assert_eq!(strip(&op), ["V1", "C", "V2"]);
        press(&op, "ItemEditDialog::save");
        assert_eq!(own_order(&app, id), None);
        refresh(&op);
        assert!(find(&op, "ServiceColumn::custom-order").is_none());
    }

    #[test]
    fn make_usual_order_saves_song_and_clears_override() {
        let app = test_app();
        let (song, id) = song_item(&app, Some(&["Chorus", "Verse 1"]));
        let op = opened(&app);
        open(&op, id);
        press(&op, "ItemEditDialog::make-usual");
        assert_eq!(usual_order(&app, song), ["Chorus", "Verse 1"]);
        assert_eq!(own_order(&app, id), None);
        assert!(g(&op).get_usual_order());
        assert_eq!(toasts(&op).last(), Some(&("Saved as the usual order.".to_string(), false)));
        assert_eq!(op.dialog(), Some(DialogKind::ItemEdit(id)), "the dialog stays open");
    }

    #[test]
    fn scripture_per_slide_saved() {
        let app = test_app();
        let id = push(&app, ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses: vec![], per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() });
        let op = opened(&app);
        open(&op, id);
        assert_eq!(g(&op).get_kind(), "scripture");
        assert!(g(&op).get_has_background());
        g(&op).set_per_slide(2);
        press(&op, "ItemEditDialog::save");
        let ItemKind::Scripture { per_slide, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!(per_slide, 2);
        assert_eq!(op.dialog(), None);
        assert_eq!(toasts(&op).last(), Some(&("Saved".to_string(), false)));
    }

    #[test]
    fn choose_slide_breaks_saves_then_opens_the_breaks_dialog() {
        let app = test_app();
        let id = kjv_john_3_16(&app);
        let op = opened(&app);
        open(&op, id);
        g(&op).set_per_slide(1);
        press(&op, "ItemEditDialog::choose-breaks");
        let ItemKind::Scripture { per_slide, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!(per_slide, 1, "the edits are saved first");
        assert_eq!(op.dialog(), Some(DialogKind::SlideBreaks(id)));
    }

    fn kjv_john_3_16(app: &App) -> Uuid {
        let verses = vec![VerseText::new("John 3:16", "old text")];
        push(app, ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses, per_slide: 2, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() })
    }

    #[test]
    fn scripture_bible_change_looks_the_verses_up_again_on_save() {
        let app = test_app();
        let id = kjv_john_3_16(&app);
        let op = opened(&app);
        open(&op, id);
        assert_eq!(g(&op).get_translation(), "KJV");
        assert!(!g(&op).get_translation_changed());
        press(&op, "ItemEditDialog::bible-pick");
        let rows = find_all(&op, "BiblePickerList::picker-row");
        assert!(!rows.is_empty(), "the picker is open");
        let web = rows.iter().find(|r| r.accessible_label().unwrap_or_default() == "WEB").expect("WEB listed");
        web.mock_single_click(slint::platform::PointerEventButton::Left);
        settle();
        assert_eq!(g(&op).get_translation(), "WEB");
        assert!(g(&op).get_translation_changed(), "the note says Save looks it up again");
        assert!(item(&app, id).is_some_and(|i| matches!(i.kind, ItemKind::Scripture { ref translation, .. } if translation == "KJV")), "nothing changes before Save");
        press(&op, "ItemEditDialog::save");
        let ItemKind::Scripture { reference, translation, verses, per_slide, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!((reference.as_str(), translation.as_str(), per_slide), ("John 3:16", "WEB", 2));
        assert_eq!(verses.len(), 1);
        assert_ne!(verses[0].text, "old text", "the verse is WEB's now");
        assert_eq!(op.dialog(), None);
        assert_eq!(toasts(&op).last(), Some(&("Saved".to_string(), false)));
        assert_eq!(app.data.lock().unwrap().settings.translation, "KJV", "the Bible box keeps its Bible");
    }

    #[test]
    fn scripture_failed_relookup_keeps_the_dialog_and_the_item() {
        let app = test_app();
        let id = kjv_john_3_16(&app);
        let op = opened(&app);
        open(&op, id);
        g(&op).invoke_pick_bible("NIV".into());
        assert_eq!(g(&op).get_translation(), "NIV");
        press(&op, "ItemEditDialog::save");
        assert!(toasts(&op).last().is_some_and(|t| t.1), "an error toast: {:?}", toasts(&op));
        assert_eq!(op.dialog(), Some(DialogKind::ItemEdit(id)), "the dialog stays open");
        let ItemKind::Scripture { translation, verses, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!((translation.as_str(), verses[0].text.as_str()), ("KJV", "old text"));
        g(&op).invoke_pick_bible("KJV".into());
        assert!(!g(&op).get_translation_changed(), "back to its own Bible");
        g(&op).invoke_pick_bible("nothing-like-this".into());
        assert_eq!(g(&op).get_translation(), "KJV", "an unknown Bible is not picked");
    }

    #[test]
    fn a_plain_save_does_not_look_the_passage_up() {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Scripture { reference: "John 3:16".into(), translation: "ebible:spaRV1909".into(), verses: vec![], per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() }, background: None, notes: String::new(), theme: None };
        assert_eq!(relookup(&item, &item, Some("ebible:spaRV1909")), None);
        assert_eq!(relookup(&item, &item, None), None);
        assert_eq!(relookup(&item, &item, Some("KJV")), Some(serde_json::json!({ "reference": "John 3:16", "translation": "KJV" })));
    }

    fn also_labels(op: &Operator) -> Vec<String> {
        use slint::Model;
        g(op).get_also_labels().iter().map(|l| l.to_string()).collect()
    }

    /// Opens "Also show" picker `slot` and clicks the row labelled `label`.
    fn pick_also_row(op: &Operator, slot: usize, label: &str) {
        press(op, &format!("ItemEditDialog::also-{slot}"));
        let rows = find_all(op, "BiblePickerList::picker-row");
        let row = rows.iter().find(|r| r.accessible_label().unwrap_or_default() == label).unwrap_or_else(|| panic!("{label} listed"));
        row.mock_single_click(slint::platform::PointerEventButton::Left);
        settle();
    }

    fn also_of(app: &App, id: Uuid) -> (Vec<(String, usize)>, ParallelLayout) {
        let ItemKind::Scripture { also, layout, .. } = item(app, id).unwrap().kind else { panic!() };
        (also.iter().map(|a| (a.translation.clone(), a.verses.len())).collect(), layout)
    }

    #[test]
    fn also_picker_adds_second_then_third_slot() {
        let app = test_app();
        let id = kjv_john_3_16(&app);
        let op = opened(&app);
        open(&op, id);
        settle();
        assert!(find(&op, "ItemEditDialog::also-0").is_some());
        assert!(find(&op, "ItemEditDialog::also-1").is_none(), "one empty slot to start");
        assert!(find(&op, "ItemEditDialog::also-layout").is_none(), "no layout switch with no Also Bible");
        pick_also_row(&op, 0, "WEB");
        assert_eq!(also_labels(&op), ["WEB"]);
        assert!(find(&op, "ItemEditDialog::also-1").is_some(), "a second slot once the first is set");
        assert!(find(&op, "ItemEditDialog::also-x-0").is_some());
        assert!(find(&op, "ItemEditDialog::also-layout").is_some());
        pick_also_row(&op, 1, "BSB");
        assert_eq!(also_labels(&op), ["WEB", "BSB"]);
        assert!(find(&op, "ItemEditDialog::also-2").is_none(), "three Bibles at most");
        pick_also_row(&op, 1, "WEB");
        assert_eq!(also_labels(&op), ["WEB"], "a Bible picked twice keeps its first place");
    }

    #[test]
    fn x_removes_and_compacts() {
        let app = test_app();
        let id = kjv_john_3_16(&app);
        let op = opened(&app);
        open(&op, id);
        pick_also_row(&op, 0, "WEB");
        pick_also_row(&op, 1, "BSB");
        press(&op, "ItemEditDialog::also-x-0");
        assert_eq!(also_labels(&op), ["BSB"], "BSB moves up");
        assert!(find(&op, "ItemEditDialog::also-x-1").is_none());
        assert!(find(&op, "ItemEditDialog::also-1").is_some(), "the second slot is empty again");
    }

    #[test]
    fn item_edit_also_changes_item_not_settings() {
        let app = test_app();
        let id = kjv_john_3_16(&app);
        let op = opened(&app);
        open(&op, id);
        pick_also_row(&op, 0, "WEB");
        g(&op).set_also_layout(1);
        press(&op, "ItemEditDialog::save");
        assert_eq!(also_of(&app, id), (vec![("WEB".to_string(), 1)], ParallelLayout::Stacked), "WEB looked up for this passage");
        let ItemKind::Scripture { translation, verses, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!(translation, "KJV", "the main Bible stays");
        assert!(verses[0].text.starts_with("For God so loved"), "looked up again with WEB: {}", verses[0].text);
        assert_eq!(op.dialog(), None);
        assert_eq!(toasts(&op).last(), Some(&("Saved".to_string(), false)));
        let s = app.data.lock().unwrap().settings.clone();
        assert!(s.also_show.is_empty(), "Settings keep their own Also Bibles");
        assert_eq!(s.parallel_layout, ParallelLayout::SideBySide);
    }

    #[test]
    fn removing_or_reordering_also_bibles_needs_no_lookup() {
        let app = test_app();
        let also = vec![
            AlsoShow { translation: "WEB".into(), verses: vec![VerseText::new("John 3:16", "web text")] },
            AlsoShow { translation: "BSB".into(), verses: vec![VerseText::new("John 3:16", "bsb text")] },
        ];
        let id = push(&app, ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses: vec![VerseText::new("John 3:16", "old text")], per_slide: 0, text_size: 0, also, layout: Default::default(), breaks: Default::default() });
        let op = opened(&app);
        open(&op, id);
        assert_eq!(also_labels(&op), ["WEB", "BSB"], "the item's own Also Bibles");
        let was = item(&app, id).unwrap();
        press(&op, "ItemEditDialog::also-x-0");
        let edited = collect(&OPEN.with(|o| o.borrow().clone()).unwrap().borrow(), &g(&op));
        assert_eq!(relookup(&was, &edited, Some("KJV")), None, "nothing new to look up");
        press(&op, "ItemEditDialog::save");
        let ItemKind::Scripture { also, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!((also.len(), also[0].translation.as_str(), also[0].verses[0].text.as_str()), (1, "BSB", "bsb text"));
    }

    #[test]
    fn also_same_as_main_is_turned_off() {
        let app = test_app();
        let id = kjv_john_3_16(&app);
        let op = opened(&app);
        open(&op, id);
        pick_also_row(&op, 0, "KJV");
        assert!(also_labels(&op).is_empty());
        assert_eq!(toasts(&op).last(), Some(&("KJV turned off (same as main).".to_string(), false)));
        pick_also_row(&op, 0, "WEB");
        g(&op).invoke_pick_bible("WEB".into());
        assert!(also_labels(&op).is_empty(), "the new main Bible is no Also Bible");
        assert_eq!(toasts(&op).last(), Some(&("WEB turned off (same as main).".to_string(), false)));
    }

    #[test]
    fn also_bible_that_cannot_be_looked_up_is_left_out_with_a_toast() {
        let app = test_app();
        let id = kjv_john_3_16(&app);
        let op = opened(&app);
        open(&op, id);
        pick_also_row(&op, 0, "WEB");
        g(&op).invoke_also_opening(1);
        g(&op).invoke_also_pick("NIV".into());
        assert_eq!(also_labels(&op), ["WEB", "NIV"]);
        press(&op, "ItemEditDialog::save");
        assert_eq!(also_of(&app, id).0, vec![("WEB".to_string(), 1)], "saved with the Bibles that work");
        assert!(toasts(&op).contains(&("NIV isn't available right now, so only KJV · WEB was added.".to_string(), true)), "{:?}", toasts(&op));
        assert_eq!(op.dialog(), None);
    }

    #[test]
    fn scripture_text_size_saved_and_automatic_again() {
        let app = test_app();
        let id = push(&app, ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses: vec![], per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() });
        let op = opened(&app);
        open(&op, id);
        assert_eq!(g(&op).get_size_mode(), 0, "Automatic until chosen");
        assert_eq!(g(&op).get_text_size(), 64, "the slider starts at the Settings size");
        g(&op).set_size_mode(1);
        g(&op).set_text_size(100);
        press(&op, "ItemEditDialog::save");
        assert!(matches!(item(&app, id).unwrap().kind, ItemKind::Scripture { text_size: 100, .. }));
        refresh(&op);
        open(&op, id);
        assert_eq!((g(&op).get_size_mode(), g(&op).get_text_size()), (1, 100));
        g(&op).set_size_mode(0);
        press(&op, "ItemEditDialog::save");
        assert!(matches!(item(&app, id).unwrap().kind, ItemKind::Scripture { text_size: 0, .. }));
    }

    #[test]
    fn video_end_action_saved() {
        let app = test_app();
        let m = media(&app, "Intro", MediaKind::Video);
        let id = push(&app, ItemKind::Media { media_id: m, autoplay: true, end: EndAction::Stop, looped: false });
        let op = opened(&app);
        open(&op, id);
        assert_eq!(g(&op).get_kind(), "video");
        assert!(!g(&op).get_has_background(), "video items have no background picker");
        assert!(find(&op, "ItemEditDialog::looped").is_none());
        click(&op, "ItemEditDialog::autoplay");
        assert!(!g(&op).get_autoplay(), "the checkbox turned off");
        g(&op).set_end_index(1);
        press(&op, "ItemEditDialog::save");
        let ItemKind::Media { autoplay, end, looped, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!((autoplay, end, looped), (false, EndAction::Hold, false));
        // N24: the fourth choice goes to the next slide.
        refresh(&op);
        open(&op, id);
        assert_eq!(g(&op).get_end_index(), 1);
        g(&op).set_end_index(3);
        press(&op, "ItemEditDialog::save");
        let ItemKind::Media { end, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!(end, EndAction::Next);

        // Audio: "Repeat until stopped".
        let a = media(&app, "Hymn", MediaKind::Audio);
        let id = push(&app, ItemKind::Media { media_id: a, autoplay: true, end: EndAction::Stop, looped: false });
        refresh(&op);
        open(&op, id);
        assert_eq!(g(&op).get_kind(), "audio");
        click(&op, "ItemEditDialog::looped");
        press(&op, "ItemEditDialog::save");
        let ItemKind::Media { looped, end, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!((looped, end), (true, EndAction::Stop));
    }

    #[test]
    fn text_item_title_and_body_saved() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
        let op = opened(&app);
        open(&op, id);
        type_into(&op, "ItemEditDialog::title-box", "Offering");
        type_into(&op, "ItemEditDialog::body-box", "Give\n\nThanks");
        type_into(&op, "ItemEditDialog::notes-box", "Slow down");
        press(&op, "ItemEditDialog::save");
        let it = item(&app, id).unwrap();
        let ItemKind::Text { title, body } = it.kind else { panic!() };
        assert_eq!((title.as_str(), body.as_str(), it.notes.as_str()), ("Offering", "Give\n\nThanks", "Slow down"));
    }

    #[test]
    fn background_pick_and_custom_colour_saved() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "A".into(), body: "a".into() });
        let op = opened(&app);
        // Tall enough for the grid under the tiles (the gradients add two rows).
        op.window().set_size(slint::LogicalSize::new(1520.0, 1400.0));
        open(&op, id);
        let tiles = || find_all(&op, "BackgroundPicker::choice");
        settle();
        assert_eq!(tiles().len(), 15, "default, six swatches, custom, six gradients, upload");
        assert!(tiles()[0].accessible_checked().unwrap_or(false), "Theme default is on");
        tiles()[2].invoke_accessible_default_action();
        settle();
        assert!(tiles()[2].accessible_checked().unwrap_or(false));
        assert!(!tiles()[0].accessible_checked().unwrap_or(true));
        press(&op, "ItemEditDialog::save");
        assert_eq!(item(&app, id).unwrap().background, Some(Background::Color { color: "#0b1a33".into() }));

        // The custom colour: its swatch opens the grid and the hex field.
        open(&op, id);
        settle();
        tiles()[7].invoke_accessible_default_action();
        settle();
        type_into(&op, "ColorGrid::hex", "#12AB3");
        type_into(&op, "ColorGrid::hex", "#12AB34");
        assert!(tiles().iter().all(|t| !t.accessible_checked().unwrap_or(false)), "a custom colour is no tile");
        press(&op, "ItemEditDialog::save");
        assert_eq!(item(&app, id).unwrap().background, Some(Background::Color { color: "#12ab34".into() }));
    }

    #[test]
    fn item_edit_theme_combo_saves() {
        use slint::Model;
        let app = test_app();
        let christmas = crate::theme::SavedTheme { id: Uuid::new_v4(), name: "Christmas".into(), style: Default::default(), background: None };
        app.data.lock().unwrap().themes.push(christmas.clone());
        let id = push(&app, ItemKind::Text { title: "A".into(), body: "a".into() });
        let op = opened(&app);
        open(&op, id);
        settle();
        let options: Vec<String> = g(&op).get_theme_options().iter().map(|s| s.to_string()).collect();
        assert_eq!(options, ["Same as service (Standard)", "Standard", "Lower third", "Christmas"]);
        assert_eq!(g(&op).get_theme_index(), 0);
        assert!(find(&op, "ItemEditDialog::item-theme").is_some());
        g(&op).set_theme_index(3);
        press(&op, "ItemEditDialog::save");
        assert_eq!(item(&app, id).unwrap().theme, Some(christmas.id));
        refresh(&op);
        open(&op, id);
        settle();
        assert_eq!(g(&op).get_theme_index(), 3, "opening again shows it");
        g(&op).set_theme_index(0);
        press(&op, "ItemEditDialog::save");
        assert_eq!(item(&app, id).unwrap().theme, None, "Same as service clears it");
    }

    #[test]
    fn media_items_have_no_theme_box() {
        use slint::Model;
        let app = test_app();
        let m = media(&app, "Sky", MediaKind::Image);
        let id = push(&app, ItemKind::Media { media_id: m, autoplay: true, end: EndAction::Stop, looped: false });
        let op = opened(&app);
        open(&op, id);
        settle();
        assert_eq!(g(&op).get_theme_options().row_count(), 0);
        assert!(find(&op, "ItemEditDialog::item-theme").is_none());
    }

    #[test]
    fn upload_imports_every_chosen_file() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "A".into(), body: "a".into() });
        let op = opened(&app);
        open(&op, id);
        let dir = std::env::temp_dir().join(format!("rp-upload-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut paths = Vec::new();
        for (n, px) in [("one.png", 1u8), ("two.png", 2)] {
            let p = dir.join(n);
            image::RgbaImage::from_pixel(4, 4, image::Rgba([px, 0, 0, 255])).save(&p).unwrap();
            paths.push(p);
        }
        crate::ui::operator::files::testing::will_pick(paths);
        settle();
        let tiles = find_all(&op, "BackgroundPicker::choice");
        tiles.last().unwrap().invoke_accessible_default_action();
        settle();
        let names: Vec<String> = app.data.lock().unwrap().media.iter().map(|m| m.name.clone()).collect();
        assert_eq!(names, ["one", "two"], "the core names media without the extension");
        assert_eq!(toasts(&op).last(), Some(&("Added to the media library — pick it here".to_string(), false)));
        let tiles = find_all(&op, "BackgroundPicker::choice");
        assert_eq!(tiles.len(), 17, "both pictures are pickable at once");
        assert_eq!(tiles[14].accessible_label().unwrap(), "one");
        tiles[15].invoke_accessible_default_action();
        settle();
        press(&op, "ItemEditDialog::save");
        let asset = app.data.lock().unwrap().media[1].asset.clone();
        assert_eq!(item(&app, id).unwrap().background, Some(Background::Image { asset, dim: 0 }));
        assert_eq!(op.dialog(), None);
    }

    #[test]
    fn rename_media_then_update() {
        let app = test_app();
        let m = media(&app, "clip.png", MediaKind::Image);
        let id = push(&app, ItemKind::Media { media_id: m, autoplay: true, end: EndAction::Stop, looped: false });
        let op = opened(&app);
        open(&op, id);
        assert_eq!(g(&op).get_name(), "clip.png");
        type_into(&op, "ItemEditDialog::name-box", "  Sunrise  ");
        type_into(&op, "ItemEditDialog::notes-box", "Hold here");
        press(&op, "ItemEditDialog::save");
        assert_eq!(app.data.lock().unwrap().media[0].name, "Sunrise", "renamed, trimmed");
        assert_eq!(item(&app, id).unwrap().notes, "Hold here", "then updated");
        assert_eq!(op.dialog(), None);

        // A blank name keeps the old one; the item is still saved.
        refresh(&op);
        open(&op, id);
        type_into(&op, "ItemEditDialog::name-box", "   ");
        press(&op, "ItemEditDialog::save");
        assert_eq!(app.data.lock().unwrap().media[0].name, "Sunrise");

        // Slides: "Name" renames the deck.
        let deck = crate::model::Deck { id: Uuid::new_v4(), title: "Announcements".into(), slides: vec![] };
        app.data.lock().unwrap().decks.push(deck.clone());
        let id = push(&app, ItemKind::Deck { deck_id: deck.id });
        refresh(&op);
        open(&op, id);
        type_into(&op, "ItemEditDialog::name-box", "Notices");
        press(&op, "ItemEditDialog::save");
        assert_eq!(app.data.lock().unwrap().decks[0].title, "Notices");
    }

    #[test]
    fn item_edit_keeps_typed_text_across_a_data_update() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
        let op = opened(&app);
        open(&op, id);
        type_into(&op, "ItemEditDialog::title-box", "Typed title");
        type_into(&op, "ItemEditDialog::notes-box", "Typed notes");
        // A phone adds an item meanwhile: a new read arrives.
        let other = push(&app, ItemKind::Text { title: "From phone".into(), body: "x".into() });
        refresh(&op);
        settle();
        assert_eq!(g(&op).get_title(), "Typed title");
        assert_eq!(g(&op).get_notes(), "Typed notes");
        assert_eq!(op.dialog(), Some(DialogKind::ItemEdit(id)));
        press(&op, "ItemEditDialog::save");
        let ItemKind::Text { title, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!(title, "Typed title");
        assert!(item(&app, other).is_some(), "the other change stays");
    }

    #[test]
    fn save_on_removed_item_shows_that_item_is_gone_and_stays_open() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
        let op = opened(&app);
        open(&op, id);
        app.data.lock().unwrap().services[0].items.retain(|i| i.id != id);
        press(&op, "ItemEditDialog::save");
        assert_eq!(toasts(&op).last(), Some(&("That item is gone.".to_string(), true)));
        assert_eq!(op.dialog(), Some(DialogKind::ItemEdit(id)), "the dialog stays open");
        assert!(op.ui.get_dialog_open());
    }

    #[test]
    fn remove_from_service_closes() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
        let op = opened(&app);
        open(&op, id);
        press(&op, "ItemEditDialog::remove");
        assert!(item(&app, id).is_none());
        assert_eq!(op.dialog(), None);
        assert!(toasts(&op).is_empty(), "no toast, as on the web");
    }

    #[test]
    fn cancel_closes_without_saving() {
        let app = test_app();
        let id = push(&app, ItemKind::Text { title: "Welcome".into(), body: "Hi".into() });
        let op = opened(&app);
        open(&op, id);
        type_into(&op, "ItemEditDialog::title-box", "Changed");
        press(&op, "ItemEditDialog::cancel");
        assert_eq!(op.dialog(), None);
        let ItemKind::Text { title, .. } = item(&app, id).unwrap().kind else { panic!() };
        assert_eq!(title, "Welcome");
    }

    #[test]
    fn song_item_offers_edit_song_or_says_deleted() {
        let app = test_app();
        let song = Song { id: Uuid::new_v4(), title: "Amazing Grace".into(), writers: String::new(), ccli: String::new(), copyright: String::new(), lyrics: "Verse 1\nAmazing grace".into(), arrangement: vec![], ..Default::default() };
        app.data.lock().unwrap().songs.push(song.clone());
        let id = push(&app, ItemKind::Song { song_id: song.id, arrangement: None });
        let gone = push(&app, ItemKind::Song { song_id: Uuid::new_v4(), arrangement: None });
        let op = opened(&app);
        open(&op, gone);
        settle();
        assert!(!g(&op).get_song_exists());
        assert!(find(&op, "ItemEditDialog::edit-song").is_none());
        open(&op, id);
        settle();
        assert!(g(&op).get_song_exists());
        press(&op, "ItemEditDialog::edit-song");
        assert_eq!(op.dialog(), Some(DialogKind::SongEditor { song_id: Some(song.id), add_after: false }));
    }

    #[test]
    fn not_ready_media_shows_its_status() {
        let app = test_app();
        let m = media(&app, "odd.mkv", MediaKind::Video);
        app.data.lock().unwrap().media[0].status = MediaStatus::Failed { reason: "Broken file".into() };
        let id = push(&app, ItemKind::Media { media_id: m, autoplay: true, end: EndAction::Stop, looped: false });
        let op = opened(&app);
        open(&op, id);
        assert!(g(&op).get_not_ready());
        assert_eq!((g(&op).get_status_text().as_str(), g(&op).get_status_state().as_str(), g(&op).get_status_reason().as_str()), ("Failed", "bad", "Broken file"));
    }
}
