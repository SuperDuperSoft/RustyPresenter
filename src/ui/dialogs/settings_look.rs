//! Settings → Look (`operator.js:893-917`): the theme fields, the logo, the default background and a 16:9 preview
//! of a sample lyric drawn off the UI thread 150 ms after each change. The preview's text render also says whether
//! the chosen font is installed; when it is not, a note says slides use Noto Sans (native spec §4).
use super::{background::{self, Picker}, settings::{Edit, read_form as read_all, strings}};
use crate::{
    app::{ImportPurpose, Imported},
    model::{MediaFile, Settings, Slide, SlideContent},
    render::parse_color,
    store::Store,
    ui::{SettingsDlg, block_on, operator::{file_input::Filter, files, view::OperatorSnapshot, window::Operator}, thumbs::{self, Pixels, TextRenderer}},
};
use slint::{ComponentHandle, Image, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::{Cell, RefCell}, path::PathBuf, rc::Rc, time::Duration};
use tokio::sync::oneshot;

/// How long after a change the preview is drawn again.
pub const PREVIEW_DELAY: Duration = Duration::from_millis(150);
/// The preview's size in pixels (the box is about 560 px wide in a full-width dialog).
const PREVIEW_SIZE: (u32, u32) = (640, 360);
/// The logo is shown 44 px high (88 px covers a 2× screen).
const LOGO_HEIGHT: u32 = 88;
/// The preview's timer, the Default background picker (made on the first fill), and a counter that drops stale
/// renders.
#[derive(Default)]
pub struct LookState {
    timer: Timer,
    generation: Cell<u64>,
    picker: RefCell<Option<Picker>>,
}

impl LookState {
    /// The dialog closed: no preview is drawn after this.
    pub fn stop(&self) {
        self.timer.stop();
        self.generation.set(self.generation.get() + 1);
    }
}

/// The font list: the web's 20, plus the saved font when it is not one of them (so the box shows the truth).
fn fonts(current: &str) -> Vec<String> {
    let mut list: Vec<String> = strings::FONTS.iter().map(|f| f.to_string()).collect();
    if !list.iter().any(|f| f == current) { list.push(current.to_string()) }
    list
}

pub(super) fn fill_form(g: &SettingsDlg, s: &Settings) {
    let list = fonts(&s.theme.font);
    let index = list.iter().position(|f| *f == s.theme.font).unwrap_or(0);
    g.set_fonts(ModelRc::new(VecModel::from(list.into_iter().map(Into::into).collect::<Vec<slint::SharedString>>())));
    g.set_font_index(index as i32);
    g.set_size(s.theme.size as i32);
    g.set_scripture_size(s.theme.scripture_size as i32);
    g.set_bible_per_slide(i32::from(s.bible_per_slide.min(4)));
    g.set_color(s.theme.color.as_str().into());
    g.set_color_swatch(swatch(&s.theme.color));
    g.set_position(i32::from(s.theme.position == "lower"));
    g.set_shadow(s.theme.shadow);
    g.set_uppercase(s.theme.uppercase);
    g.set_has_logo(s.logo.is_some());
    g.set_logo(Image::default());
    g.set_preview(Image::default());
    g.set_font_note("".into());
}

pub(super) fn read_form(g: &SettingsDlg, s: &mut Settings) {
    if let Some(font) = slint::Model::row_data(&g.get_fonts(), g.get_font_index().max(0) as usize) { s.theme.font = font.into() }
    s.theme.size = g.get_size().clamp(40, 150) as u32;
    s.theme.scripture_size = g.get_scripture_size().clamp(36, 120) as u32;
    s.bible_per_slide = g.get_bible_per_slide().clamp(0, 4) as u8;
    // A half-typed colour keeps the last good one.
    let color = g.get_color().trim().to_string();
    if is_hex_color(&color) { s.theme.color = color }
    s.theme.position = if g.get_position() == 1 { "lower" } else { "center" }.into();
    s.theme.shadow = g.get_shadow();
    s.theme.uppercase = g.get_uppercase();
}

