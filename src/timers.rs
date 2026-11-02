//! FEAT-2 timers: the stored definitions, times of day and the clock. Every stored type loads
//! leniently (spec §4.1): unknown kinds stay as raw JSON, unknown names load as the default.
use crate::lenient::Lenient;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeDelta, TimeZone, Utc};
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

/// Where a timer is in its run (spec §4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TimerState { Stopped, Running, Paused, Finished }

/// One timer's readout, the FEAT-3 contract (spec §4.4).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimerSnapshot {
    pub id: Uuid,
    pub name: String,
    pub kind: TimerKind,
    pub state: TimerState,
    /// Time left for countdowns, time gone for count-up; negative is overrun. Always whole
    /// seconds (Plan decision 2), so `text` needs no rounding of its own.
    pub value_ms: i64,
    pub overrun: bool,
    pub on_stage: bool,
    pub on_remote: bool,
}

/// The auto-advance readout: seconds to the next slide, and whether it is paused.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct AutoReadout { pub next_in_secs: u32, pub paused: bool }

/// Everything a tick publishes: timers in Timers-tab order (unknown kinds left out) and auto-advance.
#[derive(Clone, Debug, PartialEq, Default, Serialize)]
pub struct TimersView { pub timers: Vec<TimerSnapshot>, pub auto: Option<AutoReadout> }

/// The shared formatter: `m:ss` under an hour, `h:mm:ss` from an hour, `+` for overrun.
/// Truncates to whole seconds, so callers pass a `display_ms` value.
pub fn text(value_ms: i64) -> String {
    let s = value_ms.unsigned_abs() / 1000;
    let sign = if value_ms < 0 && s > 0 { "+" } else { "" };
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{sign}{h}:{m:02}:{s:02}") } else { format!("{sign}{m}:{s:02}") }
}

/// Quantises a raw value to whole seconds (Plan decision 2): a countdown with time left rounds
/// up, so it reads 0:00 only at zero; anything else rounds toward zero and keeps its sign.
pub fn display_ms(raw_ms: i64, countdown: bool) -> i64 {
    if countdown && raw_ms > 0 {
        raw_ms.saturating_add(999) / 1000 * 1000
    } else {
        raw_ms / 1000 * 1000
    }
}

/// `at` on `date` in `tz`, as an instant (spec §5.1 DST rule). An ambiguous (fall-back) time
/// uses the earlier instant; a time in the spring-forward gap uses the first valid minute after it.
pub fn resolve_local<Tz: TimeZone>(tz: &Tz, date: NaiveDate, at: TimeOfDay) -> DateTime<Utc> {
    let time = NaiveTime::from_hms_opt(at.hour.min(23).into(), at.minute.min(59).into(), 0).unwrap_or(NaiveTime::MIN);
    let start = NaiveDateTime::new(date, time);
    let mut naive = start;
    for _ in 0..=180 {
        if let Some(t) = tz.from_local_datetime(&naive).earliest() { return t.with_timezone(&Utc) }
        naive += TimeDelta::minutes(1);
    }
    // No zone has a gap this long; read the time with the offset in force at that moment.
    let offset = tz.offset_from_utc_datetime(&start).fix().local_minus_utc();
    start.and_utc() - TimeDelta::seconds(offset.into())
}

/// When a countdown-to-a-time should reach zero, or the message to show instead (spec §5.1).
/// A fixed time that passed under an hour ago is refused; one longer ago means tomorrow.
pub fn until_target<Tz: TimeZone>(target: &TimeTarget, starts_at: Option<TimeOfDay>, now: DateTime<Utc>, tz: &Tz)
    -> Result<DateTime<Utc>, String> {
    use crate::automation::strings;
    let today = now.with_timezone(tz).date_naive();
    match target {
        TimeTarget::ServiceStart => {
            let at = starts_at.ok_or_else(|| strings::NO_START_TIME.to_string())?;
            let t = resolve_local(tz, today, at);
            if t <= now { Err(strings::service_start_passed(&at.label())) } else { Ok(t) }
        }
        TimeTarget::Fixed { at } => {
            let t = resolve_local(tz, today, *at);
            if t > now {
                Ok(t)
            } else if now - t < TimeDelta::minutes(60) {
                Err(strings::time_passed(&at.label()))
            } else {
                Ok(resolve_local(tz, today.succ_opt().unwrap_or(today), *at))
            }
        }
    }
}

