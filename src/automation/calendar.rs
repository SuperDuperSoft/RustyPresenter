//! N17: the calendar. Runs a macro or one action at a set time of day on the chosen weekdays, such as "start the
//! countdown at 10:45 every Sunday". Times are the computer's local time; the ticker looks every wake.
//!
//! Rules (pure functions below, tested with a time zone that changes for summer time):
//! * An entry fires when the clock passes its time on a chosen day, at most once for each day.
//! * It fires only if the app was running when the time came (seen by an earlier tick) and at most
//!   `LATE_LIMIT_SECS` late: a laptop that wakes up at noon does not start the 10:45 countdown.
//! * A time in the spring-forward gap fires at the first minute after it; a time the clocks pass twice when they fall
//!   back fires once, the first time. A clock set back never fires an entry again for the same day.
use crate::app::{ActionError, App};
use crate::automation::macros::{Macro, MacroStep};
use crate::lenient::Lenient;
use crate::timers::{TimeOfDay, resolve_local};
use chrono::{DateTime, Datelike, NaiveDate, TimeDelta, TimeZone, Utc, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// How late an entry may still fire.
pub const LATE_LIMIT_SECS: i64 = 120;
/// The ticker wakes often when an entry is due this soon.
pub const SOON_SECS: i64 = 6;
pub const MAX_ENTRIES: usize = 50;

/// Every day of the week: bit 0 is Sunday … bit 6 Saturday.
pub const EVERY_DAY: u8 = 0x7f;

/// What an entry runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CalendarRun {
    Macro { macro_id: Uuid },
    /// One step, as a macro of one.
    Step { step: MacroStep },
}

/// One calendar entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalendarEntry {
    pub id: Uuid,
    pub name: String,
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub at: TimeOfDay,
    /// Bit 0 Sunday … bit 6 Saturday.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub days: u8,
    pub run: Lenient<CalendarRun>,
    pub on: bool,
}

impl Default for CalendarEntry {
    fn default() -> Self {
        CalendarEntry { id: Uuid::nil(), name: String::new(), at: TimeOfDay::new(10, 45), days: 1, run: Lenient::Unknown(Value::Null), on: true }
    }
}

/// The bit of `day` in `CalendarEntry::days`.
pub fn day_bit(day: Weekday) -> u8 { 1 << day.num_days_from_sunday() }

/// When `e` runs on local `date`, or `None` when that day isn't chosen.
pub fn occurrence<Tz: TimeZone>(e: &CalendarEntry, date: NaiveDate, tz: &Tz) -> Option<DateTime<Utc>> {
    (e.days & day_bit(date.weekday()) != 0).then(|| resolve_local(tz, date, e.at))
}

/// The latest time `e` ran or should have run, at or before `now` (within the last eight days).
pub fn last_due<Tz: TimeZone>(e: &CalendarEntry, now: DateTime<Utc>, tz: &Tz) -> Option<DateTime<Utc>> {
    let today = now.with_timezone(tz).date_naive();
    (0..=8).filter_map(|back| today.checked_sub_days(chrono::Days::new(back))).filter_map(|d| occurrence(e, d, tz)).find(|t| *t <= now)
}

/// The next time `e` runs, after `now`; `None` when it is off or has no days.
pub fn next_due<Tz: TimeZone>(e: &CalendarEntry, now: DateTime<Utc>, tz: &Tz) -> Option<DateTime<Utc>> {
    if !e.on || e.days & EVERY_DAY == 0 { return None }
    let today = now.with_timezone(tz).date_naive();
    (0..=8).filter_map(|ahead| today.checked_add_days(chrono::Days::new(ahead))).filter_map(|d| occurrence(e, d, tz)).find(|t| *t > now)
}

/// The calendar's memory-only state: when the ticker last looked, and when each entry last fired.
#[derive(Clone, Debug, Default)]
pub struct CalendarState {
    pub checked: Option<DateTime<Utc>>,
    pub fired: HashMap<Uuid, DateTime<Utc>>,
}

