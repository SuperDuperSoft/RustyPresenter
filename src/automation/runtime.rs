//! FEAT-2's memory-only state (spec §4.3): each timer's run, auto-advance, message auto-hide, the
//! pre-service run and the last readout sent. What is on screen is always read from the frame.
use crate::automation::advance::AutoState;
use crate::automation::messages::MessageTemplate;
use crate::automation::preservice::PreRun;
use crate::timers::{AtZero, AutoReadout, Builtin, PRE_SERVICE_TIMER_ID, RunState, TimeOfDay, TimerDef, TimerKind, TimerRun, TimersView};
use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// The ticker wakes this often while something runs (spec §5.8).
pub const WAKE_MS: u64 = 200;
/// Readouts go out at most this often.
pub const MIN_GAP_MS: i64 = 250;

/// The newest readout of the app, for the drawing code that has no `App` (bound overlay text).
static LATEST: RwLock<Option<Arc<TimersView>>> = RwLock::new(None);

/// The newest published readout (empty before the first).
pub fn latest() -> Arc<TimersView> { LATEST.read().ok().and_then(|l| l.clone()).unwrap_or_default() }

fn set_latest(view: Arc<TimersView>) { if let Ok(mut l) = LATEST.write() { *l = Some(view) } }

/// Something the ticker found due; the App turns each into an internal command.
#[derive(Clone, Debug, PartialEq)]
pub enum Due {
    TimerAtZero { timer: Uuid, run: u64 },
    PreServiceEnd { run: u64 },
}

#[derive(Default)]
pub struct AutomationRuntime {
    pub timers: HashMap<Uuid, TimerRun>,
    /// The room engine's auto-advance (there is one engine until practice mode).
    pub auto: AutoState,
    /// When each shown message (by its `shown` id) hides by itself.
    pub hide_at: HashMap<Uuid, DateTime<Utc>>,
    pub pre: Option<PreRun>,
    /// The template Show message opens on.
    pub last_message: Option<Uuid>,
    /// The `shown` id of the message last seen up, so a new one is noticed once.
    pub message_up: Option<Uuid>,
    published: Arc<TimersView>,
    published_at: Option<DateTime<Utc>>,
}

impl AutomationRuntime {
    pub fn run_mut(&mut self, id: Uuid) -> &mut TimerRun { self.timers.entry(id).or_default() }

    /// Every timer's readout in Timers-tab order; timers of an unknown kind are left out.
    pub fn view<Tz: TimeZone>(&self, defs: &[TimerDef], now: DateTime<Utc>, tz: &Tz, starts_at: Option<TimeOfDay>, auto: Option<AutoReadout>) -> TimersView {
        let stopped = TimerRun::default();
        let timers = defs.iter().filter_map(|d| self.timers.get(&d.id).unwrap_or(&stopped).snapshot(d, now, tz, starts_at)).collect();
        TimersView { timers, auto }
    }

    /// Countdowns that reached zero since the last look, once per run.
    pub fn zero_crossings(&mut self, defs: &[TimerDef], now: DateTime<Utc>) -> Vec<Due> {
        let mut out = vec![];
        for d in defs {
            let Some(run) = self.timers.get_mut(&d.id) else { continue };
            if !run.check_zero(d, now) { continue }
            if d.builtin == Some(Builtin::PreService) {
                out.push(Due::PreServiceEnd { run: run.run });
            } else if matches!(d.at_zero.known(), Some(AtZero::HideFromRoom | AtZero::RunMacro { .. })) {
                out.push(Due::TimerAtZero { timer: d.id, run: run.run });
            }
        }
        out
    }

    /// A message went up: it is the one Show message opens on next, and it may hide by itself.
    pub fn message_shown(&mut self, template: Uuid, shown: Uuid, hide_after_secs: Option<u32>, now: DateTime<Utc>) {
        self.last_message = Some(template);
        if let Some(s) = hide_after_secs { self.hide_at.insert(shown, now + TimeDelta::seconds(s.into())); }
    }

    /// The messages whose time is up, each once.
    pub fn due_hides(&mut self, now: DateTime<Utc>) -> Vec<Uuid> {
        let due: Vec<Uuid> = self.hide_at.iter().filter(|(_, at)| **at <= now).map(|(s, _)| *s).collect();
        for s in &due { self.hide_at.remove(s); }
        due
    }

    /// Forgets auto-hides of messages no longer up, and the runs of deleted timers.
    pub fn prune(&mut self, defs: &[TimerDef], shown: Option<Uuid>) {
        self.hide_at.retain(|s, _| Some(*s) == shown);
        self.timers.retain(|id, _| defs.iter().any(|d| d.id == *id));
    }

