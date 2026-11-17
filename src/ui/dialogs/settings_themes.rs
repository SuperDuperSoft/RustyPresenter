//! Settings → Themes (FEAT-1 spec §3.1 and §3.4; `ui/dialogs/settings-themes.slint`): the theme list with the church
//! default's star, the theme editor with a live preview, and the background library. Every button runs its action
//! at once (`theme/*`, `background/*`), so nothing here waits for Save settings. Standard's style is the Look tab's,
//! so its Edit button goes there.
use super::{background::{self, Picker, Stills}, confirm, prompt, settings::{Edit, redraw_on_next_read, snap}, settings_look, strings as dlg};
use crate::{
    app::{ImportPurpose, Imported},
    import::CANCELLED,
    model::{Background, Data, MediaFile, MediaKind, Settings, Slide, SlideContent},
    store::Store,
    theme::{self, STANDARD_ID, SavedTheme, TextAlign, ThemeId, layouts::{self, Drag, Frame, Layout, LayoutKind}, strings as ts},
    ui::{BgLibraryRow, SettingsDlg, ThemeListRow, ThemesDlg, operator::{file_input::Filter, files, view::OperatorSnapshot, window::Operator}, thumbs::Pixels},
};
use slint::{ComponentHandle, Image, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::{Cell, RefCell}, path::PathBuf, rc::{Rc, Weak}};
use tokio::sync::oneshot;
use uuid::Uuid;

/// The font every computer has: it is bundled with RustyPresenter.
pub const BUNDLED_FONT: &str = "Noto Sans";
/// The sample texts of the preview's Scripture and Announcement tabs (Lyrics uses the Look tab's).
pub const SAMPLE_SCRIPTURE: &str = "16 For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.";
pub const SAMPLE_REFERENCE: &str = "John 3:16 KJV";
pub const SAMPLE_ANNOUNCEMENT: &str = "Pancake breakfast\nSunday at 9am in the hall — everyone welcome!";
/// The gradient directions of the "Add colour or gradient" box, in the order of its buttons (↓ ↘ → ↗ ↑).
pub const ANGLES: [u16; 5] = [180, 135, 90, 45, 0];

/// What the tab keeps between callbacks.
#[derive(Default)]
pub struct ThemesState {
    /// The theme rows' ids and the library entries' ids, in list order.
    ids: RefCell<Vec<ThemeId>>,
    library: RefCell<Vec<Uuid>>,
    /// The theme being edited (a nil id for a new one) and its background picker.
    editing: RefCell<Option<SavedTheme>>,
    picker: RefCell<Option<Picker>>,
    stills: RefCell<Option<Stills>>,
    timer: Timer,
    generation: Cell<u64>,
    /// The text box being dragged on the preview: where it started and how it moves.
    drag: RefCell<Option<(Frame, Drag)>>,
    /// The colours of the "Add colour or gradient" box.
    add_from: RefCell<String>,
    add_to: RefCell<String>,
}

impl ThemesState {
    /// The dialog closed: no preview is drawn after this.
    pub fn stop(&self) {
        self.timer.stop();
        self.generation.set(self.generation.get() + 1);
    }
}

fn g(op: &Operator) -> ThemesDlg<'_> { op.ui.global::<ThemesDlg>() }

/// The line under a theme's name: Standard's or the default's note, then how many items and services use it.
pub fn row_sub(row: &theme::ThemeRow, items: usize, services: usize) -> String {
    let mut parts = Vec::new();
    if row.is_default { parts.push(ts::default_hint()) } else if row.id == STANDARD_ID { parts.push(ts::standard_hint()) }
    if items > 0 { parts.push(ts::used_by(items)) }
    if services > 0 { parts.push(ts::used_by_services(services)) }
    parts.join(" · ")
}

/// The list of themes: Standard, the built-ins, then saved themes by name.
pub fn theme_rows(data: &Data) -> Vec<(ThemeId, ThemeListRow)> {
    data.catalog().rows().into_iter().map(|r| {
        let (items, services) = theme::resolve::usage(data, r.id);
        let delete = if r.id == STANDARD_ID || (r.builtin && !r.overridden) { String::new() }
            else if r.builtin { ts::reset() } else { ts::delete() };
        (r.id, ThemeListRow { name: r.name.as_str().into(), sub: row_sub(&r, items, services).into(), is_default: r.is_default,
            standard: r.id == STANDARD_ID, delete_label: delete.into(), ready_made: r.builtin && !r.overridden && r.id != STANDARD_ID })
    }).collect()
}

