//! The stage screen for the people on the platform: what is live, what comes next, notes,
//! the clock and the time left on a video (same content as `web/stage.html`).
//! Classic (the default) draws today's fixed screen; any other stage layout draws its boxes (`stage::view`).
use super::{StageCellVm, StageWindow};
use crate::{
    app::{App, LiveUpdate, LiveView, Update},
    model::Slide,
    platform::monitors::{self, MonitorId},
    stage::{self, presets, strings as s, view::{self, CellContent, StageInputs}, Align, StageLayout, StageState, VAlign},
    timers::TimersView,
};
use slint::{ComponentHandle, Model, Timer, TimerMode, VecModel};
use std::{collections::HashMap, rc::Rc, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::Duration};
use tokio::sync::broadcast::error::RecvError;

use crate::stage::view::label;

/// Everything the stage shows, worked out from one live view.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StageText { pub title: String, pub now: String, pub now_dim: bool, pub notes: String, pub next: String, pub timer: String, pub message: String }

pub fn stage_text(view: &LiveView) -> StageText {
    let f = &view.frame;
    let on_air = f.slide.is_some() || f.media.is_some();
    let now = match &view.slide {
        Some(s) if on_air => label(s),
        _ if f.logo => s::LOGO.into(),
        _ => String::new(),
    };
    let timer = view.media.as_ref().or(view.audio.as_ref()).filter(|m| m.duration > 0.0).map(|m| view::countdown(m.duration - m.position));
    StageText {
        title: view.item.as_ref().map_or(s::NOTHING_LIVE, |i| i.title.as_str()).into(),
        now,
        now_dim: !on_air,
        notes: view.slide.as_ref().map(|s| s.notes.clone()).unwrap_or_default(),
        next: view.next.as_ref().map_or_else(|| s::END_OF_SERVICE.into(), |n| label(&n.slide)),
        timer: timer.unwrap_or_default(),
        message: view.stage_message.clone(),
    }
}

/// The largest font size (in % of the height) that `fits`: 9 % down to 3 % in 0.4 % steps, like `stage.html`.
pub fn fit_size(fits: impl Fn(f32) -> bool) -> f32 { fit_between(9.0, 3.0, fits) }

/// The largest size from `max` down to `min` in 0.4 % steps that `fits`; `min` when none does.
pub fn fit_between(max: f32, min: f32, fits: impl Fn(f32) -> bool) -> f32 {
    let steps = ((max - min) / 0.4 + 1e-3).floor().max(0.0) as usize;
    (0..=steps).map(|k| max - 0.4 * k as f32).find(|s| fits(*s)).unwrap_or(min)
}

fn clock(t: chrono::NaiveTime) -> String { view::clock(t, false, false) }

/// What the window draws: the live view, the stage layout and the live item's slides (for "next section").
#[derive(Clone)]
struct Shown { live: LiveUpdate, layout: StageLayout, slides: Vec<Slide> }

/// The layout on stage is the church's stage default (Classic when it names no layout).
/// The church's own layouts are not stored yet (the stage layout editor adds them), so only the built-ins are found.
fn read(app: &App, live: LiveUpdate) -> Shown {
    let default = app.data.lock().unwrap().settings.stage.layout;
    let layout = stage::active_layout(&[], None, None, Some(default)).clone();
    let slides = if layout.id == presets::CLASSIC { vec![] } else { app.live.lock().unwrap().item.as_ref().map(|i| i.slides.clone()).unwrap_or_default() };
    Shown { live, layout, slides }
}

/// The window's own state: the last thing drawn (redrawn by the clock and on resize) and fitted sizes.
#[derive(Default)]
struct StageScreen { last: Mutex<Option<Shown>>, fits: Mutex<HashMap<FitKey, f32>>, state: Mutex<StageState> }

#[derive(Hash, PartialEq, Eq)]
struct FitKey { text: String, w: i32, h: i32, bold: bool, max: i32, min: i32 }

fn tick(ui: &StageWindow, screen: &StageScreen) {
    ui.set_clock(clock(chrono::Local::now().time()).into());
    if ui.get_use_cells() { redraw(ui, screen) }
}

fn fit(ui: &StageWindow) {
    let size = fit_size(|s| { ui.set_now_size(s); ui.get_now_fits() });
    ui.set_now_size(size);
}

