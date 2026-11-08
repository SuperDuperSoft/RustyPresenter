//! FEAT-2 pre-service countdown (spec §3.6, §5.7): the locked "Start countdown" macro built
//! from the Pre-service tab, and the end rule that only changes the screen if nobody else has.
use crate::automation::macros::{ItemTarget, Macro, MacroColor, MacroStep};
use crate::automation::strings;
use crate::lenient::Lenient;
use crate::timers::{AtZero, Builtin, OverlayPlace, Overrun, PRE_SERVICE_TIMER_ID, TimeTarget, TimerDef, TimerKind};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The fixed id of the locked "Start countdown" macro (Plan decision 11).
pub const PRE_SERVICE_MACRO_ID: Uuid = Uuid::from_u128(0x52500000_0000_4000_8000_000000000021);

/// What else happens when the pre-service countdown reaches zero. The countdown always hides.
// serde allows `#[serde(other)]` only on the last variant, so the default comes last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreServiceEnd {
    Logo,
    ClearAll,
    FirstItem,
    #[default]
    #[serde(other)]
    Hide,
}

/// The Pre-service tab's settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreService {
    pub loop_title: Option<String>,
    pub music_title: Option<String>,
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub place: OverlayPlace,
    pub caption: String,
    pub end: PreServiceEnd,
    /// N16: the stage screen's layout while the countdown runs; `None` leaves it as it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_layout: Option<Uuid>,
}

impl Default for PreService {
    fn default() -> Self {
        PreService {
            loop_title: None,
            music_title: None,
            place: OverlayPlace::TopRight,
            caption: strings::service_starts_in().into(),
            end: PreServiceEnd::Hide,
            stage_layout: None,
        }
    }
}

/// Where the countdown starts out: a corner over an announcement loop, large in the middle
/// on its own.
pub fn default_place(has_loop: bool) -> OverlayPlace {
    if has_loop { OverlayPlace::TopRight } else { OverlayPlace::CenterLarge }
}

/// The macro's steps: music first so it is playing when the loop appears, then the countdown.
pub fn build(p: &PreService) -> Vec<MacroStep> {
    let show = |title: &String| MacroStep::ShowItem { target: ItemTarget::Titled { title: title.clone() }, slide: 0 };
    let mut steps: Vec<MacroStep> = [&p.music_title, &p.loop_title].into_iter().flatten().map(show).collect();
    steps.push(MacroStep::TimerRestart { timer: PRE_SERVICE_TIMER_ID });
    steps.push(MacroStep::TimerShow { timer: PRE_SERVICE_TIMER_ID });
    if let Some(layout) = p.stage_layout { steps.push(MacroStep::StageLayout { layout }) }
    steps
}

/// The built-in Pre-service timer: counts down to the service start and stops at zero.
pub fn timer_def(p: &PreService) -> TimerDef {
    TimerDef {
        id: PRE_SERVICE_TIMER_ID,
        name: strings::pre_service_timer_name().into(),
        kind: Lenient::Known(TimerKind::UntilTime { target: TimeTarget::ServiceStart }),
        overrun: Overrun::Stop,
        at_zero: Lenient::Known(AtZero::Nothing),
        place: p.place,
        caption: p.caption.clone(),
        on_stage: false,
        on_remote: false,
        builtin: Some(Builtin::PreService),
    }
}

/// The locked "Start countdown" macro. The operator's button colour and phone-remote choice
/// survive a rebuild.
pub fn macro_def(p: &PreService, existing: Option<&Macro>) -> Macro {
    Macro {
        id: PRE_SERVICE_MACRO_ID,
        name: strings::start_countdown().into(),
        color: existing.map_or(MacroColor::Amber, |m| m.color),
        steps: build(p).into_iter().map(Lenient::Known).collect(),
        remote: existing.is_some_and(|m| m.remote),
        builtin: Some(Builtin::PreService),
    }
}