/// Draws the list and the library from the newest snapshot (each tab switch and after each action).
pub(super) fn fill(op: &Rc<Operator>, edit: &Rc<Edit>, snap: &OperatorSnapshot) {
    let g = g(op);
    let rows = theme_rows(&snap.data);
    *edit.themes.ids.borrow_mut() = rows.iter().map(|(id, _)| *id).collect();
    let ready = rows.iter().filter(|(_, r)| r.ready_made).count();
    g.set_ready_made_label(crate::trn!("Show {n} ready-made theme", "Show {n} ready-made themes", ready).into());
    g.set_rows(ModelRc::new(VecModel::from(rows.into_iter().map(|(_, r)| r).collect::<Vec<_>>())));
    g.set_palette(ModelRc::new(VecModel::from(background::palette())));
    if let Some(p) = edit.themes.picker.borrow_mut().as_mut() {
        p.set_media(snap.data.media.clone());
        p.set_library(snap.data.backgrounds.clone());
    }
    draw_library(op, edit);
}

/// The background library's rows: a picture (or the colour) and what each one is.
fn draw_library(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let Some(snap) = snap(op) else { return };
    if edit.themes.stills.borrow().is_none() { *edit.themes.stills.borrow_mut() = Some(Stills::new(op)) }
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    let redraw = move || if let (Some(op), Some(edit)) = (me.upgrade(), e.upgrade()) { draw_library(&op, &edit) };
    let stills = edit.themes.stills.borrow();
    let stills = stills.as_ref().expect("made above");
    let mut ids = Vec::new();
    let rows: Vec<BgLibraryRow> = snap.data.backgrounds.iter().map(|e| {
        ids.push(e.id);
        let picture = match &e.background {
            Background::Image { asset, .. } => Some(asset.clone()),
            Background::Video { asset, .. } => e.thumb.clone().or_else(|| snap.data.media.iter().find(|m| m.asset == *asset).and_then(|m| m.thumb.clone())),
            _ => None,
        };
        let image = match (&picture, &e.background) {
            (Some(a), _) => stills.image(a, redraw.clone()),
            (None, b @ Background::Gradient { .. }) => background::gradient_image(b, (128, 72)),
            _ => Image::default(),
        };
        let swatch = match &e.background { Background::Color { color } => background::to_color(color), _ => slint::Color::default() };
        BgLibraryRow { name: e.name.as_str().into(), sub: library_sub(e).into(), swatch, has_image: picture.is_some() || matches!(e.background, Background::Gradient { .. }), image }
    }).collect();
    *edit.themes.library.borrow_mut() = ids;
    g(op).set_library(ModelRc::new(VecModel::from(rows)));
}

/// "Picture", "Video", "Colour", "Gradient", or how its conversion is going.
pub fn library_sub(e: &crate::model::BackgroundEntry) -> String {
    use crate::model::MediaStatus;
    match &e.status {
        MediaStatus::Converting { percent } => return ts::converting(*percent),
        MediaStatus::Failed { .. } | MediaStatus::Unconverted { .. } => return ts::couldnt_convert(),
        _ => {}
    }
    match e.background {
        Background::Color { .. } => ts::kind_color(),
        Background::Gradient { .. } => ts::kind_gradient(),
        Background::Image { .. } => ts::kind_picture(),
        Background::Video { .. } => ts::kind_video(),
        Background::Camera { .. } => ts::kind_camera(),
    }
}

