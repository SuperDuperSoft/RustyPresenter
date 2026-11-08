//! FEAT-2 service automation: timers, message templates, macros, auto-advance and the
//! pre-service countdown. Pure logic here; the App, live engine and UI wire it in (Part B).
//!
//! The strings live at their final path, `src/strings/automation.rs`. Task B1 turns the
//! `#[path]` line below into `pub use crate::strings::automation as strings;`.
#[path = "../strings/automation.rs"]
pub mod strings;

pub mod actions;
pub mod advance;
pub mod calendar;
pub mod macros;
pub mod messages;
pub mod music;
pub mod music_out;
pub mod preservice;
pub mod runtime;
pub mod slots;
pub mod task;

use crate::automation::macros::Macro;
use crate::automation::messages::MessageTemplate;
use crate::automation::preservice::{PRE_SERVICE_MACRO_ID, PreService};
use crate::lenient::Lenient;
use crate::timers::{
    AtZero, OverlayPlace, Overrun, PRE_SERVICE_TIMER_ID, SERMON_ID, SERVICE_START_ID, STOPWATCH_ID, TimeTarget,
    TimerDef, TimerKind,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Everything FEAT-2 saves: the timers, message templates and macros, plus the Pre-service tab.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Automation {
    pub timers: Vec<TimerDef>,
    pub messages: Vec<MessageTemplate>,
    pub macros: Vec<Macro>,
    pub pre_service: Option<PreService>,
    /// The built-ins were added once; deleting one doesn't bring it back.
    pub seeded: bool,
    /// Each service's start time, by service id. Kept here rather than on `Service`, so the many places that
    /// build services don't change.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", deserialize_with = "good_entries")]
    pub starts_at: BTreeMap<Uuid, crate::timers::TimeOfDay>,
    /// Each service item's slide auto-advance, by item id (spec §3.5).
    #[serde(skip_serializing_if = "BTreeMap::is_empty", deserialize_with = "good_entries")]
    pub advance: BTreeMap<Uuid, advance::AutoAdvance>,
    /// N13: the walk-in music.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub music: music::MusicList,
    /// N17: what runs by itself at set times.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub calendar: Vec<calendar::CalendarEntry>,
}

/// A map whose entries this version can't read are left out instead of failing the load (spec §4.1).
fn good_entries<'de, D: serde::Deserializer<'de>, V: serde::de::DeserializeOwned>(d: D) -> Result<BTreeMap<Uuid, V>, D::Error> {
    let raw: BTreeMap<String, serde_json::Value> = crate::lenient::or_default(d)?;
    Ok(raw.into_iter().filter_map(|(k, v)| Some((Uuid::parse_str(&k).ok()?, serde_json::from_value(v).ok()?))).collect())
}

impl Automation {
    /// A new install: the built-ins are already there.
    pub fn seeded() -> Self {
        let mut a = Automation::default();
        seed(&mut a);
        a
    }
}

fn builtin_timer(id: uuid::Uuid, name: &str, kind: TimerKind, overrun: Overrun, caption: &str, on_stage: bool) -> TimerDef {
    TimerDef {
        id,
        name: name.into(),
        kind: Lenient::Known(kind),
        overrun,
        at_zero: Lenient::Known(AtZero::Nothing),
        place: OverlayPlace::TopRight,
        caption: caption.into(),
        on_stage,
        on_remote: false,
        builtin: None,
    }
}

/// Adds the built-in timers and message templates the first time (Plan decisions 8, 11, 12).
/// Returns true when it changed something, so the caller saves.
pub fn seed(a: &mut Automation) -> bool {
    if a.seeded {
        return false;
    }
    a.timers.extend([
        builtin_timer(
            SERVICE_START_ID,
            &strings::service_start_name(),
            TimerKind::UntilTime { target: TimeTarget::ServiceStart },
            Overrun::Stop,
            &strings::service_starts_in(),
            false,
        ),
        builtin_timer(SERMON_ID, &strings::sermon_name(), TimerKind::Duration { secs: 1800 }, Overrun::CountUp, "", true),
        builtin_timer(STOPWATCH_ID, &strings::stopwatch_name(), TimerKind::CountUp, Overrun::Stop, "", false),
    ]);
    a.messages.extend(messages::builtin());
    a.seeded = true;
    true
}

/// Saves the Pre-service tab and rebuilds its timer and its locked "Start countdown" macro,
/// which always sits first.
pub fn apply_preservice(a: &mut Automation, p: PreService) {
    let timer = preservice::timer_def(&p);
    match a.timers.iter_mut().find(|t| t.id == PRE_SERVICE_TIMER_ID) {
        Some(t) => *t = timer,
        None => a.timers.push(timer),
    }
    let existing = a.macros.iter().position(|m| m.id == PRE_SERVICE_MACRO_ID).map(|i| a.macros.remove(i));
    a.macros.insert(0, preservice::macro_def(&p, existing.as_ref()));
    a.pre_service = Some(p);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::macros::{Macro, MacroStep};
    use crate::automation::messages::{CUSTOM_ID, NURSERY_ID, VEHICLE_ID};
    use crate::automation::preservice::{PRE_SERVICE_MACRO_ID, PreService};
    use crate::lenient::Lenient;
    use crate::timers::{Builtin, PRE_SERVICE_TIMER_ID, SERMON_ID, SERVICE_START_ID, STOPWATCH_ID, TimerKind};
    use uuid::Uuid;

    fn seeded() -> Automation {
        let mut a = Automation::default();
        seed(&mut a);
        a
    }

    #[test]
    fn seed_uses_fixed_ids_and_saves_once() {
        let mut a = Automation::default();
        assert!(seed(&mut a));
        assert!(a.seeded);
        let ids: Vec<Uuid> = a.timers.iter().map(|t| t.id).collect();
        assert_eq!(ids, [SERVICE_START_ID, SERMON_ID, STOPWATCH_ID]);
        let ids: Vec<Uuid> = a.messages.iter().map(|m| m.id).collect();
        assert_eq!(ids, [NURSERY_ID, VEHICLE_ID, CUSTOM_ID]);
        let sermon = &a.timers[1];
        assert_eq!(sermon.name, "Sermon");
        assert_eq!(sermon.kind, Lenient::Known(TimerKind::Duration { secs: 1800 }));
        assert_eq!(sermon.overrun, Overrun::CountUp);
        assert!(sermon.on_stage);
        assert_eq!(a.timers[0].name, "Service start");
        assert_eq!(a.timers[0].kind, Lenient::Known(TimerKind::UntilTime { target: TimeTarget::ServiceStart }));
        assert_eq!(a.timers[0].overrun, Overrun::Stop);
        assert_eq!(a.timers[0].caption, "Service starts in");
        assert_eq!(a.timers[2].name, "Stopwatch");
        assert_eq!(a.timers[2].kind, Lenient::Known(TimerKind::CountUp));
        assert!(a.timers.iter().all(|t| t.place == OverlayPlace::TopRight));
        assert!(!a.timers[0].on_stage && !a.timers[2].on_stage);

        let before = a.clone();
        assert!(!seed(&mut a));
        assert_eq!(a, before);
    }

    #[test]
    fn seed_not_rerun_after_delete() {
        let mut a = seeded();
        a.timers.retain(|t| t.id != SERMON_ID);
        assert!(!seed(&mut a));
        assert!(a.timers.iter().all(|t| t.id != SERMON_ID));
        assert_eq!(a.timers.len(), 2);
    }

    #[test]
    fn automation_missing_fields_default() {
        let a: Automation = serde_json::from_str("{}").unwrap();
        assert_eq!(a, Automation::default());
        let json = r#"{"timers":[{"kind":{"type":"warp"}}],"macros":[{"steps":[{"step":"x"}]}],"zzz":1}"#;
        let a: Automation = serde_json::from_str(json).unwrap();
        let back = serde_json::to_value(&a).unwrap();
        assert_eq!(back["timers"][0]["kind"], serde_json::json!({"type":"warp"}));
        assert_eq!(back["macros"][0]["steps"][0], serde_json::json!({"step":"x"}));
    }

    #[test]
    fn automation_round_trips() {
        let mut a = seeded();
        apply_preservice(&mut a, PreService { loop_title: Some("Announcements".into()), ..PreService::default() });
        let json = serde_json::to_string(&a).unwrap();
        let back: Automation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn apply_preservice_creates_locked_macro_first() {
        let mut a = seeded();
        for n in 0..3u128 {
            a.macros.push(Macro { id: Uuid::from_u128(0xc0 + n), name: format!("M{n}"), ..Macro::default() });
        }
        apply_preservice(&mut a, PreService::default());
        assert_eq!(a.macros.len(), 4);
        assert_eq!(a.macros[0].id, PRE_SERVICE_MACRO_ID);
        assert_eq!(a.macros[0].builtin, Some(Builtin::PreService));
        assert_eq!(a.macros[1].id, Uuid::from_u128(0xc0));
        assert!(a.timers.iter().any(|t| t.id == PRE_SERVICE_TIMER_ID));
        assert_eq!(a.pre_service, Some(PreService::default()));
    }

    #[test]
    fn apply_preservice_twice_updates_in_place() {
        let mut a = seeded();
        a.macros.push(Macro { id: Uuid::from_u128(0xc0), ..Macro::default() });
        apply_preservice(&mut a, PreService::default());
        let m = a.macros.iter_mut().find(|m| m.id == PRE_SERVICE_MACRO_ID).unwrap();
        m.remote = true;
        // Someone moved it down; the next apply puts it back first.
        a.macros.swap(0, 1);
        let p = PreService { music_title: Some("Walk-in music".into()), caption: "Welcome".into(), ..PreService::default() };
        apply_preservice(&mut a, p.clone());
        assert_eq!(a.macros.iter().filter(|m| m.id == PRE_SERVICE_MACRO_ID).count(), 1);
        assert_eq!(a.timers.iter().filter(|t| t.id == PRE_SERVICE_TIMER_ID).count(), 1);
        assert_eq!(a.macros[0].id, PRE_SERVICE_MACRO_ID);
        assert!(a.macros[0].remote);
        assert!(a.macros[0].steps.contains(&Lenient::Known(MacroStep::ShowItem {
            target: crate::automation::macros::ItemTarget::Titled { title: "Walk-in music".into() },
            slide: 0,
        })));
        let t = a.timers.iter().find(|t| t.id == PRE_SERVICE_TIMER_ID).unwrap();
        assert_eq!(t.caption, "Welcome");
        assert_eq!(a.pre_service, Some(p));
    }
}
