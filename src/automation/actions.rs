//! FEAT-2 Part B: the App side of automation (spec §5.3–§5.9). Timer controls, editing actions, running
//! macros, the automatic steps the ticker finds due, and auto-advance after every live change.
//!
//! Lock order for the whole app: `data` → `live` → `automation`.
use crate::app::{ActionError, App, DataPlan, Update};
use crate::automation::advance::{AdvanceInput, AutoAdvance, AutoNextReq, AutoNextStep, auto_next_step};
use crate::automation::macros::{ClearWhich, Macro, MacroStep, PlannedStep, ResolveCtx, resolve};
use crate::automation::messages::{self, MessageTemplate};
use crate::automation::preservice::{PRE_SERVICE_MACRO_ID, PreRun, PreServiceEnd, EndInput, PreService, end_plan};
use crate::automation::runtime::{AutomationRuntime, Due};
use crate::automation::{Automation, apply_preservice, slide_actions, slots, strings};
use crate::lenient::Lenient;
use crate::live::{BatchStep, Command, Envelope, Live, MediaLayer, Outcome, PlayState};
use crate::model::Data;
use crate::timers::{AtZero, Builtin, MediaClock, MediaLeft, PRE_SERVICE_TIMER_ID, SetNow, StartCtx, TimeOfDay, TimerDef, TimerKind, TimersView};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};
use uuid::Uuid;

const MAX_NAME: usize = 40;
const MAX_TIMERS: usize = 50;
const MAX_MESSAGES: usize = 100;
const MAX_MACROS: usize = 100;
const MAX_STEPS: usize = 30;

/// The actions `run` answers. None of them is on the phone's list.
pub fn handles(action: &str) -> bool { action.starts_with("automation/") || action.starts_with("timer/") || action == "auto/set" }

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ActionError> {
    serde_json::from_slice(body).map_err(|e| ActionError::bad(crate::tr!("Bad request: {error}", error = e)))
}

#[derive(Deserialize)]
struct Id { id: Uuid }