/// Sets every `ThemesDlg` callback for this Settings dialog.
pub(super) fn connect(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let g = g(op);
    let with = |f: fn(&Rc<Operator>, &Rc<Edit>)| {
        let (me, edit): (Weak<Operator>, Rc<Edit>) = (Rc::downgrade(op), edit.clone());
        move || if let Some(op) = me.upgrade() { f(&op, &edit) }
    };
    let with_i = |f: fn(&Rc<Operator>, &Rc<Edit>, usize)| {
        let (me, edit): (Weak<Operator>, Rc<Edit>) = (Rc::downgrade(op), edit.clone());
        move |i: i32| if let Some(op) = me.upgrade() { f(&op, &edit, i.max(0) as usize) }
    };
    let with_color = |f: fn(&Rc<Operator>, &Rc<Edit>, String)| {
        let (me, edit): (Weak<Operator>, Rc<Edit>) = (Rc::downgrade(op), edit.clone());
        move |c: slint::Color| if let Some(op) = me.upgrade() { f(&op, &edit, background::to_hex(c)) }
    };
    let with_hex = |f: fn(&Rc<Operator>, &Rc<Edit>, String)| {
        let (me, edit): (Weak<Operator>, Rc<Edit>) = (Rc::downgrade(op), edit.clone());
        move |t: slint::SharedString| if let (Some(op), Some(hex)) = (me.upgrade(), background::parse_hex(&t)) { f(&op, &edit, hex) }
    };
    g.set_editing(false);
    g.set_adding(false);
    g.on_make_default(with_i(make_default));
    g.on_edit(with_i(edit_row));
    g.on_duplicate(with_i(duplicate));
    g.on_delete(with_i(delete));
    g.on_new_theme(with(new_theme));
    g.on_changed(with(changed));
    g.on_text_color(with_color(|op, e, c| set_color(op, e, Part::Text, c)));
    g.on_text_hex(with_hex(|op, e, c| set_color(op, e, Part::Text, c)));
    g.on_outline_color(with_color(|op, e, c| set_color(op, e, Part::Outline, c)));
    g.on_outline_hex(with_hex(|op, e, c| set_color(op, e, Part::Outline, c)));
    g.on_band_color(with_color(|op, e, c| set_color(op, e, Part::Band, c)));
    g.on_band_hex(with_hex(|op, e, c| set_color(op, e, Part::Band, c)));
    g.on_pick_background(with_i(pick_background));
    g.on_background_color(with_color(background_color));
    g.on_background_hex(with_hex(background_color));
    g.on_upload_background(with(upload_background));
    g.on_layout_picked(with(|op, e| { show_layout(op, e); render_now(op, e) }));
    g.on_layout_edited(with(layout_edited));
    g.on_reset_layout(with(reset_layout));
    let (me, e) = (Rc::downgrade(op), edit.clone());
    g.on_drag_start(move |x, y| if let Some(op) = me.upgrade() { drag_start(&op, &e, x, y) });
    let (me, e) = (Rc::downgrade(op), edit.clone());
    g.on_drag_to(move |dx, dy| if let Some(op) = me.upgrade() { drag_to(&op, &e, dx, dy) });
    g.on_drag_done(with(drag_done));
    g.on_cancel_edit(with(close_editor));
    g.on_save_theme(with(save));
    g.on_add_pictures(with(add_pictures));
    g.on_rename_background(with_i(rename_background));
    g.on_remove_background(with_i(remove_background));
    g.on_add_from_color(with_color(|op, e, c| add_color(op, e, true, c)));
    g.on_add_from_hex(with_hex(|op, e, c| add_color(op, e, true, c)));
    g.on_add_to_color(with_color(|op, e, c| add_color(op, e, false, c)));
    g.on_add_to_hex(with_hex(|op, e, c| add_color(op, e, false, c)));
    g.on_add_changed(with(add_changed));
    g.on_add_save(with(add_save));
}

fn theme_at(op: &Operator, edit: &Edit, i: usize) -> Option<SavedTheme> {
    let id = *edit.themes.ids.borrow().get(i)?;
    theme::find(&snap(op)?.data, id)
}

/// The list again once the core has the change (the web's `await refresh(); redraw()`).
fn refill_later(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let edit = edit.clone();
    op.on_next_read(move |op| if let Some(s) = snap(op) { fill(op, &edit, &s) });
}

/// A plain action whose success is `done` (a toast) and a list redraw; a refusal is an error toast.
fn run(op: &Rc<Operator>, edit: &Rc<Edit>, action: &'static str, body: serde_json::Value, done: impl FnOnce(&Rc<Operator>, serde_json::Value) + 'static) {
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>(action, body, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) => {
                redraw_on_next_read(&op, &edit);
                done(&op, v);
            }
            Err(e) => op.toast(e, true),
        }
    });
}

/// The star: asks first, then makes it the church default.
fn make_default(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(t) = theme_at(op, edit, i) else { return };
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    confirm(op, &ts::make_default_question(&t.name), &ts::make_default(), &dlg::cancel(), false, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        let name = t.name.clone();
        run(&op, &edit, "theme/make-default", serde_json::json!({ "id": t.id }), move |op, _| op.toast(ts::now_default(&name), false));
    });
}

/// Edit: Standard is the Look tab; any other theme opens the editor.
fn edit_row(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(t) = theme_at(op, edit, i) else { return };
    if t.id == STANDARD_ID {
        let s = op.ui.global::<SettingsDlg>();
        let look = super::settings::tab_index(super::SettingsTab::Look);
        s.set_tab(look);
        s.invoke_tab_changed(look);
        return;
    }
    open_editor(op, edit, t);
}

/// A name no other theme has: `base`, else "<base> copy", "<base> copy 2"…
fn free_name(data: &Data, base: &str) -> String {
    let cat = data.catalog();
    let taken = |n: &str| cat.name_taken(n, Uuid::nil());
    if !taken(base) { return base.to_string() }
    theme::copy_name(base, taken)
}

fn duplicate(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let (Some(t), Some(s)) = (theme_at(op, edit, i), snap(op)) else { return };
    let name = theme::copy_name(&t.name, |n| s.data.catalog().name_taken(n, Uuid::nil()));
    open_editor(op, edit, SavedTheme { id: Uuid::nil(), name, ..t });
}

