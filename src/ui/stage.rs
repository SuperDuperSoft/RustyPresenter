//! The stage screen for the people on the platform: what is live, what comes next, notes,
//! the clock and the time left on a video (same content as `web/stage.html`).
//! Classic (the default) draws today's fixed screen; any other stage layout draws its boxes (`stage::view`).
use super::{ChordPieceVm, StageCellVm, StageWindow};
use crate::{
    app::{App, LiveUpdate, LiveView, Update},
    model::{Slide, SlideContent},
    platform::monitors::{self, MonitorId},
    stage::{self, chords, presets, strings as s, view::{self, CellContent, StageInputs}, Align, StageLayout, StageState, VAlign},
    timers::TimersView,
};
use slint::{ComponentHandle, Model, Timer, TimerMode, VecModel};
use std::{collections::HashMap, rc::Rc, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::Duration};
use tokio::sync::broadcast::error::RecvError;

use crate::stage::view::label;
use crate::network::live_anim::LiveGroup;
use crate::screen_fix::{ScreenFix, TestPattern};

/// Everything the stage shows, worked out from one live view.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StageText {
    pub title: String, pub now: String, pub now_dim: bool, pub notes: String, pub next: String, pub timer: String, pub message: String,
    /// N29: `now` is a slide sent to the stage only.
    pub stage_only: bool,
}

pub fn stage_text(view: &LiveView) -> StageText {
    let f = &view.frame;
    let on_air = f.slide.is_some() || f.media.is_some();
    let now = match &view.slide {
        Some(s) if on_air => label(s),
        _ if f.logo => s::logo().into(),
        _ => String::new(),
    };
    let timer = view.media.as_ref().or(view.audio.as_ref()).filter(|m| m.duration > 0.0).map(|m| view::countdown(m.duration - m.position));
    let only = view.stage_only.as_ref().map(|o| &o.slide);
    StageText {
        stage_only: only.is_some(),
        title: view.item.as_ref().map_or_else(s::nothing_live, |i| i.title.clone()).into(),
        now: only.map_or(now, label),
        now_dim: !on_air && only.is_none(),
        notes: only.or(view.slide.as_ref()).map(|s| s.notes.clone()).unwrap_or_default(),
        next: view.next.as_ref().map_or_else(|| s::end_of_service().into(), |n| label(&n.slide)),
        timer: timer.unwrap_or_default(),
        message: view.stage_message.clone(),
    }
}

/// The largest font size (in % of the height) that `fits`: 9 % down to 3 % in 0.4 % steps, like `stage.html`.
pub fn fit_size(fits: impl Fn(f32) -> bool) -> f32 { fit_between(9.0, 3.0, fits) }

/// The smallest words on the stage screen, in % of its height (13 px on a 1080p screen): smaller than a box's own
/// smallest size, but all the words are seen. (Jake, November 15, 2026: long notes were cut off.)
pub const FLOOR: f32 = 1.2;

/// The largest size from `max` down to `min` in 0.4 % steps that `fits`. When none does, on down from `min` in
/// 0.2 % steps to `FLOOR`: words that do not fit are made smaller, never cut off. `FLOOR` when even that does not
/// fit (a whole page of notes in a small box: the end is cut off then).
pub fn fit_between(max: f32, min: f32, fits: impl Fn(f32) -> bool) -> f32 {
    let steps = ((max - min) / 0.4 + 1e-3).floor().max(0.0) as usize;
    if let Some(s) = (0..=steps).map(|k| max - 0.4 * k as f32).find(|s| fits(*s)) { return s }
    let below = ((min - FLOOR) / 0.2 + 1e-3).floor().max(0.0) as usize;
    (1..=below).map(|k| min - 0.2 * k as f32).find(|s| fits(*s)).unwrap_or(FLOOR.min(min))
}

/// Classic's notes box: at most this share of the screen's height (in %), its words 3.2 % at most.
const NOTES_MAX_H: f32 = 30.0;
const NOTES_SIZE: f32 = 3.2;

/// Classic's notes at the biggest size up to `NOTES_SIZE` at which they fit their box.
fn fit_notes(ui: &StageWindow) {
    let notes = ui.get_notes();
    if notes.is_empty() { return ui.set_notes_size(NOTES_SIZE) }
    let (ww, wh) = (ui.get_window_width(), ui.get_window_height());
    let (vh, vw) = (wh / 100.0, ww / 100.0);
    // The box's padding: 1.6 % of the width each side, 1.2 % of the height top and bottom.
    let aw = ww - 2.0 * 1.6 * vw;
    let ah = NOTES_MAX_H * vh - 2.0 * 1.2 * vh;
    ui.set_probe_text(notes);
    ui.set_probe_width(aw.max(1.0));
    ui.set_probe_bold(false);
    let size = fit_between(NOTES_SIZE, NOTES_SIZE, |s| { ui.set_probe_size(s); ui.get_probe_height() <= ah && ui.get_probe_min_width() <= aw });
    ui.set_notes_size(size);
}

fn clock(t: chrono::NaiveTime) -> String { view::clock(t, false, false) }

/// What the window draws: the live view, the stage layout and the live item's slides (for "next section").
#[derive(Clone)]
struct Shown { live: LiveUpdate, layout: StageLayout, slides: Vec<Slide>, timers: Arc<TimersView>, next_picture: Option<super::thumbs::Pixels>,
    /// The live designed slide's picture, and its click group to play once (taken by the window) when it is new.
    live_picture: Option<super::thumbs::Pixels>, group: Option<Arc<Mutex<Option<LiveGroup>>>> }

/// The size the stage draws designed slides at.
const DESIGNED: (u32, u32) = (960, 540);

/// The layout on stage is the church's stage default (Classic when it names no layout), built-in or the church's own.
fn read(app: &App, live: LiveUpdate, screen: &StageScreen) -> Shown {
    let pictures = &screen.pictures;
    let layout = {
        let d = app.data.lock().unwrap();
        stage::active_layout(&d.stage_layouts, None, None, Some(d.settings.stage.layout)).clone()
    };
    let slides = if layout.id == presets::CLASSIC { vec![] } else { app.live.lock().unwrap().item.as_ref().map(|i| i.slides.clone()).unwrap_or_default() };
    let timers = app.timers();
    // The slide pictures the boxes will show (current and next PowerPoint slides), decoded here, not on the window.
    let contents: Vec<CellContent> = {
        let state = screen.state.lock().unwrap();
        let inputs = StageInputs { live: &live, slides: &slides, timers: &timers, state: &state, now: chrono::Local::now().time() };
        view::build(&inputs, &layout).cells.into_iter().map(|c| c.content).collect()
    };
    let dir = app.store.assets();
    for c in &contents { if let CellContent::Picture { asset, .. } = c { screen.slide_pictures.load(&dir, asset) } }
    let wants = contents.iter().any(|c| matches!(c, CellContent::NextPicture { .. } | CellContent::Designed { next: true, .. }));
    let next_picture = wants.then(|| pictures.get(app, live.version));
    // A designed slide live: its picture, and its click group once per live version.
    let designed = contents.iter().any(|c| matches!(c, CellContent::Designed { next: false, .. }));
    let live_picture = designed.then(|| screen.live_pictures.get(app, live.version));
    let group = designed.then(|| {
        let mut made = screen.group_made.lock().unwrap();
        if *made == Some(live.version) { return None }
        *made = Some(live.version);
        crate::network::live_anim::live_group(app, DESIGNED).filter(|g| g.version == live.version)
    }).flatten().map(|g| Arc::new(Mutex::new(Some(g))));
    Shown { live, layout, slides, timers, next_picture, live_picture, group }
}

/// N10: the next slide's picture, drawn again only when the live view changes. Off the UI thread.
#[derive(Default)]
struct NextPictures { last: Mutex<Option<(u64, super::thumbs::Pixels)>>, renderer: super::thumbs::TextRenderer }

impl NextPictures {
    fn get(&self, app: &App, version: u64) -> super::thumbs::Pixels {
        let mut last = self.last.lock().unwrap();
        if let Some((v, px)) = last.as_ref() && *v == version { return px.clone() }
        let px = super::thumbs::next_slide(app, (640, 360), &self.renderer);
        *last = Some((version, px.clone()));
        px
    }
}

/// The live designed slide's picture, drawn again only when the live view changes. Off the UI thread.
#[derive(Default)]
struct LivePictures { last: Mutex<Option<(u64, super::thumbs::Pixels)>>, renderer: super::thumbs::TextRenderer }