#[derive(Deserialize)]
struct Delete { id: Uuid, #[serde(default)] confirm: bool }

/// The current service's start time.
pub fn starts_at(data: &Data) -> Option<TimeOfDay> { data.service().and_then(|s| data.automation.starts_at.get(&s.id).copied()) }

/// What auto-advance needs from the engine, as plain values.
fn advance_input(data: &Data, live: &Live) -> Option<AdvanceInput> {
    let (item, cursor) = (live.item.as_ref()?, live.cursor.as_ref()?);
    Some(AdvanceInput {
        item_id: item.item_id,
        slides: item.slides.len(),
        advance: data.automation.advance.get(&item.item_id).copied(),
        cursor_slide: cursor.slide,
        slide_ms: item.slides.get(cursor.slide).and_then(|s| s.advance_ms),
        slide_layer_up: live.frame.slide.is_some(),
        version: live.version,
        visual_version: live.visual_version,
    })
}

fn name_ok(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME { return Err(strings::name_length()) }
    Ok(name.to_string())
}

/// Save checks for a timer (spec §5.9).
pub fn validate_timer(a: &Automation, mut t: TimerDef) -> Result<TimerDef, String> {
    t.name = name_ok(&t.name)?;
    if let Some(TimerKind::Duration { secs }) = t.kind.known() && !(1..=86_399).contains(secs) { return Err(strings::duration_range()) }
    if let Some(AtZero::RunMacro { macro_id }) = t.at_zero.known() {
        let m = a.macros.iter().find(|m| m.id == *macro_id).ok_or_else(strings::at_zero_macro_missing)?;
        if !m.is_overlay_only() { return Err(strings::at_zero_overlay_only()) }
    }
    if !a.timers.iter().any(|x| x.id == t.id) && a.timers.len() >= MAX_TIMERS { return Err(strings::too_many_timers()) }
    Ok(t)
}

/// Save checks for a message; `Ok` carries the stray-brace warning, if any.
pub fn validate_message(a: &Automation, mut t: MessageTemplate) -> Result<(MessageTemplate, Option<String>), String> {
    t.name = name_ok(&t.name)?;
    if !a.messages.iter().any(|x| x.id == t.id) && a.messages.len() >= MAX_MESSAGES { return Err(strings::too_many_messages()) }
    let warning = messages::parse(&t.text).warning;
    Ok((t, warning))
}

/// Save checks for a macro. The Pre-service macro is changed only from its tab.
pub fn validate_macro(a: &Automation, mut m: Macro) -> Result<Macro, String> {
    if m.id == PRE_SERVICE_MACRO_ID || m.builtin == Some(Builtin::PreService) { return Err(strings::preservice_locked()) }
    m.name = name_ok(&m.name)?;
    if m.steps.len() > MAX_STEPS { return Err(strings::too_many_steps()) }
    if m.steps.iter().any(|s| matches!(s.known(), Some(MacroStep::RunMacro { .. }))) { return Err(crate::tr!("A macro can't run another macro.")) }
    if !a.macros.iter().any(|x| x.id == m.id) && a.macros.len() >= MAX_MACROS { return Err(strings::too_many_macros()) }
    for s in &m.steps {
        if let Some(MacroStep::MessageShow { template, values }) = s.known()
            && let Some(t) = a.messages.iter().find(|t| t.id == *template)
        {
            messages::fill(&t.text, values)?;
        }
    }
    Ok(m)
}

/// How many macros name `id` in a step, and how many timers run macro `id` at zero.
pub fn uses(a: &Automation, id: Uuid) -> (usize, usize) {
    let names = |s: &MacroStep| match s {
        MacroStep::TimerShow { timer } | MacroStep::TimerHide { timer } | MacroStep::TimerStart { timer } | MacroStep::TimerPause { timer }
        | MacroStep::TimerReset { timer } | MacroStep::TimerRestart { timer } => *timer == id,
        MacroStep::MessageShow { template, .. } => *template == id,
        MacroStep::RunMacro { macro_id } => *macro_id == id,
        _ => false,
    };
    let macros = a.macros.iter().filter(|m| m.id != id && m.steps.iter().any(|s| s.known().is_some_and(names))).count();
    let timers = a.timers.iter().filter(|t| matches!(t.at_zero.known(), Some(AtZero::RunMacro { macro_id }) if *macro_id == id)).count();
    (macros, timers)
}

/// The question to ask before deleting something that is used, or `None`.
pub fn delete_question(name: &str, (macros, timers): (usize, usize)) -> Option<String> {
    if macros > 0 { Some(strings::used_by(name, macros)) } else if timers > 0 { Some(strings::used_at_zero(name, timers)) } else { None }
}

pub async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    let bad = ActionError::bad;
    match action {
        a if a.starts_with("automation/music/") => super::music::run(app, a, body).await,
        a if a.starts_with("automation/calendar/") => super::calendar::run(app, a, body).await,
        "timer/start" | "timer/pause" | "timer/reset" | "timer/restart" => {
            let Id { id } = parse(body)?;
            app.timer_control(action, id, None).map_err(bad)?;
            Ok(json!({}))
        }
        "timer/set-now" => {
            #[derive(Deserialize)] struct S { id: Uuid, #[serde(default)] secs: Option<u32>, #[serde(default)] at: Option<String> }
            let S { id, secs, at } = parse(body)?;
            let change = match (secs, at) {
                (Some(s), _) => SetNow::Secs(s),
                (None, Some(at)) => SetNow::At(TimeOfDay::parse(&at).ok_or_else(|| bad(strings::set_now_wrong_kind()))?),
                _ => return Err(bad(strings::set_now_wrong_kind())),
            };
            app.timer_control(action, id, Some(change)).map_err(bad)?;
            Ok(json!({}))
        }
        "auto/set" => {
            #[derive(Deserialize)] struct On { on: bool }
            let On { on } = parse(body)?;
            {
                let data = app.data.lock().unwrap();
                let live = app.live.lock().unwrap();
                app.automation.lock().unwrap().auto.set_on(on, advance_input(&data, &live).as_ref(), Utc::now());
            }
            app.automation_changed();
            Ok(json!({}))
        }
        "automation/run" => {
            #[derive(Deserialize)] struct R { id: Uuid, #[serde(default)] base: Option<u64>, #[serde(default)] seen_clear: Option<u64> }
            let R { id, base, seen_clear } = parse(body)?;
            let env = Envelope { id: Uuid::new_v4().to_string(), base, cmd: Command::RunMacro { macro_id: id, seen_clear } };
            let (out, toast) = app.run_macro(&env);
            let problem = toast.as_ref().is_some_and(|t| t.1);
            Ok(json!({ "ok": out.ok, "error": out.error, "toast": toast.map(|t| t.0), "problem": problem }))
        }
        "automation/timer/save" => {
            let t: TimerDef = parse(body)?;
            let id = if t.id.is_nil() { Uuid::new_v4() } else { t.id };
            app.edit(|d| {
                let t = validate_timer(&d.automation, TimerDef { id, ..t })?;
                match d.automation.timers.iter_mut().find(|x| x.id == id) { Some(x) => *x = t, None => d.automation.timers.push(t) }
                Ok(())
            }).map_err(bad)?;
            app.automation_changed();
            Ok(json!({ "id": id }))
        }
        "automation/message/save" => {
            let t: MessageTemplate = parse(body)?;
            let id = if t.id.is_nil() { Uuid::new_v4() } else { t.id };
            let warning = app.edit(|d| {
                let (t, warning) = validate_message(&d.automation, MessageTemplate { id, ..t })?;
                match d.automation.messages.iter_mut().find(|x| x.id == id) { Some(x) => *x = t, None => d.automation.messages.push(t) }
                Ok(warning)
            }).map_err(bad)?;
            Ok(json!({ "id": id, "warning": warning }))
        }
        "automation/macro/save" => {
            let m: Macro = parse(body)?;
            let id = if m.id.is_nil() { Uuid::new_v4() } else { m.id };
            app.edit(|d| {
                let m = validate_macro(&d.automation, Macro { id, ..m })?;
                match d.automation.macros.iter_mut().find(|x| x.id == id) { Some(x) => *x = m, None => d.automation.macros.push(m) }
                Ok(())
            }).map_err(bad)?;
            Ok(json!({ "id": id }))
        }
        "automation/timer/delete" | "automation/message/delete" | "automation/macro/delete" => {
            let Delete { id, confirm } = parse(body)?;
            let (name, used) = {
                let data = app.data.lock().unwrap();
                let a = &data.automation;
                let name = a.timers.iter().map(|t| (t.id, &t.name)).chain(a.messages.iter().map(|t| (t.id, &t.name))).chain(a.macros.iter().map(|m| (m.id, &m.name)))
                    .find(|(i, _)| *i == id).map(|(_, n)| n.clone()).unwrap_or_default();
                (name, uses(a, id))
            };
            if !confirm && let Some(q) = delete_question(&name, used) { return Ok(json!({ "question": q })) }
            // Something on the screen goes first (a live command), then the definition.
            let hide = match action {
                "automation/timer/delete" => Some(Command::HideTimer { timer: id }),
                "automation/message/delete" => {
                    let up = app.live.lock().unwrap().frame.overlays.iter().any(|s| matches!(&s.source, crate::theme::overlay::OverlaySource::Message { template_id, .. } if *template_id == id));
                    up.then_some(Command::HideMessage)
                }
                _ => None,
            };
            if let Some(cmd) = hide { app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd }); }
            app.edit(|d| {
                let a = &mut d.automation;
                if id == PRE_SERVICE_MACRO_ID { a.pre_service = None; a.timers.retain(|t| t.id != PRE_SERVICE_TIMER_ID); }
                a.timers.retain(|t| t.id != id);
                a.messages.retain(|t| t.id != id);
                a.macros.retain(|m| m.id != id);
                Ok(())
            }).map_err(bad)?;
            app.automation_changed();
            Ok(json!({}))
        }
        "automation/macro/move" => {
            #[derive(Deserialize)] struct M { id: Uuid, to: usize }
            let M { id, to } = parse(body)?;
            app.edit(|d| {
                let ms = &mut d.automation.macros;
                let from = ms.iter().position(|m| m.id == id).ok_or_else(strings::macro_gone)?;
                let m = ms.remove(from);
                // The Pre-service macro stays first.
                let first = usize::from(ms.first().is_some_and(|m| m.id == PRE_SERVICE_MACRO_ID));
                ms.insert(to.clamp(first, ms.len()), m);
                Ok(())
            }).map_err(bad)?;
            Ok(json!({}))
        }
        "automation/preservice/save" => {
            let p: PreService = parse(body)?;
            app.edit(|d| { apply_preservice(&mut d.automation, p); Ok(()) }).map_err(bad)?;
            app.automation_changed();
            Ok(json!({}))
        }
        "automation/starts-at" => {
            #[derive(Deserialize)] struct S { service: Uuid, at: Option<String> }
            let S { service, at } = parse(body)?;
            let at = match at.as_deref().map(str::trim) {
                None | Some("") => None,
                Some(t) => Some(TimeOfDay::parse(t).ok_or_else(|| bad(crate::tr!("Type the time like 10:30.")))?),
            };
            app.edit(|d| {
                match at { Some(t) => { d.automation.starts_at.insert(service, t); } None => { d.automation.starts_at.remove(&service); } }
                Ok(())
            }).map_err(bad)?;
            app.automation_changed();
            Ok(json!({}))
        }
        "automation/advance" => {
            #[derive(Deserialize)] struct A { item: Uuid, #[serde(default)] secs: Option<u16>, #[serde(default = "yes")] loop_item: bool }
            fn yes() -> bool { true }
            let A { item, secs, loop_item } = parse(body)?;
            if let Some(s) = secs { AutoAdvance::validate(s).map_err(bad)? }
            app.edit(|d| {
                match secs { Some(secs) => { d.automation.advance.insert(item, AutoAdvance { secs, loop_item }); } None => { d.automation.advance.remove(&item); } }
                Ok(())
            }).map_err(bad)?;
            Ok(json!({}))
        }
        "automation/slide-actions/add" => {
            #[derive(Deserialize)] struct A { item: Uuid, slide: usize, steps: Vec<MacroStep> }
            let A { item, slide, steps } = parse(body)?;
            app.edit(|d| slide_actions::add(&mut d.automation, item, slide, steps)).map_err(bad)?;
            Ok(json!({}))
        }
        "automation/slide-actions/remove" => {
            #[derive(Deserialize)] struct R { item: Uuid, slide: usize, index: usize }
            let R { item, slide, index } = parse(body)?;
            app.edit(|d| slide_actions::remove(&mut d.automation, item, slide, index)).map_err(bad)?;
            Ok(json!({}))
        }
        "automation/watching" => {
            #[derive(Deserialize)] struct W { on: bool }
            let W { on } = parse(body)?;
            app.automation_watching.store(on, Ordering::Relaxed);
            app.automation_changed();
            Ok(json!({}))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}