/// New theme: a copy of the church default, named "New theme".
fn new_theme(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let Some(s) = snap(op) else { return };
    let base = theme::church_default(&s.data);
    open_editor(op, edit, SavedTheme { id: Uuid::nil(), name: free_name(&s.data, &ts::new_theme_name()), ..base });
}

/// Delete asks with how many items and services use it; Reset puts a built-in back as it came.
fn delete(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(t) = theme_at(op, edit, i) else { return };
    let builtin = theme::builtin().iter().any(|b| b.id == t.id);
    if builtin { return ask_delete(op, edit, t.id, true, ts::reset_question(&t.name)) }
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>("theme/usage", serde_json::json!({ "id": t.id }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) => {
                let n = |k: &str| v.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
                let is_default = v.get("is_default").and_then(serde_json::Value::as_bool).unwrap_or(false);
                ask_delete(&op, &edit, t.id, false, ts::delete_theme_question(&t.name, n("items"), n("services"), is_default));
            }
            Err(e) => op.toast(e, true),
        }
    });
}

fn ask_delete(op: &Rc<Operator>, edit: &Rc<Edit>, id: ThemeId, builtin: bool, question: String) {
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    let label = if builtin { ts::reset() } else { ts::delete() };
    confirm(op, &question, &label, &dlg::cancel(), true, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        run(&op, &edit, "theme/delete", serde_json::json!({ "id": id }), move |op, _| op.toast(if builtin { ts::theme_reset() } else { ts::theme_deleted() }, false));
    });
}

// ---- The editor ----

/// The editor's fonts: Noto Sans (bundled), then the web's 20, then the theme's own when it is none of them.
pub fn editor_fonts(current: &str) -> Vec<String> {
    let mut list = vec![BUNDLED_FONT.to_string()];
    list.extend(settings_look::fonts(current).into_iter().filter(|f| f != BUNDLED_FONT));
    list
}

/// Weight as the Regular / Semibold / Bold buttons show it.
pub fn weight_index(w: u16) -> i32 { match w { w if w < 500 => 0, w if w < 650 => 1, _ => 2 } }
/// Outline width as the Off / Thin / Thick buttons show it.
pub fn outline_index(o: u8) -> i32 { match o { 0 => 0, 1..=4 => 1, _ => 2 } }

fn open_editor(op: &Rc<Operator>, edit: &Rc<Edit>, t: SavedTheme) {
    let Some(s) = snap(op) else { return };
    let g = g(op);
    let st = &t.style;
    let fonts = editor_fonts(&st.font);
    g.set_name(t.name.as_str().into());
    g.set_font_index(fonts.iter().position(|f| *f == st.font).unwrap_or(0) as i32);
    g.set_fonts(ModelRc::new(VecModel::from(fonts.into_iter().map(Into::into).collect::<Vec<slint::SharedString>>())));
    g.set_weight(weight_index(st.weight));
    g.set_size(st.size as i32);
    g.set_scripture_size(st.scripture_size as i32);
    g.set_align(match st.align { TextAlign::Left => 0, TextAlign::Center => 1, TextAlign::Right => 2 });
    g.set_position(match st.position.as_str() { "top" => 0, "lower" => 2, _ => 1 });
    g.set_uppercase(st.uppercase);
    g.set_shadow(st.shadow);
    g.set_outline(outline_index(st.outline));
    g.set_band(st.band);
    g.set_band_clear(i32::from(100 - st.band_opacity.min(100)).min(90));
    g.set_darken(i32::from(t.background.as_ref().map_or(0, Background::dim)));
    g.set_preview(Image::default());
    g.set_font_note("".into());
    g.set_layout_names(ModelRc::new(VecModel::from(LayoutKind::ALL.iter().map(|k| k.label().into()).collect::<Vec<slint::SharedString>>())));
    g.set_layout_index(LayoutKind::Lyrics.index() as i32);
    g.set_guides_x(ModelRc::default());
    g.set_guides_y(ModelRc::default());
    *edit.themes.drag.borrow_mut() = None;
    *edit.themes.picker.borrow_mut() = Some(Picker::with_default(op, t.background.clone(), s.data.media.clone(), Some(ts::standard_background())));
    *edit.themes.editing.borrow_mut() = Some(t);
    show_swatches(op, edit);
    show_layout(op, edit);
    draw_backgrounds(op, edit);
    g.set_editing(true);
    render_now(op, edit);
}

/// Back to the list (Cancel, a tab switch); nothing is saved.
pub(super) fn close_editor(op: &Rc<Operator>, edit: &Rc<Edit>) {
    edit.themes.stop();
    edit.themes.editing.borrow_mut().take();
    edit.themes.picker.borrow_mut().take();
    let g = g(op);
    g.set_editing(false);
    g.set_preview(Image::default());
    g.set_backgrounds(ModelRc::default());
}