impl LivePictures {
    fn get(&self, app: &App, version: u64) -> super::thumbs::Pixels {
        let mut last = self.last.lock().unwrap();
        if let Some((v, px)) = last.as_ref() && *v == version { return px.clone() }
        let px = super::thumbs::live_slide(app, DESIGNED, &self.renderer);
        *last = Some((version, px.clone()));
        px
    }
}

/// PowerPoint / PDF / Keynote slide pictures for the stage boxes, decoded once (off the window's thread) and
/// kept for the last few slides.
#[derive(Default)]
struct SlidePictures { cache: Mutex<Vec<(String, super::thumbs::Pixels)>> }

impl SlidePictures {
    const KEEP: usize = 6;
    /// The picture for `asset`, from the cache.
    fn get(&self, asset: &str) -> Option<super::thumbs::Pixels> {
        self.cache.lock().unwrap().iter().find(|(a, _)| a == asset).map(|(_, px)| px.clone())
    }
    /// Decodes `asset` from the media folder unless it is already kept. A file that can't be read is skipped.
    fn load(&self, assets: &std::path::Path, asset: &str) {
        if self.get(asset).is_some() { return }
        let Ok(px) = super::room::prepare::picture(&assets.join(asset), (1280, 720), false) else { return };
        let px = match px { super::room::prepare::Px::Straight(b) | super::room::prepare::Px::Premul(b) => b };
        let mut cache = self.cache.lock().unwrap();
        cache.retain(|(a, _)| a != asset);
        cache.push((asset.to_string(), px));
        if cache.len() > Self::KEEP { cache.remove(0); }
    }
}

/// The window's own state: the last thing drawn (redrawn by the clock and on resize) and fitted sizes.
#[derive(Default)]
struct StageScreen { last: Mutex<Option<Shown>>, fits: Mutex<HashMap<FitKey, f32>>, state: Mutex<StageState>, pictures: NextPictures,
    /// Text measured for chord sheets: (text, weight) → width and height at `MEASURE_AT` pixels.
    measures: Mutex<HashMap<(String, i32), (f32, f32)>>,
    slide_pictures: SlidePictures, live_pictures: LivePictures,
    /// The live version whose click group was last made, so each plays once.
    group_made: Mutex<Option<u64>> }

/// The stage's click-group player (UI thread only): the shared frame timer, the window's sprites and the live
/// version it plays.
struct StagePlayer { anim: crate::ui::room::anim::Animator, sprites: Rc<VecModel<crate::ui::Sprite>>, version: std::cell::Cell<u64> }

thread_local! {
    /// Each open stage window's player, by its `StageScreen`.
    static PLAYERS: std::cell::RefCell<Vec<(usize, Rc<StagePlayer>)>> = std::cell::RefCell::default();
}

fn screen_key(screen: &StageScreen) -> usize { screen as *const StageScreen as usize }

fn player(ui: &StageWindow, screen: &StageScreen) -> Rc<StagePlayer> {
    let key = screen_key(screen);
    PLAYERS.with_borrow_mut(|all| {
        if let Some((_, p)) = all.iter().find(|(k, _)| *k == key) { return p.clone() }
        let p = Rc::new(StagePlayer { anim: Default::default(), sprites: Rc::default(), version: Default::default() });
        ui.set_anim_sprites(p.sprites.clone().into());
        all.push((key, p.clone()));
        p
    })
}

/// Plays a new designed slide's click group in the current slide's box (the room's player and pictures, drawn
/// at the box's size); any other live slide stops it. The frame timer runs only while it plays.
fn play_group(ui: &StageWindow, screen: &StageScreen, shown: &Shown) {
    use crate::ui::room::anim;
    let p = player(ui, screen);
    match shown.group.as_ref().and_then(|g| g.lock().unwrap().take()) {
        Some(g) => {
            p.version.set(g.version);
            let canvas = (g.base.width(), g.base.height());
            ui.set_anim_base(slint::Image::from_rgba8(g.base));
            ui.set_animating(true);
            let run = anim::Running { imgs: anim::Layers { canvas, layers: g.layers }.images(), play: g.play, area: anim::area(canvas), lead_ms: 0.0 };
            let (w, sprites) = (ui.as_weak(), p.sprites.clone());
            p.anim.start(run, move |rows| match rows {
                Some((rows, _)) => anim::set_rows(&sprites, rows),
                None => { anim::set_rows(&sprites, vec![]); if let Some(ui) = w.upgrade() { ui.set_animating(false) } }
            });
        }
        None if shown.live.version != p.version.get() => if p.anim.stop() { anim::set_rows(&p.sprites, vec![]); ui.set_animating(false) },
        None => {}
    }
}

#[derive(Hash, PartialEq, Eq)]
struct FitKey { text: String, w: i32, h: i32, bold: bool, max: i32, min: i32 }

fn tick(ui: &StageWindow, screen: &StageScreen) {
    ui.set_clock(clock(chrono::Local::now().time()).into());
    if ui.get_use_cells() { redraw(ui, screen) }
}

fn fit(ui: &StageWindow) {
    // The notes first: their box's height decides the room the current slide has.
    fit_notes(ui);
    let size = fit_size(|s| { ui.set_now_size(s); ui.get_now_fits() });
    ui.set_now_size(size);
}

fn show(ui: &StageWindow, screen: &StageScreen, shown: Shown) {
    // Classic's plain text screen can't show a PowerPoint/PDF slide: it switches to its boxes for those.
    let cells = shown.layout.id != presets::CLASSIC || picture_slide(&shown.live);
    ui.set_use_cells(cells);
    if cells { draw_cells(ui, screen, &shown) } else { show_classic(ui, &shown.live, &classic_timer(screen, &shown.timers)) }
    play_group(ui, screen, &shown);
    *screen.last.lock().unwrap() = Some(shown);
}

/// The live slide is a picture (PowerPoint, PDF, Keynote) or a designed slide: the stage shows the slide itself.
fn picture_slide(v: &LiveView) -> bool { matches!(v.slide.as_ref().map(|s| &s.content), Some(SlideContent::Image { .. } | SlideContent::Design { .. })) }

fn redraw(ui: &StageWindow, screen: &StageScreen) {
    let last = screen.last.lock().unwrap().clone();
    if let Some(shown) = last { if ui.get_use_cells() { draw_cells(ui, screen, &shown) } else { fit(ui) } }
}

/// The current "show on stage" timer for Classic's timer line (FEAT-2 §5.8), with its name.
fn classic_timer(screen: &StageScreen, timers: &TimersView) -> String {
    let mut state = screen.state.lock().unwrap();
    state.track_timers(timers);
    state.current(timers).map(|t| format!("{} {}", t.name, crate::timers::text(t.value_ms))).unwrap_or_default()
}

fn show_classic(ui: &StageWindow, view: &LiveView, timer: &str) {
    let mut t = stage_text(view);
    // A playing video or audio countdown keeps the line.
    if t.timer.is_empty() { t.timer = timer.to_string() }
    ui.set_item_title(t.title.into());
    ui.set_now(t.now.into());
    ui.set_now_dim(t.now_dim);
    ui.set_stage_only(t.stage_only);
    ui.set_notes(t.notes.into());
    ui.set_next(t.next.into());
    ui.set_timer(t.timer.into());
    ui.set_message(t.message.into());
    ui.set_follow(crate::stage::view::follow_line(view).unwrap_or_default().into());
    fit(ui);
}

fn color(c: &str) -> slint::Color { let [r, g, b, a] = crate::render::parse_color(c); slint::Color::from_argb_u8(a, r, g, b) }