/// A timer step or control, applied to the runtime. `clear_epoch` is what a start captures.
fn timer_op(rt: &mut AutomationRuntime, data: &Data, def: &TimerDef, op: &str, set: Option<SetNow>, now: DateTime<Utc>, clear_epoch: u64) -> Result<(), String> {
    let ctx = StartCtx { now, tz: &Local, starts_at: starts_at(data), clear_epoch };
    match op {
        "timer/start" => rt.run_mut(def.id).start(def, &ctx),
        "timer/pause" => rt.run_mut(def.id).pause(now),
        "timer/reset" => { rt.reset_timer(def.id); Ok(()) }
        "timer/restart" => {
            rt.run_mut(def.id).restart(def, &ctx)?;
            if def.id == PRE_SERVICE_TIMER_ID { rt.pre = None }
            Ok(())
        }
        _ => match set { Some(change) => rt.run_mut(def.id).set_now(def, change, &ctx), None => Ok(()) },
    }
}

/// What the video and audio timers read now (N3): the video on the screen, and the audio item playing or else the
/// walk-in music.
pub fn media_left(live: &Live, music: &super::music::MusicRun) -> MediaLeft {
    let status = |inst: u64, s: &Option<crate::live::PlayStatus>, paused: bool| {
        s.as_ref().filter(|s| s.inst == inst).and_then(|s| MediaClock::from_secs(s.position, s.duration, paused || s.state == PlayState::Paused))
    };
    let video = live.frame.media.as_ref().and_then(|m| match m.v {
        MediaLayer::Video { paused, .. } => status(m.inst, &live.media_status, paused),
        MediaLayer::Still { .. } => None,
    });
    let audio = live.frame.audio.as_ref().map_or_else(|| music.clock(), |a| status(a.inst, &live.audio_status, a.v.paused));
    MediaLeft { video, audio }
}

