//! FEAT-3 stage layouts, the pure core (no UI): boxes on the stage screen, what each can show,
//! the built-in layouts (`presets`) and the compositor that fills the boxes from live state (`view`).
pub mod geom;
pub mod lossless;
pub mod presets;
pub mod refs;
pub mod view;
#[path = "../strings/stage.rs"]
pub mod strings;

use crate::{lenient::Lenient, timers::{TimerSnapshot, TimerState, TimersView}};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// A stage layout: a background and boxes, drawn in order (later boxes on top).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageLayout {
    pub id: Uuid,
    pub name: String,
    #[serde(default = "default_background")]
    pub background: String,
    /// Empty = the bundled font.
    #[serde(default)]
    pub font: String,
    /// A box a newer version wrote stays `Unknown`: kept, never drawn, written back as it was.
    #[serde(default, deserialize_with = "lossless::lenient_vec")]
    pub elements: Vec<Lenient<StageElement>>,
}

pub const BACKGROUND: &str = "#07080a";
fn default_background() -> String { BACKGROUND.into() }

/// One box: what it shows, where, and how.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageElement {
    pub id: Uuid,
    pub kind: ElementKind,
    pub rect: PctRect,
    #[serde(default)]
    pub style: ElementStyle,
}

/// A box's place, in % of the window (0..=100).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct PctRect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }

impl PctRect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self { PctRect { x, y, w, h } }

    /// Inside the window and at least 2 % each way. A position that isn't a number is 0; a size, 30 × 15.
    pub fn sanitised(self) -> PctRect {
        let or = |v: f32, d: f32| if v.is_finite() { v } else { d };
        let (w, h) = (or(self.w, 30.0).clamp(2.0, 100.0), or(self.h, 15.0).clamp(2.0, 100.0));
        let (x, y) = (or(self.x, 0.0), or(self.y, 0.0));
        PctRect { x: x.clamp(0.0, 100.0 - w), y: y.clamp(0.0, 100.0 - h), w, h }
    }
}

/// How a box looks. `None` = the kind's default (`presets::kind_defaults`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct ElementStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Biggest text, % of the window height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<f32>,
    /// Smallest text before it clips, % of the window height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valign: Option<VAlign>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption_text: Option<String>,
    /// `None` = no fill (transparent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Align { Left, Center, Right }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VAlign { Top, Middle, Bottom }

/// What a box shows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ElementKind {
    /// The slide on air. `pictures` shows picture slides as pictures; `keep_after_clear` keeps dim words after Clear Slide.
    CurrentSlide { #[serde(default)] pictures: bool, #[serde(default)] keep_after_clear: bool },
    NextSlide { #[serde(default)] pictures: bool },
    /// The live song section ("Chorus").
    SectionName,
    /// The next song section that differs from the live one.
    NextSection,
    ItemTitle,
    /// "3 slides left" in the live item.
    SlidesLeft,
    Notes,
    Clock { #[serde(default)] seconds: bool, #[serde(default)] h24: bool },
    /// A FEAT-2 timer, by id or "the current one".
    Timer { timer: TimerRef },
    MediaTimeLeft,
    StageMessage,
    Text { text: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum TimerRef { Id { id: Uuid }, Current }

/// A style with every default filled in.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedStyle {
    pub color: String,
    pub max_size: f32,
    pub min_size: f32,
    pub align: Align,
    pub valign: VAlign,
    pub bold: bool,
    /// The caption's words when the caption is on.
    pub caption: Option<String>,
    pub fill: Option<String>,
}

/// The church's stage settings (`Settings.stage`). Old data loads as Classic.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct StageSettings {
    /// The stage default layout.
    pub layout: Uuid,
}

impl Default for StageSettings {
    fn default() -> Self { StageSettings { layout: presets::CLASSIC } }
}

/// Which layout is on stage: the "for now" choice, then the live item's, then the default, then Classic.
/// Ids that name no layout (built-in or the church's) are skipped.
pub fn active_layout<'a>(church: &'a [StageLayout], manual: Option<Uuid>, item: Option<Uuid>, default: Option<Uuid>) -> &'a StageLayout {
    active_layout_checked(church, manual, item, default).0
}

