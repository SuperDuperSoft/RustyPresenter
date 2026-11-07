//! FEAT-2 macros (spec §3.4, §4.2, §5.3): the stored steps, which steps change slides or only
//! overlays, and resolving every step's target up front so a deleted target runs nothing.
use crate::automation::messages::{self, MessageTemplate};
use crate::automation::strings;
use crate::lenient::Lenient;
use crate::timers::{Builtin, RunState, TimeOfDay, TimerDef, TimerKind, TimerRun, until_target};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

/// Which service item a ShowItem step puts up. Saved by number or title, so a macro keeps
/// working on next week's copy of the service.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemTarget {
    /// 1-based, as the volunteer counts.
    Number { n: usize },
    /// The first item with this title, ignoring case and outer spaces.
    Titled { title: String },
}

// Unit enums: serde allows `#[serde(other)]` only on the last variant, so the default comes last.

/// What a Clear step clears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClearWhich {
    Slide,
    Media,
    Audio,
    Overlays,
    #[default]
    #[serde(other)]
    All,
}

/// A macro button's colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroColor {
    Red,
    Orange,
    Green,
    Teal,
    Blue,
    Purple,
    Gray,
    #[default]
    #[serde(other)]
    Amber,
}

/// One saved macro step (spec §4.2). Stored as `Lenient<MacroStep>`, so a step from a newer
/// version loads and is written back unchanged. Jump, Refresh and RunMacro are never steps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum MacroStep {
    // Live steps: applied inside one Live batch.
    /// `slide` is 0-based; the editor shows `slide + 1`.
    ShowItem { target: ItemTarget, #[serde(default)] slide: usize },
    Next,
    Prev,
    Clear { which: ClearWhich },
    Logo,
    /// A FEAT-1 saved overlay.
    Overlay { id: Uuid, on: bool },
    /// FEAT-7: `screen` is the `OutputRole` name, such as `"room"` (Plan decision 9).
    ScreenView { screen: String, view: Uuid },
    TimerShow { timer: Uuid },
    TimerHide { timer: Uuid },
    MessageShow { template: Uuid, values: BTreeMap<String, String> },
    MessageHide,
    // Runtime steps: change the automation runtime inside the same checkpoint.
    TimerStart { timer: Uuid },
    TimerPause { timer: Uuid },
    TimerReset { timer: Uuid },
    TimerRestart { timer: Uuid },
    AutoAdvance { on: bool },
    // Action steps: FEAT-3 stage actions, run after the batch commits.
    StageMessageShow { text: String },
    StageMessageHide,
    StageLayout { layout: Uuid },
    StageLayoutAuto,
}

impl MacroStep {
    /// Changes slides or what a screen shows, so it needs the operator's fresh look.
    fn is_relative(&self) -> bool {
        use MacroStep::*;
        matches!(self, ShowItem { .. } | Next | Prev | Clear { .. } | Logo | ScreenView { .. })
    }

    /// Touches only overlays, timers, messages or the stage, never the slide.
    fn is_overlay_only(&self) -> bool {
        use MacroStep::*;
        matches!(self, TimerShow { .. } | TimerHide { .. } | MessageShow { .. } | MessageHide | Overlay { .. }
            | TimerStart { .. } | TimerPause { .. } | TimerReset { .. } | TimerRestart { .. }
            | StageMessageShow { .. } | StageMessageHide | StageLayout { .. } | StageLayoutAuto)
    }
}

/// A saved macro: a named button that runs its steps as one change.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Macro {
    pub id: Uuid,
    pub name: String,
    pub color: MacroColor,
    pub steps: Vec<Lenient<MacroStep>>,
    pub remote: bool,
    pub builtin: Option<Builtin>,
}

impl Macro {
    /// True if any step changes slides or a screen view. A step from a newer version counts,
    /// since nobody can tell what it does.
    pub fn is_relative(&self) -> bool {
        self.steps.iter().any(|s| s.known().is_none_or(MacroStep::is_relative))
    }

    /// True if every step only touches overlays, timers, messages or the stage.
    pub fn is_overlay_only(&self) -> bool {
        self.steps.iter().all(|s| s.known().is_some_and(MacroStep::is_overlay_only))
    }
}