/// A video or audio timer exists and its player is playing, so its readout moves.
fn media_ticking(defs: &[TimerDef], media: &MediaLeft) -> bool {
    let moving = |c: Option<MediaClock>| c.is_some_and(|c| !c.paused);
    defs.iter().filter_map(|d| d.kind.known()).any(|k| match k {
        TimerKind::VideoLeft => moving(media.video),
        TimerKind::AudioLeft => moving(media.audio),
        _ => false,
    })
}

impl App {
    /// The newest timer readout: the same `Arc` the last `Update::Timers` carried (spec §4.4).
    pub fn timers(&self) -> Arc<TimersView> { self.automation.lock().unwrap().current() }

    fn timer_control(&self, op: &str, id: Uuid, set: Option<SetNow>) -> Result<(), String> {
        {
            let data = self.data.lock().unwrap();
            let epoch = self.live.lock().unwrap().clear_epoch;
            let def = data.automation.timers.iter().find(|t| t.id == id).ok_or_else(strings::timer_gone)?;
            timer_op(&mut self.automation.lock().unwrap(), &data, def, op, set, Utc::now(), epoch)?;
        }
        self.automation_changed();
        Ok(())
    }

    /// Something the ticker should look at changed: publish now and wake it.
    pub(crate) fn automation_changed(&self) {
        self.publish_timers(Utc::now(), true);
        self.ticker_wake.notify_one();
    }

    /// Sends `Update::Timers` when the readout changed (`force`: even within 250 ms of the last).
    pub(crate) fn publish_timers(&self, now: DateTime<Utc>, force: bool) {
        let sent = {
            let data = self.data.lock().unwrap();
            let live = self.live.lock().unwrap();
            let mut rt = self.automation.lock().unwrap();
            let auto = rt.auto.readout(now);
            let media = media_left(&live, &rt.music);
            let music = rt.music.readout(&data.automation.music);
            let view = rt.view_with(&data.automation.timers, now, &Local, starts_at(&data), auto, &media, music);
            if force { rt.offer_now(view, now) } else { rt.offer(view, now) }
        };
        if let Some(v) = sent { let _ = self.tx.send(Update::Timers(v)); }
    }

