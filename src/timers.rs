//! FEAT-2 timers: the stored definitions, times of day and the clock. Every stored type loads
//! leniently (spec §4.1): unknown kinds stay as raw JSON, unknown names load as the default.
use crate::lenient::Lenient;
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Mutex;
use uuid::Uuid;

/// A wall-clock time with no date, such as a service start. Saved as `"HH:MM"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeOfDay { pub hour: u8, pub minute: u8 }

impl TimeOfDay {
    /// Out-of-range values clamp to 23:59.
    pub fn new(hour: u8, minute: u8) -> Self { TimeOfDay { hour: hour.min(23), minute: minute.min(59) } }

    /// `H:MM` or `HH:MM`, spaces around it allowed; out-of-range values clamp. `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        let (h, m) = s.trim().split_once(':')?;
        let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
        if !(1..=2).contains(&h.len()) || m.len() != 2 || !digits(h) || !digits(m) { return None }
        Some(TimeOfDay::new(h.parse().ok()?, m.parse().ok()?))
    }

    /// 12-hour form with AM/PM: `"10:30 AM"`, `"12:05 PM"`, `"12:00 AM"`.
    pub fn label(&self) -> String {
        let h = match self.hour % 12 { 0 => 12, h => h };
        format!("{h}:{:02} {}", self.minute, if self.hour < 12 { "AM" } else { "PM" })
    }
}

impl Serialize for TimeOfDay {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{:02}:{:02}", self.hour, self.minute))
    }
}

impl<'de> Deserialize<'de> for TimeOfDay {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        TimeOfDay::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("not a time of day: {s:?}")))
    }
}

/// `deserialize_with` for an optional time that must never fail the load: null, a non-string
/// or unreadable text all give `None`. Use with `#[serde(default, deserialize_with = "lenient_time")]`.
pub fn lenient_time<'de, D: Deserializer<'de>>(d: D) -> Result<Option<TimeOfDay>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_str().and_then(TimeOfDay::parse))
}

/// What a timer measures.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimerKind {
    /// Counts down a fixed length.
    Duration { secs: u32 },
    /// Counts down to a time of day.
    UntilTime { target: TimeTarget },
    #[default]
    CountUp,
}

impl TimerKind {
    pub fn is_countdown(&self) -> bool { matches!(self, TimerKind::Duration { .. } | TimerKind::UntilTime { .. }) }
}

/// The time a countdown-to-a-time aims at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimeTarget {
    /// The service's start time.
    ServiceStart,
    Fixed { at: TimeOfDay },
}

/// What happens when a countdown reaches zero.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AtZero {
    #[default]
    Nothing,
    HideFromRoom,
    RunMacro { macro_id: Uuid },
}

// Unit enums: serde allows `#[serde(other)]` only on the last variant, so the default comes last.

/// What a countdown does after zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Overrun {
    CountUp,
    #[default]
    #[serde(other)]
    Stop,
}

/// Where a timer overlay sits (Plan decision 1; FEAT-1 §4.5 maps it to a `Placement`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayPlace {
    TopLeft,
    BottomRight,
    BottomLeft,
    CenterLarge,
    #[default]
    #[serde(other)]
    TopRight,
}

/// Which built-in a timer is, when it is one with special handling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Builtin {
    PreService,
    #[default]
    #[serde(other)]
    Unknown,
}

/// A saved timer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimerDef {
    pub id: Uuid,
    pub name: String,
    pub kind: Lenient<TimerKind>,
    pub overrun: Overrun,
    pub at_zero: Lenient<AtZero>,
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub place: OverlayPlace,
    pub caption: String,
    pub on_stage: bool,
    pub on_remote: bool,
    pub builtin: Option<Builtin>,
}

pub const SERVICE_START_ID: Uuid = Uuid::from_u128(0x52500000_0000_4000_8000_000000000001);
pub const SERMON_ID: Uuid = Uuid::from_u128(0x52500000_0000_4000_8000_000000000002);
pub const STOPWATCH_ID: Uuid = Uuid::from_u128(0x52500000_0000_4000_8000_000000000003);
pub const PRE_SERVICE_TIMER_ID: Uuid = Uuid::from_u128(0x52500000_0000_4000_8000_000000000004);

/// The time now. Only `SystemClock` reads the computer's clock; logic takes `now` as an input.
pub trait Clock: Send + Sync { fn now(&self) -> DateTime<Utc>; }

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> { Utc::now() }
}

/// A clock that only moves when told to. Plain `pub` because Part B's app tests use it too.
pub struct FakeClock(Mutex<DateTime<Utc>>);

impl FakeClock {
    pub fn new(at: DateTime<Utc>) -> Self { FakeClock(Mutex::new(at)) }

    pub fn set(&self, at: DateTime<Utc>) { *self.0.lock().unwrap_or_else(|e| e.into_inner()) = at; }