/// Draw the layout's boxes. Rows that did not change are left alone.
fn draw_cells(ui: &StageWindow, screen: &StageScreen, shown: &Shown) {
    let timers = shown.timers.clone();
    let mut state = screen.state.lock().unwrap();
    state.track_timers(&timers);
    let inputs = StageInputs { live: &shown.live, slides: &shown.slides, timers: &timers, state: &state, now: chrono::Local::now().time() };
    let v = view::build(&inputs, &shown.layout);
    drop(state);
    ui.set_bg(color(&v.background));
    ui.set_banner(v.message_fallback.unwrap_or_default().into());
    ui.set_follow(v.follow_line.unwrap_or_default().into());
    let picture = shown.next_picture.clone().map(slint::Image::from_rgba8).unwrap_or_default();
    let live = shown.live_picture.clone().map(slint::Image::from_rgba8).unwrap_or_default();
    let rows: Vec<StageCellVm> = v.cells.iter().filter_map(|c| cell_vm(ui, screen, c, &picture, &live)).collect();
    let model = ui.get_cells();
    match model.as_any().downcast_ref::<VecModel<StageCellVm>>() {
        Some(m) if m.row_count() == rows.len() => {
            for (i, row) in rows.into_iter().enumerate() { if m.row_data(i).as_ref() != Some(&row) { m.set_row_data(i, row) } }
        }
        Some(m) => m.set_vec(rows),
        None => ui.set_cells(Rc::new(VecModel::from(rows)).into()),
    }
}

/// One box for the window, its words sized to fit. Empty boxes are not drawn.
fn cell_vm(ui: &StageWindow, screen: &StageScreen, c: &view::Cell, next_picture: &slint::Image, live_picture: &slint::Image) -> Option<StageCellVm> {
    let st = &c.style;
    let base = || StageCellVm {
        x: c.rect.x, y: c.rect.y, w: c.rect.w, h: c.rect.h, color: color(&st.color),
        fill: st.fill.as_deref().map_or(slint::Color::from_argb_u8(0, 0, 0, 0), color), shape: -1, ..Default::default()
    };
    let (text, caption, dim, red, message) = match &c.content {
        CellContent::Empty => return None,
        CellContent::Shape { shape } => return Some(StageCellVm { shape: shape.index() as i32, ..base() }),
        CellContent::NextPicture { caption } => return Some(StageCellVm {
            caption: caption.clone().unwrap_or_default().to_uppercase().into(), has_picture: true, picture: next_picture.clone(), ..base()
        }),
        CellContent::Designed { next, caption, .. } => return Some(StageCellVm {
            caption: caption.clone().unwrap_or_default().to_uppercase().into(), has_picture: true, anim: !next,
            picture: if *next { next_picture.clone() } else { live_picture.clone() }, ..base()
        }),
        CellContent::Chords { lines, caption, chord_color } => return Some(chord_cell(ui, screen, c, lines, caption.as_deref().unwrap_or_default(), chord_color, base())),
        CellContent::Text { text, dim, caption, red } => (text.clone(), caption.clone().unwrap_or_default(), *dim, *red, false),
        // The slide itself (a PowerPoint/PDF slide); its name only while the picture can't be read.
        CellContent::Picture { asset, caption } => match screen.slide_pictures.get(asset) {
            Some(px) => return Some(StageCellVm {
                caption: caption.clone().unwrap_or_default().to_uppercase().into(), has_picture: true, picture: slint::Image::from_rgba8(px), ..base()
            }),
            None => (s::slide_picture(), caption.clone().unwrap_or_default(), false, false, false),
        },
        CellContent::Message { text } => (text.clone(), String::new(), false, false, true),
    };
    let bold = st.bold || message;
    let size = fit_cell(ui, screen, &text, c, !caption.is_empty(), bold);
    Some(StageCellVm {
        text: text.into(),
        caption: caption.to_uppercase().into(),
        size,
        align: match st.align { Align::Left => 0, Align::Center => 1, Align::Right => 2 },
        valign: match st.valign { VAlign::Top => 0, VAlign::Middle => 1, VAlign::Bottom => 2 },
        bold, dim, red, message,
        ..base()
    })
}

/// Box padding and caption height in % of the window height (as `ui/stage.slint` draws them).
const PAD: f32 = 1.2;
const CAPTION: f32 = 2.75 + 0.6;

/// The biggest size from the box's `max_size` down to its `min_size` at which the words fit the box.
fn fit_cell(ui: &StageWindow, screen: &StageScreen, text: &str, c: &view::Cell, caption: bool, bold: bool) -> f32 {
    let (max, min) = (c.style.max_size, c.style.min_size);
    if text.is_empty() { return max; }
    let (ww, wh) = (ui.get_window_width(), ui.get_window_height());
    let vh = wh / 100.0;
    let aw = c.rect.w * ww / 100.0 - 2.0 * PAD * vh;
    let ah = c.rect.h * vh - 2.0 * PAD * vh - if caption { CAPTION * vh } else { 0.0 };
    let key = FitKey { text: text.into(), w: aw.round() as i32, h: ah.round() as i32, bold, max: (max * 100.0) as i32, min: (min * 100.0) as i32 };
    let mut fits = screen.fits.lock().unwrap();
    if let Some(size) = fits.get(&key) { return *size; }
    ui.set_probe_text(text.into());
    ui.set_probe_width(aw.max(1.0));
    ui.set_probe_bold(bold);
    let size = fit_between(max, min, |s| { ui.set_probe_size(s); ui.get_probe_height() <= ah && ui.get_probe_min_width() <= aw });
    if fits.len() > 512 { fits.clear() }
    fits.insert(key, size);
    size
}

/// Chords are drawn a little smaller than the words, and bold.
const CHORD_SCALE: f32 = 0.9;
const CHORD_WEIGHT: i32 = 800;
/// The size text is measured at (in pixels); other sizes are worked out from it.
const MEASURE_AT: f32 = 100.0;

thread_local! {
    /// The last chord sheets drawn, so a redraw (the clock, every second) hands the window the same pieces and
    /// it does not build them again.
    static SHEETS: std::cell::RefCell<Vec<(SheetKey, slint::ModelRc<ChordPieceVm>)>> = std::cell::RefCell::default();
}

#[derive(PartialEq, Clone)]
struct SheetKey { text: String, w: i32, size: i32, bold: bool }

/// How wide and high `text` is drawn at `MEASURE_AT` pixels in `weight`, measured once by the window.
fn measured(ui: &StageWindow, screen: &StageScreen, text: &str, weight: i32) -> (f32, f32) {
    let key = (text.to_string(), weight);
    if let Some(m) = screen.measures.lock().unwrap().get(&key) { return *m }
    ui.set_measure_size(MEASURE_AT);
    ui.set_measure_weight(weight);
    let at = |t: String| { ui.set_measure_text(t.into()); (ui.get_measure_width(), ui.get_measure_height()) };
    // Between two bars, so spaces at either end count.
    let (with, h) = at(format!("|{text}|"));
    let (bars, _) = at("||".into());
    let m = ((with - bars).max(0.0), h);
    let mut all = screen.measures.lock().unwrap();
    if all.len() > 4096 { all.clear() }
    all.insert(key, m);
    m
}

/// A chord sheet box: the chords above the words (`stage::chords`), at the biggest size from the box's own down
/// at which the whole sheet fits (both rows of every line count); never cut off, as `fit_between` says.
fn chord_cell(ui: &StageWindow, screen: &StageScreen, c: &view::Cell, lines: &[chords::ChordLine], caption: &str, chord_color: &str, base: StageCellVm) -> StageCellVm {
    let st = &c.style;
    let (ww, wh) = (ui.get_window_width(), ui.get_window_height());
    let vh = wh / 100.0;
    let aw = c.rect.w * ww / 100.0 - 2.0 * PAD * vh;
    let ah = c.rect.h * vh - 2.0 * PAD * vh - if caption.is_empty() { 0.0 } else { CAPTION * vh };
    let words_weight = if st.bold { 650 } else { 500 };
    let text = chords::plain(lines);
    let lay = |size: f32| {
        let (px, cpx) = (size * vh, size * CHORD_SCALE * vh);
        let line_h = |weight| measured(ui, screen, "Ag", weight).1 / MEASURE_AT;
        let m = chords::Metrics { chord_h: line_h(CHORD_WEIGHT) * cpx, words_h: line_h(words_weight) * px, gap: 0.4 * px };
        chords::lay_out(lines, aw, m, |t, chord| if chord { measured(ui, screen, t, CHORD_WEIGHT).0 * cpx } else { measured(ui, screen, t, words_weight).0 * px } / MEASURE_AT)
    };
    let (max, min) = (st.max_size, st.min_size);
    let key = FitKey { text: format!("\u{1}chords\u{1}{text}"), w: aw.round() as i32, h: ah.round() as i32, bold: st.bold, max: (max * 100.0) as i32, min: (min * 100.0) as i32 };
    let cached = screen.fits.lock().unwrap().get(&key).copied();
    let size = cached.unwrap_or_else(|| {
        let size = fit_between(max, min, |s| { let l = lay(s); l.w <= aw + 0.5 && l.h <= ah });
        let mut fits = screen.fits.lock().unwrap();
        if fits.len() > 512 { fits.clear() }
        fits.insert(key, size);
        size
    });
    let laid = lay(size);
    let skey = SheetKey { text: text.clone(), w: aw.round() as i32, size: (size * 100.0) as i32, bold: st.bold };
    let pieces = SHEETS.with_borrow_mut(|all| {
        if let Some((_, m)) = all.iter().find(|(k, _)| *k == skey) { return m.clone() }
        let rows: Vec<ChordPieceVm> = laid.pieces.iter().map(|p| ChordPieceVm { x: p.x, y: p.y, text: p.text.as_str().into(), chord: p.chord }).collect();
        let m: slint::ModelRc<ChordPieceVm> = Rc::new(VecModel::from(rows)).into();
        all.push((skey, m.clone()));
        if all.len() > 8 { all.remove(0); }
        m
    });
    StageCellVm {
        text: text.into(),
        caption: caption.to_uppercase().into(),
        size,
        align: match st.align { Align::Left => 0, Align::Center => 1, Align::Right => 2 },
        valign: match st.valign { VAlign::Top => 0, VAlign::Middle => 1, VAlign::Bottom => 2 },
        bold: st.bold,
        sheet: true, sheet_w: laid.w, sheet_h: laid.h, pieces, chord_color: color(chord_color), chord_size: size * CHORD_SCALE,
        ..base
    }
}