fn is_hex_color(c: &str) -> bool {
    c.strip_prefix('#').is_some_and(|h| matches!(h.len(), 3 | 4 | 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit()))
}

fn swatch(c: &str) -> slint::Color {
    let [r, g, b, a] = parse_color(c);
    slint::Color::from_argb_u8(a, r, g, b)
}

/// The Default background picker (`allow_default: false`) from the newest library, drawn "on" at the edited value.
pub(super) fn fill_backgrounds(op: &Rc<Operator>, edit: &Rc<Edit>, snap: &OperatorSnapshot) {
    let value = Some(edit.settings.borrow().background.clone());
    let mut picker = edit.look.picker.borrow_mut();
    match picker.as_mut() {
        Some(p) => { p.set_media(snap.data.media.clone()); p.value = value }
        None => *picker = Some(Picker::new(op, value, snap.data.media.clone(), false)),
    }
    drop(picker);
    redraw_backgrounds(op, edit);
}

/// Redraws the background tiles (a pick, an upload, or a thumbnail that arrived).
fn redraw_backgrounds(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let (me, e) = (Rc::downgrade(op), Rc::downgrade(edit));
    let redraw = move || if let (Some(op), Some(edit)) = (me.upgrade(), e.upgrade()) { redraw_backgrounds(&op, &edit) };
    let rows = edit.look.picker.borrow().as_ref().map(|p| p.rows(redraw)).unwrap_or_default();
    op.ui.global::<SettingsDlg>().set_backgrounds(ModelRc::new(VecModel::from(rows)));
}

/// The picker's value into the edited settings, then the tiles and the preview.
fn background_changed(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let value = edit.look.picker.borrow().as_ref().and_then(|p| p.value.clone());
    if let Some(v) = value { edit.settings.borrow_mut().background = v }
    redraw_backgrounds(op, edit);
    schedule(op, edit);
}

pub(super) fn pick_background(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let picked = edit.look.picker.borrow_mut().as_mut().is_some_and(|p| p.pick(i));
    if picked { background_changed(op, edit) }
}

/// The custom background colour: a grid colour or a whole `#rrggbb` typed into its hex field.
pub(super) fn background_color(op: &Rc<Operator>, edit: &Rc<Edit>, hex: String) {
    if let Some(p) = edit.look.picker.borrow_mut().as_mut() { p.set_color(hex) }
    background_changed(op, edit);
}

/// "+ Upload…": the imported pictures and videos become pickable at once.
pub(super) fn upload_background(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    background::upload(op, move |files| {
        let Some(op) = me.upgrade() else { return };
        if let Some(p) = edit.look.picker.borrow_mut().as_mut() { p.add_media(files) }
        redraw_backgrounds(&op, &edit);
    });
}

/// Text color from the grid or its hex field: the form's field, then the preview.
pub(super) fn text_color(op: &Rc<Operator>, edit: &Rc<Edit>, hex: String) {
    op.ui.global::<SettingsDlg>().set_color(hex.into());
    changed(op, edit);
}

/// Any Look field changed: take it into the clone and draw the preview 150 ms later.
pub(super) fn changed(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let g = op.ui.global::<SettingsDlg>();
    read_all(&g, &mut edit.settings.borrow_mut());
    g.set_color_swatch(swatch(&edit.settings.borrow().theme.color));
    schedule(op, edit);
}

/// Choose logo: pick a picture, import it as a bare asset and use it (`operator.js:900`).
pub(super) fn choose_logo(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    files::pick(op, Filter::Pictures, false, move |paths| {
        let (Some(op), Some(path)) = (me.upgrade(), paths.into_iter().next()) else { return };
        logo_from(&op, &edit, path);
    });
}