/// What the screen looked like when "Start countdown" ran, so the end can tell if the operator
/// has taken over since.
#[derive(Clone, Debug, PartialEq)]
pub struct PreRun {
    pub manual_version: u64,
    pub loop_item: Option<Uuid>,
    pub timer_run: u64,
}

/// The screen now, when the countdown reaches zero.
pub struct EndInput {
    pub timer_run: u64,
    pub manual_version: u64,
    pub cursor_item: Option<Uuid>,
    pub visual_live: bool,
}

/// What to do at zero besides hiding the countdown, which always happens.
#[derive(Debug, PartialEq)]
pub struct EndPlan {
    pub extra: Option<PreServiceEnd>,
    pub notice: Option<String>,
}

/// The guarded end: the extra action runs only if this is the run "Start countdown" began, the
/// operator hasn't changed anything by hand, and the loop is still up (or nothing visual is).
pub fn end_plan(end: PreServiceEnd, pre: Option<&PreRun>, input: &EndInput) -> EndPlan {
    if end == PreServiceEnd::Hide {
        return EndPlan { extra: None, notice: None };
    }
    let untouched = pre.is_some_and(|pre| {
        pre.timer_run == input.timer_run
            && pre.manual_version == input.manual_version
            && ((pre.loop_item.is_some() && input.cursor_item == pre.loop_item) || !input.visual_live)
    });
    if untouched {
        EndPlan { extra: Some(end), notice: None }
    } else {
        EndPlan { extra: None, notice: Some(strings::countdown_ended_nothing_else()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOP: Uuid = Uuid::from_u128(0xb1);
    const OTHER: Uuid = Uuid::from_u128(0xb2);

    fn show(title: &str) -> MacroStep {
        MacroStep::ShowItem { target: ItemTarget::Titled { title: title.into() }, slide: 0 }
    }

    fn pre() -> PreRun { PreRun { manual_version: 7, loop_item: Some(LOOP), timer_run: 3 } }

    fn input(cursor: Option<Uuid>, visual_live: bool) -> EndInput {
        EndInput { timer_run: 3, manual_version: 7, cursor_item: cursor, visual_live }
    }

    fn notice() -> EndPlan {
        EndPlan { extra: None, notice: Some("Countdown ended — nothing else changed because the service has started.".into()) }
    }

    #[test]
    fn build_puts_music_before_loop() {
        let p = PreService {
            loop_title: Some("Announcements".into()),
            music_title: Some("Walk-in music".into()),
            ..PreService::default()
        };
        assert_eq!(build(&p), vec![
            show("Walk-in music"),
            show("Announcements"),
            MacroStep::TimerRestart { timer: PRE_SERVICE_TIMER_ID },
            MacroStep::TimerShow { timer: PRE_SERVICE_TIMER_ID },
        ]);
        assert_eq!(build(&PreService::default()), vec![
            MacroStep::TimerRestart { timer: PRE_SERVICE_TIMER_ID },
            MacroStep::TimerShow { timer: PRE_SERVICE_TIMER_ID },
        ]);
    }

    #[test]
    fn build_sets_the_stage_layout_last() {
        let layout = Uuid::from_u128(0x5);
        let p = PreService { music_title: Some("Walk-in music".into()), stage_layout: Some(layout), ..PreService::default() };
        let steps = build(&p);
        assert_eq!(steps.first(), Some(&show("Walk-in music")));
        assert_eq!(steps.last(), Some(&MacroStep::StageLayout { layout }));
        let back: PreService = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn preservice_missing_fields_default() {
        let p: PreService = serde_json::from_str("{}").unwrap();
        assert_eq!(p.caption, "Service starts in");
        assert_eq!(p.end, PreServiceEnd::Hide);
        assert_eq!(p.place, OverlayPlace::TopRight);
        assert_eq!(p, PreService::default());
        let p: PreService = serde_json::from_str(r#"{"end":"fireworks"}"#).unwrap();
        assert_eq!(p.end, PreServiceEnd::Hide);
        let p: PreService = serde_json::from_str(r#"{"place":{"anchor":"center","width":80}}"#).unwrap();
        assert_eq!(p.place, OverlayPlace::TopRight);
        assert_eq!(default_place(true), OverlayPlace::TopRight);
        assert_eq!(default_place(false), OverlayPlace::CenterLarge);
    }

    #[test]
    fn timer_def_is_until_service_start_and_stops() {
        let p = PreService { place: OverlayPlace::CenterLarge, caption: "Welcome".into(), ..PreService::default() };
        let t = timer_def(&p);
        assert_eq!(t.id, PRE_SERVICE_TIMER_ID);
        assert_eq!(t.name, "Pre-service");
        assert_eq!(t.kind, Lenient::Known(TimerKind::UntilTime { target: TimeTarget::ServiceStart }));
        assert_eq!(t.overrun, Overrun::Stop);
        assert_eq!(t.at_zero, Lenient::Known(AtZero::Nothing));
        assert_eq!(t.place, OverlayPlace::CenterLarge);
        assert_eq!(t.caption, "Welcome");
        assert_eq!(t.builtin, Some(Builtin::PreService));
        assert!(!t.on_stage && !t.on_remote);
    }

    #[test]
    fn macro_def_keeps_remote_and_colour() {
        let p = PreService { loop_title: Some("Announcements".into()), ..PreService::default() };
        let fresh = macro_def(&p, None);
        assert_eq!(fresh.id, PRE_SERVICE_MACRO_ID);
        assert_eq!(fresh.name, "Start countdown");
        assert_eq!(fresh.color, MacroColor::Amber);
        assert!(!fresh.remote);
        assert_eq!(fresh.builtin, Some(Builtin::PreService));
        let steps: Vec<_> = build(&p).into_iter().map(Lenient::Known).collect();
        assert_eq!(fresh.steps, steps);

        let old = Macro { color: MacroColor::Teal, remote: true, steps: vec![], ..fresh.clone() };
        let m = macro_def(&p, Some(&old));
        assert_eq!(m.color, MacroColor::Teal);
        assert!(m.remote);
        assert_eq!(m.steps, steps);
        assert_eq!(m.name, "Start countdown");
    }

    #[test]
    fn preservice_end_first_item_when_untouched() {
        let want = EndPlan { extra: Some(PreServiceEnd::FirstItem), notice: None };
        assert_eq!(end_plan(PreServiceEnd::FirstItem, Some(&pre()), &input(Some(LOOP), true)), want);
        assert_eq!(end_plan(PreServiceEnd::FirstItem, Some(&pre()), &input(None, false)), want);
    }

    #[test]
    fn preservice_end_does_nothing_after_operator_took_an_item() {
        let moved = EndInput { manual_version: 8, ..input(Some(LOOP), true) };
        assert_eq!(end_plan(PreServiceEnd::FirstItem, Some(&pre()), &moved), notice());
        assert_eq!(end_plan(PreServiceEnd::Logo, Some(&pre()), &input(Some(OTHER), true)), notice());
    }

    #[test]
    fn preservice_end_after_restart_only_hides() {
        assert_eq!(end_plan(PreServiceEnd::ClearAll, None, &input(Some(LOOP), true)), notice());
    }

    #[test]
    fn preservice_end_other_timer_run_only_hides() {
        let other_run = EndInput { timer_run: 4, ..input(Some(LOOP), true) };
        assert_eq!(end_plan(PreServiceEnd::Logo, Some(&pre()), &other_run), notice());
    }

    #[test]
    fn hide_end_never_notices() {
        let none = EndPlan { extra: None, notice: None };
        assert_eq!(end_plan(PreServiceEnd::Hide, Some(&pre()), &input(Some(LOOP), true)), none);
        assert_eq!(end_plan(PreServiceEnd::Hide, None, &input(Some(OTHER), true)), none);
        let moved = EndInput { manual_version: 99, timer_run: 99, ..input(None, true) };
        assert_eq!(end_plan(PreServiceEnd::Hide, Some(&pre()), &moved), none);
    }
}