/// When a resolved step runs (spec §5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepClass {
    /// Inside the one Live batch.
    Live,
    /// Changes the automation runtime, inside the same checkpoint.
    Runtime,
    /// A FEAT-3 action after the batch commits.
    Action,
}

/// A step with its target looked up and anything time-based worked out.
#[derive(Clone, Debug, PartialEq)]
pub enum PlannedStep {
    ShowItem { item_id: Uuid, slide: usize },
    Next,
    Prev,
    Clear(ClearWhich),
    Logo,
    Overlay { id: Uuid, on: bool },
    ScreenView { screen: String, view: Uuid },
    TimerShow { timer: Uuid },
    TimerHide { timer: Uuid },
    /// The filled text, or the reason it couldn't be filled (a run-time step failure, Plan decision 6).
    MessageShow { template: Uuid, filled: Result<String, String> },
    MessageHide,
    /// `target` is set for a countdown to a time that starts a new run.
    TimerStart { timer: Uuid, target: Option<DateTime<Utc>> },
    TimerPause { timer: Uuid },
    TimerReset { timer: Uuid },
    TimerRestart { timer: Uuid, target: Option<DateTime<Utc>> },
    AutoAdvance { on: bool },
    StageMessageShow { text: String },
    StageMessageHide,
    StageLayout { layout: Uuid },
    StageLayoutAuto,
}

impl PlannedStep {
    pub fn class(&self) -> StepClass {
        use PlannedStep::*;
        match self {
            ShowItem { .. } | Next | Prev | Clear(_) | Logo | Overlay { .. } | ScreenView { .. } | TimerShow { .. }
            | TimerHide { .. } | MessageShow { .. } | MessageHide => StepClass::Live,
            TimerStart { .. } | TimerPause { .. } | TimerReset { .. } | TimerRestart { .. } | AutoAdvance { .. } => {
                StepClass::Runtime
            }
            StageMessageShow { .. } | StageMessageHide | StageLayout { .. } | StageLayoutAuto => StepClass::Action,
        }
    }

    /// Puts something new on the screen, so a clear since the operator last looked skips it.
    pub fn is_show(&self) -> bool {
        matches!(self, PlannedStep::Overlay { on: true, .. } | PlannedStep::TimerShow { .. }
            | PlannedStep::MessageShow { .. })
    }
}

/// A resolved step and its 1-based number in the macro, for the toasts.
#[derive(Clone, Debug, PartialEq)]
pub struct Planned { pub number: usize, pub step: PlannedStep }

/// A macro ready to run: every target exists.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved { pub name: String, pub relative: bool, pub steps: Vec<Planned> }

/// Everything resolving needs, passed in so nothing here reads the App.
pub struct ResolveCtx<'a, Tz: TimeZone> {
    pub timers: &'a [TimerDef],
    pub runs: &'a HashMap<Uuid, TimerRun>,
    pub templates: &'a [MessageTemplate],
    pub overlays: &'a [Uuid],
    pub stage_layouts: &'a [Uuid],
    pub screen_views: &'a [Uuid],
    /// The current service's items, in order: id and title.
    pub items: &'a [(Uuid, String)],
    pub now: DateTime<Utc>,
    pub tz: &'a Tz,
    pub starts_at: Option<TimeOfDay>,
}

impl<Tz: TimeZone> ResolveCtx<'_, Tz> {
    fn item(&self, target: &ItemTarget) -> Option<Uuid> {
        match target {
            ItemTarget::Number { n } => self.items.get(n.checked_sub(1)?).map(|(id, _)| *id),
            ItemTarget::Titled { title } => {
                let want = title.trim().to_lowercase();
                self.items.iter().find(|(_, t)| t.trim().to_lowercase() == want).map(|(id, _)| *id)
            }
        }
    }

    fn timer(&self, id: Uuid) -> Option<&TimerDef> { self.timers.iter().find(|t| t.id == id) }

    /// The target a countdown to a time aims at, if `fresh_only` is false or its run is
    /// Stopped or Finished. `None` for any other kind.
    fn target(&self, def: &TimerDef, fresh_only: bool) -> Result<Option<DateTime<Utc>>, String> {
        let Some(TimerKind::UntilTime { target }) = def.kind.known() else { return Ok(None) };
        let state = self.runs.get(&def.id).map_or(&RunState::Stopped, |r| &r.state);
        if fresh_only && !matches!(state, RunState::Stopped | RunState::Finished) { return Ok(None) }
        until_target(target, self.starts_at, self.now, self.tz).map(Some)
    }
}