/// The edited theme with what the form says now.
fn read_editor(op: &Operator, edit: &Edit) -> Option<SavedTheme> {
    let g = g(op);
    let mut t = edit.themes.editing.borrow().clone()?;
    t.name = g.get_name().to_string();
    let s = &mut t.style;
    if let Some(font) = slint::Model::row_data(&g.get_fonts(), g.get_font_index().max(0) as usize) { s.font = font.into() }
    s.weight = [400, 600, 700][g.get_weight().clamp(0, 2) as usize];
    s.size = g.get_size().clamp(40, 150) as u32;
    s.scripture_size = g.get_scripture_size().clamp(36, 120) as u32;
    s.align = [TextAlign::Left, TextAlign::Center, TextAlign::Right][g.get_align().clamp(0, 2) as usize];
    s.position = ["top", "center", "lower"][g.get_position().clamp(0, 2) as usize].into();
    s.uppercase = g.get_uppercase();
    s.shadow = g.get_shadow();
    s.outline = [0, 3, 6][g.get_outline().clamp(0, 2) as usize];
    s.band = g.get_band();
    s.band_opacity = (100 - g.get_band_clear().clamp(0, 90)) as u8;
    let dim = g.get_darken().clamp(0, 80) as u8;
    t.background = edit.themes.picker.borrow().as_ref().and_then(|p| p.value.clone()).map(|b| b.with_dim(dim));
    Some(t)
}

fn changed(op: &Rc<Operator>, edit: &Rc<Edit>) {
    if let Some(t) = read_editor(op, edit) { *edit.themes.editing.borrow_mut() = Some(t) }
    show_swatches(op, edit);
    g(op).set_darken_shown(background::darken_shown(&edit.themes.editing.borrow().as_ref().and_then(|t| t.background.clone())));
    schedule(op, edit);
}

#[derive(Clone, Copy)]
enum Part { Text, Outline, Band }

fn set_color(op: &Rc<Operator>, edit: &Rc<Edit>, part: Part, hex: String) {
    if let Some(t) = edit.themes.editing.borrow_mut().as_mut() {
        let slot = match part { Part::Text => &mut t.style.color, Part::Outline => &mut t.style.outline_color, Part::Band => &mut t.style.band_color };
        *slot = hex;
    }
    show_swatches(op, edit);
    changed(op, edit);
}

fn show_swatches(op: &Operator, edit: &Edit) {
    let Some(t) = edit.themes.editing.borrow().clone() else { return };
    let g = g(op);
    g.set_color_swatch(settings_look::swatch(&t.style.color));
    g.set_outline_swatch(settings_look::swatch(&t.style.outline_color));
    g.set_band_swatch(settings_look::swatch(&t.style.band_color));
    let font = &t.style.font;
    g.set_font_hint(if font == BUNDLED_FONT { ts::bundled_font_note() } else { String::new() }.into());
}

fn draw_backgrounds(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    let redraw = move || if let (Some(op), Some(edit)) = (me.upgrade(), e.upgrade()) { draw_backgrounds(&op, &edit) };
    let rows = edit.themes.picker.borrow().as_ref().map(|p| p.rows(redraw)).unwrap_or_default();
    let g = g(op);
    g.set_backgrounds(ModelRc::new(VecModel::from(rows)));
    g.set_darken_shown(background::darken_shown(&edit.themes.picker.borrow().as_ref().and_then(|p| p.value.clone())));
}

fn background_changed(op: &Rc<Operator>, edit: &Rc<Edit>) {
    draw_backgrounds(op, edit);
    changed(op, edit);
}

fn pick_background(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let picked = edit.themes.picker.borrow_mut().as_mut().is_some_and(|p| p.pick(i));
    if picked { background_changed(op, edit) }
}

fn background_color(op: &Rc<Operator>, edit: &Rc<Edit>, hex: String) {
    if let Some(p) = edit.themes.picker.borrow_mut().as_mut() { p.set_color(hex) }
    background_changed(op, edit);
}

fn upload_background(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    background::upload(op, move |files| {
        let Some(op) = me.upgrade() else { return };
        if let Some(p) = edit.themes.picker.borrow_mut().as_mut() { p.add_media(files) }
        draw_backgrounds(&op, &edit);
    });
}

// ---- N25: the theme's layouts ----

fn picked_layout(op: &Operator) -> LayoutKind { LayoutKind::from_index(g(op).get_layout_index()) }

/// The box a layout's text sits in: its own, else the screen inside its margins.
fn frame_of(l: &Layout) -> Frame { l.frame.unwrap_or(layouts::SAFE) }

