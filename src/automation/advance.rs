//! FEAT-2 slide auto-advance (spec §3.5, §5.6): arm when the visual changes, wait for the
//! commit, then turn slides every N seconds, looping or stopping on the last slide.
use crate::automation::strings;
use crate::timers::AutoReadout;
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// The interval a new item's Item edit section starts with.
pub const DEFAULT_SECS: u16 = 8;
const MIN_SECS: u16 = 2;
const MAX_SECS: u16 = 600;
/// The shortest time a slide's own auto-advance waits (PowerPoint allows 0).
const MIN_SLIDE_MS: u32 = 500;

fn yes() -> bool { true }

/// An item's auto-advance setting. `secs` has no default on purpose: `{}` is not a setting,
/// so `lenient_advance` loads it as `None`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct AutoAdvance {
    pub secs: u16,
    #[serde(default = "yes")]
    pub loop_item: bool,
}

impl Default for AutoAdvance {
    fn default() -> Self { AutoAdvance { secs: DEFAULT_SECS, loop_item: true } }
}

impl AutoAdvance {
    /// Time on each slide, kept to 2–600 s even if a stored value is outside it.
    pub fn interval(&self) -> TimeDelta { TimeDelta::seconds(self.secs.clamp(MIN_SECS, MAX_SECS).into()) }

    pub fn validate(secs: u16) -> Result<(), String> {
        if (MIN_SECS..=MAX_SECS).contains(&secs) { Ok(()) } else { Err(strings::advance_range()) }
    }
}

/// `deserialize_with` for `ServiceItem.advance` / `LiveItem.advance`: any value this version
/// can't read loads as `None` instead of failing the service.
pub fn lenient_advance<'de, D: Deserializer<'de>>(d: D) -> Result<Option<AutoAdvance>, D::Error> {
    crate::lenient::or_default::<_, Option<AutoAdvance>>(d)
}

/// What auto-advance needs to know about one engine's live state, as plain values.
/// The caller passes `None` when nothing is live.
#[derive(Clone, Debug, PartialEq)]
pub struct AdvanceInput {
    pub item_id: Uuid,
    pub slides: usize,
    /// The LiveItem snapshot, so an edit takes effect next time the item goes live.
    pub advance: Option<AutoAdvance>,
    pub cursor_slide: usize,
    /// The live slide's own time (a PowerPoint slide's "After"), used before the item's interval.
    pub slide_ms: Option<u32>,
    pub slide_layer_up: bool,
    pub version: u64,
    pub visual_version: u64,
}

impl AdvanceInput {
    fn interval(&self) -> Option<TimeDelta> {
        match self.slide_ms {
            Some(ms) => Some(TimeDelta::milliseconds(ms.max(MIN_SLIDE_MS).into())),
            None => self.advance.map(|a| a.interval()),
        }
    }
}

/// One engine's running auto-advance.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoRun {
    pub item_id: Uuid,
    pub slide: usize,
    pub armed_visual: u64,
    /// `None` once stopped on the last slide of a non-looping item.
    pub due: Option<DateTime<Utc>>,
    /// The version whose commit restarts the count; nothing is due until it arrives.
    pub waiting_for: Option<u64>,
}

/// What the ticker sends when a slide's time is up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoNextReq { pub item_id: Uuid, pub from_slide: usize, pub armed_visual: u64 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoNextStep { Goto(usize), StopOnLast, Ignore }

/// Auto-advance for one engine: the run and the item the operator paused (Plan decision 5).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AutoState { pub run: Option<AutoRun>, pub paused_item: Option<Uuid> }

impl AutoState {
    /// Runs after every accepted command and every report that changed the engine.
    /// Re-arms only when the visual changes, so overlays and adoptions don't restart the count.
    pub fn reconcile(&mut self, live: Option<&AdvanceInput>, now: DateTime<Utc>) {
        if self.paused_item.is_some() && self.paused_item != live.map(|l| l.item_id) {
            self.paused_item = None;
        }
        let Some(l) = live.filter(|l| l.slides >= 2 && l.slide_layer_up) else {
            self.run = None;
            return;
        };
        let Some(interval) = l.interval() else {
            self.run = None;
            return;
        };
        let current = self.run.as_ref().is_some_and(|r| r.item_id == l.item_id && r.armed_visual == l.visual_version);
        if !current {
            self.run = Some(AutoRun {
                item_id: l.item_id,
                slide: l.cursor_slide,
                armed_visual: l.visual_version,
                due: Some(now + interval),
                waiting_for: Some(l.version),
            });
        }
    }