    /// After every accepted live change and room report: auto-advance follows the engine, a new message may
    /// hide by itself, an adoption remaps the auto slide, and stale bookkeeping goes. `committed`: the room
    /// committed this version. Takes the locks itself.
    pub(crate) fn automation_after(&self, committed: Option<u64>) {
        self.slide_actions_fire();
        let now = Utc::now();
        {
            let data = self.data.lock().unwrap();
            let mut live = self.live.lock().unwrap();
            let mut rt = self.automation.lock().unwrap();
            if let Some((item, map)) = live.adopted.take() { rt.on_adopt(item, &map) }
            let input = advance_input(&data, &live);
            rt.auto.reconcile(input.as_ref(), now);
            // With no room window nothing ever commits: count the change as committed when it is made.
            let closed = self.room_conns.load(Ordering::Relaxed) == 0;
            let c = committed.or(closed.then_some(live.version));
            if let Some(c) = c { rt.auto.on_committed(c, input.as_ref(), now) }
            let up = slots::shown_message(&live.frame.overlays);
            if up.is_some() && up != rt.message_up {
                let shown = live.frame.overlays.iter().find_map(|s| match &s.source {
                    crate::theme::overlay::OverlaySource::Message { template_id, .. } if s.key == slots::MESSAGE_KEY => Some(*template_id),
                    _ => None,
                });
                if let (Some(shown_id), Some(template)) = (up, shown) {
                    let hide = data.automation.messages.iter().find(|t| t.id == template).and_then(|t| t.hide_after_secs);
                    rt.message_shown(template, shown_id, hide, now);
                }
            }
            rt.message_up = up;
            rt.prune(&data.automation.timers, up);
        }
        let clears = self.live.lock().unwrap().audio_clears;
        if self.music_after_clear(clears) { self.music_changed() }
        self.publish_timers(now, false);
        self.ticker_wake.notify_one();
    }

    /// Runs macro `env` (a `Command::RunMacro`): every target is checked first, then the screen steps go as one
    /// change, then the stage steps (spec §5.3). Returns the outcome and the operator's toast.
    pub fn run_macro(&self, env: &Envelope) -> (Outcome, Option<(String, bool)>) {
        let Command::RunMacro { macro_id, seen_clear } = env.cmd else { return (fail(0, strings::macro_gone()), None) };
        let now = Utc::now();
        let (out, toast, stage) = {
            let data = self.data.lock().unwrap();
            let mut live = self.live.lock().unwrap();
            let mut rt = self.automation.lock().unwrap();
            let Some(m) = data.automation.macros.iter().find(|m| m.id == macro_id) else { return (fail(live.version, strings::macro_gone()), None) };
            match self.run_resolved(&data, &mut live, &mut rt, m, &env.id, env.base, seen_clear, now, None) {
                Ok(r) => r,
                Err(e) => {
                    live.notice = Some(e.clone());
                    (fail(live.version, e), None, vec![])
                }
            }
        };
        let ok = out.ok && !out.duplicate;
        for s in stage { self.stage_step(s) }
        if ok {
            self.after_command();
            // Its timer steps show at once, not on the ticker's next wake.
            self.publish_timers(Utc::now(), true);
        }
        (out, toast)
    }

