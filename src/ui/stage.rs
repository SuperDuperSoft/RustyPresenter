//! The stage screen for the people on the platform: what is live, what comes next, notes,
//! the clock and the time left on a video (same content as `web/stage.html`).
use super::StageWindow;
use crate::{app::{App, LiveUpdate, LiveView, Update}, model::{Slide, SlideContent}, platform::monitors::{self, MonitorId}};
use slint::{ComponentHandle, Timer, TimerMode};
use std::{rc::Rc, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::Duration};
use tokio::sync::broadcast::error::RecvError;

/// Everything the stage shows, worked out from one live view.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StageText { pub title: String, pub now: String, pub now_dim: bool, pub notes: String, pub next: String, pub timer: String, pub message: String }

/// A slide as the stage names it: its text, or what kind of picture or media it is.
fn label(s: &Slide) -> String {
    match &s.content {
        SlideContent::Text { text, parallel, .. } => crate::model::all_bibles_text(text, parallel.as_ref()),
        SlideContent::Image { .. } => "(slide picture)".into(),
        SlideContent::Video { .. } => "(video)".into(),
        SlideContent::Still { .. } => "(picture)".into(),
        SlideContent::Audio { .. } => "(audio)".into(),
    }
}

/// `−m:ss`, never below zero.
fn countdown(left: f64) -> String { let s = left.max(0.0).round() as u64; format!("−{}:{:02}", s / 60, s % 60) }

pub fn stage_text(view: &LiveView) -> StageText {
    let f = &view.frame;
    let on_air = f.slide.is_some() || f.media.is_some();
    let now = match &view.slide {
        Some(s) if on_air => label(s),
        _ if f.logo => "(logo)".into(),
        _ => String::new(),
    };
    let timer = view.media.as_ref().or(view.audio.as_ref()).filter(|m| m.duration > 0.0).map(|m| countdown(m.duration - m.position));
    StageText {
        title: view.item.as_ref().map_or("Nothing live", |i| i.title.as_str()).into(),
        now,
        now_dim: !on_air,
        notes: view.slide.as_ref().map(|s| s.notes.clone()).unwrap_or_default(),
        next: view.next.as_ref().map_or_else(|| "End of service".into(), |n| label(&n.slide)),
        timer: timer.unwrap_or_default(),
        message: view.stage_message.clone(),
    }
}

/// The largest font size (in % of the height) that `fits`: 9 % down to 3 % in 0.4 % steps, like `stage.html`.
pub fn fit_size(fits: impl Fn(f32) -> bool) -> f32 {
    (0..=15).map(|k| 9.0 - 0.4 * k as f32).find(|s| fits(*s)).unwrap_or(3.0)
}

fn clock(t: chrono::NaiveTime) -> String { t.format("%-I:%M %p").to_string() }

fn tick(ui: &StageWindow) { ui.set_clock(clock(chrono::Local::now().time()).into()); }

fn fit(ui: &StageWindow) {
    let size = fit_size(|s| { ui.set_now_size(s); ui.get_now_fits() });
    ui.set_now_size(size);
}

fn show(ui: &StageWindow, view: &LiveView) {
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
        let weak = ui.as_weak();
        ui.on_refit(move || if let Some(ui) = weak.upgrade() { fit(&ui) });
        let clock = Timer::default();
        let weak = ui.as_weak();
        clock.start(TimerMode::Repeated, Duration::from_secs(1), move || if let Some(ui) = weak.upgrade() { tick(&ui) });
        tick(&ui);
        // Subscribe before reading the view so no update falls in between.
        let rx = app.subscribe();
        show(&ui, &app.live_view());
        ui.set_connected(true);
        ui.show().map_err(|e| format!("Could not open the stage screen: {e}"))?;
        if let Some(m) = &monitor {
            if let Err(e) = monitors::fullscreen_on(&ui, m) { let _ = ui.hide(); return Err(e); }
        }
        let closed = Arc::new(AtomicBool::new(false));
        follow(app, rx, ui.as_weak(), closed.clone());
        Ok(Rc::new(Stage { ui, _clock: clock, closed }))
    }

    pub fn window(&self) -> &slint::Window { self.ui.window() }
}

impl Drop for Stage {
    fn drop(&mut self) { self.closed.store(true, Ordering::Relaxed); let _ = self.ui.hide(); }
}

/// Forward live updates to the window from a plain thread (no tokio runtime needed on the UI side).
/// Only the newest view is drawn: updates that arrive while one is queued replace it.
fn follow(app: Arc<App>, mut rx: tokio::sync::broadcast::Receiver<Update>, ui: slint::Weak<StageWindow>, closed: Arc<AtomicBool>) {
    let latest: Arc<Mutex<Option<LiveUpdate>>> = Arc::default();
    let _ = std::thread::Builder::new().name("stage-updates".into()).spawn(move || loop {
        let update = match rx.blocking_recv() {
            Ok(Update::Live(u)) => u,
            Ok(_) => continue,
            Err(RecvError::Lagged(_)) => LiveUpdate::new(app.live_view()),
            Err(RecvError::Closed) => {
                let _ = ui.upgrade_in_event_loop(|ui| ui.set_connected(false));
                return;
            }
        };
        if closed.load(Ordering::Relaxed) { return; }
        if latest.lock().unwrap().replace(update).is_some() { continue; } // already queued: it will draw this one
        let latest = latest.clone();
        let sent = ui.upgrade_in_event_loop(move |ui| if let Some(u) = latest.lock().unwrap().take() { show(&ui, &u) });
        if sent.is_err() { return; }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::{LiveItemSummary, NextUp, test_app}, live::*, model::*};
    use slint::ComponentHandle;
    use uuid::Uuid;

    fn text(t: &str) -> Slide { Slide { label: t.into(), content: SlideContent::Text { text: t.into(), footer: None, reference: None, size: 0, parallel: None }, notes: String::new() } }
    fn slide(content: SlideContent) -> Slide { Slide { label: String::new(), content, notes: String::new() } }

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
            item: now.as_ref().map(|_| LiveItemSummary { item_id: Uuid::nil(), title: "Amazing Grace".into(), kind: "song".into(), count: 2 }),
            slide: now,
            next: next.map(|slide| NextUp { title: "Amazing Grace".into(), slide }),
            media: None, audio: None, notice: None, theme: Theme::default(), logo: None, clear_epoch: 0, stage_message: String::new(),
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
    fn stage_shows_every_bible_with_its_code() {
        let p = crate::scripture::Parallel {
            main_code: "KJV".into(),
            also: vec![crate::scripture::ParallelText { code: "WEB".into(), text: "For God so loved".into() }],
            layout: Default::default(),
        };
        let s = Slide { label: "John 3:16".into(), content: SlideContent::Text { text: "For God so loved".into(), footer: None, reference: None, size: 0, parallel: Some(p) }, notes: String::new() };
        assert_eq!(label(&s), "KJV  For God so loved\n\nWEB  For God so loved");
    }
}