fn show(ui: &StageWindow, screen: &StageScreen, shown: Shown) {
    let cells = shown.layout.id != presets::CLASSIC;
    ui.set_use_cells(cells);
    if cells { draw_cells(ui, screen, &shown) } else { show_classic(ui, &shown.live) }
    *screen.last.lock().unwrap() = Some(shown);
}

fn redraw(ui: &StageWindow, screen: &StageScreen) {
    let last = screen.last.lock().unwrap().clone();
    if let Some(shown) = last { if ui.get_use_cells() { draw_cells(ui, screen, &shown) } else { fit(ui) } }
}

fn show_classic(ui: &StageWindow, view: &LiveView) {
    let t = stage_text(view);
    ui.set_item_title(t.title.into());
    ui.set_now(t.now.into());
    ui.set_now_dim(t.now_dim);
    ui.set_notes(t.notes.into());
    ui.set_next(t.next.into());
    ui.set_timer(t.timer.into());
    ui.set_message(t.message.into());
    fit(ui);
}

fn color(c: &str) -> slint::Color { let [r, g, b, a] = crate::render::parse_color(c); slint::Color::from_argb_u8(a, r, g, b) }

/// Draw the layout's boxes. Rows that did not change are left alone.
fn draw_cells(ui: &StageWindow, screen: &StageScreen, shown: &Shown) {
    // FEAT-2 timers are not published by the app yet: timer boxes stay empty until they are.
    let timers = TimersView::default();
    let mut state = screen.state.lock().unwrap();
    state.track_timers(&timers);
    let inputs = StageInputs { live: &shown.live, slides: &shown.slides, timers: &timers, state: &state, now: chrono::Local::now().time() };
    let v = view::build(&inputs, &shown.layout);
    drop(state);
    ui.set_bg(color(&v.background));
    ui.set_banner(v.message_fallback.unwrap_or_default().into());
    let rows: Vec<StageCellVm> = v.cells.iter().filter_map(|c| cell_vm(ui, screen, c)).collect();
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
fn cell_vm(ui: &StageWindow, screen: &StageScreen, c: &view::Cell) -> Option<StageCellVm> {
    let (text, caption, dim, red, message) = match &c.content {
        CellContent::Empty => return None,
        CellContent::Text { text, dim, caption, red } => (text.clone(), caption.clone().unwrap_or_default(), *dim, *red, false),
        // Pictures on stage are not drawn yet: the box names the slide instead.
        CellContent::Picture { caption, .. } => (s::SLIDE_PICTURE.to_string(), caption.clone().unwrap_or_default(), false, false, false),
        CellContent::Message { text } => (text.clone(), String::new(), false, false, true),
    };
    let st = &c.style;
    let bold = st.bold || message;
    let size = fit_cell(ui, screen, &text, c, !caption.is_empty(), bold);
    Some(StageCellVm {
        x: c.rect.x, y: c.rect.y, w: c.rect.w, h: c.rect.h,
        text: text.into(),
        caption: caption.to_uppercase().into(),
        color: color(&st.color),
        fill: st.fill.as_deref().map_or(slint::Color::from_argb_u8(0, 0, 0, 0), color),
        size,
        align: match st.align { Align::Left => 0, Align::Center => 1, Align::Right => 2 },
        valign: match st.valign { VAlign::Top => 0, VAlign::Middle => 1, VAlign::Bottom => 2 },
        bold, dim, red, message,
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

/// An open stage window. It follows the live view until it is dropped.
pub struct Stage { ui: StageWindow, _clock: Timer, closed: Arc<AtomicBool> }

impl Stage {
    /// Open the stage window, full screen on `monitor` (a plain window without one).
    pub fn open(app: Arc<App>, monitor: Option<MonitorId>) -> Result<Rc<Stage>, String> {
        let ui = StageWindow::new().map_err(|e| format!("Could not open the stage screen: {e}"))?;
        // Slint picks the computer's language for the first window; show the app's.
        crate::i18n::sync_slint();
        // Its frames never wait for the monitor's refresh, so the room's are not held up behind them.
        super::pacing::attach(ui.window(), "stage", false);
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
        show(&ui, &screen, read(&app, LiveUpdate::new(app.live_view())));
        ui.set_connected(true);
        ui.show().map_err(|e| format!("Could not open the stage screen: {e}"))?;
        if let Some(m) = &monitor {
            if let Err(e) = monitors::fullscreen_on(&ui, m) { let _ = ui.hide(); return Err(e); }
        }
        let closed = Arc::new(AtomicBool::new(false));
        follow(app, rx, ui.as_weak(), screen, closed.clone());
        Ok(Rc::new(Stage { ui, _clock: clock, closed }))
    }

    pub fn window(&self) -> &slint::Window { self.ui.window() }
}

impl Drop for Stage {
    fn drop(&mut self) { self.closed.store(true, Ordering::Relaxed); let _ = self.ui.hide(); }
}

/// Forward live and data updates to the window from a plain thread (no tokio runtime needed on the UI side).
/// Only the newest view is drawn: updates that arrive while one is queued replace it.
fn follow(app: Arc<App>, mut rx: tokio::sync::broadcast::Receiver<Update>, ui: slint::Weak<StageWindow>, screen: Arc<StageScreen>, closed: Arc<AtomicBool>) {
    let latest: Arc<Mutex<Option<Shown>>> = Arc::default();
    let _ = std::thread::Builder::new().name("stage-updates".into()).spawn(move || loop {
        let update = match rx.blocking_recv() {
            Ok(Update::Live(u)) => u,
            // Settings may have a new stage layout.
            Ok(Update::Data) | Err(RecvError::Lagged(_)) => LiveUpdate::new(app.live_view()),
            Ok(_) => continue,
            Err(RecvError::Closed) => {
                let _ = ui.upgrade_in_event_loop(|ui| ui.set_connected(false));
                return;
            }
        };
        if closed.load(Ordering::Relaxed) { return; }
        let shown = read(&app, update);
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
            media: None, audio: None, notice: None, sections: vec![], changed: None, theme: Theme::default(), logo: None, clear_epoch: 0, stage_message: String::new(),
        }
    }

    #[test]
    fn stage_text_for_lyric_with_next() {
        let mut now = text("Amazing grace, how sweet the sound");
        now.notes = "Slow, hold the last line".into();
        let t = stage_text(&view(Some(now), Some(text("That saved a wretch like me"))));
        assert_eq!(t, StageText {
            title: "Amazing Grace".into(), now: "Amazing grace, how sweet the sound".into(), now_dim: false,
            notes: "Slow, hold the last line".into(), next: "That saved a wretch like me".into(), timer: String::new(), message: String::new(),
        });
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
            (SlideContent::Image { asset: "a.png".into() }, "(slide picture)"),
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
    fn fit_steps_down_to_three_percent() {
        assert_eq!(fit_size(|_| true), 9.0);
        assert!((fit_size(|s| s < 7.0) - 6.6).abs() < 1e-4);
        assert!((fit_size(|_| false) - 3.0).abs() < 1e-4);
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
            let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Grace and peace to you\n\nPlease stand".into() }, background: None, notes: String::new(), theme: None };
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
    fn fit_between_steps_from_the_boxes_biggest_size() {
        assert_eq!(fit_between(18.0, 3.0, |_| true), 18.0);
        assert!((fit_between(6.0, 3.0, |s| s < 5.0) - 4.8).abs() < 1e-4);
        assert_eq!(fit_between(6.0, 3.0, |_| false), 3.0);
        assert_eq!(fit_between(2.5, 2.5, |_| false), 2.5);
    }

    /// A text item live, its first slide on air, with `layout` as the stage default.
    fn app_with_layout(layout: Uuid) -> Arc<App> {
        let app = test_app();
        {
            let mut data = app.data.lock().unwrap();
            data.settings.stage.layout = layout;
            let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Grace and peace to you\n\nPlease stand".into() }, background: None, notes: String::new(), theme: None };
            data.services[0].items.push(item);
        }
        assert!(app.command(Envelope { id: "n1".into(), base: Some(0), cmd: Command::Next }).ok);
        app
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
        show(&stage.ui, &screen, Shown { live: LiveUpdate::new(v), layout: crate::stage::presets::by_id(crate::stage::presets::BAND).unwrap().clone(), slides: vec![] });
        let cell = stage.ui.get_cells().iter().find(|c| c.text == long.as_str()).unwrap();
        assert!(cell.size < 10.0, "long words get smaller: {}", cell.size);
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
}