/// This window's screen fix and test pattern (N20).
fn fix_of(app: &App, role: &str) -> (ScreenFix, TestPattern) {
    app.data.lock().unwrap().settings.screens.screen_for(role).map_or_else(Default::default, |s| (s.fix, s.test))
}

/// Puts a test pattern over the stage, or takes it away.
fn show_test(ui: &StageWindow, test: TestPattern) {
    let size = ui.window().size();
    let (w, h) = if size.width > 0 && size.height > 0 { (size.width, size.height) } else { (1920, 1080) };
    let px = crate::screen_fix::pattern_pixels(test, (w, h));
    ui.set_test_pattern(px.map(|p| slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(&p, w, h))).unwrap_or_default());
    ui.set_testing(!test.is_off());
}

/// An open stage window. It follows the live view until it is dropped.
pub struct Stage { ui: StageWindow, _clock: Timer, closed: Arc<AtomicBool>, key: usize }

impl Stage {
    /// Open the stage window, full screen on `monitor` (a plain window without one).
    pub fn open(app: Arc<App>, monitor: Option<MonitorId>) -> Result<Rc<Stage>, String> { Self::open_as(app, monitor, "stage") }

    /// `open` for the window named `role` ("stage", "stage:<id>"), which follows that screen's fix (N20).
    pub fn open_as(app: Arc<App>, monitor: Option<MonitorId>, role: &str) -> Result<Rc<Stage>, String> {
        let ui = StageWindow::new().map_err(|e| crate::tr!("Could not open the stage screen: {error}", error = e))?;
        // Slint picks the computer's language for the first window; show the app's.
        crate::i18n::sync_slint();
        // Its frames never wait for the monitor's refresh, so the room's are not held up behind them.
        let (fix, test) = fix_of(&app, role);
        let shared_fix: crate::video::postpass::SharedFix = Arc::new(Mutex::new(fix));
        crate::video::postpass::attach(ui.window(), "stage", false, shared_fix.clone());
        show_test(&ui, test);
        let screen = Arc::new(StageScreen::default());
        let weak = ui.as_weak();
        let sc = screen.clone();
        ui.on_refit(move || if let Some(ui) = weak.upgrade() { redraw(&ui, &sc) });
        let clock = Timer::default();
        let weak = ui.as_weak();
        let sc = screen.clone();
        clock.start(TimerMode::Repeated, Duration::from_secs(1), move || if let Some(ui) = weak.upgrade() { tick(&ui, &sc) });
        tick(&ui, &screen);
        // Subscribe before reading the view so no update falls in between.
        let rx = app.subscribe();
        show(&ui, &screen, read(&app, LiveUpdate::new(app.live_view()), &screen));
        ui.set_connected(true);
        ui.show().map_err(|e| crate::tr!("Could not open the stage screen: {error}", error = e))?;
        if let Some(m) = &monitor {
            if let Err(e) = monitors::fullscreen_on(&ui, m) { let _ = ui.hide(); return Err(e); }
        }
        let closed = Arc::new(AtomicBool::new(false));
        let key = screen_key(&screen);
        follow(app, rx, ui.as_weak(), screen, closed.clone(), (role.to_string(), shared_fix, test));
        Ok(Rc::new(Stage { ui, _clock: clock, closed, key }))
    }

    pub fn window(&self) -> &slint::Window { self.ui.window() }
}

impl Drop for Stage {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
        let _ = self.ui.hide();
        PLAYERS.with_borrow_mut(|all| all.retain(|(k, _)| *k != self.key));
    }
}

/// The drawing panel's size: the live slide, small, with the phone drawing on it.
const PANEL: (u32, u32) = (640, 360);

/// What the drawing panel shows: nothing (`None`), or the live slide with its drawing, straight RGBA. `slide` keeps
/// the slide picture for the live version it was drawn for, so each new stroke only redraws the lines.
pub fn drawing_panel(app: &App, canvas: &mut crate::render::drawing::DrawingCanvas, slide: &mut Option<(u64, super::thumbs::Pixels)>,
    renderer: &super::thumbs::TextRenderer) -> Option<super::thumbs::Pixels> {
    let on = app.data.lock().unwrap().settings.stage_drawings;
    let (version, drawing) = { let l = app.live.lock().unwrap(); (l.version, l.drawing.clone()) };
    if !on || drawing.is_empty() { *slide = None; return None }
    if slide.as_ref().is_none_or(|(v, _)| *v != version) { *slide = Some((version, super::thumbs::live_slide(app, PANEL, renderer))) }
    let mut img = slide.as_ref()?.1.clone();
    if let Some(lines) = canvas.render(&drawing, PANEL) { super::thumbs::put_over(&mut img, &lines) }
    Some(img)
}

/// Draws the panel on a thread of its own: each poke draws it once from the newest state (pokes that pile up while
/// it draws count as one).
fn panel_thread(app: Arc<App>, ui: slint::Weak<StageWindow>, closed: Arc<AtomicBool>) -> std::sync::mpsc::Sender<()> {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let _ = std::thread::Builder::new().name("stage-drawing".into()).spawn(move || {
        let (mut canvas, mut slide, renderer) = (crate::render::drawing::DrawingCanvas::new(), None, super::thumbs::TextRenderer::new());
        let mut showing = false;
        loop {
            // While ink is fading the panel draws again by itself.
            let wait = app.live.lock().unwrap().drawing.next_change(std::time::Instant::now());
            let got = match wait { Some(w) => rx.recv_timeout(w).or_else(|e| if e == std::sync::mpsc::RecvTimeoutError::Timeout { Ok(()) } else { Err(()) }), None => rx.recv().map_err(|_| ()) };
            if got.is_err() { return }
            while rx.try_recv().is_ok() {}
            if closed.load(Ordering::Relaxed) { return }
            let px = drawing_panel(&app, &mut canvas, &mut slide, &renderer);
            if px.is_none() && !showing { continue }
            showing = px.is_some();
            let _ = ui.upgrade_in_event_loop(move |ui| {
                ui.set_drawing_on(px.is_some());
                ui.set_drawing(px.map(slint::Image::from_rgba8).unwrap_or_default());
            });
        }
    });
    tx
}