impl CalendarState {
    /// The entries due at `now`, each with the time it is for; remembers them as fired and `now` as looked at.
    /// The first look after the app starts only notes the time.
    pub fn due<Tz: TimeZone>(&mut self, entries: &[CalendarEntry], now: DateTime<Utc>, tz: &Tz) -> Vec<(Uuid, DateTime<Utc>)> {
        // A clock set back starts the next look from the new time; `fired` keeps it from firing an entry twice.
        let checked = self.checked.replace(now);
        let Some(checked) = checked else { return vec![] };
        let mut out = vec![];
        for e in entries.iter().filter(|e| e.on && e.run.known().is_some()) {
            let Some(t) = last_due(e, now, tz) else { continue };
            let new = t > checked && now - t <= TimeDelta::seconds(LATE_LIMIT_SECS);
            let again = self.fired.get(&e.id).is_some_and(|f| *f >= t);
            if new && !again {
                self.fired.insert(e.id, t);
                out.push((e.id, t));
            }
        }
        self.fired.retain(|id, _| entries.iter().any(|e| e.id == *id));
        out
    }

    /// An entry is due within `SOON_SECS`, so the ticker should not sleep.
    pub fn soon<Tz: TimeZone>(entries: &[CalendarEntry], now: DateTime<Utc>, tz: &Tz) -> bool {
        entries.iter().filter_map(|e| next_due(e, now, tz)).any(|t| t - now <= TimeDelta::seconds(SOON_SECS))
    }
}

/// The macro an entry runs: its own, or a macro of one step named after the entry.
pub fn as_macro(e: &CalendarEntry, macros: &[Macro]) -> Result<Macro, String> {
    match e.run.known() {
        Some(CalendarRun::Macro { macro_id }) => macros.iter().find(|m| m.id == *macro_id).cloned().ok_or_else(|| super::strings::calendar_macro_gone(&e.name)),
        Some(CalendarRun::Step { step }) => Ok(Macro { id: e.id, name: e.name.clone(), steps: vec![Lenient::Known(step.clone())], ..Macro::default() }),
        None => Err(super::strings::timer_needs_newer()),
    }
}

/// Save checks for an entry.
pub fn validate(a: &super::Automation, mut e: CalendarEntry) -> Result<CalendarEntry, String> {
    e.name = e.name.trim().to_string();
    if e.name.is_empty() || e.name.chars().count() > 40 { return Err(super::strings::name_length()) }
    e.days &= EVERY_DAY;
    if e.days == 0 { return Err(super::strings::calendar_no_days()) }
    match e.run.known() {
        Some(CalendarRun::Macro { macro_id }) if !a.macros.iter().any(|m| m.id == *macro_id) => return Err(super::strings::calendar_macro_gone(&e.name)),
        None => return Err(super::strings::calendar_pick_what()),
        _ => {}
    }
    if !a.calendar.iter().any(|x| x.id == e.id) && a.calendar.len() >= MAX_ENTRIES { return Err(super::strings::calendar_too_many()) }
    Ok(e)
}