/// Imports `path` as the logo.
pub(crate) fn logo_from(op: &Rc<Operator>, edit: &Rc<Edit>, path: PathBuf) {
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().import(path, ImportPurpose::Asset, |_| {}, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(Imported::Asset(asset)) => {
                edit.settings.borrow_mut().logo = Some(asset);
                op.ui.global::<SettingsDlg>().set_has_logo(true);
                render_now(&op, &edit);
            }
            Ok(_) => {}
            Err(e) => op.toast(e, true),
        }
    });
}

pub(super) fn remove_logo(op: &Rc<Operator>, edit: &Rc<Edit>) {
    edit.settings.borrow_mut().logo = None;
    let g = op.ui.global::<SettingsDlg>();
    g.set_has_logo(false);
    g.set_logo(Image::default());
    render_now(op, edit);
}

/// Leaving Look: the preview image goes, and any render still coming is dropped.
pub(super) fn drop_preview(op: &Operator, edit: &Edit) {
    edit.look.stop();
    op.ui.global::<SettingsDlg>().set_preview(Image::default());
}

fn schedule(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let (me, e) = (Rc::downgrade(op), edit.clone());
    edit.look.timer.start(TimerMode::SingleShot, PREVIEW_DELAY, move || if let Some(op) = me.upgrade() { render_now(&op, &e) });
}

/// What one preview render produced.
struct Drawn { preview: Pixels, font_missing: bool, logo: Option<Pixels> }

/// The thumbnails' composer with a text renderer of its own, shared by every preview (its cache makes the second
/// render of the same text, for `font_fallback`, free).
static RENDERER: TextRenderer = TextRenderer::new();

fn draw(settings: &Settings, media: &[MediaFile], store_root: PathBuf) -> Drawn {
    let slide = Slide::new(String::new(), SlideContent::Text { text: strings::sample_text(), footer: Some(strings::sample_footer()), reference: None, size: 0, parallel: None });
    let (key, job) = thumbs::thumb(&slide, "song", &[], None, settings, media, PREVIEW_SIZE);
    let store = Store { root: store_root };
    let preview = thumbs::compose(&job, key.size, &RENDERER, &store);
    let font_missing = match (job.text, RENDERER.get()) {
        (Some(text), Some(r)) => block_on(r.render(text)).font_fallback,
        _ => false,
    };
    let logo = settings.logo.as_ref().and_then(|a| {
        let img = image::ImageReader::open(store.asset_path(a)).ok()?.with_guessed_format().ok()?.decode().ok()?;
        let img = img.resize(LOGO_HEIGHT * 8, LOGO_HEIGHT, image::imageops::FilterType::Triangle).into_rgba8();
        Some(Pixels::clone_from_slice(img.as_raw(), img.width(), img.height()))
    });
    Drawn { preview, font_missing, logo }
}

/// Draws the preview from the current edits now: on a worker thread in the app, at once in tests (inline `Ops`).
pub(crate) fn render_now(op: &Rc<Operator>, edit: &Rc<Edit>) {
    edit.look.timer.stop();
    let generation = edit.look.generation.get() + 1;
    edit.look.generation.set(generation);
    let settings = edit.settings.borrow().clone();
    let root = op.app().store.root.clone();
    let media = op.app().data.lock().unwrap().media.clone();
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    let apply = move |d: Drawn| {
        let Some(op) = me.upgrade() else { return };
        if edit.look.generation.get() != generation { return }
        let g = op.ui.global::<SettingsDlg>();
        g.set_preview(Image::from_rgba8(d.preview));
        g.set_font_note(if d.font_missing { strings::font_missing(&edit.settings.borrow().theme.font) } else { String::new() }.into());
        g.set_logo(d.logo.map(Image::from_rgba8).unwrap_or_default());
    };
    if op.ops().is_inline() { return apply(draw(&settings, &media, root)) }
    let (tx, rx) = oneshot::channel::<Drawn>();
    let started = std::thread::Builder::new().name("settings-preview".into()).spawn(move || { let _ = tx.send(draw(&settings, &media, root)); });
    if started.is_ok() {
        let _ = slint::spawn_local(async move { if let Ok(d) = rx.await { apply(d) } });
    }
}