    /// The body of a macro run with the locks held. `at_zero`: the timer whose zero runs it (its name).
    #[allow(clippy::too_many_arguments)]
    fn run_resolved(&self, data: &Data, live: &mut Live, rt: &mut AutomationRuntime, m: &Macro, id: &str, base: Option<u64>,
        seen_clear: Option<u64>, now: DateTime<Utc>, at_zero: Option<&str>) -> Result<(Outcome, Option<(String, bool)>, Vec<PlannedStep>), String> {
        let items: Vec<(Uuid, String)> = data.service().map(|s| s.items.iter().map(|i| (i.id, crate::slides::build_item(data, i).0)).collect()).unwrap_or_default();
        let overlays: Vec<Uuid> = data.overlays.iter().map(|o| o.id).collect();
        let layouts: Vec<Uuid> = crate::stage::presets::builtins().iter().chain(&data.stage_layouts).map(|l| l.id).collect();
        let ctx = ResolveCtx { timers: &data.automation.timers, runs: &rt.timers, templates: &data.automation.messages, overlays: &overlays,
            stage_layouts: &layouts, screen_views: &[], items: &items, now, tz: &Local, starts_at: starts_at(data) };
        let resolved = resolve(m, &ctx)?;
        if resolved.relative && base.is_none() { return Ok((fail(live.version, strings::stale_try_again()), None, vec![])) }
        let epoch = live.clear_epoch;
        // A relative macro's fresh base already proves the sender saw every clear.
        let seen = if resolved.relative { Some(epoch) } else { seen_clear };
        let mut steps = vec![];
        let mut stage = vec![];
        let mut numbers = vec![];
        for (i, p) in resolved.steps.iter().enumerate() {
            let show = p.step.is_show();
            let cmd = match &p.step {
                PlannedStep::ShowItem { item_id, slide } => Some(Command::Show { item_id: *item_id, slide: *slide }),
                PlannedStep::Next => Some(Command::Next),
                PlannedStep::Prev => Some(Command::Prev),
                PlannedStep::Clear(w) => Some(match w {
                    ClearWhich::All => Command::ClearAll, ClearWhich::Slide => Command::ClearSlide, ClearWhich::Media => Command::ClearMedia,
                    ClearWhich::Audio => Command::ClearAudio, ClearWhich::Overlays => Command::ClearOverlays,
                }),
                PlannedStep::Logo => Some(Command::Logo),
                PlannedStep::Overlay { id, on } => Some(Command::Overlay { id: *id, on: *on, seen_clear: None }),
                PlannedStep::TimerShow { timer } => Some(Command::ShowTimer { timer: *timer, seen_clear: None }),
                PlannedStep::TimerHide { timer } => Some(Command::HideTimer { timer: *timer }),
                PlannedStep::MessageShow { template, .. } => {
                    let values = match m.steps.get(p.number - 1).and_then(Lenient::known) { Some(MacroStep::MessageShow { values, .. }) => values.clone(), _ => Default::default() };
                    Some(Command::ShowMessage { template: *template, values, seen_clear: None })
                }
                PlannedStep::MessageHide => Some(Command::HideMessage),
                PlannedStep::PlayMedia { item_id } => Some(Command::PlayMedia { item_id: *item_id }),
                PlannedStep::UseTheme { theme } => Some(Command::UseTheme { theme: *theme }),
                // Nothing live: a runtime step that says so.
                PlannedStep::GoToSlide { slide } => live.cursor.as_ref().map(|c| Command::Show { item_id: c.item_id, slide: *slide }),
                PlannedStep::StageMessageShow { .. } | PlannedStep::StageMessageHide | PlannedStep::StageLayout { .. } | PlannedStep::StageLayoutAuto | PlannedStep::StreamScene { .. }
                | PlannedStep::Music { .. } | PlannedStep::CameraPosition { .. } => {
                    stage.push((p.number, p.step.clone()));
                    continue;
                }
                _ => None,
            };
            numbers.push(p.number);
            steps.push(match cmd { Some(cmd) => BatchStep::Cmd { cmd, show }, None => BatchStep::Runtime(i) });
        }
        let input = advance_input(data, live);
        let mut runtime = |i: usize| -> Result<(), String> {
            let def = |t: &Uuid| data.automation.timers.iter().find(|d| d.id == *t).ok_or_else(strings::timer_gone);
            match &resolved.steps[i].step {
                PlannedStep::TimerStart { timer, .. } => timer_op(rt, data, def(timer)?, "timer/start", None, now, epoch),
                PlannedStep::TimerPause { timer } => timer_op(rt, data, def(timer)?, "timer/pause", None, now, epoch),
                PlannedStep::TimerReset { timer } => timer_op(rt, data, def(timer)?, "timer/reset", None, now, epoch),
                PlannedStep::TimerRestart { timer, .. } => timer_op(rt, data, def(timer)?, "timer/restart", None, now, epoch),
                PlannedStep::AutoAdvance { on } => { rt.auto.set_on(*on, input.as_ref(), now); Ok(()) }
                PlannedStep::ScreenView { .. } => Err(strings::screen_views_not_yet()),
                PlannedStep::PlayPlaylist { .. } => Err(slide_actions::playlists_not_ready()),
                PlannedStep::GoToSlide { .. } => Err(crate::tr!("Nothing is live yet.")),
                _ => Ok(()),
            }
        };
        let b = live.apply_batch(id, if resolved.relative { base } else { None }, seen, &steps, &DataPlan(data), &mut runtime);
        if !b.outcome.ok || b.outcome.duplicate {
            let mut o = b.outcome;
            if o.error.as_deref().is_some_and(|e| e.starts_with("stale")) { o.error = Some(strings::stale_try_again()) }
            return Ok((o, None, vec![]));
        }
        // A slide that runs Start countdown counts as its button (`slide_actions::as_macro`).
        if m.id == PRE_SERVICE_MACRO_ID || m.builtin == Some(Builtin::PreService) {
            let loop_item = data.automation.pre_service.as_ref().and_then(|p| p.loop_title.as_ref())
                .and_then(|t| items.iter().find(|(_, x)| x.trim().to_lowercase() == t.trim().to_lowercase()).map(|(id, _)| *id));
            rt.pre = Some(PreRun { manual_version: live.manual_version, loop_item, timer_run: rt.timers.get(&PRE_SERVICE_TIMER_ID).map_or(0, |r| r.run) });
        }
        let problem = !b.failed.is_empty() || !b.skipped_shows.is_empty();
        let toast = if let Some((i, why)) = b.failed.first() {
            strings::ran_but_step_failed(&m.name, numbers[*i], why)
        } else if !b.skipped_shows.is_empty() {
            match at_zero { Some(t) => strings::at_zero_skipped_shows(t, &m.name), None => strings::ran_but_skipped_shows(&m.name) }
        } else {
            strings::ran(&m.name)
        };
        if problem { live.notice = Some(toast.clone()) }
        let toast = Some((toast, problem));
        Ok((b.outcome, toast, stage.into_iter().map(|(_, s)| s).collect()))
    }