    /// Nothing runs, waits or shows a changing readout, so the ticker can sleep (Plan decision 15).
    pub fn is_idle(&self, defs: &[TimerDef], room_timers: &[Uuid], watching: bool) -> bool {
        let busy = defs.iter().any(|d| {
            let state = self.timers.get(&d.id).map(|r| &r.state);
            let running = matches!(state, Some(RunState::Running { .. } | RunState::RunningTo { .. }));
            let to_time = matches!(d.kind.known(), Some(TimerKind::UntilTime { .. }));
            let stopped = matches!(state, None | Some(RunState::Stopped));
            running || (to_time && stopped && (room_timers.contains(&d.id) || d.on_remote || d.on_stage || watching))
        });
        !busy && self.hide_at.is_empty() && self.auto.run.as_ref().is_none_or(|r| r.due.is_none() && r.waiting_for.is_none())
    }

    /// Reset; resetting the pre-service timer also forgets its run.
    pub fn reset_timer(&mut self, id: Uuid) {
        self.run_mut(id).reset();
        if id == PRE_SERVICE_TIMER_ID { self.pre = None }
    }

    /// The template Show message opens on: the last one used if it still exists, else the first.
    pub fn show_message_template(&self, templates: &[MessageTemplate]) -> Option<Uuid> {
        self.last_message.filter(|id| templates.iter().any(|t| t.id == *id)).or(templates.first().map(|t| t.id))
    }

    /// FEAT-4 adopted an edited live item: follow auto-advance through the old → new slide map.
    pub fn on_adopt(&mut self, item_id: Uuid, map: &[Option<usize>]) { self.auto.on_adopt(item_id, map) }

    /// Publishes `view` when it differs from the last one and the last went out at least 250 ms ago.
    pub fn offer(&mut self, view: TimersView, now: DateTime<Utc>) -> Option<Arc<TimersView>> {
        if view == *self.published { return None }
        if self.published_at.is_some_and(|at| (now - at).num_milliseconds() < MIN_GAP_MS) { return None }
        let view = Arc::new(view);
        self.published = view.clone();
        self.published_at = Some(now);
        set_latest(view.clone());
        Some(view)
    }

    /// Publishes `view` now if it differs, however soon after the last (a button was pressed).
    pub fn offer_now(&mut self, view: TimersView, now: DateTime<Utc>) -> Option<Arc<TimersView>> {
        self.published_at = None;
        self.offer(view, now)
    }

