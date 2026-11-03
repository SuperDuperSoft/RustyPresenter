//! Settings → Look (`operator.js:893-917`): the theme fields, the logo, the default background and a 16:9 preview
//! of a sample lyric drawn off the UI thread 150 ms after each change. The preview's text render also says whether
//! the chosen font is installed; when it is not, a note says slides use Noto Sans (native spec §4).
use super::settings::{Edit, pick, read_form as read_all, strings};
use crate::{
    app::{ImportPurpose, Imported},
    model::{Background, MediaKind, MediaStatus, Settings, Slide, SlideContent},
    render::parse_color,
    store::Store,
    ui::{SettingsBgRow, SettingsDlg, block_on, operator::{view::OperatorSnapshot, window::Operator}, thumbs::{self, Pixels, TextRenderer}},
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
/// The six swatches of the background picker (`operator.js:701`).
const SWATCHES: [&str; 6] = ["#000000", "#0b1a33", "#1d2b1f", "#2b1530", "#3a2412", "#ffffff"];

/// The preview's timer, the choices the background grid shows, and a counter that drops stale renders.
#[derive(Default)]
pub struct LookState {
    timer: Timer,
    generation: Cell<u64>,
    backgrounds: RefCell<Vec<Background>>,
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

/// Stand-in until Task 18 merges `background::choices`: the six swatches, then every ready picture and video.
pub(super) fn fill_backgrounds(g: &SettingsDlg, edit: &Edit, snap: &OperatorSnapshot) {
    let mut values: Vec<Background> = SWATCHES.iter().map(|c| Background::Color { color: c.to_string() }).collect();
    let mut rows: Vec<SettingsBgRow> = SWATCHES.iter().map(|c| SettingsBgRow { swatch: swatch(c), ..Default::default() }).collect();
    for m in snap.data.media.iter().filter(|m| m.status == MediaStatus::Ready) {
        let (value, label) = match m.kind {
            MediaKind::Image => (Background::Image { asset: m.asset.clone() }, m.name.clone()),
            MediaKind::Video => (Background::Video { asset: m.asset.clone() }, format!("▶ {}", m.name)),
            _ => continue,
        };
        values.push(value);
        rows.push(SettingsBgRow { label: label.into(), tooltip: m.name.as_str().into(), swatch: slint::Color::from_argb_u8(255, 0, 0, 0), ..Default::default() });
    }
    let current = &edit.settings.borrow().background;
    for (row, value) in rows.iter_mut().zip(&values) { row.on = value == current }
    g.set_backgrounds(ModelRc::new(VecModel::from(rows)));
    *edit.look.backgrounds.borrow_mut() = values;
}

/// Any Look field changed: take it into the clone and draw the preview 150 ms later.
pub(super) fn changed(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let g = op.ui.global::<SettingsDlg>();
    read_all(&g, &mut edit.settings.borrow_mut());
    g.set_color_swatch(swatch(&edit.settings.borrow().theme.color));
    schedule(op, edit);
}

pub(super) fn pick_background(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(value) = edit.look.backgrounds.borrow().get(i).cloned() else { return };
    edit.settings.borrow_mut().background = value;
    if let Some(s) = super::settings::snap(op) { fill_backgrounds(&op.ui.global::<SettingsDlg>(), edit, &s) }
    schedule(op, edit);
}

/// Choose logo: pick a picture, import it as a bare asset and use it (`operator.js:900`).
pub(super) fn choose_logo(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    pick(op, false, move |paths| {
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

fn draw(settings: &Settings, store_root: PathBuf) -> Drawn {
    let slide = Slide {
        label: String::new(),
        content: SlideContent::Text { text: strings::SAMPLE_TEXT.into(), footer: Some(strings::SAMPLE_FOOTER.into()), reference: None },
        notes: String::new(),
    };
    let (key, job) = thumbs::thumb(&slide, "song", None, settings, PREVIEW_SIZE);
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
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    let apply = move |d: Drawn| {
        let Some(op) = me.upgrade() else { return };
        if edit.look.generation.get() != generation { return }
        let g = op.ui.global::<SettingsDlg>();
        g.set_preview(Image::from_rgba8(d.preview));
        g.set_font_note(if d.font_missing { strings::font_missing(&edit.settings.borrow().theme.font) } else { String::new() }.into());
        g.set_logo(d.logo.map(Image::from_rgba8).unwrap_or_default());
    };
    if op.ops().is_inline() { return apply(draw(&settings, root)) }
    let (tx, rx) = oneshot::channel::<Drawn>();
    let started = std::thread::Builder::new().name("settings-preview".into()).spawn(move || { let _ = tx.send(draw(&settings, root)); });
    if started.is_ok() {
        let _ = slint::spawn_local(async move { if let Ok(d) = rx.await { apply(d) } });
    }
}