/// `active_layout`, plus the first asked-for id that names no layout (to say once that it was deleted).
pub fn active_layout_checked<'a>(church: &'a [StageLayout], manual: Option<Uuid>, item: Option<Uuid>, default: Option<Uuid>)
    -> (&'a StageLayout, Option<Uuid>) {
    let mut missing = None;
    for id in [manual, item, default].into_iter().flatten() {
        match presets::find_layout(church, id) {
            Some(l) => return (l, missing),
            None => { missing.get_or_insert(id); }
        }
    }
    (presets::classic(), missing)
}

/// Stage state that is not stored: which timer is "the current one".
#[derive(Clone, Debug, Default)]
pub struct StageState {
    /// The latched current timer (FEAT-3 spec §5.3).
    pub current_timer: Option<Uuid>,
    seen: HashMap<Uuid, TimerState>,
}

impl StageState {
    /// Follow a new timer readout: the most recently started "show on stage" timer becomes current and
    /// stays while running, paused or finished; it is let go when it is reset or another one starts.
    pub fn track_timers(&mut self, view: &TimersView) {
        for t in view.timers.iter().filter(|t| t.on_stage) {
            let started = matches!(self.seen.get(&t.id), Some(TimerState::Stopped | TimerState::Finished));
            if started && t.state == TimerState::Running { self.current_timer = Some(t.id); }
        }
        if let Some(id) = self.current_timer {
            let keep = view.timers.iter().any(|t| t.id == id && t.on_stage && t.state != TimerState::Stopped);
            if !keep { self.current_timer = None; }
        }
        self.seen = view.timers.iter().map(|t| (t.id, t.state)).collect();
    }