/// Forward live and data updates to the window from a plain thread (no tokio runtime needed on the UI side).
/// Only the newest view is drawn: updates that arrive while one is queued replace it.
fn follow(app: Arc<App>, mut rx: tokio::sync::broadcast::Receiver<Update>, ui: slint::Weak<StageWindow>, screen: Arc<StageScreen>, closed: Arc<AtomicBool>,
    (role, fix, mut test): (String, crate::video::postpass::SharedFix, TestPattern)) {
    let latest: Arc<Mutex<Option<Shown>>> = Arc::default();
    let panel = panel_thread(app.clone(), ui.clone(), closed.clone());
    let _ = panel.send(());
    let _ = std::thread::Builder::new().name("stage-updates".into()).spawn(move || loop {
        let update = match rx.blocking_recv() {
            Ok(Update::Drawing(_)) => { let _ = panel.send(()); continue }
            Ok(Update::Live(u)) => { let _ = panel.send(()); u }
            // Settings may have a new stage layout.
            Ok(Update::Data) | Err(RecvError::Lagged(_)) => {
                let _ = panel.send(());
                // The screen's fix or test pattern may have changed (N20).
                let (f, t) = fix_of(&app, &role);
                let fix_changed = std::mem::replace(&mut *fix.lock().unwrap(), f) != f;
                if fix_changed || t != test {
                    let redo_test = std::mem::replace(&mut test, t) != t;
                    let _ = ui.upgrade_in_event_loop(move |ui| { if redo_test { show_test(&ui, t) } ui.window().request_redraw() });
                }
                LiveUpdate::new(app.live_view())
            }
            // A new timer readout: the same live view with the new times.
            Ok(Update::Timers(_)) => match latest.lock().unwrap().as_ref().map(|s| s.live.clone()) {
                Some(live) => live,
                None => LiveUpdate::new(app.live_view()),
            },
            Ok(_) => continue,
            Err(RecvError::Closed) => {
                let _ = ui.upgrade_in_event_loop(|ui| ui.set_connected(false));
                return;
            }
        };
        if closed.load(Ordering::Relaxed) { return; }
        let shown = read(&app, update, &screen);
        if latest.lock().unwrap().replace(shown).is_some() { continue; } // already queued: it will draw this one
        let (latest, screen) = (latest.clone(), screen.clone());
        let sent = ui.upgrade_in_event_loop(move |ui| if let Some(s) = latest.lock().unwrap().take() { show(&ui, &screen, s) });
        if sent.is_err() { return; }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::{LiveItemSummary, NextUp, test_app}, live::*, model::*};
    use slint::ComponentHandle;
    use uuid::Uuid;

    fn text(t: &str) -> Slide { Slide::new(t, SlideContent::Text { text: t.into(), footer: None, reference: None, size: 0, parallel: None }) }
    fn slide(content: SlideContent) -> Slide { Slide::new(String::new(), content) }

    /// A view with `now` live as a text slide in "Amazing Grace" and `next` after it.
    fn view(now: Option<Slide>, next: Option<Slide>) -> LiveView {
        let on_air = now.as_ref().map(|s| match &s.content {
            SlideContent::Text { text, .. } => Some(SlideLayer::Text { text: text.clone(), footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None, theme: None }),
            _ => None,
        });
        LiveView {
            version: 1, committed: 1, pending: false,
            frame: Frame { slide: on_air.flatten(), ..Frame::default() },
            cursor: None,
            item: now.as_ref().map(|_| LiveItemSummary { item_id: Uuid::nil(), title: "Amazing Grace".into(), kind: "song".into(), count: 2, song_id: None }),
            slide: now,
            next: next.map(|slide| NextUp { title: "Amazing Grace".into(), slide }),
            media: None, audio: None, notice: None, sections: vec![], changed: None, theme: Theme::default(), logo: None, clear_epoch: 0, stage_message: String::new(), drawing: Default::default(), stage_only: None, look_fade_ms: 0, follow: None,
        }
    }

    #[test]
    fn stage_text_for_lyric_with_next() {
        let mut now = text("Amazing grace, how sweet the sound");
        now.notes = "Slow, hold the last line".into();
        let t = stage_text(&view(Some(now), Some(text("That saved a wretch like me"))));
        assert_eq!(t, StageText {
            title: "Amazing Grace".into(), now: "Amazing grace, how sweet the sound".into(), now_dim: false,
            notes: "Slow, hold the last line".into(), next: "That saved a wretch like me".into(), timer: String::new(), message: String::new(), stage_only: false,
        });
    }

    #[test]
    fn classic_shows_a_stage_only_slide_instead_of_the_live_one() {
        let mut v = view(None, Some(text("Next up")));
        let mut note = text("Watch me for the ending");
        note.notes = "Two more times".into();
        v.stage_only = Some(crate::app::StageOnly { item_id: Uuid::nil(), index: 0, slide: note });
        let t = stage_text(&v);
        assert_eq!((t.now.as_str(), t.now_dim, t.stage_only, t.notes.as_str()), ("Watch me for the ending", false, true, "Two more times"));
        assert_eq!(t.next, "Next up", "the room's next slide is still next");
    }

    #[test]
    fn stage_text_carries_the_operators_message() {
        let mut v = view(Some(text("Amen")), None);
        v.stage_message = "Wrap up, please".into();
        assert_eq!(stage_text(&v).message, "Wrap up, please");
    }

    #[test]
    fn stage_text_end_of_service() {
        let t = stage_text(&view(Some(text("Amen")), None));
        assert_eq!(t.next, "End of service");
        let empty = stage_text(&view(None, None));
        assert_eq!((empty.title.as_str(), empty.now.as_str(), empty.now_dim), ("Nothing live", "", true));
    }

    #[test]
    fn stage_text_dims_when_cleared() {
        let mut v = view(Some(text("Amazing grace")), None);
        v.frame.slide = None; // Clear Slide: the cursor's slide is still known but nothing is on air
        let t = stage_text(&v);
        assert!(t.now_dim);
        assert_eq!(t.now, "");
        v.frame.logo = true;
        assert_eq!(stage_text(&v).now, "(logo)");
    }

    #[test]
    fn stage_text_timer_counts_down() {
        let mut v = view(Some(slide(SlideContent::Video { asset: "v.mp4".into(), autoplay: true, end: EndAction::Stop })), None);
        v.frame.media = Some(Inst { inst: 7, v: MediaLayer::Video { asset: "v.mp4".into(), end: EndAction::Stop, paused: false } });
        v.media = Some(PlayStatus { inst: 7, state: PlayState::Playing, position: 10.4, duration: 90.0 });
        assert_eq!(stage_text(&v).timer, "−1:20");
        v.media.as_mut().unwrap().position = 85.0;
        assert_eq!(stage_text(&v).timer, "−0:05");
        v.media.as_mut().unwrap().position = 95.0; // never below zero
        assert_eq!(stage_text(&v).timer, "−0:00");
        // Audio counts down when no video reports; no duration yet shows nothing.
        v.media = None;
        v.audio = Some(PlayStatus { inst: 8, state: PlayState::Playing, position: 0.0, duration: 0.0 });
        assert_eq!(stage_text(&v).timer, "");
        v.audio.as_mut().unwrap().duration = 125.0;
        assert_eq!(stage_text(&v).timer, "−2:05");
    }

    #[test]
    fn stage_text_labels_non_text_slides() {
        let cases = [
            (SlideContent::Image { asset: "a.png".into(), transition: None }, "(slide picture)"),
            (SlideContent::Video { asset: "v.mp4".into(), autoplay: true, end: EndAction::Stop }, "(video)"),
            (SlideContent::Still { asset: "s.jpg".into() }, "(picture)"),
            (SlideContent::Audio { asset: "m.mp3".into(), looped: false }, "(audio)"),
        ];
        for (content, label) in cases {
            let mut v = view(None, Some(slide(content.clone())));
            v.slide = Some(slide(content));
            v.frame.media = Some(Inst { inst: 1, v: MediaLayer::Still { asset: "s.jpg".into() } });
            let t = stage_text(&v);
            assert_eq!((t.now.as_str(), t.next.as_str()), (label, label));
        }
    }

    #[test]
    fn fit_steps_down_to_three_percent_then_on_rather_than_cut_words_off() {
        assert_eq!(fit_size(|_| true), 9.0);
        assert!((fit_size(|s| s < 7.0) - 6.6).abs() < 1e-4);
        // Too much for 3 %: smaller still, in 0.2 % steps, until it fits.
        assert!((fit_size(|s| s < 2.1) - 2.0).abs() < 1e-4);
        assert!((fit_size(|_| false) - FLOOR).abs() < 1e-4, "never below the floor");
    }

    #[test]
    fn clock_shows_hours_and_minutes() {
        let t = chrono::NaiveTime::from_hms_opt(9, 5, 30).unwrap();
        assert_eq!(clock(t), "9:05 AM");
        assert_eq!(clock(chrono::NaiveTime::from_hms_opt(18, 40, 0).unwrap()), "6:40 PM");
    }

    #[test]
    fn now_text_element_shows_the_live_lyric() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        {
            let mut data = app.data.lock().unwrap();
            let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Grace and peace to you\n\nPlease stand".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
            data.services[0].items.push(item);
        }
        assert!(app.command(Envelope { id: "n1".into(), base: Some(0), cmd: Command::Next }).ok);
        let stage = Stage::open(app.clone(), None).unwrap();
        let now = i_slint_backend_testing::ElementHandle::find_by_element_id(&stage.ui, "StageWindow::now-t").next().expect("now text");
        assert_eq!(now.accessible_label().unwrap_or_default(), "Grace and peace to you");
        let next = i_slint_backend_testing::ElementHandle::find_by_element_id(&stage.ui, "StageWindow::next-t").next().expect("next text");
        assert_eq!(next.accessible_label().unwrap_or_default(), "Please stand");
        assert_eq!(stage.ui.get_item_title(), "Welcome");
        assert!(stage.ui.get_now_fits(), "the lyric must fit at the chosen size");
        stage.ui.hide().unwrap();
    }

    #[test]
    fn a_test_pattern_covers_the_stage() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        app.edit(|d| { d.settings.screens.assign("stage", ScreenTarget::Window); d.settings.screens.stage.screens[0].test = crate::screen_fix::TestPattern::Grid; Ok(()) }).unwrap();
        let stage = Stage::open_as(app.clone(), None, "stage").unwrap();
        assert!(stage.ui.get_testing());
        assert!(stage.ui.get_test_pattern().size().width > 0);
        assert!(!Stage::open_as(app, None, "stage:other").unwrap().ui.get_testing(), "another screen has none");
    }

    #[test]
    fn fit_between_steps_from_the_boxes_biggest_size() {
        assert_eq!(fit_between(18.0, 3.0, |_| true), 18.0);
        assert!((fit_between(6.0, 3.0, |s| s < 5.0) - 4.8).abs() < 1e-4);
        assert!((fit_between(6.0, 3.0, |s| s < 2.5) - 2.4).abs() < 1e-4, "below the box's smallest size rather than cut off");
        assert_eq!(fit_between(6.0, 3.0, |_| false), FLOOR);
        assert_eq!(fit_between(1.0, 1.0, |_| false), 1.0, "a box whose smallest is under the floor keeps it");
    }

    /// A text item live, its first slide on air, with `layout` as the stage default.
    fn app_with_layout(layout: Uuid) -> Arc<App> {
        let app = test_app();
        {
            let mut data = app.data.lock().unwrap();
            data.settings.stage.layout = layout;
            let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Grace and peace to you\n\nPlease stand".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
            data.services[0].items.push(item);
        }
        assert!(app.command(Envelope { id: "n1".into(), base: Some(0), cmd: Command::Next }).ok);
        app
    }

    #[test]
    fn a_powerpoint_slide_shows_itself_on_the_classic_stage() {
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        std::fs::create_dir_all(app.store.assets()).unwrap();
        image::RgbaImage::from_pixel(32, 18, image::Rgba([200, 30, 30, 255])).save(app.store.asset_path("s1.png")).unwrap();
        {
            let mut data = app.data.lock().unwrap();
            let deck = crate::model::Deck { id: Uuid::new_v4(), title: "Talk".into(), slides: vec!["s1.png".into()], notes: vec![], transitions: vec![], advance_ms: vec![], show: None };
            let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Deck { deck_id: deck.id }, background: None, notes: String::new(), theme: None };
            data.decks.push(deck);
            data.services[0].items.push(item);
        }
        assert!(app.command(Envelope { id: "n1".into(), base: Some(0), cmd: Command::Next }).ok);
        let stage = Stage::open(app, None).unwrap();
        stage.ui.window().set_size(slint::LogicalSize::new(1280.0, 720.0));
        assert!(stage.ui.get_use_cells(), "Classic switches to its boxes for a picture slide");
        let cells: Vec<StageCellVm> = stage.ui.get_cells().iter().collect();
        assert!(cells.iter().any(|c| c.has_picture && c.picture.size().width > 0), "the slide is drawn, not named");
        assert!(cells.iter().all(|c| c.text != "(slide picture)"));
        stage.ui.hide().unwrap();
    }

    #[test]
    fn a_designed_slide_shows_itself_and_plays_its_clicks_on_stage() {
        use crate::design::{Anim, AnimKind, Design, Effect, Obj, ObjKind, PctRect, Shape, Start};
        i_slint_backend_testing::init_no_event_loop();
        let app = test_app();
        let shape = |x| { let mut o = Obj::new(ObjKind::Shape { shape: Shape::Rect }, PctRect::new(x, 10.0, 20.0, 20.0)); o.look.fill = Some("#ff0000".into()); o };
        let (a, b) = (shape(10.0), shape(60.0));
        let mut fly = Anim::new(b.id, AnimKind::Entrance, Effect::Fly, Start::OnClick);
        fly.dur_ms = 200;
        let id = Uuid::new_v4();
        let kind = ItemKind::Design { title: "Talk".into(), slides: vec![Design { objects: vec![a, b], anims: vec![fly], ..Default::default() }] };
        app.data.lock().unwrap().services[0].items.push(ServiceItem { id, kind, background: None, notes: String::new(), theme: None });
        let cmd = |cmd| { let base = app.live_view().version; assert!(app.command(Envelope { id: Uuid::new_v4().to_string(), base: Some(base), cmd }).ok) };
        cmd(Command::Show { item_id: id, slide: 0 });
        let stage = Stage::open(app.clone(), None).unwrap();
        stage.ui.window().set_size(slint::LogicalSize::new(1280.0, 720.0));
        assert!(stage.ui.get_use_cells(), "Classic switches to its boxes for a designed slide");
        let cells: Vec<StageCellVm> = stage.ui.get_cells().iter().collect();
        let now = cells.iter().find(|c| c.anim).expect("the live slide's box");
        assert!(now.has_picture && now.picture.size().width > 0, "the slide is drawn, not its words");
        assert!(cells.iter().any(|c| c.has_picture && !c.anim && c.picture.size().width > 0), "so is the next one, still");
        assert!(!stage.ui.get_animating(), "nothing plays as it comes up");
        // The click plays in the current slide's box, once.
        cmd(Command::Next);
        let screen = Arc::new(StageScreen::default());
        show(&stage.ui, &screen, read(&app, LiveUpdate::new(app.live_view()), &screen));
        assert!(stage.ui.get_animating());
        assert_eq!(stage.ui.get_anim_sprites().row_count(), 2, "the still shape and the flying one");
        assert!(stage.ui.get_anim_base().size().width > 0);
        show(&stage.ui, &screen, read(&app, LiveUpdate::new(app.live_view()), &screen));
        assert!(stage.ui.get_animating(), "a redraw (the clock) does not stop it");
        std::thread::sleep(Duration::from_millis(300));
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(300));
        assert!(!stage.ui.get_animating(), "over: the picture shows");
        assert_eq!(stage.ui.get_anim_sprites().row_count(), 0);
        show(&stage.ui, &screen, read(&app, LiveUpdate::new(app.live_view()), &screen));
        assert!(!stage.ui.get_animating(), "it plays once");
        stage.ui.hide().unwrap();
    }

    #[test]
    fn classic_default_keeps_todays_screen() {
        i_slint_backend_testing::init_no_event_loop();
        let stage = Stage::open(app_with_layout(crate::stage::presets::CLASSIC), None).unwrap();
        assert!(!stage.ui.get_use_cells());
        assert_eq!(stage.ui.get_item_title(), "Welcome");
        stage.ui.hide().unwrap();
        // A layout id this version doesn't know is Classic too.
        let stage = Stage::open(app_with_layout(Uuid::from_u128(42)), None).unwrap();
        assert!(!stage.ui.get_use_cells());
        stage.ui.hide().unwrap();
    }

    #[test]
    fn stage_window_places_cells_from_layout() {
        i_slint_backend_testing::init_no_event_loop();
        let stage = Stage::open(app_with_layout(crate::stage::presets::BAND), None).unwrap();
        stage.ui.window().set_size(slint::LogicalSize::new(1280.0, 720.0));
        assert!(stage.ui.get_use_cells());
        let cells: Vec<StageCellVm> = stage.ui.get_cells().iter().collect();
        let now = cells.iter().find(|c| c.text == "Grace and peace to you").expect("current slide box");
        assert_eq!((now.x, now.y, now.w, now.h), (3.0, 13.0, 94.0, 50.0));
        assert!(now.size <= 10.0 && now.size >= 3.0);
        let next = cells.iter().find(|c| c.text == "Please stand").expect("next slide box");
        assert_eq!(next.caption, "NEXT");
        assert!(cells.iter().any(|c| c.text == "1 slide left"));
        // Empty boxes (section name for a non-song, media time, timer, message) are not drawn.
        assert!(cells.iter().all(|c| !c.text.is_empty()));
        let texts: Vec<String> = i_slint_backend_testing::ElementHandle::find_by_element_id(&stage.ui, "StageWindow::cell-t")
            .map(|e| e.accessible_label().unwrap_or_default().to_string()).collect();
        assert!(texts.iter().any(|t| t == "Grace and peace to you"), "{texts:?}");
        stage.ui.hide().unwrap();
    }

    #[test]
    fn church_layout_as_the_default_is_drawn() {
        i_slint_backend_testing::init_no_event_loop();
        let ours = crate::stage::StageLayout {
            id: Uuid::new_v4(), name: "Ours".into(), background: "#102030".into(), font: String::new(),
            elements: vec![crate::lenient::Lenient::Known(crate::stage::StageElement {
                id: Uuid::new_v4(), kind: crate::stage::ElementKind::ItemTitle, rect: crate::stage::PctRect::new(10.0, 20.0, 50.0, 10.0), style: Default::default(), locked: false, hidden: false,
            })],
        };
        let app = app_with_layout(ours.id);
        app.data.lock().unwrap().stage_layouts.push(ours);
        let stage = Stage::open(app, None).unwrap();
        assert!(stage.ui.get_use_cells());
        let cells: Vec<StageCellVm> = stage.ui.get_cells().iter().collect();
        assert_eq!(cells.len(), 1);
        assert_eq!((cells[0].text.as_str(), cells[0].x, cells[0].y), ("Welcome", 10.0, 20.0));
        stage.ui.hide().unwrap();
    }

    #[test]
    fn every_preset_opens_and_draws_its_boxes() {
        i_slint_backend_testing::init_no_event_loop();
        for l in crate::stage::presets::builtins().iter().skip(1) {
            let stage = Stage::open(app_with_layout(l.id), None).unwrap();
            assert!(stage.ui.get_use_cells(), "{}", l.name);
            let cells: Vec<StageCellVm> = stage.ui.get_cells().iter().collect();
            assert!(!cells.is_empty(), "{}", l.name);
            stage.ui.hide().unwrap();
        }
        let stage = Stage::open(app_with_layout(crate::stage::presets::CLOCK_COUNTDOWN), None).unwrap();
        let clock = stage.ui.get_cells().iter().next().expect("clock box");
        assert_eq!((clock.x, clock.y, clock.w, clock.h), (5.0, 8.0, 90.0, 40.0));
        assert!(clock.text.ends_with("AM") || clock.text.ends_with("PM"), "{}", clock.text);
        stage.ui.hide().unwrap();
    }

    /// The Band layout with a slide of chords live (`chords` as typed in the song), at 1280 × 720.
    fn band_with_chords(chords: &str) -> (std::rc::Rc<Stage>, Vec<StageCellVm>) {
        let app = app_with_layout(crate::stage::presets::BAND);
        let stage = Stage::open(app.clone(), None).unwrap();
        stage.ui.window().set_size(slint::LogicalSize::new(1280.0, 720.0));
        let mut v = app.live_view();
        let slide = v.slide.as_mut().unwrap();
        slide.chords = Some(chords.into());
        slide.content = SlideContent::Text { text: crate::slides::strip_chords(chords).unwrap(), footer: None, reference: None, size: 0, parallel: None };
        let screen = StageScreen::default();
        show(&stage.ui, &screen, Shown { live: LiveUpdate::new(v), layout: crate::stage::presets::by_id(crate::stage::presets::BAND).unwrap().clone(), slides: vec![], timers: Default::default(), next_picture: None, live_picture: None, group: None });
        let cells = stage.ui.get_cells().iter().collect();
        (stage, cells)
    }

    #[test]
    fn the_band_layout_shows_chords_above_the_words() {
        i_slint_backend_testing::init_no_event_loop();
        let verse = "[D]  Sometimes the [A/D]night is beautiful\n[G/B]  Sometimes it seemed to stoop so [Asus]close\nYou could [A7]touch it, but your heart would [F#m]break\n[D]      [G/D]    [A/D]";
        let (stage, cells) = band_with_chords(verse);
        let cell = cells.iter().find(|c| c.sheet).expect("the current slide's box is a chord sheet");
        assert_eq!(cell.text, verse, "the chords as written, for screen readers");
        let pieces: Vec<ChordPieceVm> = cell.pieces.iter().collect();
        let at = |t: &str| pieces.iter().find(|p| p.text == t).unwrap_or_else(|| panic!("no {t:?} in {pieces:?}")).clone();
        // Each chord starts exactly over its letter, on the row above.
        for (chord, word) in [("A/D", "night "), ("Asus", "close"), ("A7", "touch "), ("F#m", "break")] {
            let (c, w) = (at(chord), at(word));
            assert!(c.chord && !w.chord);
            assert_eq!(c.x, w.x, "{chord} over {word}");
            assert!(c.y < w.y, "{chord} above {word}");
        }
        assert!(pieces.iter().all(|p| !p.text.contains('[')), "no brackets");
        // The intro line of chords alone: three chords on one row, left to right, and no words.
        let alone: Vec<&ChordPieceVm> = pieces.iter().filter(|p| ["G/D", "A/D"].contains(&p.text.as_str()) && p.y == at("G/D").y).collect();
        assert_eq!(alone.len(), 2);
        assert!(alone[0].x < alone[1].x);
        // Chords never overlap: on each row, a chord starts after the one before it ends.
        let mut chords: Vec<&ChordPieceVm> = pieces.iter().filter(|p| p.chord).collect();
        chords.sort_by(|a, b| (a.y, a.x).partial_cmp(&(b.y, b.x)).unwrap());
        for pair in chords.windows(2) { if pair[0].y == pair[1].y { assert!(pair[1].x > pair[0].x, "{pair:?}") } }
        // The whole sheet fits the box.
        let (ww, wh) = (stage.ui.get_window_width(), stage.ui.get_window_height());
        let vh = wh / 100.0;
        assert!(cell.sheet_w <= cell.w * ww / 100.0 - 2.0 * PAD * vh + 0.5, "{} wide", cell.sheet_w);
        assert!(cell.sheet_h <= cell.h * vh - 2.0 * PAD * vh, "{} high", cell.sheet_h);
        assert_eq!(cell.chord_color, slint::Color::from_rgb_u8(0xf5, 0xc6, 0x6b), "gold chords over white words");
        assert!((cell.chord_size - cell.size * CHORD_SCALE).abs() < 1e-4);
        let drawn: Vec<String> = i_slint_backend_testing::ElementHandle::find_by_element_id(&stage.ui, "StageWindow::sheet-t")
            .map(|e| e.accessible_label().unwrap_or_default().to_string()).collect();
        assert_eq!(drawn.len(), 1, "drawn as a sheet, not as text: {drawn:?}");
        stage.ui.hide().unwrap();
    }

    #[test]
    fn a_long_chord_sheet_gets_smaller_and_wraps_rather_than_being_cut_off() {
        i_slint_backend_testing::init_no_event_loop();
        let line = "[G]Amazing grace how [C]sweet the sound that [G]saved a wretch like me, I [D]once was lost but now am found, was [G]blind but now I see";
        let long = [line; 8].join("\n");
        let (stage, cells) = band_with_chords(&long);
        let cell = cells.iter().find(|c| c.sheet).unwrap();
        assert!(cell.size < 10.0, "smaller: {}", cell.size);
        let (ww, wh) = (stage.ui.get_window_width(), stage.ui.get_window_height());
        let vh = wh / 100.0;
        assert!(cell.sheet_w <= cell.w * ww / 100.0 - 2.0 * PAD * vh + 0.5);
        assert!(cell.sheet_h <= cell.h * vh - 2.0 * PAD * vh);
        stage.ui.hide().unwrap();
    }

    #[test]
    fn long_words_shrink_to_fit_their_box() {
        i_slint_backend_testing::init_no_event_loop();
        let app = app_with_layout(crate::stage::presets::BAND);
        let stage = Stage::open(app.clone(), None).unwrap();
        stage.ui.window().set_size(slint::LogicalSize::new(1280.0, 720.0));
        let mut v = app.live_view();
        let long = "Amazing grace how sweet the sound that saved a wretch like me, I once was lost but now am found, was blind but now I see. ".repeat(3);
        v.slide.as_mut().unwrap().content = SlideContent::Text { text: long.clone(), footer: None, reference: None, size: 0, parallel: None };
        let screen = StageScreen::default();
        show(&stage.ui, &screen, Shown { live: LiveUpdate::new(v), layout: crate::stage::presets::by_id(crate::stage::presets::BAND).unwrap().clone(), slides: vec![], timers: Default::default(), next_picture: None, live_picture: None, group: None });
        let cell = stage.ui.get_cells().iter().find(|c| c.text == long.as_str()).unwrap();
        assert!(cell.size < 10.0, "long words get smaller: {}", cell.size);
        stage.ui.hide().unwrap();
    }

    /// A page of notes (Jake, November 15, 2026: "sometimes notes cut off when there are too much").
    fn long_notes() -> String {
        "Hold the last chorus twice, then the band drops out and we sing it once with just the keys. Watch for the pastor's nod before the bridge; \
         if he walks to the front, go straight to the outro and keep it soft for the prayer. ".repeat(10)
    }

    /// Shows `layout` with `notes` on the live slide, at 1280 × 720.
    fn stage_with_notes(layout: Uuid, notes: &str) -> std::rc::Rc<Stage> {
        let app = app_with_layout(layout);
        let stage = Stage::open(app.clone(), None).unwrap();
        stage.ui.window().set_size(slint::LogicalSize::new(1280.0, 720.0));
        let mut v = app.live_view();
        v.slide.as_mut().unwrap().notes = notes.into();
        let screen = StageScreen::default();
        show(&stage.ui, &screen, Shown { live: LiveUpdate::new(v), layout: crate::stage::presets::by_id(layout).unwrap().clone(), slides: vec![], timers: Default::default(),
            next_picture: None, live_picture: None, group: None });
        stage
    }

    #[test]
    fn long_notes_in_a_box_are_made_smaller_until_all_of_them_fit() {
        i_slint_backend_testing::init_no_event_loop();
        let notes = long_notes();
        let mut checked = vec![];
        for layout in [crate::stage::presets::SLIDES_NOTES, crate::stage::presets::SPEAKER] {
            let stage = stage_with_notes(layout, &notes);
            let Some(cell) = stage.ui.get_cells().iter().find(|c| c.text == notes.as_str()) else { stage.ui.hide().unwrap(); continue };
            // Measured as the box measures: its room less padding (and the caption above, if any).
            let (ww, wh) = (stage.ui.get_window_width(), stage.ui.get_window_height());
            let vh = wh / 100.0;
            let aw = cell.w * ww / 100.0 - 2.0 * PAD * vh;
            let ah = cell.h * vh - 2.0 * PAD * vh - if cell.caption.is_empty() { 0.0 } else { CAPTION * vh };
            stage.ui.set_probe_text(notes.as_str().into());
            stage.ui.set_probe_width(aw);
            stage.ui.set_probe_bold(cell.bold);
            stage.ui.set_probe_size(cell.size);
            assert!(stage.ui.get_probe_height() <= ah, "{layout}: the notes at {} % are {} px high in a {ah} px box: cut off",
                cell.size, stage.ui.get_probe_height());
            eprintln!("REAL notes box {layout}: {} % of the height", cell.size);
            checked.push(layout);
            stage.ui.hide().unwrap();
        }
        assert!(checked.contains(&crate::stage::presets::SLIDES_NOTES), "the Slides and notes layout has a notes box: {checked:?}");
    }

    #[test]
    fn long_notes_on_the_classic_screen_fit_their_box() {
        i_slint_backend_testing::init_no_event_loop();
        let notes = long_notes();
        let stage = stage_with_notes(crate::stage::presets::CLASSIC, &notes);
        let size = stage.ui.get_notes_size();
        assert!(size < NOTES_SIZE, "a page of notes is made smaller: {size}");
        let (ww, wh) = (stage.ui.get_window_width(), stage.ui.get_window_height());
        let (vh, vw) = (wh / 100.0, ww / 100.0);
        stage.ui.set_probe_text(notes.as_str().into());
        stage.ui.set_probe_width(ww - 2.0 * 1.6 * vw);
        stage.ui.set_probe_bold(false);
        stage.ui.set_probe_size(size);
        assert!(stage.ui.get_probe_height() <= NOTES_MAX_H * vh - 2.4 * vh, "the notes at {size} % do not fit their box");
        stage.ui.hide().unwrap();
        // A short note stays at its usual size.
        let stage = stage_with_notes(crate::stage::presets::CLASSIC, "Slow, hold the last line");
        assert_eq!(stage.ui.get_notes_size(), NOTES_SIZE);
        stage.ui.hide().unwrap();
    }

    #[test]
    fn shapes_and_the_next_slide_picture_are_drawn_and_hidden_boxes_are_not() {
        use crate::stage::{ElementKind, ShapeKind, StageElement, PctRect};
        i_slint_backend_testing::init_no_event_loop();
        let el = |n: u128, kind, hidden| crate::lenient::Lenient::Known(StageElement {
            id: Uuid::from_u128(n), kind, rect: PctRect::new(0.0, 0.0, 40.0, 40.0), style: Default::default(), locked: false, hidden,
        });
        let ours = crate::stage::StageLayout {
            id: Uuid::new_v4(), name: "Ours".into(), background: "#000000".into(), font: String::new(),
            elements: vec![el(1, ElementKind::Shape { shape: ShapeKind::Ellipse }, false), el(2, ElementKind::NextPicture, false), el(3, ElementKind::ItemTitle, true)],
        };
        let app = app_with_layout(ours.id);
        app.data.lock().unwrap().stage_layouts.push(ours);
        let stage = Stage::open(app, None).unwrap();
        let cells: Vec<StageCellVm> = stage.ui.get_cells().iter().collect();
        assert_eq!(cells.len(), 2, "the hidden title is not drawn");
        assert_eq!((cells[0].shape, cells[0].has_picture), (2, false));
        assert_eq!((cells[1].shape, cells[1].has_picture, cells[1].caption.as_str()), (-1, true, "NEXT"));
        let size = cells[1].picture.size();
        assert_eq!((size.width, size.height), (640, 360));
        stage.ui.hide().unwrap();
    }

    #[test]
    fn stage_shows_every_bible_with_its_code() {
        let p = crate::scripture::Parallel {
            main_code: "KJV".into(),
            also: vec![crate::scripture::ParallelText { code: "WEB".into(), text: "For God so loved".into() }],
            layout: Default::default(),
        };
        let s = Slide::new("John 3:16", SlideContent::Text { text: "For God so loved".into(), footer: None, reference: None, size: 0, parallel: Some(p) });
        assert_eq!(label(&s), "KJV  For God so loved\n\nWEB  For God so loved");
    }

    #[test]
    fn the_stage_shows_the_slide_with_its_drawing_unless_turned_off() {
        use crate::live::drawing::{DrawCmd, Pen, PALETTE};
        let app = test_app();
        let phone = uuid::Uuid::new_v4();
        app.phones.lock().unwrap().push(crate::app::Phone { id: phone, name: "iPad".into(), hash: String::new(), last_seen: 0, may_draw: true });
        let (mut canvas, mut slide, renderer) = (crate::render::drawing::DrawingCanvas::new(), None, crate::ui::thumbs::TextRenderer::new());
        let mut panel = || drawing_panel(&app, &mut canvas, &mut slide, &renderer);
        assert!(panel().is_none(), "nothing drawn: no panel");
        app.draw(phone, &DrawCmd::Start { stroke: 1, color: PALETTE[1].into(), width: 40.0, pen: Pen::Pen, points: vec![[0.1, 0.5], [0.9, 0.5]], ink: Default::default() }).unwrap();
        let img = panel().expect("on by default");
        assert_eq!((img.width(), img.height()), PANEL);
        let mid = img.as_slice()[(180 * 640 + 320) as usize];
        assert_eq!((mid.r, mid.g, mid.b), (0xff, 0x3b, 0x30), "the red line over the (black) slide");
        app.data.lock().unwrap().settings.stage_drawings = false;
        assert!(panel().is_none(), "Settings → Show drawings on the stage screen, off");
    }
}
