//! The App's side of drawing: phones draw (if allowed, within the limits when Settings → "Limit phone drawing" is
//! on, and always under the hard ceilings of `Drawing::apply_at`), fading ink is taken away when its time is up,
//! the operator clears, and each paired phone's "May draw on slides" switch.
use super::DrawCmd;
use crate::app::{ActionError, App, Update};
use crate::live::Live;
use crate::model::Settings;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, atomic::Ordering};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// "Limit phone drawing": the defaults, and the ranges the settings are kept to.
pub const MAX_STROKES_DEFAULT: u32 = 100;
pub const MAX_PER_SECOND_DEFAULT: u32 = 30;
pub const MAX_STROKES_RANGE: std::ops::RangeInclusive<u32> = 1..=10_000;
pub const MAX_PER_SECOND_RANGE: std::ops::RangeInclusive<u32> = 1..=200;
/// How often faded ink is looked for while some is on the slide.
const FADE_TICK: Duration = Duration::from_millis(100);

/// Why a phone may not draw (its switch is off in the Phone remote dialog).
pub fn not_allowed() -> String { crate::tr!("This phone may not draw on slides. Ask the operator to allow it.") }
pub fn too_many_lines() -> String { crate::tr!("The drawing is full. Clear it to draw more.") }
pub fn too_fast() -> String { crate::tr!("Drawing too fast. Slow down a little.") }

/// Each phone's drawing updates in the current second: (when the second began, how many).
static RATE: LazyLock<Mutex<HashMap<Uuid, (Instant, u32)>>> = LazyLock::new(Default::default);

/// One more update from `phone` at `now`: false when that is more than `max` in a second.
fn within_rate(phone: Uuid, max: u32, now: Instant) -> bool {
    let mut rate = RATE.lock().unwrap();
    let e = rate.entry(phone).or_insert((now, 0));
    if now.saturating_duration_since(e.0) >= Duration::from_secs(1) { *e = (now, 0) }
    e.1 += 1;
    e.1 <= max
}

/// Puts the "fading ink disappears after" setting on the live drawing.
pub fn sync_fade(live: &mut Live, settings: &Settings) {
    let after = Duration::from_secs(settings.fade_secs.clamp(*super::FADE_SECS_RANGE.start(), *super::FADE_SECS_RANGE.end()).into());
    if live.drawing.fade_after != after { Arc::make_mut(&mut live.drawing).fade_after = after }
}

impl App {
    /// A drawing command from paired phone `phone`. Every screen hears of a change as `Update::Drawing`.
    pub fn draw(self: &Arc<Self>, phone: Uuid, cmd: &DrawCmd) -> Result<(), String> { self.draw_at(phone, cmd, Instant::now()) }

    /// `draw`, at `now` (the speed limit counts from it).
    pub fn draw_at(self: &Arc<Self>, phone: Uuid, cmd: &DrawCmd, now: Instant) -> Result<(), String> {
        let may = self.phones.lock().unwrap().iter().find(|p| p.id == phone).map(|p| p.may_draw);
        match may {
            None => return Err(crate::tr!("Pair this phone first.")),
            Some(false) => return Err(not_allowed()),
            Some(true) => {}
        }
        let settings = self.data.lock().unwrap().settings.clone();
        // Limits only when the church turned them on; taking lines away is never limited.
        let limited = settings.drawing_limit && matches!(cmd, DrawCmd::Start { .. } | DrawCmd::More { .. });
        if limited && !within_rate(phone, settings.drawing_max_per_second.max(1), now) { return Err(too_fast()) }
        let (drawing, fading) = {
            let mut live = self.live.lock().unwrap();
            if limited && matches!(cmd, DrawCmd::Start { .. }) && live.drawing.strokes.len() >= settings.drawing_max_strokes.max(1) as usize {
                return Err(too_many_lines());
            }
            sync_fade(&mut live, &settings);
            // `apply` refuses before it changes anything.
            let changed = Arc::make_mut(&mut live.drawing).apply_at(phone, cmd, now)?;
            if !changed { return Ok(()) }
            (live.drawing.clone(), live.drawing.has_fading())
        };
        let _ = self.tx.send(Update::Drawing(drawing));
        if fading { self.fade_ticker() }
        Ok(())
    }

    /// Takes faded ink away now. True when any went (every screen is told).
    pub fn expire_ink(&self, now: Instant) -> bool {
        let drawing = {
            let mut live = self.live.lock().unwrap();
            let after = live.drawing.fade_after;
            if !live.drawing.strokes.iter().any(|s| s.opacity(now, after) <= 0.0) { return false }
            Arc::make_mut(&mut live.drawing).expire(now);
            live.drawing.clone()
        };
        let _ = self.tx.send(Update::Drawing(drawing));
        true
    }