/// The picked layout's controls and box, from the edited theme.
fn show_layout(op: &Operator, edit: &Edit) {
    let Some(t) = edit.themes.editing.borrow().clone() else { return };
    let kind = picked_layout(op);
    let l = t.style.layouts.get(kind);
    let drawn = layouts::apply(&t.style, kind);
    let g = g(op);
    g.set_layout_size(if l.size == 0 { 100 } else { i32::from(l.size) });
    g.set_layout_place(match drawn.position.as_str() { "top" => 0, "lower" => 2, _ => 1 });
    g.set_layout_align(match drawn.align { TextAlign::Left => 0, TextAlign::Center => 1, TextAlign::Right => 2 });
    g.set_layout_changed(t.style.layouts.changed(kind));
    show_frame(op, frame_of(&l));
}

fn show_frame(op: &Operator, f: Frame) {
    let g = g(op);
    g.set_frame_x(i32::from(f.x));
    g.set_frame_y(i32::from(f.y));
    g.set_frame_w(i32::from(f.w));
    g.set_frame_h(i32::from(f.h));
}

/// Changes the picked layout of the edited theme with `f`.
fn change_layout(op: &Operator, edit: &Edit, f: impl FnOnce(&mut Layout)) {
    let kind = picked_layout(op);
    let mut editing = edit.themes.editing.borrow_mut();
    let Some(t) = editing.as_mut() else { return };
    let mut l = t.style.layouts.get(kind);
    f(&mut l);
    t.style.layouts.set(kind, l);
    g(op).set_layout_changed(t.style.layouts.changed(kind));
}

/// The size, place or alignment of the picked layout changed.
fn layout_edited(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let g = g(op);
    let size = g.get_layout_size().clamp(40, 160) as u16;
    let place = ["top", "center", "lower"][g.get_layout_place().clamp(0, 2) as usize];
    let align = [TextAlign::Left, TextAlign::Center, TextAlign::Right][g.get_layout_align().clamp(0, 2) as usize];
    change_layout(op, edit, |l| {
        l.size = if size == 100 { 0 } else { size };
        l.position = place.into();
        l.align = Some(align);
    });
    schedule(op, edit);
}

/// Reset this layout: back to how it came.
fn reset_layout(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let kind = picked_layout(op);
    if let Some(t) = edit.themes.editing.borrow_mut().as_mut() { t.style.layouts.0.remove(&kind); }
    show_layout(op, edit);
    render_now(op, edit);
}

/// How far (in thousandths) from the box's bottom-right corner a press still grabs the corner.
const CORNER_GRAB: f32 = 40.0;

/// The pointer went down on the preview at (`x`, `y`) thousandths: on the corner it resizes the box, inside it moves it.
fn drag_start(op: &Rc<Operator>, edit: &Rc<Edit>, x: f32, y: f32) {
    let Some(t) = edit.themes.editing.borrow().clone() else { return };
    let f = frame_of(&t.style.layouts.get(picked_layout(op)));
    let (right, bottom) = (f32::from(f.x + f.w), f32::from(f.y + f.h));
    let near_corner = (x - right).abs() <= CORNER_GRAB && (y - bottom).abs() <= CORNER_GRAB;
    let inside = x >= f32::from(f.x) && x <= right && y >= f32::from(f.y) && y <= bottom;
    *edit.themes.drag.borrow_mut() = if near_corner { Some((f, Drag::Resize)) } else if inside { Some((f, Drag::Move)) } else { None };
}

/// The pointer moved by (`dx`, `dy`) thousandths since it went down.
fn drag_to(op: &Rc<Operator>, edit: &Rc<Edit>, dx: f32, dy: f32) {
    let Some((start, mode)) = *edit.themes.drag.borrow() else { return };
    let snapped = layouts::drag(start, mode, dx.round() as i32, dy.round() as i32, g(op).get_snap());
    let ints = |v: &[u16]| ModelRc::new(VecModel::from(v.iter().map(|n| i32::from(*n)).collect::<Vec<i32>>()));
    let g = g(op);
    g.set_guides_x(ints(&snapped.guides_x));
    g.set_guides_y(ints(&snapped.guides_y));
    show_frame(op, snapped.frame);
    change_layout(op, edit, |l| l.frame = Some(snapped.frame));
    schedule(op, edit);
}

fn drag_done(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let g = g(op);
    g.set_guides_x(ModelRc::default());
    g.set_guides_y(ModelRc::default());
    if edit.themes.drag.borrow_mut().take().is_some() { render_now(op, edit) }
}