    /// The room committed `committed`: if that covers the version we wait for, the full
    /// interval starts now (slow preparation doesn't shorten a slide).
    pub fn on_committed(&mut self, committed: u64, live: Option<&AdvanceInput>, now: DateTime<Utc>) {
        let Some(interval) = live.and_then(AdvanceInput::interval) else { return };
        if let Some(run) = self.run.as_mut()
            && run.waiting_for.is_some_and(|w| committed >= w)
        {
            run.due = Some(now + interval);
            run.waiting_for = None;
        }
    }

    pub fn due(&self, now: DateTime<Utc>) -> Option<AutoNextReq> {
        let run = self.run.as_ref()?;
        if self.paused_item == Some(run.item_id) || run.waiting_for.is_some() || run.due? > now {
            return None;
        }
        Some(AutoNextReq { item_id: run.item_id, from_slide: run.slide, armed_visual: run.armed_visual })
    }

    /// The last slide of a non-looping item: stopped until the visual changes.
    pub fn stop_on_last(&mut self) {
        if let Some(run) = self.run.as_mut() {
            run.due = None;
        }
    }

    /// `auto/set {on}`: pausing holds the live item; resuming gives it a full interval.
    pub fn set_on(&mut self, on: bool, live: Option<&AdvanceInput>, now: DateTime<Utc>) {
        if !on {
            self.paused_item = live.map(|l| l.item_id);
            return;
        }
        self.paused_item = None;
        let Some(interval) = live.and_then(AdvanceInput::interval) else { return };
        if let Some(run) = self.run.as_mut() {
            run.due = Some(now + interval);
            run.waiting_for = None;
        }
    }

    pub fn readout(&self, now: DateTime<Utc>) -> Option<AutoReadout> {
        let run = self.run.as_ref()?;
        let left_ms = (run.due? - now).num_milliseconds().max(0);
        let next_in_secs = u32::try_from((left_ms + 999) / 1000).unwrap_or(u32::MAX);
        Some(AutoReadout { next_in_secs, paused: self.paused_item == Some(run.item_id) })
    }

    /// FEAT-4 adopted an edited item: follow the run's slide through the old → new map, or
    /// drop the run if its slide is gone. `due` is kept.
    pub fn on_adopt(&mut self, item_id: Uuid, map: &[Option<usize>]) {
        let Some(run) = self.run.as_mut().filter(|r| r.item_id == item_id) else { return };
        match map.get(run.slide).copied().flatten() {
            Some(slide) => run.slide = slide,
            None => self.run = None,
        }
    }
}