    pub fn advance_ms(&self, ms: i64) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) += TimeDelta::milliseconds(ms);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> DateTime<Utc> { *self.0.lock().unwrap_or_else(|e| e.into_inner()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lenient::Lenient;
    use chrono::{TimeDelta, TimeZone, Utc};
    use serde::Deserialize;

    #[test]
    fn time_of_day_serializes_hh_mm() {
        assert_eq!(serde_json::to_string(&TimeOfDay::new(9, 5)).unwrap(), "\"09:05\"");
        assert_eq!(TimeOfDay::parse("9:05"), Some(TimeOfDay::new(9, 5)));
        assert_eq!(TimeOfDay::parse(" 09:05 "), Some(TimeOfDay::new(9, 5)));
        for bad in ["9:5", "9", "", "abc"] { assert_eq!(TimeOfDay::parse(bad), None, "{bad:?}"); }
        assert_eq!(TimeOfDay::new(9, 5).label(), "9:05 AM");
        assert_eq!(TimeOfDay::new(0, 0).label(), "12:00 AM");
        assert_eq!(TimeOfDay::new(12, 5).label(), "12:05 PM");
    }

    #[test]
    fn bad_time_of_day_clamps_or_drops_field() {
        assert_eq!(TimeOfDay::parse("25:70"), Some(TimeOfDay::new(23, 59)));
        #[derive(Deserialize)] struct Holder { #[serde(default, deserialize_with = "lenient_time")] at: Option<TimeOfDay> }
        for src in [r#"{"at":"soon"}"#, r#"{"at":null}"#, r#"{"at":7}"#, r#"{}"#] {
            assert_eq!(serde_json::from_str::<Holder>(src).unwrap().at, None, "{src}");
        }
        let k: Lenient<TimerKind> = serde_json::from_str(r#"{"type":"until_time","target":{"type":"fixed","at":"soon"}}"#).unwrap();
        assert!(matches!(k, Lenient::Unknown(_)));
    }

    #[test]
    fn unknown_timer_kind_loads_and_round_trips() {
        let src = r#"{"type":"lap_timer","laps":3}"#;
        let k: Lenient<TimerKind> = serde_json::from_str(src).unwrap();
        assert!(matches!(k, Lenient::Unknown(_)));
        assert_eq!(serde_json::to_value(&k).unwrap(), serde_json::from_str::<serde_json::Value>(src).unwrap());
    }

    #[test]
    fn unknown_unit_enum_value_loads_as_default() {
        let t: TimerDef = serde_json::from_str(r#"{"overrun":"explode","place":"middle","builtin":"robot"}"#).unwrap();
        assert_eq!(t.overrun, Overrun::Stop);
        assert_eq!(t.place, OverlayPlace::TopRight);
        assert_eq!(t.builtin, Some(Builtin::Unknown));
    }

    #[test]
    fn place_object_from_newer_version_loads_as_default() {
        let t: TimerDef = serde_json::from_str(r#"{"name":"X","place":{"anchor":"bottom_left","width":25,"margin":4}}"#).unwrap();
        assert_eq!(t.place, OverlayPlace::TopRight);
        assert_eq!(t.name, "X");
        let t: TimerDef = serde_json::from_str(r#"{"place":"center_large"}"#).unwrap();
        assert_eq!(t.place, OverlayPlace::CenterLarge);
    }

    #[test]
    fn timer_def_missing_fields_default() {
        let t: TimerDef = serde_json::from_str("{}").unwrap();
        assert_eq!(t, TimerDef::default());
        assert_eq!(t.kind, Lenient::Known(TimerKind::CountUp));
        assert_eq!(t.at_zero, Lenient::Known(AtZero::Nothing));
    }

    #[test]
    fn timer_kind_json_shape() {
        fn v<T: serde::Serialize>(x: &T) -> serde_json::Value { serde_json::to_value(x).unwrap() }
        assert_eq!(v(&TimerKind::Duration { secs: 1800 }), serde_json::json!({"type":"duration","secs":1800}));
        assert_eq!(v(&TimerKind::UntilTime { target: TimeTarget::ServiceStart }),
            serde_json::json!({"type":"until_time","target":{"type":"service_start"}}));
        assert_eq!(v(&TimeTarget::Fixed { at: TimeOfDay::new(10, 30) }), serde_json::json!({"type":"fixed","at":"10:30"}));
        let id = uuid::Uuid::from_u128(7);
        assert_eq!(v(&AtZero::RunMacro { macro_id: id }), serde_json::json!({"type":"run_macro","macro_id":id.to_string()}));
    }

    #[test]
    fn fake_clock_advances() {
        let t0 = Utc.with_ymd_and_hms(2026, 11, 1, 10, 0, 0).unwrap();
        let c = FakeClock::new(t0);
        assert_eq!(c.now(), t0);
        c.advance_ms(1500);
        assert_eq!(c.now(), t0 + TimeDelta::milliseconds(1500));
        let t1 = t0 + TimeDelta::hours(1);
        c.set(t1);
        assert_eq!(c.now(), t1);
    }
}