/// Checks every step's target in order and works out what each will do. The first problem
/// wins and nothing runs: a step from a newer version, a deleted target, or a refused time.
pub fn resolve<Tz: TimeZone>(m: &Macro, ctx: &ResolveCtx<Tz>) -> Result<Resolved, String> {
    let mut steps = Vec::with_capacity(m.steps.len());
    for (i, step) in m.steps.iter().enumerate() {
        let number = i + 1;
        let step = step.known().ok_or_else(|| strings::cant_run_newer(&m.name, number))?;
        let deleted = |thing: &str| strings::cant_run_deleted(&m.name, number, thing);
        let found = |ok: bool, thing: &str| if ok { Ok(()) } else { Err(deleted(thing)) };
        let timer = |id: Uuid| ctx.timer(id).ok_or_else(|| deleted("timer"));
        let planned = match step {
            MacroStep::ShowItem { target, slide } => {
                PlannedStep::ShowItem { item_id: ctx.item(target).ok_or_else(|| deleted("item"))?, slide: *slide }
            }
            MacroStep::Next => PlannedStep::Next,
            MacroStep::Prev => PlannedStep::Prev,
            MacroStep::Clear { which } => PlannedStep::Clear(*which),
            MacroStep::Logo => PlannedStep::Logo,
            MacroStep::Overlay { id, on } => {
                found(ctx.overlays.contains(id), "overlay")?;
                PlannedStep::Overlay { id: *id, on: *on }
            }
            MacroStep::ScreenView { screen, view } => {
                found(ctx.screen_views.contains(view), "screen view")?;
                PlannedStep::ScreenView { screen: screen.clone(), view: *view }
            }
            MacroStep::TimerShow { timer: id } => PlannedStep::TimerShow { timer: timer(*id)?.id },
            MacroStep::TimerHide { timer: id } => PlannedStep::TimerHide { timer: timer(*id)?.id },
            MacroStep::MessageShow { template, values } => {
                let t = ctx.templates.iter().find(|t| t.id == *template).ok_or_else(|| deleted("message"))?;
                PlannedStep::MessageShow { template: t.id, filled: messages::fill(&t.text, values) }
            }
            MacroStep::MessageHide => PlannedStep::MessageHide,
            MacroStep::TimerStart { timer: id } => {
                let def = timer(*id)?;
                PlannedStep::TimerStart { timer: def.id, target: ctx.target(def, true)? }
            }
            MacroStep::TimerPause { timer: id } => PlannedStep::TimerPause { timer: timer(*id)?.id },
            MacroStep::TimerReset { timer: id } => PlannedStep::TimerReset { timer: timer(*id)?.id },
            MacroStep::TimerRestart { timer: id } => {
                let def = timer(*id)?;
                PlannedStep::TimerRestart { timer: def.id, target: ctx.target(def, false)? }
            }
            MacroStep::AutoAdvance { on } => PlannedStep::AutoAdvance { on: *on },
            MacroStep::StageMessageShow { text } => PlannedStep::StageMessageShow { text: text.clone() },
            MacroStep::StageMessageHide => PlannedStep::StageMessageHide,
            MacroStep::StageLayout { layout } => {
                found(ctx.stage_layouts.contains(layout), "stage layout")?;
                PlannedStep::StageLayout { layout: *layout }
            }
            MacroStep::StageLayoutAuto => PlannedStep::StageLayoutAuto,
        };
        steps.push(Planned { number, step: planned });
    }
    Ok(Resolved { name: m.name.clone(), relative: m.is_relative(), steps })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::messages::{MessagePlace, MessageTemplate};
    use crate::automation::strings::no_start_time;
    use crate::lenient::Lenient;
    use crate::timers::{RunState, TimeTarget, TimerDef, TimerKind, TimerRun};
    use chrono::{DateTime, TimeZone, Utc};
    use std::collections::{BTreeMap, HashMap};
    use uuid::Uuid;

    const A: Uuid = Uuid::from_u128(0xa);
    const B: Uuid = Uuid::from_u128(0xb);
    const C: Uuid = Uuid::from_u128(0xc);
    const TIMER: Uuid = Uuid::from_u128(0x71);
    const TEMPLATE: Uuid = Uuid::from_u128(0x72);
    const OVERLAY: Uuid = Uuid::from_u128(0x73);
    const LAYOUT: Uuid = Uuid::from_u128(0x74);
    const VIEW: Uuid = Uuid::from_u128(0x75);
    const GONE: Uuid = Uuid::from_u128(0xdead);

    fn now() -> DateTime<Utc> { Utc.with_ymd_and_hms(2026, 11, 8, 15, 0, 0).unwrap() }

    fn walk_in(steps: Vec<MacroStep>) -> Macro {
        Macro { name: "Walk-in".into(), steps: steps.into_iter().map(Lenient::Known).collect(), ..Macro::default() }
    }

    struct World {
        timers: Vec<TimerDef>,
        runs: HashMap<Uuid, TimerRun>,
        templates: Vec<MessageTemplate>,
        overlays: Vec<Uuid>,
        layouts: Vec<Uuid>,
        views: Vec<Uuid>,
        items: Vec<(Uuid, String)>,
        starts_at: Option<crate::timers::TimeOfDay>,
    }

    impl World {
        fn new() -> Self {
            World {
                timers: vec![TimerDef { id: TIMER, name: "Sermon".into(), kind: TimerKind::Duration { secs: 60 }.into(),
                                        ..TimerDef::default() }],
                runs: HashMap::new(),
                templates: vec![MessageTemplate { id: TEMPLATE, name: "Plate".into(), text: "{Plate}".into(),
                                                  place: MessagePlace::Bottom, ..MessageTemplate::default() }],
                overlays: vec![OVERLAY],
                layouts: vec![LAYOUT],
                views: vec![VIEW],
                items: vec![(A, "Welcome".into()), (B, "Song".into()), (C, "welcome".into())],
                starts_at: None,
            }
        }

        fn resolve(&self, m: &Macro) -> Result<Resolved, String> {
            resolve(m, &ResolveCtx {
                timers: &self.timers, runs: &self.runs, templates: &self.templates, overlays: &self.overlays,
                stage_layouts: &self.layouts, screen_views: &self.views, items: &self.items, now: now(), tz: &Utc,
                starts_at: self.starts_at,
            })
        }
    }

    fn json(s: &str) -> serde_json::Value { serde_json::from_str(s).unwrap() }

    #[test]
    fn unknown_macro_step_loads_and_round_trips() {
        let m: Macro = serde_json::from_str(r#"{"steps":[{"step":"fade_audio","secs":3}]}"#).unwrap();
        assert_eq!(m.steps.len(), 1);
        assert!(matches!(m.steps[0], Lenient::Unknown(_)));
        assert_eq!(serde_json::to_value(&m).unwrap()["steps"], json(r#"[{"step":"fade_audio","secs":3}]"#));
    }

    #[test]
    fn jump_and_refresh_are_not_steps() {
        for src in [r#"{"step":"jump"}"#, r#"{"step":"refresh"}"#,
                    r#"{"step":"run_macro","macro_id":"52500000-0000-4000-8000-000000000021"}"#] {
            let s: Lenient<MacroStep> = serde_json::from_str(src).unwrap();
            assert!(matches!(s, Lenient::Unknown(_)), "{src}");
        }
    }

    #[test]
    fn step_json_shape() {
        let show = MacroStep::ShowItem { target: ItemTarget::Number { n: 1 }, slide: 0 };
        let src = r#"{"step":"show_item","target":{"type":"number","n":1},"slide":0}"#;
        assert_eq!(serde_json::to_value(&show).unwrap(), json(src));
        assert_eq!(serde_json::from_str::<MacroStep>(src).unwrap(), show);
        assert_eq!(serde_json::from_str::<MacroStep>(r#"{"step":"show_item","target":{"type":"number","n":1}}"#).unwrap(),
                   show);
        let clear = MacroStep::Clear { which: ClearWhich::Overlays };
        assert_eq!(serde_json::to_value(&clear).unwrap(), json(r#"{"step":"clear","which":"overlays"}"#));
        assert_eq!(serde_json::from_str::<MacroStep>(r#"{"step":"clear","which":"overlays"}"#).unwrap(), clear);
        assert_eq!(serde_json::from_str::<MacroStep>(r#"{"step":"clear","which":"sparkles"}"#).unwrap(),
                   MacroStep::Clear { which: ClearWhich::All });
        let titled = MacroStep::ShowItem { target: ItemTarget::Titled { title: "Welcome".into() }, slide: 2 };
        assert_eq!(serde_json::to_value(&titled).unwrap(),
                   json(r#"{"step":"show_item","target":{"type":"titled","title":"Welcome"},"slide":2}"#));
        let m: Macro = serde_json::from_str(r#"{"color":"chartreuse"}"#).unwrap();
        assert_eq!(m.color, MacroColor::Amber);
    }

    #[test]
    fn relative_and_overlay_only_classes() {
        let t = TIMER;
        // (step, relative, overlay-only)
        let rows: Vec<(MacroStep, bool, bool)> = vec![
            (MacroStep::ShowItem { target: ItemTarget::Number { n: 1 }, slide: 0 }, true, false),
            (MacroStep::Next, true, false),
            (MacroStep::Prev, true, false),
            (MacroStep::Clear { which: ClearWhich::All }, true, false),
            (MacroStep::Logo, true, false),
            (MacroStep::Overlay { id: OVERLAY, on: true }, false, true),
            (MacroStep::ScreenView { screen: "room".into(), view: VIEW }, true, false),
            (MacroStep::TimerShow { timer: t }, false, true),
            (MacroStep::TimerHide { timer: t }, false, true),
            (MacroStep::MessageShow { template: TEMPLATE, values: BTreeMap::new() }, false, true),
            (MacroStep::MessageHide, false, true),
            (MacroStep::TimerStart { timer: t }, false, true),
            (MacroStep::TimerPause { timer: t }, false, true),
            (MacroStep::TimerReset { timer: t }, false, true),
            (MacroStep::TimerRestart { timer: t }, false, true),
            (MacroStep::AutoAdvance { on: true }, false, false),
            (MacroStep::StageMessageShow { text: "Wrap up".into() }, false, true),
            (MacroStep::StageMessageHide, false, true),
            (MacroStep::StageLayout { layout: LAYOUT }, false, true),
            (MacroStep::StageLayoutAuto, false, true),
        ];
        for (step, relative, overlay_only) in rows {
            let m = walk_in(vec![step.clone()]);
            assert_eq!(m.is_relative(), relative, "relative {step:?}");
            assert_eq!(m.is_overlay_only(), overlay_only, "overlay-only {step:?}");
        }
        let unknown = Macro { steps: vec![Lenient::Unknown(json(r#"{"step":"fade_audio"}"#))], ..Macro::default() };
        assert!(unknown.is_relative());
        assert!(!unknown.is_overlay_only());
        let mixed = walk_in(vec![MacroStep::TimerShow { timer: t }, MacroStep::Next]);
        assert!(mixed.is_relative());
        assert!(!mixed.is_overlay_only());
    }

    #[test]
    fn item_target_by_number_and_title_case_insensitive() {
        let w = World::new();
        let item = |target: ItemTarget| {
            w.resolve(&walk_in(vec![MacroStep::ShowItem { target, slide: 0 }])).map(|r| r.steps[0].step.clone())
        };
        assert_eq!(item(ItemTarget::Titled { title: "  WELCOME ".into() }), Ok(PlannedStep::ShowItem { item_id: A, slide: 0 }));
        assert_eq!(item(ItemTarget::Number { n: 2 }), Ok(PlannedStep::ShowItem { item_id: B, slide: 0 }));
        let deleted = Err("Couldn't run “Walk-in”: step 1's item was deleted. Edit the macro.".to_string());
        assert_eq!(item(ItemTarget::Number { n: 0 }), deleted);
        assert_eq!(item(ItemTarget::Number { n: 4 }), deleted);
        assert_eq!(item(ItemTarget::Titled { title: "Sermon".into() }), deleted);
    }

    #[test]
    fn deleted_target_runs_nothing() {
        let w = World::new();
        let err = |step: MacroStep| w.resolve(&walk_in(vec![MacroStep::Next, step])).unwrap_err();
        let msg = |thing: &str| format!("Couldn't run “Walk-in”: step 2's {thing} was deleted. Edit the macro.");
        assert_eq!(err(MacroStep::TimerStart { timer: GONE }), msg("timer"));
        assert_eq!(err(MacroStep::TimerShow { timer: GONE }), msg("timer"));
        assert_eq!(err(MacroStep::MessageShow { template: GONE, values: BTreeMap::new() }), msg("message"));
        assert_eq!(err(MacroStep::Overlay { id: GONE, on: false }), msg("overlay"));
        assert_eq!(err(MacroStep::StageLayout { layout: GONE }), msg("stage layout"));
        assert_eq!(err(MacroStep::ScreenView { screen: "room".into(), view: GONE }), msg("screen view"));
        let two_bad = walk_in(vec![MacroStep::Logo, MacroStep::Overlay { id: GONE, on: true },
                                   MacroStep::TimerHide { timer: GONE }]);
        assert_eq!(w.resolve(&two_bad).unwrap_err(), msg("overlay"));
    }

    #[test]
    fn unknown_step_refuses_with_newer_message() {
        let w = World::new();
        let mut m = walk_in(vec![MacroStep::Next, MacroStep::Prev, MacroStep::Logo]);
        m.steps.push(Lenient::Unknown(json(r#"{"step":"fade_audio","secs":3}"#)));
        assert_eq!(w.resolve(&m).unwrap_err(), "Couldn't run “Walk-in”: step 4 needs a newer RustyPresenter.");
    }

    #[test]
    fn refused_timer_target_runs_nothing() {
        let mut w = World::new();
        w.timers[0].kind = TimerKind::UntilTime { target: TimeTarget::ServiceStart }.into();
        let m = walk_in(vec![MacroStep::TimerStart { timer: TIMER }]);
        assert_eq!(w.resolve(&m).unwrap_err(), no_start_time());
        let running = TimerRun { state: RunState::RunningTo { target: now() }, ..TimerRun::default() };
        w.runs.insert(TIMER, running);
        let r = w.resolve(&m).unwrap();
        assert_eq!(r.steps[0].step, PlannedStep::TimerStart { timer: TIMER, target: None });
        // Restart always computes a target for a countdown to a time.
        assert_eq!(w.resolve(&walk_in(vec![MacroStep::TimerRestart { timer: TIMER }])).unwrap_err(), no_start_time());
        w.starts_at = Some(crate::timers::TimeOfDay::new(16, 30));
        let r = w.resolve(&walk_in(vec![MacroStep::TimerRestart { timer: TIMER }])).unwrap();
        let at = Utc.with_ymd_and_hms(2026, 11, 8, 16, 30, 0).unwrap();
        assert_eq!(r.steps[0].step, PlannedStep::TimerRestart { timer: TIMER, target: Some(at) });
        // Start computes one too, from Stopped or Finished.
        w.runs.insert(TIMER, TimerRun { state: RunState::Finished, ..TimerRun::default() });
        let r = w.resolve(&walk_in(vec![MacroStep::TimerStart { timer: TIMER }])).unwrap();
        assert_eq!(r.steps[0].step, PlannedStep::TimerStart { timer: TIMER, target: Some(at) });
        // A countdown of a length never gets a target.
        w.timers[0].kind = TimerKind::Duration { secs: 60 }.into();
        let r = w.resolve(&walk_in(vec![MacroStep::TimerStart { timer: TIMER }, MacroStep::TimerRestart { timer: TIMER }]))
            .unwrap();
        assert_eq!(r.steps[0].step, PlannedStep::TimerStart { timer: TIMER, target: None });
        assert_eq!(r.steps[1].step, PlannedStep::TimerRestart { timer: TIMER, target: None });
    }

    #[test]
    fn show_item_step_slide_one_is_first_slide() {
        let w = World::new();
        let r = w.resolve(&walk_in(vec![MacroStep::ShowItem { target: ItemTarget::Number { n: 1 }, slide: 0 }])).unwrap();
        assert_eq!(r.steps, [Planned { number: 1, step: PlannedStep::ShowItem { item_id: A, slide: 0 } }]);
        assert_eq!(r.name, "Walk-in");
        assert!(r.relative);
    }

    #[test]
    fn message_step_fill_error_is_kept_for_run_time() {
        let w = World::new();
        let r = w.resolve(&walk_in(vec![MacroStep::MessageShow { template: TEMPLATE, values: BTreeMap::new() }])).unwrap();
        assert_eq!(r.steps[0].step,
                   PlannedStep::MessageShow { template: TEMPLATE, filled: Err("Fill in “Plate”.".to_string()) });
        let values: BTreeMap<String, String> = [("Plate".to_string(), " ABC 123 ".to_string())].into();
        let r = w.resolve(&walk_in(vec![MacroStep::MessageShow { template: TEMPLATE, values }])).unwrap();
        assert_eq!(r.steps[0].step, PlannedStep::MessageShow { template: TEMPLATE, filled: Ok("ABC 123".to_string()) });
        assert!(!r.relative);
    }

    #[test]
    fn planned_step_classes_and_show_flags() {
        use PlannedStep as P;
        use StepClass::{Action, Live, Runtime};
        let rows: Vec<(PlannedStep, StepClass, bool)> = vec![
            (P::ShowItem { item_id: A, slide: 0 }, Live, false),
            (P::Next, Live, false),
            (P::Prev, Live, false),
            (P::Clear(ClearWhich::Media), Live, false),
            (P::Logo, Live, false),
            (P::Overlay { id: OVERLAY, on: true }, Live, true),
            (P::Overlay { id: OVERLAY, on: false }, Live, false),
            (P::ScreenView { screen: "room".into(), view: VIEW }, Live, false),
            (P::TimerShow { timer: TIMER }, Live, true),
            (P::TimerHide { timer: TIMER }, Live, false),
            (P::MessageShow { template: TEMPLATE, filled: Ok("Hi".into()) }, Live, true),
            (P::MessageHide, Live, false),
            (P::TimerStart { timer: TIMER, target: None }, Runtime, false),
            (P::TimerPause { timer: TIMER }, Runtime, false),
            (P::TimerReset { timer: TIMER }, Runtime, false),
            (P::TimerRestart { timer: TIMER, target: None }, Runtime, false),
            (P::AutoAdvance { on: false }, Runtime, false),
            (P::StageMessageShow { text: "Wrap up".into() }, Action, false),
            (P::StageMessageHide, Action, false),
            (P::StageLayout { layout: LAYOUT }, Action, false),
            (P::StageLayoutAuto, Action, false),
        ];
        for (step, class, show) in rows {
            assert_eq!(step.class(), class, "{step:?}");
            assert_eq!(step.is_show(), show, "{step:?}");
        }
    }

    #[test]
    fn macro_strings_are_exact() {
        use crate::automation::strings::*;
        assert_eq!(ran("Walk-in"), "Ran “Walk-in”");
        assert_eq!(ran_but_step_failed("Walk-in", 6, "Fill in “Plate”."),
                   "Ran “Walk-in”, but step 6 didn't work: Fill in “Plate”.");
        assert_eq!(ran_but_skipped_shows("Walk-in"),
                   "Ran “Walk-in”, but skipped showing things because the screen was cleared.");
        assert_eq!(at_zero_changes_slides("Sermon", "Walk-in"),
                   "“Sermon” reached zero, but “Walk-in” changes slides, so it didn't run.");
        assert_eq!(at_zero_skipped_shows("Sermon", "Walk-in"),
                   "“Sermon” reached zero, but “Walk-in” didn't show anything because the screen was cleared.");
        assert_eq!(cant_run_deleted("Walk-in", 3, "timer"),
                   "Couldn't run “Walk-in”: step 3's timer was deleted. Edit the macro.");
        assert_eq!(cant_run_newer("Walk-in", 4), "Couldn't run “Walk-in”: step 4 needs a newer RustyPresenter.");
    }
}