/// `automation/calendar/*`: save, delete, and run now (to try one).
pub async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    let bad = ActionError::bad;
    let parse = |b: &[u8]| serde_json::from_slice::<Value>(b).map_err(|e| bad(crate::tr!("Bad request: {error}", error = e)));
    match action {
        "automation/calendar/save" => {
            let e: CalendarEntry = serde_json::from_value(parse(body)?).map_err(|e| bad(crate::tr!("Bad request: {error}", error = e)))?;
            let id = if e.id.is_nil() { Uuid::new_v4() } else { e.id };
            app.edit(|d| {
                let e = validate(&d.automation, CalendarEntry { id, ..e })?;
                match d.automation.calendar.iter_mut().find(|x| x.id == id) { Some(x) => *x = e, None => d.automation.calendar.push(e) }
                Ok(())
            }).map_err(bad)?;
            app.automation_changed();
            Ok(json!({ "id": id }))
        }
        "automation/calendar/delete" => {
            let id = parse(body)?["id"].as_str().and_then(|s| Uuid::parse_str(s).ok()).ok_or_else(|| bad(crate::tr!("Bad request: {error}", error = "id")))?;
            app.edit(|d| { d.automation.calendar.retain(|e| e.id != id); Ok(()) }).map_err(bad)?;
            Ok(json!({}))
        }
        "automation/calendar/run" => {
            let id = parse(body)?["id"].as_str().and_then(|s| Uuid::parse_str(s).ok()).ok_or_else(|| bad(crate::tr!("Bad request: {error}", error = "id")))?;
            let toast = app.calendar_run(id, Utc::now()).map_err(bad)?;
            Ok(json!({ "toast": toast }))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timers::test_zone::Chicago2026;
    use chrono::FixedOffset;

    fn sunday_at(h: u8, m: u8) -> CalendarEntry {
        CalendarEntry { id: Uuid::from_u128(1), name: "Countdown".into(), at: TimeOfDay::new(h, m), days: day_bit(Weekday::Sun),
            run: Lenient::Known(CalendarRun::Step { step: MacroStep::Next }), on: true }
    }
    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> { Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap() }

    /// Looks every `step` seconds from `from` to `to`; returns when each fire happened.
    fn run_clock<Tz: TimeZone>(st: &mut CalendarState, e: &CalendarEntry, from: DateTime<Utc>, to: DateTime<Utc>, step: i64, tz: &Tz) -> Vec<DateTime<Utc>> {
        let mut out = vec![];
        let mut now = from;
        while now <= to {
            if !st.due(std::slice::from_ref(e), now, tz).is_empty() { out.push(now) }
            now += TimeDelta::seconds(step);
        }
        out
    }

    #[test]
    fn fires_on_the_chosen_weekday_in_local_time() {
        // 10:45 in Chicago in June is 15:45 UTC (summer time, UTC−5). June 7, 2026 is a Sunday.
        let e = sunday_at(10, 45);
        let mut st = CalendarState::default();
        let fires = run_clock(&mut st, &e, utc(2026, 6, 7, 15, 40, 0), utc(2026, 6, 7, 16, 0, 0), 5, &Chicago2026);
        assert_eq!(fires, [utc(2026, 6, 7, 15, 45, 0)]);
        // Monday at the same time: nothing.
        let fires = run_clock(&mut st, &e, utc(2026, 6, 8, 15, 40, 0), utc(2026, 6, 8, 16, 0, 0), 5, &Chicago2026);
        assert!(fires.is_empty());
        // In winter the same entry is 16:45 UTC (UTC−6).
        let fires = run_clock(&mut st, &e, utc(2026, 12, 6, 16, 40, 0), utc(2026, 12, 6, 16, 50, 0), 5, &Chicago2026);
        assert_eq!(fires, [utc(2026, 12, 6, 16, 45, 0)]);
    }

    #[test]
    fn the_weekday_is_the_local_one_not_utc() {
        // Saturday 20:00 in Chicago is already Sunday in UTC; a Saturday entry fires, a Sunday one does not.
        let sat = CalendarEntry { days: day_bit(Weekday::Sat), ..sunday_at(20, 0) };
        let sun = CalendarEntry { days: day_bit(Weekday::Sun), ..sunday_at(20, 0) };
        let (from, to) = (utc(2026, 6, 7, 0, 55, 0), utc(2026, 6, 7, 1, 5, 0));
        assert_eq!(run_clock(&mut CalendarState::default(), &sat, from, to, 5, &Chicago2026), [utc(2026, 6, 7, 1, 0, 0)]);
        assert!(run_clock(&mut CalendarState::default(), &sun, from, to, 5, &Chicago2026).is_empty());
    }

    #[test]
    fn the_first_look_after_start_never_fires_and_late_wakes_are_skipped() {
        let e = sunday_at(10, 45);
        let mut st = CalendarState::default();
        // Started a few seconds after 10:45: it wasn't running when the time came.
        assert!(st.due(std::slice::from_ref(&e), utc(2026, 6, 7, 15, 45, 3), &Chicago2026).is_empty());
        // The laptop slept from 10:40 and woke at 11:30: too late to start a countdown.
        let mut st = CalendarState::default();
        st.due(std::slice::from_ref(&e), utc(2026, 6, 7, 15, 40, 0), &Chicago2026);
        assert!(st.due(std::slice::from_ref(&e), utc(2026, 6, 7, 16, 30, 0), &Chicago2026).is_empty());
        // Woke at 10:46: still in time.
        let mut st = CalendarState::default();
        st.due(std::slice::from_ref(&e), utc(2026, 6, 7, 15, 40, 0), &Chicago2026);
        assert_eq!(st.due(std::slice::from_ref(&e), utc(2026, 6, 7, 15, 46, 0), &Chicago2026), [(e.id, utc(2026, 6, 7, 15, 45, 0))]);
    }

    #[test]
    fn a_clock_set_back_never_fires_twice() {
        let e = sunday_at(10, 45);
        let mut st = CalendarState::default();
        let fires = run_clock(&mut st, &e, utc(2026, 6, 7, 15, 44, 0), utc(2026, 6, 7, 15, 46, 0), 1, &Chicago2026);
        assert_eq!(fires.len(), 1);
        // Someone sets the clock back five minutes; it passes 10:45 again.
        let again = run_clock(&mut st, &e, utc(2026, 6, 7, 15, 41, 0), utc(2026, 6, 7, 15, 47, 0), 1, &Chicago2026);
        assert!(again.is_empty());
    }

    #[test]
    fn spring_forward_gap_fires_at_the_first_minute_after() {
        // March 8, 2026 (a Sunday): Chicago clocks jump from 2:00 to 3:00. 2:30 doesn't exist; it fires at 3:00 CDT = 8:00 UTC.
        let e = sunday_at(2, 30);
        let mut st = CalendarState::default();
        let fires = run_clock(&mut st, &e, utc(2026, 3, 8, 7, 50, 0), utc(2026, 3, 8, 9, 0, 0), 30, &Chicago2026);
        assert_eq!(fires, [utc(2026, 3, 8, 8, 0, 0)]);
    }

    #[test]
    fn fall_back_fires_once_at_the_first_pass() {
        // November 1, 2026 (a Sunday): 1:30 happens twice in Chicago, at 6:30 and 7:30 UTC. Only the first fires.
        let e = sunday_at(1, 30);
        let mut st = CalendarState::default();
        let fires = run_clock(&mut st, &e, utc(2026, 11, 1, 6, 0, 0), utc(2026, 11, 1, 8, 0, 0), 30, &Chicago2026);
        assert_eq!(fires, [utc(2026, 11, 1, 6, 30, 0)]);
    }

    #[test]
    fn off_entries_and_entries_from_a_newer_version_never_fire() {
        let off = CalendarEntry { on: false, ..sunday_at(10, 45) };
        let newer = CalendarEntry { run: Lenient::Unknown(json!({"type":"teleport"})), ..sunday_at(10, 45) };
        for e in [off, newer] {
            assert!(run_clock(&mut CalendarState::default(), &e, utc(2026, 6, 7, 15, 44, 0), utc(2026, 6, 7, 15, 46, 0), 5, &Chicago2026).is_empty());
        }
    }

    #[test]
    fn next_due_and_soon() {
        let e = CalendarEntry { days: day_bit(Weekday::Sun) | day_bit(Weekday::Wed), ..sunday_at(10, 45) };
        // From Sunday 11:00 Chicago, the next is Wednesday 10:45 (15:45 UTC on June 10).
        assert_eq!(next_due(&e, utc(2026, 6, 7, 16, 0, 0), &Chicago2026), Some(utc(2026, 6, 10, 15, 45, 0)));
        assert_eq!(next_due(&CalendarEntry { days: 0, ..e.clone() }, utc(2026, 6, 7, 16, 0, 0), &Chicago2026), None);
        assert!(CalendarState::soon(std::slice::from_ref(&e), utc(2026, 6, 10, 15, 44, 56), &Chicago2026));
        assert!(!CalendarState::soon(std::slice::from_ref(&e), utc(2026, 6, 10, 15, 44, 0), &Chicago2026));
        // A fixed zone with no summer time works the same.
        let tokyo = FixedOffset::east_opt(9 * 3600).unwrap();
        assert_eq!(next_due(&e, utc(2026, 6, 7, 0, 0, 0), &tokyo), Some(utc(2026, 6, 7, 1, 45, 0)));
    }

    #[test]
    fn as_macro_and_validation() {
        let mut a = super::super::Automation::default();
        let e = sunday_at(10, 45);
        let m = as_macro(&e, &[]).unwrap();
        assert_eq!((m.name.as_str(), m.steps.len()), ("Countdown", 1));
        let gone = CalendarEntry { run: Lenient::Known(CalendarRun::Macro { macro_id: Uuid::from_u128(9) }), ..e.clone() };
        assert!(as_macro(&gone, &[]).is_err());
        assert!(validate(&a, gone).is_err());
        assert_eq!(validate(&a, CalendarEntry { days: 0, ..e.clone() }), Err(super::super::strings::calendar_no_days()));
        assert_eq!(validate(&a, CalendarEntry { name: "  ".into(), ..e.clone() }), Err(super::super::strings::name_length()));
        a.calendar.push(e.clone());
        assert_eq!(validate(&a, CalendarEntry { days: 0xff, ..e.clone() }).unwrap().days, EVERY_DAY);
    }

    #[test]
    fn entries_load_leniently() {
        let e: CalendarEntry = serde_json::from_str(r#"{"name":"X","at":"10:45","days":"lots","run":{"type":"macro","macro_id":"00000000-0000-0000-0000-000000000009"}}"#).unwrap();
        assert_eq!(e.days, 0);
        assert!(e.on);
        assert!(matches!(e.run.known(), Some(CalendarRun::Macro { .. })));
        let back = serde_json::to_value(&e).unwrap();
        assert_eq!(back["run"]["type"], "macro");
    }
}