/// Save theme: `theme/save`, then back to the list with "Theme saved".
fn save(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let Some(t) = read_editor(op, edit) else { return };
    let (me, e) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>("theme/save", serde_json::to_value(&t).unwrap_or_default(), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) => {
                close_editor(&op, &e);
                refill_later(&op, &e);
                let views: Vec<String> = v.get("screen_views").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                let mut toast = ts::theme_saved();
                if !views.is_empty() { toast = format!("{toast} · {}", ts::used_by_screen_views(&views)) }
                op.toast(toast, false);
            }
            Err(err) => op.toast(err, true),
        }
    });
}

fn schedule(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let (me, e) = (Rc::downgrade(op), edit.clone());
    edit.themes.timer.start(TimerMode::SingleShot, settings_look::PREVIEW_DELAY, move || if let Some(op) = me.upgrade() { render_now(&op, &e) });
}

/// The preview's sample for each layout: lyrics, a Bible verse, an announcement, a title…
pub fn sample(kind: LayoutKind) -> (Slide, &'static str) {
    let text = |text: &str, footer: Option<&str>, reference: Option<&str>| SlideContent::Text {
        text: text.into(), footer: footer.map(Into::into), reference: reference.map(Into::into), size: 0, parallel: None,
    };
    let lyrics = || text(&super::settings::strings::sample_text(), Some(&super::settings::strings::sample_footer()), None);
    let (content, kind) = match kind {
        LayoutKind::Scripture => (text(SAMPLE_SCRIPTURE, None, Some(SAMPLE_REFERENCE)), "scripture"),
        LayoutKind::Text => (text(SAMPLE_ANNOUNCEMENT, None, None), "text"),
        LayoutKind::Lyrics | LayoutKind::LowerThirdLyrics => (lyrics(), "song"),
        LayoutKind::Title => (text(&crate::tr!("Welcome"), None, None), "text"),
        LayoutKind::TitleSubtitle => (text(&crate::tr!("Easter Sunday\nServices at 9 and 11 am"), None, None), "text"),
        LayoutKind::Quote => (text(&crate::tr!("“Be still, and know that I am God.”\nPsalm 46:10"), None, None), "text"),
        LayoutKind::MainPoint => (text(&crate::tr!("God's grace is enough"), None, None), "text"),
        LayoutKind::Picture => (text(&crate::tr!("Youth camp in the mountains"), None, None), "text"),
        LayoutKind::Points => (text(&crate::tr!("Pray every day\nRead the Bible\nServe your neighbour"), None, None), "text"),
    };
    (Slide::new(String::new(), content), kind)
}

/// The settings a preview draws with: the theme's style in layout `kind`, and its background or else Standard's (as
/// edited on Look).
fn preview_settings(base: &Settings, t: &SavedTheme, kind: LayoutKind) -> Settings {
    let mut s = base.clone();
    s.theme = layouts::apply(&t.style, kind);
    if let Some(b) = &t.background { s.background = b.clone() }
    s
}

fn draw(settings: &Settings, layout: LayoutKind, media: &[MediaFile], root: PathBuf) -> (Pixels, bool) {
    let (slide, kind) = sample(layout);
    settings_look::draw_sample(settings, &slide, kind, media, &Store { root })
}

/// Draws the preview now: on a worker thread in the app, at once in tests (inline `Ops`).
pub(crate) fn render_now(op: &Rc<Operator>, edit: &Rc<Edit>) {
    edit.themes.timer.stop();
    let Some(t) = edit.themes.editing.borrow().clone() else { return };
    let generation = edit.themes.generation.get() + 1;
    edit.themes.generation.set(generation);
    let tab = picked_layout(op);
    let settings = preview_settings(&edit.settings.borrow(), &t, tab);
    let root = op.app().store.root.clone();
    let media = op.app().data.lock().unwrap().media.clone();
    let font = t.style.font.clone();
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    let apply = move |(px, missing): (Pixels, bool)| {
        let Some(op) = me.upgrade() else { return };
        if edit.themes.generation.get() != generation { return }
        let g = g(&op);
        g.set_preview(Image::from_rgba8(px));
        g.set_font_note(if missing { ts::font_missing(&font) } else { String::new() }.into());
    };
    if op.ops().is_inline() { return apply(draw(&settings, tab, &media, root)) }
    let (tx, rx) = oneshot::channel::<(Pixels, bool)>();
    let started = std::thread::Builder::new().name("theme-preview".into()).spawn(move || { let _ = tx.send(draw(&settings, tab, &media, root)); });
    if started.is_ok() {
        let _ = slint::spawn_local(async move { if let Ok(d) = rx.await { apply(d) } });
    }
}

// ---- The background library ----

/// "Add picture or video…": each file goes into the media library, then into the background library.
fn add_pictures(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    files::pick(op, Filter::PicturesAndVideos, true, move |paths| {
        if let Some(op) = me.upgrade() && !paths.is_empty() { add_files(&op, &edit, paths) }
    });
}