    /// The readout last published.
    pub fn current(&self) -> Arc<TimersView> { self.published.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::advance::AutoRun;
    use crate::lenient::Lenient;
    use crate::timers::{Overrun, SERMON_ID, StartCtx, TimeTarget, TimerState};

    fn t0() -> DateTime<Utc> { Utc.with_ymd_and_hms(2026, 6, 7, 15, 0, 0).unwrap() }
    fn ms(n: i64) -> DateTime<Utc> { t0() + TimeDelta::milliseconds(n) }
    fn def(id: u128, kind: TimerKind, at_zero: AtZero) -> TimerDef {
        TimerDef { id: Uuid::from_u128(id), name: format!("T{id}"), kind: Lenient::Known(kind), at_zero: Lenient::Known(at_zero), overrun: Overrun::Stop, ..TimerDef::default() }
    }
    fn ctx(now: DateTime<Utc>) -> StartCtx<'static, Utc> { StartCtx { now, tz: &Utc, starts_at: None, clear_epoch: 0 } }

    #[test]
    fn view_in_definition_order_without_unknown_kinds_or_deleted_runs() {
        let a = def(1, TimerKind::Duration { secs: 60 }, AtZero::Nothing);
        let mut b = def(2, TimerKind::CountUp, AtZero::Nothing);
        let unknown = TimerDef { kind: Lenient::Unknown(serde_json::json!({"type": "lap"})), ..def(3, TimerKind::CountUp, AtZero::Nothing) };
        let mut rt = AutomationRuntime::default();
        rt.run_mut(Uuid::from_u128(99)).start(&a, &ctx(t0())).unwrap();
        rt.run_mut(b.id).start(&b, &ctx(t0())).unwrap();
        b.name = "B".into();
        let v = rt.view(&[b.clone(), unknown, a.clone()], ms(2000), &Utc, None, None);
        assert_eq!(v.timers.iter().map(|t| t.id).collect::<Vec<_>>(), [b.id, a.id]);
        assert_eq!(v.timers[0].state, TimerState::Running);
        assert_eq!(v.timers[1].value_ms, 60_000);
    }

    #[test]
    fn zero_crossing_emits_at_zero_once_and_ignores_nothing_and_deleted() {
        let hide = def(1, TimerKind::Duration { secs: 1 }, AtZero::HideFromRoom);
        let quiet = def(2, TimerKind::Duration { secs: 1 }, AtZero::Nothing);
        let mut rt = AutomationRuntime::default();
        for d in [&hide, &quiet] { rt.run_mut(d.id).start(d, &ctx(t0())).unwrap() }
        assert!(rt.zero_crossings(&[hide.clone(), quiet.clone()], ms(500)).is_empty());
        assert_eq!(rt.zero_crossings(&[hide.clone(), quiet.clone()], ms(1000)), [Due::TimerAtZero { timer: hide.id, run: 0 }]);
        assert!(rt.zero_crossings(&[hide.clone(), quiet], ms(2000)).is_empty());
        let gone = def(5, TimerKind::Duration { secs: 1 }, AtZero::HideFromRoom);
        rt.run_mut(gone.id).start(&gone, &ctx(t0())).unwrap();
        assert!(rt.zero_crossings(&[hide], ms(3000)).is_empty());
    }

    #[test]
    fn preservice_timer_emits_preservice_end() {
        let pre = TimerDef { id: PRE_SERVICE_TIMER_ID, builtin: Some(Builtin::PreService), ..def(4, TimerKind::Duration { secs: 1 }, AtZero::Nothing) };
        let mut rt = AutomationRuntime::default();
        rt.run_mut(pre.id).start(&pre, &ctx(t0())).unwrap();
        assert_eq!(rt.zero_crossings(&[pre], ms(1000)), [Due::PreServiceEnd { run: 0 }]);
    }

    #[test]
    fn message_hide_after_and_due_hides_once() {
        let (n, s1) = (Uuid::from_u128(0x11), Uuid::from_u128(0xa1));
        let mut rt = AutomationRuntime::default();
        rt.message_shown(n, s1, Some(30), t0());
        assert_eq!(rt.last_message, Some(n));
        assert!(rt.due_hides(ms(29_000)).is_empty());
        assert_eq!(rt.due_hides(ms(30_000)), [s1]);
        assert!(rt.due_hides(ms(31_000)).is_empty());
        rt.message_shown(n, Uuid::from_u128(0xa2), None, t0());
        assert!(rt.hide_at.is_empty());
    }

    #[test]
    fn prune_drops_hides_of_messages_not_up() {
        let mut rt = AutomationRuntime::default();
        rt.message_shown(Uuid::nil(), Uuid::from_u128(1), Some(30), t0());
        rt.prune(&[], Some(Uuid::from_u128(1)));
        assert_eq!(rt.hide_at.len(), 1);
        rt.prune(&[], None);
        assert!(rt.hide_at.is_empty());
    }

    #[test]
    fn idle_only_when_nothing_runs_or_waits() {
        let d = def(1, TimerKind::Duration { secs: 60 }, AtZero::Nothing);
        let to_time = def(2, TimerKind::UntilTime { target: TimeTarget::ServiceStart }, AtZero::Nothing);
        let mut rt = AutomationRuntime::default();
        assert!(rt.is_idle(&[d.clone(), to_time.clone()], &[], false));
        assert!(!rt.is_idle(&[to_time.clone()], &[to_time.id], false), "a stopped countdown to a time in the room changes every second");
        assert!(!rt.is_idle(&[to_time.clone()], &[], true));
        assert!(rt.is_idle(&[d.clone()], &[d.id], false), "a stopped length doesn't change");
        rt.run_mut(d.id).start(&d, &ctx(t0())).unwrap();
        assert!(!rt.is_idle(&[d.clone()], &[], false));
        rt.run_mut(d.id).reset();
        rt.auto.run = Some(AutoRun { item_id: Uuid::nil(), slide: 0, armed_visual: 1, due: Some(t0()), waiting_for: None });
        assert!(!rt.is_idle(&[d], &[], false));
    }

    #[test]
    fn show_message_opens_on_last_template_or_first() {
        let t = |id| MessageTemplate { id: Uuid::from_u128(id), ..MessageTemplate::default() };
        let mut rt = AutomationRuntime::default();
        assert_eq!(rt.show_message_template(&[t(1), t(2)]), Some(Uuid::from_u128(1)));
        rt.last_message = Some(Uuid::from_u128(2));
        assert_eq!(rt.show_message_template(&[t(1), t(2)]), Some(Uuid::from_u128(2)));
        assert_eq!(rt.show_message_template(&[t(1)]), Some(Uuid::from_u128(1)));
    }

    #[test]
    fn publishes_changes_at_most_every_250ms_and_shares_the_arc() {
        let mut rt = AutomationRuntime::default();
        let v = |n| TimersView { timers: vec![], auto: Some(AutoReadout { next_in_secs: n, paused: false }) };
        let first = rt.offer(v(5), t0()).expect("a change goes out");
        assert!(Arc::ptr_eq(&first, &rt.current()));
        assert!(rt.offer(v(5), ms(1000)).is_none(), "same view");
        assert!(rt.offer(v(4), ms(1100)).is_some());
        assert!(rt.offer(v(3), ms(1200)).is_none(), "too soon");
        assert!(rt.offer(v(3), ms(1350)).is_some());
    }

    #[test]
    fn reset_preservice_timer_clears_pre() {
        let mut rt = AutomationRuntime::default();
        rt.pre = Some(PreRun { manual_version: 1, loop_item: None, timer_run: 0 });
        rt.reset_timer(SERMON_ID);
        assert!(rt.pre.is_some());
        rt.reset_timer(PRE_SERVICE_TIMER_ID);
        assert!(rt.pre.is_none());
    }
}