#[cfg(test)]
mod test_zone {
    //! America/Chicago for 2026 only, so tests need no time zone database (replaces `chrono-tz`).
    use chrono::{FixedOffset, MappedLocalTime, NaiveDate, NaiveDateTime, TimeZone, Utc};

    #[derive(Clone, Copy, Debug)]
    pub struct Chicago2026;

    fn cdt() -> FixedOffset { FixedOffset::west_opt(5 * 3600).unwrap() }
    fn cst() -> FixedOffset { FixedOffset::west_opt(6 * 3600).unwrap() }

    impl TimeZone for Chicago2026 {
        type Offset = FixedOffset;

        fn from_offset(_: &FixedOffset) -> Self { Chicago2026 }

        fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<FixedOffset> {
            self.offset_from_local_datetime(&local.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<FixedOffset> {
            let fits = |o: FixedOffset| self.offset_from_utc_datetime(&(*local - o)) == o;
            match (fits(cdt()), fits(cst())) {
                (true, true) => MappedLocalTime::Ambiguous(cdt(), cst()),
                (true, false) => MappedLocalTime::Single(cdt()),
                (false, true) => MappedLocalTime::Single(cst()),
                (false, false) => MappedLocalTime::None,
            }
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> FixedOffset {
            self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
            let begins = Utc.with_ymd_and_hms(2026, 3, 8, 8, 0, 0).unwrap().naive_utc();
            let ends = Utc.with_ymd_and_hms(2026, 11, 1, 7, 0, 0).unwrap().naive_utc();
            if (begins..ends).contains(utc) { cdt() } else { cst() }
        }
    }
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

    #[test]
    fn timer_text_shared_formatter() {
        for (ms, s) in [(0, "0:00"), (59_000, "0:59"), (60_000, "1:00"), (3_599_000, "59:59"), (3_600_000, "1:00:00"),
                        (-72_000, "+1:12"), (-3_900_000, "+1:05:00"), (-400, "0:00")] {
            assert_eq!(text(ms), s, "{ms}");
        }
        assert_eq!(display_ms(1, true), 1000);
        assert_eq!(display_ms(1500, true), 2000);
        assert_eq!(display_ms(2000, true), 2000);
        assert_eq!(display_ms(0, true), 0);
        assert_eq!(display_ms(1999, false), 1000);
        assert_eq!(display_ms(-1999, true), -1000);
    }

    fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap()
    }

    fn fixed(h: u8, m: u8) -> TimeTarget { TimeTarget::Fixed { at: TimeOfDay::new(h, m) } }

    #[test]
    fn service_start_without_time_errors() {
        let chi = test_zone::Chicago2026;
        let now = utc(2026, 6, 7, 15, 45);
        assert_eq!(until_target(&TimeTarget::ServiceStart, None, now, &chi),
            Err(crate::automation::strings::NO_START_TIME.into()));
    }

    #[test]
    fn start_after_target_runs_at_zero_or_refuses() {
        let chi = test_zone::Chicago2026;
        let now = utc(2026, 6, 7, 15, 45); // 10:45 CDT, a Sunday
        assert_eq!(until_target(&TimeTarget::ServiceStart, Some(TimeOfDay::new(10, 30)), now, &chi),
            Err("The service start time (10:30 AM) has passed.".into()));
        assert_eq!(until_target(&fixed(10, 15), None, now, &chi), Err("That time (10:15 AM) has passed.".into()));
        assert_eq!(until_target(&fixed(7, 45), None, now, &chi), Ok(utc(2026, 6, 8, 12, 45)));
        assert_eq!(until_target(&fixed(11, 0), None, now, &chi), Ok(utc(2026, 6, 7, 16, 0)));
    }

    #[test]
    fn until_time_across_dst_change() {
        let chi = test_zone::Chicago2026;
        let march = utc(2026, 3, 8, 7, 0); // 1:00 CST
        assert_eq!(until_target(&fixed(2, 30), None, march, &chi), Ok(utc(2026, 3, 8, 8, 0)));
        assert_eq!(until_target(&fixed(10, 30), None, march, &chi), Ok(utc(2026, 3, 8, 15, 30)));
        let november = utc(2026, 11, 1, 5, 30); // 0:30 CDT
        assert_eq!(until_target(&fixed(1, 30), None, november, &chi), Ok(utc(2026, 11, 1, 6, 30)));
        assert_eq!(until_target(&fixed(10, 30), None, november, &chi), Ok(utc(2026, 11, 1, 16, 30)));
    }
}