/// Imports `paths` one by one as backgrounds; the first failure stops the rest with the core's message.
pub(crate) fn add_files(op: &Rc<Operator>, edit: &Rc<Edit>, paths: Vec<PathBuf>) {
    fn next(op: &Rc<Operator>, edit: Rc<Edit>, mut rest: std::vec::IntoIter<PathBuf>, added: usize) {
        let Some(path) = rest.next() else {
            if added > 0 {
                refill_later(op, &edit);
                op.toast(ts::added_to_backgrounds(), false);
            }
            return;
        };
        let me = Rc::downgrade(op);
        op.ops().import(path, ImportPurpose::Media, |_| {}, move |r| {
            let Some(op) = me.upgrade() else { return };
            let m = match r {
                Ok(Imported::Media(m)) => m,
                Ok(_) => return next(&op, edit, rest, added),
                Err(e) if e == CANCELLED => return,
                Err(e) => return op.toast(e, true),
            };
            let background = match m.kind {
                MediaKind::Image => Background::Image { asset: m.asset.clone(), dim: 0 },
                MediaKind::Video => Background::Video { asset: m.asset.clone(), dim: 0 },
                MediaKind::Audio => return op.toast(ts::background_kind(), true),
            };
            let me = Rc::downgrade(&op);
            op.ops().call::<serde_json::Value>("background/add", serde_json::json!({ "name": m.name, "background": background }), move |r| {
                let Some(op) = me.upgrade() else { return };
                match r {
                    Ok(_) => next(&op, edit, rest, added + 1),
                    Err(e) => op.toast(e, true),
                }
            });
        });
    }
    next(op, edit.clone(), paths.into_iter(), 0);
}

fn library_id(edit: &Edit, i: usize) -> Option<Uuid> { edit.themes.library.borrow().get(i).copied() }

fn library_name(op: &Operator, id: Uuid) -> Option<String> {
    snap(op)?.data.backgrounds.iter().find(|e| e.id == id).map(|e| e.name.clone())
}

fn rename_background(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(id) = library_id(edit, i) else { return };
    let Some(name) = library_name(op, id) else { return };
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    prompt(op, &ts::rename_background(), &name, move |typed| {
        let (Some(op), Some(typed)) = (me.upgrade(), typed) else { return };
        run(&op, &edit, "background/rename", serde_json::json!({ "id": id, "name": typed }), |_, _| {});
    });
}

fn remove_background(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(id) = library_id(edit, i) else { return };
    let Some(name) = library_name(op, id) else { return };
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    confirm(op, &ts::remove_background_question(&name), &ts::remove(), &dlg::cancel(), true, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        run(&op, &edit, "background/remove", serde_json::json!({ "id": id }), |_, _| {});
    });
}

/// The colour or gradient the "Add" box would add.
fn add_value(op: &Operator, edit: &Edit) -> Background {
    let g = g(op);
    let from = edit.themes.add_from.borrow().clone();
    if g.get_add_kind() == 0 { return Background::Color { color: from } }
    let angle = ANGLES[g.get_add_angle().clamp(0, 4) as usize];
    Background::Gradient { from, to: edit.themes.add_to.borrow().clone(), angle, dim: 0 }
}

fn add_color(op: &Rc<Operator>, edit: &Rc<Edit>, from: bool, hex: String) {
    let slot = if from { &edit.themes.add_from } else { &edit.themes.add_to };
    *slot.borrow_mut() = hex;
    add_changed(op, edit);
}

/// The Add box's swatches and its preview.
fn add_changed(op: &Rc<Operator>, edit: &Rc<Edit>) {
    if edit.themes.add_from.borrow().is_empty() { *edit.themes.add_from.borrow_mut() = "#0b1a33".into() }
    if edit.themes.add_to.borrow().is_empty() { *edit.themes.add_to.borrow_mut() = "#000000".into() }
    let g = g(op);
    g.set_add_from(background::to_color(&edit.themes.add_from.borrow()));
    g.set_add_to(background::to_color(&edit.themes.add_to.borrow()));
    g.set_add_preview(background::gradient_image(&add_value(op, edit), (160, 90)));
}

fn add_save(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let Some(s) = snap(op) else { return };
    let name = g(op).get_add_name().trim().to_string();
    let name = if name.is_empty() { ts::numbered_background(s.data.backgrounds.len() + 1) } else { name };
    let body = serde_json::json!({ "name": name, "background": add_value(op, edit) });
    run(op, edit, "background/add", body, |op, _| {
        let g = g(op);
        g.set_adding(false);
        g.set_add_name("".into());
        op.toast(ts::added_to_backgrounds(), false);
    });
}

#[cfg(test)]
#[path = "settings_themes_tests.rs"]
mod tests;