    /// While fading ink is on the slide, a thread takes it away when its time is up. One at a time; it ends when
    /// none is left.
    fn fade_ticker(self: &Arc<Self>) {
        if self.fade_ticking.swap(true, Ordering::AcqRel) { return }
        let me = Arc::downgrade(self);
        let started = std::thread::Builder::new().name("drawing-fade".into()).spawn(move || loop {
            std::thread::sleep(FADE_TICK);
            let Some(app) = me.upgrade() else { return };
            app.expire_ink(Instant::now());
            let live = app.live.lock().unwrap();
            if !live.drawing.has_fading() { app.fade_ticking.store(false, Ordering::Release); return }
        });
        if started.is_err() { self.fade_ticking.store(false, Ordering::Release) }
    }

    /// The operator's Clear drawing. False when nothing was drawn.
    pub fn clear_drawing(&self) -> bool {
        let drawing = {
            let mut live = self.live.lock().unwrap();
            if !live.clear_drawing() { return false }
            live.drawing.clone()
        };
        let _ = self.tx.send(Update::Drawing(drawing));
        true
    }

    /// Turns "May draw on slides" on or off for a paired phone.
    pub fn set_may_draw(&self, phone: Uuid, on: bool) -> Result<(), String> {
        let mut phones = self.phones.lock().unwrap();
        let p = phones.iter_mut().find(|p| p.id == phone).ok_or_else(|| crate::tr!("That phone is no longer paired."))?;
        p.may_draw = on;
        self.save_phones(&phones);
        drop(phones);
        self.broadcast_data();
        Ok(())
    }
}

pub fn handles(action: &str) -> bool { matches!(action, "drawing/clear" | "phone/may_draw") }