    /// N15: a slide just went live (by a click, Next, a macro or auto-advance): its actions run once, as one change
    /// after it. A slide an action goes to doesn't run its own, so two slides can never set each other off.
    fn slide_actions_fire(&self) {
        let (ran, stage) = {
            let data = self.data.lock().unwrap();
            let mut live = self.live.lock().unwrap();
            let mut rt = self.automation.lock().unwrap();
            if live.went == rt.slide_went { return }
            rt.slide_went = live.went;
            let Some(c) = live.cursor.clone() else { return };
            let steps = slide_actions::of(&data.automation, c.item_id, c.slide);
            if steps.is_empty() { return }
            let m = slide_actions::as_macro(&data.automation, slide_actions::run_name(c.slide), steps);
            let (base, seen) = (Some(live.version), Some(live.clear_epoch));
            let r = self.run_resolved(&data, &mut live, &mut rt, &m, &format!("slide-{}", Uuid::new_v4()), base, seen, Utc::now(), None);
            rt.slide_went = live.went;
            match r {
                Ok((o, _, stage)) => (o.ok && !o.duplicate, stage),
                Err(e) => { live.notice = Some(e); (true, vec![]) }
            }
        };
        for s in stage { self.stage_step(s) }
        if ran {
            self.after_command();
            self.publish_timers(Utc::now(), true);
        }
    }

    /// A stage step, after the screen steps (FEAT-3).
    fn stage_step(&self, step: PlannedStep) {
        match step {
            PlannedStep::StageMessageShow { text } => self.set_stage_message(text.trim()),
            PlannedStep::StageMessageHide => self.set_stage_message(""),
            PlannedStep::StageLayout { layout } => { let _ = self.edit(|d| { d.settings.stage.layout = layout; Ok(()) }); }
            PlannedStep::StageLayoutAuto => self.set_notice(&strings::stage_auto_not_yet()),
            PlannedStep::StreamScene { scene } => self.set_stream_scene(scene),
            PlannedStep::CameraPosition { camera, position } => self.move_camera(camera, position),
            PlannedStep::Music { play } => {
                let op = if play { super::music::MusicOp::Play } else { super::music::MusicOp::Stop };
                if let Err(e) = self.music(op) { self.set_notice(&e) }
            }
            _ => {}
        }
    }

    /// After a command or internal step changed live: tell every screen, then let automation follow.
    pub(crate) fn after_command(&self) {
        self.broadcast_live();
        self.automation_after(None);
    }

    /// One wake of the ticker at `now`: fire what is due, then publish the readout. True when nothing runs, so
    /// the ticker can sleep until woken.
    pub fn automation_tick(&self, now: DateTime<Utc>) -> bool {
        let (dues, hides, next, calendar) = {
            let data = self.data.lock().unwrap();
            let _live = self.live.lock().unwrap();
            let mut rt = self.automation.lock().unwrap();
            let dues = rt.zero_crossings(&data.automation.timers, now);
            let calendar = rt.calendar.due(&data.automation.calendar, now, &Local);
            (dues, rt.due_hides(now), rt.auto.due(now), calendar)
        };
        for (id, _) in calendar {
            match self.calendar_run(id, now) {
                Ok(toast) => self.set_notice(&toast),
                Err(e) => self.set_notice(&e),
            }
        }
        self.music_save_volume(now);
        for d in dues {
            match d {
                Due::TimerAtZero { timer, run } => self.timer_at_zero(timer, run, now),
                Due::PreServiceEnd { run } => self.preservice_end(run),
            }
        }
        for shown in hides { self.auto_hide(shown) }
        if let Some(req) = next { self.auto_next(req, now) }
        self.publish_timers(now, false);
        let data = self.data.lock().unwrap();
        let live = self.live.lock().unwrap();
        let rt = self.automation.lock().unwrap();
        // A calendar entry due in a few seconds, or a volume waiting to be saved, keeps the ticker awake.
        let soon = super::calendar::CalendarState::soon(&data.automation.calendar, now, &Local) || rt.music.volume.is_some()
            || media_ticking(&data.automation.timers, &media_left(&live, &rt.music));
        !soon && rt.is_idle(&data.automation.timers, &slots::room_timers(&live.frame.overlays), self.automation_watching.load(Ordering::Relaxed))
    }

    /// Runs calendar entry `id` now (N17), as its time came or from "Try it now". Returns the operator's note.
    pub fn calendar_run(&self, id: Uuid, now: DateTime<Utc>) -> Result<String, String> {
        let (out, toast, stage) = {
            let data = self.data.lock().unwrap();
            let mut live = self.live.lock().unwrap();
            let mut rt = self.automation.lock().unwrap();
            let e = data.automation.calendar.iter().find(|e| e.id == id).ok_or_else(strings::macro_gone)?;
            let m = super::calendar::as_macro(e, &data.automation.macros)?;
            // Nobody is looking: it runs on the screen as it is now.
            let (base, seen) = (Some(live.version), Some(live.clear_epoch));
            let name = e.name.clone();
            let run_id = format!("calendar-{}", Uuid::new_v4());
            let (out, toast, stage) = self.run_resolved(&data, &mut live, &mut rt, &m, &run_id, base, seen, now, None)?;
            let toast = match toast { Some((t, true)) => t, _ => strings::calendar_ran(&name) };
            (out, toast, stage)
        };
        if !out.ok { return Err(out.error.unwrap_or_default()) }
        for s in stage { self.stage_step(s) }
        if !out.duplicate { self.after_command() }
        self.publish_timers(Utc::now(), true);
        Ok(toast)
    }