    /// The current timer: the latched one, else the first "show on stage" timer that isn't stopped.
    pub fn current<'a>(&self, view: &'a TimersView) -> Option<&'a TimerSnapshot> {
        self.current_timer.and_then(|id| view.timers.iter().find(|t| t.id == id))
            .or_else(|| view.timers.iter().find(|t| t.on_stage && t.state != TimerState::Stopped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timers::TimerKind;

    fn timer(n: u128, state: TimerState, on_stage: bool) -> TimerSnapshot {
        TimerSnapshot { id: Uuid::from_u128(n), name: format!("T{n}"), kind: TimerKind::CountUp, state, value_ms: 0, overrun: false, on_stage, on_remote: false }
    }
    fn view(ts: Vec<TimerSnapshot>) -> TimersView { TimersView { timers: ts, auto: None } }
    use TimerState::*;

    #[test]
    fn unknown_element_kind_kept_as_unknown_and_written_back() {
        let json = r#"{"id":"00000000-0000-0000-0000-000000000001","name":"Mine","elements":[
            {"id":"00000000-0000-0000-0000-000000000002","kind":{"type":"hologram","depth":3},"rect":{"x":1,"y":2,"w":3,"h":4}},
            {"id":"00000000-0000-0000-0000-000000000003","kind":{"type":"clock"},"rect":{"x":1,"y":2,"w":30,"h":10}}]}"#;
        let l: StageLayout = serde_json::from_str(json).unwrap();
        assert_eq!(l.background, BACKGROUND);
        assert!(matches!(l.elements[0], Lenient::Unknown(_)));
        assert_eq!(l.elements[1].known().unwrap().kind, ElementKind::Clock { seconds: false, h24: false });
        let back: serde_json::Value = serde_json::to_value(&l).unwrap();
        let orig: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(back["elements"][0], orig["elements"][0]);
    }

    #[test]
    fn unknown_align_or_timer_ref_makes_element_unknown_not_load_error() {
        let json = r#"{"id":"00000000-0000-0000-0000-000000000001","name":"Mine","elements":[
            {"id":"00000000-0000-0000-0000-000000000002","kind":{"type":"timer","timer":{"by":"name","name":"x"}},"rect":{"x":1,"y":2,"w":3,"h":4}},
            {"id":"00000000-0000-0000-0000-000000000003","kind":{"type":"notes"},"rect":{"x":1,"y":2,"w":3,"h":4},"style":{"align":"justify"}}]}"#;
        let l: StageLayout = serde_json::from_str(json).unwrap();
        assert!(l.elements.iter().all(|e| matches!(e, Lenient::Unknown(_))));
    }

    #[test]
    fn known_value_round_trips() {
        for l in presets::builtins() {
            let back: StageLayout = serde_json::from_str(&serde_json::to_string(l).unwrap()).unwrap();
            assert_eq!(&back, l);
        }
    }

    #[test]
    fn stage_settings_load_old_and_bad_data_as_classic() {
        use crate::model::Settings;
        let old: Settings = serde_json::from_str(r#"{"lines_per_slide":3}"#).unwrap();
        assert_eq!((old.stage.layout, old.lines_per_slide), (presets::CLASSIC, 3));
        let bad: Settings = serde_json::from_str(r#"{"lines_per_slide":3,"stage":{"layout":42}}"#).unwrap();
        assert_eq!((bad.stage.layout, bad.lines_per_slide), (presets::CLASSIC, 3));
        let s: Settings = serde_json::from_str(&format!(r#"{{"stage":{{"layout":"{}"}}}}"#, presets::BAND)).unwrap();
        assert_eq!(s.stage.layout, presets::BAND);
        assert_eq!(serde_json::to_value(&s).unwrap()["stage"]["layout"], presets::BAND.to_string());
    }

    #[test]
    fn unknown_field_in_known_box_kept_as_unknown() {
        use serde_json::json;
        let (id, el) = ("00000000-0000-0000-0000-000000000001", "00000000-0000-0000-0000-000000000002");
        let r = json!({"x": 0, "y": 0, "w": 10, "h": 10});
        let extended = [
            json!({"id": el, "kind": {"type": "clock", "seconds": true, "blink": true}, "rect": r}),
            json!({"id": el, "kind": {"type": "notes", "x": 1}, "rect": r}),
            json!({"id": el, "kind": {"type": "item_title"}, "rect": r, "style": {"shadow": 1}}),
            json!({"id": el, "kind": {"type": "item_title"}, "rect": {"x": 0, "y": 0, "w": 10, "h": 10, "r": 4}}),
            json!({"id": el, "kind": {"type": "item_title"}, "rect": r, "locked": true}),
        ];
        for e in extended {
            let l: StageLayout = serde_json::from_value(json!({"id": id, "name": "X", "elements": [e.clone()]})).unwrap();
            assert_eq!(l.elements, vec![Lenient::Unknown(e.clone())], "{e}");
            assert_eq!(serde_json::to_value(&l).unwrap()["elements"][0], e);
        }
        // No false alarms: null style fields and whole-number rects stay Known.
        let plain = json!({"id": el, "kind": {"type": "item_title"}, "rect": {"x": 3, "y": 2, "w": 44, "h": 6}, "style": {"color": null}});
        let l: StageLayout = serde_json::from_value(json!({"id": id, "name": "X", "elements": [plain]})).unwrap();
        assert!(matches!(l.elements[0], Lenient::Known(_)));
    }

    #[test]
    fn active_layout_finds_church_layouts_and_reports_missing() {
        let mut ours = presets::classic().clone();
        ours.id = Uuid::from_u128(77);
        let church = vec![ours];
        assert_eq!(active_layout(&church, None, None, Some(Uuid::from_u128(77))).id, Uuid::from_u128(77));
        let gone = Uuid::from_u128(404);
        let (l, missing) = active_layout_checked(&church, Some(gone), None, None);
        assert_eq!((l.id, missing), (presets::CLASSIC, Some(gone)));
        assert_eq!(active_layout_checked(&church, None, None, Some(presets::BAND)).1, None);
    }

    #[test]
    fn sanitise_clamps_and_fixes_nan() {
        assert_eq!(PctRect::new(f32::NAN, 0.0, f32::INFINITY, f32::NAN).sanitised(), PctRect::new(0.0, 0.0, 30.0, 15.0));
        assert_eq!(PctRect::new(95.0, -5.0, 1.0, 200.0).sanitised(), PctRect::new(95.0, 0.0, 2.0, 100.0));
        assert_eq!(PctRect::new(90.0, 90.0, 20.0, 20.0).sanitised(), PctRect::new(80.0, 80.0, 20.0, 20.0));
    }

    #[test]
    fn active_layout_manual_then_item_then_default_then_classic() {
        let (band, speaker) = (Some(presets::BAND), Some(presets::SPEAKER));
        assert_eq!(active_layout(&[], band, speaker, speaker).id, presets::BAND);
        assert_eq!(active_layout(&[], None, speaker, band).id, presets::SPEAKER);
        assert_eq!(active_layout(&[], None, None, band).id, presets::BAND);
        assert_eq!(active_layout(&[], None, None, None).id, presets::CLASSIC);
    }

    #[test]
    fn deleted_layout_falls_back() {
        let gone = Some(Uuid::from_u128(9));
        assert_eq!(active_layout(&[], gone, gone, Some(presets::SPEAKER)).id, presets::SPEAKER);
        assert_eq!(active_layout(&[], gone, None, gone).id, presets::CLASSIC);
    }

    #[test]
    fn current_timer_latches_most_recent_start() {
        let mut s = StageState::default();
        s.track_timers(&view(vec![timer(1, Stopped, true), timer(2, Stopped, true)]));
        assert_eq!(s.current_timer, None);
        s.track_timers(&view(vec![timer(1, Stopped, true), timer(2, Running, true)]));
        assert_eq!(s.current_timer, Some(Uuid::from_u128(2)));
        s.track_timers(&view(vec![timer(1, Running, true), timer(2, Running, true)]));
        assert_eq!(s.current_timer, Some(Uuid::from_u128(1)));
        let v = view(vec![timer(1, Running, true), timer(2, Running, true)]);
        s.track_timers(&v);
        assert_eq!(s.current(&v).unwrap().id, Uuid::from_u128(1));
    }

    #[test]
    fn current_timer_stays_when_paused_or_finished() {
        let mut s = StageState::default();
        s.track_timers(&view(vec![timer(1, Stopped, true), timer(2, Stopped, true)]));
        s.track_timers(&view(vec![timer(1, Running, true), timer(2, Stopped, true)]));
        s.track_timers(&view(vec![timer(1, Paused, true), timer(2, Stopped, true)]));
        assert_eq!(s.current_timer, Some(Uuid::from_u128(1)));
        s.track_timers(&view(vec![timer(1, Finished, true), timer(2, Stopped, true)]));
        assert_eq!(s.current_timer, Some(Uuid::from_u128(1)));
        // Restarted after finishing: still the current one.
        s.track_timers(&view(vec![timer(1, Running, true), timer(2, Stopped, true)]));
        assert_eq!(s.current_timer, Some(Uuid::from_u128(1)));
    }

    #[test]
    fn current_timer_released_on_reset_to_stopped() {
        let mut s = StageState::default();
        s.track_timers(&view(vec![timer(1, Stopped, true), timer(2, Paused, true)]));
        s.track_timers(&view(vec![timer(1, Running, true), timer(2, Paused, true)]));
        let v = view(vec![timer(1, Stopped, true), timer(2, Paused, true)]);
        s.track_timers(&v);
        assert_eq!(s.current_timer, None);
        // Nothing latched: the first one that isn't stopped.
        assert_eq!(s.current(&v).unwrap().id, Uuid::from_u128(2));
        assert!(s.current(&view(vec![timer(1, Stopped, true)])).is_none());
    }

    #[test]
    fn current_timer_ignores_not_on_stage() {
        let mut s = StageState::default();
        s.track_timers(&view(vec![timer(1, Stopped, false)]));
        let v = view(vec![timer(1, Running, false)]);
        s.track_timers(&v);
        assert_eq!(s.current_timer, None);
        assert!(s.current(&v).is_none());
    }
}