/// What an `AutoNext` does, checked against the engine as it is now. Never crosses items.
pub fn auto_next_step(req: &AutoNextReq, live: Option<&AdvanceInput>) -> AutoNextStep {
    let Some(l) = live.filter(|l| {
        l.item_id == req.item_id
            && l.cursor_slide == req.from_slide
            && l.slide_layer_up
            && l.visual_version == req.armed_visual
    }) else {
        return AutoNextStep::Ignore;
    };
    if req.from_slide + 1 < l.slides {
        AutoNextStep::Goto(req.from_slide + 1)
    } else if l.advance.is_some_and(|a| a.loop_item) {
        AutoNextStep::Goto(0)
    } else {
        AutoNextStep::StopOnLast
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};
    use serde::Deserialize;
    use uuid::Uuid;

    const ITEM: Uuid = Uuid::from_u128(0xa1);
    const OTHER: Uuid = Uuid::from_u128(0xa2);

    fn t0() -> DateTime<Utc> { Utc.with_ymd_and_hms(2026, 11, 1, 10, 0, 0).unwrap() }
    fn at(ms: i64) -> DateTime<Utc> { t0() + TimeDelta::milliseconds(ms) }

    fn live(slide: usize, version: u64, visual: u64) -> AdvanceInput {
        AdvanceInput {
            item_id: ITEM,
            slides: 3,
            advance: Some(AutoAdvance { secs: 8, loop_item: true }),
            cursor_slide: slide,
            slide_ms: None,
            slide_layer_up: true,
            version,
            visual_version: visual,
        }
    }

    /// Armed at t0 on `slide` and committed at t0, so due at t0 + 8 s.
    fn running(slide: usize) -> (AutoState, AdvanceInput) {
        let l = live(slide, 1, 1);
        let mut s = AutoState::default();
        s.reconcile(Some(&l), t0());
        s.on_committed(1, Some(&l), t0());
        (s, l)
    }

    fn due_at(s: &AutoState) -> Option<DateTime<Utc>> { s.run.as_ref().and_then(|r| r.due) }

    #[test]
    fn auto_advance_json_defaults() {
        let a: AutoAdvance = serde_json::from_str(r#"{"secs":8}"#).unwrap();
        assert_eq!(a, AutoAdvance { secs: 8, loop_item: true });
        assert_eq!(AutoAdvance::default(), AutoAdvance { secs: DEFAULT_SECS, loop_item: true });
        assert_eq!(AutoAdvance { secs: 1, loop_item: true }.interval(), TimeDelta::seconds(2));
        assert_eq!(AutoAdvance { secs: 9000, loop_item: true }.interval(), TimeDelta::seconds(600));
        assert_eq!(AutoAdvance::validate(2), Ok(()));
        assert_eq!(AutoAdvance::validate(600), Ok(()));
        assert_eq!(AutoAdvance::validate(1), Err(crate::automation::strings::advance_range()));
        assert_eq!(AutoAdvance::validate(601), Err(crate::automation::strings::advance_range()));
    }

    #[test]
    fn bad_advance_drops_field() {
        #[derive(Deserialize)]
        struct Holder {
            #[serde(default, deserialize_with = "lenient_advance")]
            advance: Option<AutoAdvance>,
            #[serde(default)]
            name: String,
        }
        let h: Holder = serde_json::from_str(r#"{"advance":{},"name":"A"}"#).unwrap();
        assert_eq!(h.advance, None);
        assert_eq!(h.name, "A");
        for src in [r#"{"advance":{"secs":"x"}}"#, r#"{"advance":{"secs":70000}}"#, r#"{"advance":5}"#,
                    r#"{"advance":null}"#, r#"{}"#] {
            assert_eq!(serde_json::from_str::<Holder>(src).unwrap().advance, None, "{src}");
        }
        let h: Holder = serde_json::from_str(r#"{"advance":{"secs":10,"loop_item":false}}"#).unwrap();
        assert_eq!(h.advance, Some(AutoAdvance { secs: 10, loop_item: false }));
    }

    #[test]
    fn a_slides_own_time_turns_it_and_stops_on_the_last() {
        let own = |slide: usize, visual: u64, ms: Option<u32>| AdvanceInput { advance: None, slide_ms: ms, ..live(slide, visual, visual) };
        let l = own(0, 1, Some(5_000));
        let mut s = AutoState::default();
        s.reconcile(Some(&l), t0());
        s.on_committed(1, Some(&l), t0());
        assert_eq!(s.due(at(4_900)), None);
        let req = s.due(at(5_000)).unwrap();
        assert_eq!(auto_next_step(&req, Some(&l)), AutoNextStep::Goto(1));
        // The next slide has no time of its own: nothing more.
        s.reconcile(Some(&own(1, 2, None)), at(5_000));
        assert_eq!(s.run, None);
        // A slide's own time wins over the item's interval; the last slide stops there.
        let l = AdvanceInput { advance: Some(AutoAdvance { secs: 8, loop_item: false }), ..own(2, 3, Some(0)) };
        s.reconcile(Some(&l), at(6_000));
        s.on_committed(3, Some(&l), at(6_000));
        let req = s.due(at(6_500)).unwrap();
        assert_eq!(auto_next_step(&req, Some(&l)), AutoNextStep::StopOnLast);
        assert_eq!(auto_next_step(&req, Some(&AdvanceInput { advance: None, ..l.clone() })), AutoNextStep::StopOnLast);
        // Back or Next restarts the count on the new slide's time.
        let l = own(1, 4, Some(3_000));
        s.reconcile(Some(&l), at(7_000));
        s.on_committed(4, Some(&l), at(7_000));
        assert_eq!(due_at(&s), Some(at(10_000)));
        // Clearing stops it.
        s.reconcile(Some(&AdvanceInput { slide_layer_up: false, ..own(1, 5, Some(3_000)) }), at(8_000));
        assert_eq!(s.run, None);
    }

    #[test]
    fn auto_next_turns_slides_on_interval() {
        let (s, l) = running(0);
        assert_eq!(s.due(at(7_900)), None);
        let req = s.due(at(8_000)).unwrap();
        assert_eq!(req, AutoNextReq { item_id: ITEM, from_slide: 0, armed_visual: 1 });
        assert_eq!(auto_next_step(&req, Some(&l)), AutoNextStep::Goto(1));
    }

    #[test]
    fn loops_to_first_slide() {
        let (s, l) = running(2);
        let req = s.due(at(8_000)).unwrap();
        assert_eq!(auto_next_step(&req, Some(&l)), AutoNextStep::Goto(0));
    }

    #[test]
    fn stops_on_last_without_loop() {
        let mut l = live(2, 1, 1);
        l.advance = Some(AutoAdvance { secs: 8, loop_item: false });
        let mut s = AutoState::default();
        s.reconcile(Some(&l), t0());
        s.on_committed(1, Some(&l), t0());
        let req = s.due(at(8_000)).unwrap();
        assert_eq!(auto_next_step(&req, Some(&l)), AutoNextStep::StopOnLast);
        s.stop_on_last();
        assert_eq!(s.due(at(60_000)), None);
        assert_eq!(s.readout(at(60_000)), None);
    }

    #[test]
    fn stopped_auto_rearms_only_on_visual_change() {
        let (mut s, _) = running(2);
        s.stop_on_last();
        s.reconcile(Some(&live(2, 2, 1)), at(9_000));
        assert_eq!(due_at(&s), None);
        s.reconcile(Some(&live(0, 3, 2)), at(10_000));
        let run = s.run.clone().unwrap();
        assert_eq!((run.slide, run.armed_visual, run.due, run.waiting_for), (0, 2, Some(at(18_000)), Some(3)));
    }

    #[test]
    fn never_crosses_into_next_item() {
        for loop_item in [true, false] {
            for slide in 0..3 {
                let mut l = live(slide, 1, 1);
                l.advance = Some(AutoAdvance { secs: 8, loop_item });
                let req = AutoNextReq { item_id: ITEM, from_slide: slide, armed_visual: 1 };
                match auto_next_step(&req, Some(&l)) {
                    AutoNextStep::Goto(i) => assert!(i < l.slides),
                    AutoNextStep::StopOnLast => assert!(!loop_item && slide == 2),
                    AutoNextStep::Ignore => panic!("ignored a valid request"),
                }
            }
        }
    }

    #[test]
    fn manual_next_restarts_interval() {
        let (mut s, _) = running(0);
        let l = live(1, 2, 2);
        s.reconcile(Some(&l), at(5_000));
        s.on_committed(2, Some(&l), at(5_000));
        assert_eq!(due_at(&s), Some(at(13_000)));
        assert_eq!(s.due(at(12_900)), None);
        assert_eq!(s.due(at(13_000)).map(|r| r.from_slide), Some(1));
    }

    #[test]
    fn clear_slide_stops_until_next_show() {
        let (mut s, _) = running(0);
        let mut down = live(0, 2, 2);
        down.slide_layer_up = false;
        s.reconcile(Some(&down), at(3_000));
        assert_eq!(s.run, None);
        let up = live(0, 3, 3);
        s.reconcile(Some(&up), at(20_000));
        s.on_committed(3, Some(&up), at(20_000));
        assert_eq!(due_at(&s), Some(at(28_000)));
    }

    #[test]
    fn commit_restarts_due_time() {
        let l = live(0, 1, 1);
        let mut s = AutoState::default();
        s.reconcile(Some(&l), t0());
        s.on_committed(1, Some(&l), at(3_000));
        assert_eq!(due_at(&s), Some(at(11_000)));
        assert_eq!(s.run.as_ref().unwrap().waiting_for, None);
    }

    #[test]
    fn commit_at_or_after_waiting_for_restarts_due() {
        let l = live(0, 5, 1);
        let mut s = AutoState::default();
        s.reconcile(Some(&l), t0());
        assert_eq!(s.run.as_ref().unwrap().waiting_for, Some(5));
        s.on_committed(4, Some(&l), at(1_000));
        assert_eq!(s.run.as_ref().unwrap().waiting_for, Some(5));
        assert_eq!(due_at(&s), Some(at(8_000)));
        s.on_committed(6, Some(&l), at(2_000));
        assert_eq!(s.run.as_ref().unwrap().waiting_for, None);
        assert_eq!(due_at(&s), Some(at(10_000)));
    }

    #[test]
    fn does_not_fire_while_waiting_for_commit() {
        let mut s = AutoState::default();
        s.reconcile(Some(&live(0, 1, 1)), t0());
        assert_eq!(s.due(at(20_000)), None);
    }

    #[test]
    fn auto_next_ignored_after_operator_moved() {
        let (s, _) = running(0);
        let req = s.due(at(8_000)).unwrap();
        assert_eq!(auto_next_step(&req, Some(&live(1, 2, 1))), AutoNextStep::Ignore);
        assert_eq!(auto_next_step(&req, Some(&live(0, 2, 2))), AutoNextStep::Ignore);
        let mut down = live(0, 2, 1);
        down.slide_layer_up = false;
        assert_eq!(auto_next_step(&req, Some(&down)), AutoNextStep::Ignore);
        let mut other = live(0, 2, 1);
        other.item_id = OTHER;
        assert_eq!(auto_next_step(&req, Some(&other)), AutoNextStep::Ignore);
        assert_eq!(auto_next_step(&req, None), AutoNextStep::Ignore);
    }

    #[test]
    fn message_show_does_not_restart_interval() {
        let (mut s, _) = running(0);
        let l = live(0, 2, 1);
        s.reconcile(Some(&l), at(5_000));
        s.on_committed(2, Some(&l), at(5_000));
        assert_eq!(due_at(&s), Some(at(8_000)));
    }

    #[test]
    fn one_slide_item_never_arms() {
        let mut l = live(0, 1, 1);
        l.slides = 1;
        let mut s = AutoState::default();
        s.reconcile(Some(&l), t0());
        assert_eq!(s.run, None);
        let mut none = live(0, 1, 1);
        none.advance = None;
        s.reconcile(Some(&none), t0());
        assert_eq!(s.run, None);
        s.reconcile(None, t0());
        assert_eq!(s.run, None);
    }

    #[test]
    fn video_item_ignores_advance() {
        let mut l = live(0, 1, 1);
        l.slide_layer_up = false;
        let mut s = AutoState::default();
        s.reconcile(Some(&l), t0());
        s.on_committed(1, Some(&l), t0());
        assert_eq!(s.run, None);
        assert_eq!(s.due(at(60_000)), None);
    }

    #[test]
    fn adoption_does_not_rearm() {
        let (mut s, _) = running(1);
        s.on_adopt(ITEM, &[Some(0), Some(2), Some(1)]);
        // Adoption is None or Silent: same visual_version, so reconcile keeps the run as is.
        s.reconcile(Some(&live(2, 2, 1)), at(5_000));
        let run = s.run.clone().unwrap();
        assert_eq!((run.slide, run.due, run.waiting_for), (2, Some(at(8_000)), None));
    }

    #[test]
    fn on_adopt_remaps_auto_run_slide() {
        let map = [Some(0), None, Some(1)];
        let (mut s, _) = running(2);
        s.on_adopt(OTHER, &map);
        assert_eq!(s.run.as_ref().unwrap().slide, 2);
        s.on_adopt(ITEM, &map);
        assert_eq!(s.run.as_ref().unwrap().slide, 1);
        assert_eq!(due_at(&s), Some(at(8_000)));
        let (mut s, _) = running(1);
        s.on_adopt(ITEM, &map);
        assert_eq!(s.run, None);
    }

    #[test]
    fn on_adopt_drops_stale_auto_next() {
        let (mut s, _) = running(2);
        let stale = s.due(at(8_000)).unwrap();
        s.on_adopt(ITEM, &[Some(0), None, Some(1)]);
        let l = live(1, 2, 1);
        assert_eq!(auto_next_step(&stale, Some(&l)), AutoNextStep::Ignore);
        let fresh = s.due(at(8_000)).unwrap();
        assert_eq!(fresh.from_slide, 1);
        assert_eq!(auto_next_step(&fresh, Some(&l)), AutoNextStep::Goto(2));
    }

    #[test]
    fn pause_and_resume_rearms_full_interval() {
        let (mut s, l) = running(0);
        s.set_on(false, Some(&l), at(3_000));
        assert_eq!(s.paused_item, Some(ITEM));
        assert_eq!(s.due(at(30_000)), None);
        assert!(s.readout(at(3_000)).unwrap().paused);
        s.set_on(true, Some(&l), at(30_000));
        assert_eq!(s.paused_item, None);
        assert_eq!(s.due(at(37_900)), None);
        assert!(s.due(at(38_000)).is_some());
        let r = s.readout(at(30_500)).unwrap();
        assert_eq!((r.next_in_secs, r.paused), (8, false));
        assert_eq!(s.readout(at(40_000)).unwrap().next_in_secs, 0);
    }

    #[test]
    fn pause_ends_when_another_item_goes_live() {
        let (mut s, l) = running(0);
        s.set_on(false, Some(&l), at(1_000));
        let mut next = live(0, 2, 2);
        next.item_id = OTHER;
        s.reconcile(Some(&next), at(2_000));
        assert_eq!(s.paused_item, None);
        s.on_committed(2, Some(&next), at(2_000));
        assert_eq!(s.due(at(10_000)).map(|r| r.item_id), Some(OTHER));
    }
}