    fn internal(&self, cmd: Command) -> bool {
        let out = { let data = self.data.lock().unwrap(); self.live.lock().unwrap().apply(Envelope { id: format!("auto-{}", Uuid::new_v4()), base: None, cmd }, &DataPlan(&data)) };
        let changed = out.ok && out.version > 0;
        if changed { self.after_command() }
        changed
    }

    /// A message whose time is up hides, unless another has replaced it since.
    fn auto_hide(&self, shown: Uuid) {
        let up = slots::shown_message(&self.live.lock().unwrap().frame.overlays);
        if up == Some(shown) { self.internal(Command::HideMessage); }
    }

    fn auto_next(&self, req: AutoNextReq, now: DateTime<Utc>) {
        let moved = {
            let data = self.data.lock().unwrap();
            let mut live = self.live.lock().unwrap();
            let mut rt = self.automation.lock().unwrap();
            match auto_next_step(&req, advance_input(&data, &live).as_ref()) {
                AutoNextStep::Goto(slide) => live.auto_goto(slide),
                AutoNextStep::StopOnLast => { rt.auto.stop_on_last(); false }
                AutoNextStep::Ignore => { rt.auto.reconcile(advance_input(&data, &live).as_ref(), now); false }
            }
        };
        if moved { self.after_command() }
    }

    fn timer_at_zero(&self, timer: Uuid, run: u64, now: DateTime<Utc>) {
        let (def, seen) = {
            let data = self.data.lock().unwrap();
            let rt = self.automation.lock().unwrap();
            let Some(def) = data.automation.timers.iter().find(|t| t.id == timer).cloned() else { return };
            let Some(r) = rt.timers.get(&timer).filter(|r| r.run == run) else { return };
            (def, r.seen_clear)
        };
        match def.at_zero.known() {
            Some(AtZero::HideFromRoom) => { self.internal(Command::HideTimer { timer }); }
            Some(AtZero::RunMacro { macro_id }) => {
                let (stage, ok) = {
                    let data = self.data.lock().unwrap();
                    let mut live = self.live.lock().unwrap();
                    let mut rt = self.automation.lock().unwrap();
                    let Some(m) = data.automation.macros.iter().find(|m| m.id == *macro_id) else { return };
                    if !m.is_overlay_only() {
                        live.notice = Some(strings::at_zero_changes_slides(&def.name, &m.name));
                        (vec![], true)
                    } else {
                        let id = format!("auto-{}", Uuid::new_v4());
                        match self.run_resolved(&data, &mut live, &mut rt, m, &id, None, Some(seen), now, Some(&def.name)) {
                            Ok((o, _, stage)) => (stage, o.ok),
                            Err(e) => { live.notice = Some(e); (vec![], true) }
                        }
                    }
                };
                for s in stage { self.stage_step(s) }
                if ok { self.after_command() }
            }
            _ => {}
        }
    }

    /// The pre-service countdown reached zero: it hides, and the chosen end happens only if nobody has changed
    /// the screen since Start countdown (spec §5.4).
    fn preservice_end(&self, run: u64) {
        {
            let data = self.data.lock().unwrap();
            let mut live = self.live.lock().unwrap();
            let mut rt = self.automation.lock().unwrap();
            let end = data.automation.pre_service.as_ref().map(|p| p.end).unwrap_or_default();
            let f = &live.frame;
            let input = EndInput { timer_run: run, manual_version: live.manual_version, cursor_item: live.cursor.as_ref().map(|c| c.item_id),
                visual_live: f.slide.is_some() || f.media.is_some() || f.background.is_some() || f.logo };
            let plan = end_plan(end, rt.pre.as_ref(), &input);
            rt.pre = None;
            let mut steps = vec![BatchStep::Cmd { cmd: Command::HideTimer { timer: PRE_SERVICE_TIMER_ID }, show: false }];
            let first = data.service().and_then(|s| s.items.first()).map(|i| i.id);
            match plan.extra {
                Some(PreServiceEnd::Logo) => steps.push(BatchStep::Cmd { cmd: Command::Logo, show: false }),
                Some(PreServiceEnd::ClearAll) => steps.push(BatchStep::Cmd { cmd: Command::ClearAll, show: false }),
                Some(PreServiceEnd::FirstItem) => if let Some(item_id) = first { steps.push(BatchStep::Cmd { cmd: Command::Show { item_id, slide: 0 }, show: false }) },
                _ => {}
            }
            let epoch = live.clear_epoch;
            live.apply_batch(&format!("auto-{}", Uuid::new_v4()), None, Some(epoch), &steps, &DataPlan(&data), &mut |_| Ok(()));
            if let Some(n) = plan.notice { live.notice = Some(n) }
        }
        self.after_command();
    }
}

fn fail(version: u64, e: String) -> Outcome { Outcome { ok: false, version, error: Some(e), duplicate: false } }

#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;