pub fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        "drawing/clear" => Ok(json!({ "cleared": app.clear_drawing() })),
        _ => {
            #[derive(Deserialize)] struct May { id: Uuid, on: bool }
            let May { id, on } = serde_json::from_slice(body).map_err(|e| ActionError::bad(e.to_string()))?;
            app.set_may_draw(id, on).map_err(ActionError::bad)?;
            Ok(json!({}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Phone, test_app};
    use crate::live::drawing::{Pen, PALETTE};

    fn start(id: u32) -> DrawCmd { DrawCmd::Start { stroke: id, color: PALETTE[0].into(), width: 12.0, pen: Pen::Pen, points: vec![[0.5, 0.5]], ink: Default::default() } }
    fn phone(app: &App, may_draw: bool) -> Uuid {
        let id = Uuid::new_v4();
        app.phones.lock().unwrap().push(Phone { id, name: "iPad".into(), hash: String::new(), last_seen: 0, may_draw });
        id
    }

    #[test]
    fn only_paired_phones_allowed_to_draw_may_draw() {
        let app = test_app();
        let (yes, no) = (phone(&app, true), phone(&app, false));
        let mut rx = app.subscribe();
        assert_eq!(app.draw(yes, &start(1)), Ok(()));
        assert!(matches!(rx.try_recv(), Ok(Update::Drawing(d)) if d.strokes.len() == 1), "every screen is told");
        assert_eq!(app.draw(no, &start(1)), Err(not_allowed()));
        assert_eq!(app.draw(Uuid::new_v4(), &start(1)), Err("Pair this phone first.".into()));
        assert_eq!(app.live.lock().unwrap().drawing.strokes.len(), 1);
        // The switch, as the Phone remote dialog flips it.
        app.set_may_draw(no, true).unwrap();
        assert_eq!(app.draw(no, &start(1)), Ok(()));
        app.set_may_draw(yes, false).unwrap();
        assert_eq!(app.draw(yes, &DrawCmd::Undo), Err(not_allowed()));
        assert!(app.set_may_draw(Uuid::new_v4(), true).is_err());
    }

    #[test]
    fn new_and_old_phones_may_draw_by_default() {
        let old: Phone = serde_json::from_value(json!({ "id": Uuid::nil(), "name": "Old", "hash": "h", "last_seen": 1 })).unwrap();
        assert!(old.may_draw, "a phone paired before drawing existed");
        let off: Phone = serde_json::from_value(json!({ "id": Uuid::nil(), "name": "Off", "hash": "h", "last_seen": 1, "may_draw": false })).unwrap();
        assert!(!off.may_draw);
    }

    #[tokio::test]
    async fn the_operator_clears_the_drawing() {
        let app = test_app();
        let me = phone(&app, true);
        app.draw(me, &start(1)).unwrap();
        assert_eq!(app.do_action("drawing/clear", b"{}").await.ok().unwrap()["cleared"], true);
        assert!(app.live.lock().unwrap().drawing.is_empty());
        assert_eq!(app.do_action("drawing/clear", b"{}").await.ok().unwrap()["cleared"], false);
        app.do_action("phone/may_draw", json!({ "id": me, "on": false }).to_string().as_bytes()).await.ok().unwrap();
        assert!(!app.phones.lock().unwrap()[0].may_draw);
        assert!(app.live_view().drawing.is_empty());
    }

    #[test]
    fn the_keep_setting_reaches_the_live_engine() {
        let app = test_app();
        assert!(!app.live.lock().unwrap().keep_drawing, "off by default");
        app.edit(|d| { d.settings.keep_drawings = true; Ok(()) }).unwrap();
        assert!(app.live.lock().unwrap().keep_drawing);
    }

    fn more(id: u32) -> DrawCmd { DrawCmd::More { stroke: id, points: vec![[0.6, 0.6]] } }

    #[test]
    fn no_limits_unless_the_setting_is_on() {
        let app = test_app();
        let me = phone(&app, true);
        let now = Instant::now();
        assert!(!app.data.lock().unwrap().settings.drawing_limit, "off by default");
        for id in 0..500 { app.draw_at(me, &start(id), now).unwrap() }
        for _ in 0..500 { app.draw_at(me, &more(499), now).unwrap() }
        assert_eq!(app.live.lock().unwrap().drawing.strokes.len(), 500, "any number of lines, as fast as they come");
    }

    /// B9: the ceilings hold with "Limit phone drawing" off (the default).
    #[test]
    fn the_hard_ceilings_apply_without_the_optional_limit() {
        let app = test_app();
        let me = phone(&app, true);
        assert!(!app.data.lock().unwrap().settings.drawing_limit);
        let line = |n: usize| (0..n).map(|i| [i as f32 / n as f32, 0.5]).collect::<Vec<[f32; 2]>>();
        let long = DrawCmd::Start { stroke: 1, color: "#ffd60a".into(), width: 12.0, pen: crate::live::drawing::Pen::Pen, points: line(crate::live::drawing::MAX_POINTS_PER_REQUEST + 1), ink: Default::default() };
        assert_eq!(app.draw(me, &long), Err(too_many_lines()));
        assert!(app.live.lock().unwrap().drawing.is_empty(), "nothing was drawn");
        assert_eq!(app.draw(me, &start(1)), Ok(()));
    }

    #[test]
    fn with_limits_on_lines_on_screen_and_speed_are_capped() {
        let app = test_app();
        let (me, you) = (phone(&app, true), phone(&app, true));
        {
            let mut d = app.data.lock().unwrap();
            assert_eq!((d.settings.drawing_max_strokes, d.settings.drawing_max_per_second), (100, 30), "the defaults");
            d.settings.drawing_limit = true;
            d.settings.drawing_max_strokes = 3;
            d.settings.drawing_max_per_second = 5;
        }
        let t0 = Instant::now();
        for id in 0..3 { app.draw_at(me, &start(id), t0).unwrap() }
        assert_eq!(app.draw_at(you, &start(9), t0 + Duration::from_secs(5)), Err(too_many_lines()), "three lines on screen at most");
        app.draw_at(me, &DrawCmd::Undo, t0 + Duration::from_secs(5)).unwrap();
        app.draw_at(you, &start(9), t0 + Duration::from_secs(5)).unwrap();
        // Speed: 5 updates a second from one phone; the next second starts fresh, and another phone has its own.
        let t1 = t0 + Duration::from_secs(10);
        for _ in 0..5 { app.draw_at(you, &more(9), t1).unwrap() }
        assert_eq!(app.draw_at(you, &more(9), t1 + Duration::from_millis(500)), Err(too_fast()));
        app.draw_at(me, &DrawCmd::Undo, t1).unwrap();
        app.draw_at(me, &start(20), t1).unwrap();
        app.draw_at(you, &more(9), t1 + Duration::from_millis(1_100)).unwrap();
        // Bad input is refused whatever the setting.
        app.data.lock().unwrap().settings.drawing_limit = false;
        assert!(app.draw_at(you, &DrawCmd::More { stroke: 9, points: vec![[f32::NAN, 0.1]] }, t1).is_err());
    }

    #[test]
    fn faded_ink_is_taken_away_and_every_screen_told() {
        let app = test_app();
        let me = phone(&app, true);
        app.data.lock().unwrap().settings.fade_secs = 1;
        let t0 = Instant::now();
        app.draw_at(me, &start(1), t0).unwrap();
        let fade = DrawCmd::Start { stroke: 2, color: PALETTE[1].into(), width: 12.0, pen: Pen::Pen, points: vec![[0.5, 0.5]], ink: crate::live::drawing::Ink::Fades };
        app.draw_at(me, &fade, t0).unwrap();
        app.draw_at(me, &DrawCmd::End { stroke: 2 }, t0).unwrap();
        assert_eq!(app.live.lock().unwrap().drawing.fade_after, Duration::from_secs(1), "the setting");
        assert!(!app.expire_ink(t0 + Duration::from_millis(1_400)));
        let mut rx = app.subscribe();
        assert!(app.expire_ink(t0 + Duration::from_millis(1_500)));
        assert!(matches!(rx.try_recv(), Ok(Update::Drawing(d)) if d.strokes.len() == 1));
        // The fade thread does the same on its own, then stops.
        app.draw(me, &fade).unwrap();
        app.draw(me, &DrawCmd::End { stroke: 2 }).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while app.live.lock().unwrap().drawing.has_fading() && Instant::now() < end { std::thread::sleep(Duration::from_millis(20)) }
        assert!(!app.live.lock().unwrap().drawing.has_fading(), "gone after 1 s + the fade");
        assert_eq!(app.live.lock().unwrap().drawing.strokes.len(), 1, "the staying line is still there");
        std::thread::sleep(Duration::from_millis(300));
        assert!(!app.fade_ticking.load(Ordering::Acquire), "the thread ended");
    }
}
