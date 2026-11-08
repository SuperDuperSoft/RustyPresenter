//! FEAT-2 Part B through the App: timers, messages, macros, auto-advance and the pre-service end.
use super::*;
use crate::app::test_app;
use crate::automation::macros::ItemTarget;
use crate::automation::messages::NURSERY_ID;
use crate::live::RoomEvent;
use crate::theme::overlay::OverlaySource;
use crate::timers::{SERMON_ID, STOPWATCH_ID, TimerState};
use chrono::TimeDelta;
use std::collections::BTreeMap;

fn block_on<F: std::future::Future>(f: F) -> F::Output { tokio::runtime::Runtime::new().unwrap().block_on(f) }
fn act(app: &Arc<App>, action: &str, body: Value) -> Result<Value, ActionError> { block_on(app.do_action(action, body.to_string().as_bytes())) }
fn cmd(app: &App, cmd: Command, base: Option<u64>) -> Outcome { app.command(Envelope { id: Uuid::new_v4().to_string(), base, cmd }) }
fn epoch(app: &App) -> Option<u64> { Some(app.live.lock().unwrap().clear_epoch) }
fn version(app: &App) -> u64 { app.live.lock().unwrap().version }
fn keys(app: &App) -> Vec<String> { app.live.lock().unwrap().frame.overlays.iter().map(|s| s.key.clone()).collect() }
fn snapshot(app: &App, id: Uuid) -> crate::timers::TimerSnapshot { app.timers().timers.iter().find(|t| t.id == id).cloned().unwrap() }

/// A text item with `n` slides, added to the open service.
fn text_item(app: &Arc<App>, title: &str, n: usize) -> Uuid {
    let text = (1..=n).map(|i| format!("{title} {i}")).collect::<Vec<_>>().join("\n\n");
    let item = serde_json::json!({ "kind": "text", "title": title, "body": text });
    let r = act(app, "service/add", serde_json::json!({ "item": item })).unwrap();
    serde_json::from_value(r["id"].clone()).unwrap()
}
fn slide(app: &App) -> Option<(Uuid, usize)> { app.live.lock().unwrap().cursor.as_ref().map(|c| (c.item_id, c.slide)) }
fn show(app: &App, item_id: Uuid, s: usize) -> Outcome { let v = version(app); cmd(app, Command::Show { item_id, slide: s }, Some(v)) }

#[test]
fn new_data_has_the_built_ins_and_old_data_loads_with_them() {
    let app = test_app();
    let data = app.data.lock().unwrap();
    assert!(data.automation.seeded);
    assert_eq!(data.automation.timers.len(), 3);
    assert_eq!(data.automation.messages[0].id, NURSERY_ID);
    let old: crate::model::Data = serde_json::from_str(r#"{"format":2,"services":[]}"#).unwrap();
    assert!(old.automation.seeded, "a file from before FEAT-2 gets the built-ins");
    let mut empty: crate::model::Data = serde_json::from_str(r#"{"automation":{}}"#).unwrap();
    assert!(empty.after_load(), "seeded once, so the store saves");
    assert!(!empty.after_load());
}

#[test]
fn bad_start_time_or_advance_is_left_out_not_fatal() {
    let json = r#"{"automation":{"seeded":true,"starts_at":{"6f7b3c1e-1a2b-4c3d-8e9f-001122334477":"soon","6f7b3c1e-1a2b-4c3d-8e9f-001122334478":"10:30"},
        "advance":{"6f7b3c1e-1a2b-4c3d-8e9f-001122334477":{},"x":{"secs":8}}}}"#;
    let d: crate::model::Data = serde_json::from_str(json).unwrap();
    assert_eq!(d.automation.starts_at.len(), 1);
    assert!(d.automation.advance.is_empty());
}

#[test]
fn timer_actions_round_trip_state_and_publish() {
    let app = test_app();
    let mut rx = app.subscribe();
    act(&app, "timer/start", serde_json::json!({ "id": SERMON_ID })).unwrap();
    assert_eq!(snapshot(&app, SERMON_ID).state, TimerState::Running);
    let got = std::iter::from_fn(|| rx.try_recv().ok()).find_map(|u| match u { Update::Timers(v) => Some(v), _ => None }).expect("a readout went out");
    assert!(Arc::ptr_eq(&got, &app.timers()), "timers() is the Arc the update carried");
    act(&app, "timer/pause", serde_json::json!({ "id": SERMON_ID })).unwrap();
    assert_eq!(snapshot(&app, SERMON_ID).state, TimerState::Paused);
    act(&app, "timer/reset", serde_json::json!({ "id": SERMON_ID })).unwrap();
    let s = snapshot(&app, SERMON_ID);
    assert_eq!((s.state, crate::timers::text(s.value_ms)), (TimerState::Stopped, "30:00".into()));
}

#[test]
fn set_for_now_changes_the_run_only() {
    let app = test_app();
    act(&app, "timer/set-now", serde_json::json!({ "id": SERMON_ID, "secs": 1500 })).unwrap();
    assert_eq!(crate::timers::text(snapshot(&app, SERMON_ID).value_ms), "25:00");
    let def = app.data.lock().unwrap().automation.timers.iter().find(|t| t.id == SERMON_ID).unwrap().kind.clone();
    assert_eq!(def, Lenient::Known(TimerKind::Duration { secs: 1800 }));
}

#[test]
fn service_start_countdown_needs_a_start_time() {
    let app = test_app();
    let e = act(&app, "timer/start", serde_json::json!({ "id": crate::timers::SERVICE_START_ID })).unwrap_err();
    assert_eq!(e.message, strings::no_start_time());
    let service = app.data.lock().unwrap().service().unwrap().id;
    // An hour from now (or the next day, near midnight): always in the future for "today", or refused as passed.
    let soon = (Local::now() + TimeDelta::minutes(61)).format("%H:%M").to_string();
    act(&app, "automation/starts-at", serde_json::json!({ "service": service, "at": soon })).unwrap();
    let r = act(&app, "timer/start", serde_json::json!({ "id": crate::timers::SERVICE_START_ID }));
    if Local::now().date_naive() == (Local::now() + TimeDelta::minutes(61)).date_naive() {
        r.unwrap();
        let s = snapshot(&app, crate::timers::SERVICE_START_ID);
        assert_eq!(s.state, TimerState::Running);
        assert!(s.value_ms > 59 * 60_000 && s.value_ms <= 61 * 60_000, "{}", s.value_ms);
    }
    act(&app, "automation/starts-at", serde_json::json!({ "service": service, "at": null })).unwrap();
    assert!(app.data.lock().unwrap().automation.starts_at.is_empty());
}

#[test]
fn show_timer_needs_seen_clear_and_clears_follow_the_matrix() {
    let app = test_app();
    assert!(!cmd(&app, Command::ShowTimer { timer: SERMON_ID, seen_clear: None }, None).ok);
    let e = epoch(&app);
    assert!(cmd(&app, Command::ShowTimer { timer: SERMON_ID, seen_clear: e }, None).ok);
    assert!(cmd(&app, Command::ShowMessage { template: NURSERY_ID, values: BTreeMap::from([("Number".into(), "27".into())]), seen_clear: e }, None).ok);
    assert_eq!(keys(&app), [slots::timer_key(SERMON_ID), slots::MESSAGE_KEY.to_string()]);
    let text = app.live.lock().unwrap().frame.overlays.iter().find_map(|s| match &s.source { OverlaySource::Message { text, .. } => Some(text.clone()), _ => None });
    assert_eq!(text.as_deref(), Some("Parent of child #27, please come to the nursery."));
    // Logo takes the timer down and keeps the message.
    cmd(&app, Command::Logo, None);
    assert_eq!(keys(&app), [slots::MESSAGE_KEY.to_string()]);
    // A show sent before that clear is refused; a hide is always fine.
    assert_eq!(cmd(&app, Command::ShowTimer { timer: SERMON_ID, seen_clear: e }, None).error, Some(crate::theme::strings::screen_cleared().into()));
    assert!(cmd(&app, Command::HideMessage, None).ok);
    assert!(keys(&app).is_empty());
}

#[test]
fn empty_message_field_is_refused() {
    let app = test_app();
    let o = cmd(&app, Command::ShowMessage { template: NURSERY_ID, values: BTreeMap::new(), seen_clear: epoch(&app) }, None);
    assert_eq!(o.error, Some(strings::fill_in("Number")));
}

#[test]
fn fifth_timer_in_room_refused() {
    let app = test_app();
    for i in 0..4 {
        let t = TimerDef { name: format!("T{i}"), ..TimerDef::default() };
        let id: Uuid = serde_json::from_value(act(&app, "automation/timer/save", serde_json::to_value(t).unwrap()).unwrap()["id"].clone()).unwrap();
        assert!(cmd(&app, Command::ShowTimer { timer: id, seen_clear: epoch(&app) }, None).ok);
    }
    let o = cmd(&app, Command::ShowTimer { timer: SERMON_ID, seen_clear: epoch(&app) }, None);
    assert_eq!(o.error, Some(strings::too_many_room_timers()));
}

#[test]
fn message_hide_after_auto_hides_unless_replaced() {
    let app = test_app();
    let mut t = app.data.lock().unwrap().automation.messages[0].clone();
    t.hide_after_secs = Some(15);
    act(&app, "automation/message/save", serde_json::to_value(&t).unwrap()).unwrap();
    let values = BTreeMap::from([("Number".into(), "3".into())]);
    cmd(&app, Command::ShowMessage { template: NURSERY_ID, values: values.clone(), seen_clear: epoch(&app) }, None);
    let now = Utc::now();
    app.automation_tick(now + TimeDelta::seconds(5));
    assert_eq!(keys(&app), [slots::MESSAGE_KEY]);
    let v = version(&app);
    app.automation_tick(now + TimeDelta::seconds(16));
    assert!(keys(&app).is_empty());
    assert_eq!(version(&app), v + 1, "one bump");
    // Replaced by one with no auto-hide: the old time does nothing.
    cmd(&app, Command::ShowMessage { template: NURSERY_ID, values, seen_clear: epoch(&app) }, None);
    let custom = BTreeMap::from([("Message".into(), "Hi".into())]);
    cmd(&app, Command::ShowMessage { template: crate::automation::messages::CUSTOM_ID, values: custom, seen_clear: epoch(&app) }, None);
    app.automation_tick(Utc::now() + TimeDelta::seconds(20));
    assert_eq!(keys(&app), [slots::MESSAGE_KEY]);
    assert_eq!(app.automation.lock().unwrap().last_message, Some(crate::automation::messages::CUSTOM_ID));
    // Clear All takes it down, and the bookkeeping goes with it.
    cmd(&app, Command::ClearAll, None);
    assert!(app.automation.lock().unwrap().hide_at.is_empty());
}

fn save_macro(app: &Arc<App>, name: &str, steps: Vec<MacroStep>) -> Uuid {
    let m = Macro { name: name.into(), steps: steps.into_iter().map(Lenient::Known).collect(), ..Macro::default() };
    serde_json::from_value(act(app, "automation/macro/save", serde_json::to_value(m).unwrap()).unwrap()["id"].clone()).unwrap()
}
fn run_macro(app: &Arc<App>, id: Uuid, base: Option<u64>, seen: Option<u64>) -> Value {
    act(app, "automation/run", serde_json::json!({ "id": id, "base": base, "seen_clear": seen })).unwrap()
}

#[test]
fn macro_is_one_version_bump_and_toasts() {
    let app = test_app();
    let item = text_item(&app, "Welcome", 3);
    let walk_in = save_macro(&app, "Walk-in", vec![
        MacroStep::ShowItem { target: ItemTarget::Titled { title: " welcome ".into() }, slide: 1 },
        MacroStep::TimerStart { timer: STOPWATCH_ID },
        MacroStep::TimerShow { timer: STOPWATCH_ID },
        MacroStep::MessageShow { template: NURSERY_ID, values: BTreeMap::from([("Number".into(), "9".into())]) },
    ]);
    let v = version(&app);
    let r = run_macro(&app, walk_in, Some(v), None);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["toast"], strings::ran("Walk-in"));
    assert_eq!(version(&app), v + 1);
    assert_eq!(slide(&app), Some((item, 1)));
    assert_eq!(keys(&app), [slots::timer_key(STOPWATCH_ID), slots::MESSAGE_KEY.to_string()]);
    assert_eq!(snapshot(&app, STOPWATCH_ID).state, TimerState::Running);
}

#[test]
fn relative_macro_needs_a_fresh_base() {
    let app = test_app();
    text_item(&app, "Welcome", 2);
    let m = save_macro(&app, "Go", vec![MacroStep::Next]);
    assert_eq!(run_macro(&app, m, None, None)["error"], strings::stale_try_again());
    let v = version(&app);
    cmd(&app, Command::ClearAll, None);
    assert_eq!(run_macro(&app, m, Some(v), None)["error"], strings::stale_try_again(), "a clear the sender hadn't seen");
    assert_eq!(run_macro(&app, m, Some(version(&app)), None)["ok"], true);
}

#[test]
fn overlay_only_macro_skips_shows_after_an_unseen_clear() {
    let app = test_app();
    let m = save_macro(&app, "Sermon", vec![MacroStep::TimerStart { timer: SERMON_ID }, MacroStep::TimerShow { timer: SERMON_ID }]);
    let seen = epoch(&app);
    cmd(&app, Command::ClearAll, None);
    let r = run_macro(&app, m, None, seen);
    assert_eq!(r["toast"], strings::ran_but_skipped_shows("Sermon"));
    assert!(keys(&app).is_empty());
    assert_eq!(snapshot(&app, SERMON_ID).state, TimerState::Running, "the runtime step still ran");
}

#[test]
fn deleted_target_runs_nothing_and_failing_step_continues() {
    let app = test_app();
    let gone = Uuid::from_u128(0xdead);
    let m = save_macro(&app, "Walk-in", vec![MacroStep::TimerStart { timer: SERMON_ID }, MacroStep::TimerShow { timer: gone }]);
    let r = run_macro(&app, m, None, epoch(&app));
    assert_eq!(r["error"], strings::cant_run_deleted("Walk-in", 2, strings::StepTarget::Timer));
    assert_eq!(snapshot(&app, SERMON_ID).state, TimerState::Stopped);
    // A message whose field became empty fails at run time; the other steps still run.
    let m = save_macro(&app, "Kids", vec![MacroStep::MessageShow { template: crate::automation::messages::CUSTOM_ID, values: BTreeMap::from([("Message".into(), "x".into())]) }, MacroStep::TimerStart { timer: SERMON_ID }]);
    app.data.lock().unwrap().automation.macros.iter_mut().find(|x| x.id == m).unwrap().steps[0] =
        Lenient::Known(MacroStep::MessageShow { template: crate::automation::messages::CUSTOM_ID, values: BTreeMap::new() });
    let r = run_macro(&app, m, None, epoch(&app));
    assert_eq!(r["toast"], strings::ran_but_step_failed("Kids", 1, &strings::fill_in("Message")));
    assert_eq!(snapshot(&app, SERMON_ID).state, TimerState::Running);
}

#[test]
fn same_id_runs_once() {
    let app = test_app();
    let m = save_macro(&app, "Stopwatch", vec![MacroStep::TimerRestart { timer: STOPWATCH_ID }]);
    let env = Envelope { id: "same".into(), base: None, cmd: Command::RunMacro { macro_id: m, seen_clear: epoch(&app) } };
    assert!(app.command(env.clone()).ok);
    let run = app.automation.lock().unwrap().timers[&STOPWATCH_ID].run;
    assert!(app.command(env).duplicate);
    assert_eq!(app.automation.lock().unwrap().timers[&STOPWATCH_ID].run, run);
}

#[test]
fn at_zero_rejects_a_macro_that_changes_slides() {
    let app = test_app();
    let m = save_macro(&app, "Go", vec![MacroStep::Next]);
    let t = TimerDef { name: "Short".into(), kind: Lenient::Known(TimerKind::Duration { secs: 5 }), at_zero: Lenient::Known(AtZero::RunMacro { macro_id: m }), ..TimerDef::default() };
    let e = act(&app, "automation/timer/save", serde_json::to_value(t).unwrap()).unwrap_err();
    assert_eq!(e.message, strings::at_zero_overlay_only());
}

#[test]
fn at_zero_hides_the_timer_from_the_room() {
    let app = test_app();
    let t = TimerDef { name: "Short".into(), kind: Lenient::Known(TimerKind::Duration { secs: 5 }), at_zero: Lenient::Known(AtZero::HideFromRoom), ..TimerDef::default() };
    let id: Uuid = serde_json::from_value(act(&app, "automation/timer/save", serde_json::to_value(t).unwrap()).unwrap()["id"].clone()).unwrap();
    act(&app, "timer/start", serde_json::json!({ "id": id })).unwrap();
    cmd(&app, Command::ShowTimer { timer: id, seen_clear: epoch(&app) }, None);
    app.automation_tick(Utc::now() + TimeDelta::seconds(2));
    assert_eq!(keys(&app), [slots::timer_key(id)]);
    app.automation_tick(Utc::now() + TimeDelta::seconds(6));
    assert!(keys(&app).is_empty());
    assert_eq!(snapshot(&app, id).state, TimerState::Finished);
}

#[test]
fn delete_used_timer_asks_first_and_hides_it() {
    let app = test_app();
    save_macro(&app, "A", vec![MacroStep::TimerShow { timer: SERMON_ID }]);
    save_macro(&app, "B", vec![MacroStep::TimerStart { timer: SERMON_ID }]);
    cmd(&app, Command::ShowTimer { timer: SERMON_ID, seen_clear: epoch(&app) }, None);
    let r = act(&app, "automation/timer/delete", serde_json::json!({ "id": SERMON_ID })).unwrap();
    assert_eq!(r["question"], strings::used_by("Sermon", 2));
    act(&app, "automation/timer/delete", serde_json::json!({ "id": SERMON_ID, "confirm": true })).unwrap();
    assert!(keys(&app).is_empty());
    assert!(app.data.lock().unwrap().automation.timers.iter().all(|t| t.id != SERMON_ID));
}

#[test]
fn preservice_save_creates_locked_macro_first() {
    let app = test_app();
    save_macro(&app, "Other", vec![MacroStep::Logo]);
    act(&app, "automation/preservice/save", serde_json::to_value(PreService::default()).unwrap()).unwrap();
    let data = app.data.lock().unwrap();
    assert_eq!(data.automation.macros[0].id, PRE_SERVICE_MACRO_ID);
    drop(data);
    let locked = app.data.lock().unwrap().automation.macros[0].clone();
    assert_eq!(act(&app, "automation/macro/save", serde_json::to_value(locked).unwrap()).unwrap_err().message, strings::preservice_locked());
}

/// Auto-advance on a text item: every 8 s, starting over after the last slide.
fn advancing(app: &Arc<App>, n: usize, loop_item: bool) -> Uuid {
    let item = text_item(app, "Announcements", n);
    act(app, "automation/advance", serde_json::json!({ "item": item, "secs": 8, "loop_item": loop_item })).unwrap();
    item
}

#[test]
fn auto_advances_with_room_closed_and_loops() {
    let app = test_app();
    let item = advancing(&app, 3, true);
    show(&app, item, 0);
    let t = Utc::now();
    app.automation_tick(t + TimeDelta::seconds(5));
    assert_eq!(slide(&app), Some((item, 0)));
    app.automation_tick(t + TimeDelta::seconds(9));
    assert_eq!(slide(&app), Some((item, 1)));
    let (manual, v) = { let l = app.live.lock().unwrap(); (l.manual_version, l.version) };
    assert_eq!(manual, v - 1, "a slide turn isn't a hand change");
    // The interval starts again when the slide turned (now), so 8 s after now.
    app.automation_tick(Utc::now() + TimeDelta::seconds(9));
    assert_eq!(slide(&app), Some((item, 2)));
    app.automation_tick(Utc::now() + TimeDelta::seconds(9));
    assert_eq!(slide(&app), Some((item, 0)), "starts over");
}

#[test]
fn auto_advance_stops_on_last_without_loop_and_on_clear() {
    let app = test_app();
    let item = advancing(&app, 2, false);
    show(&app, item, 1);
    app.automation_tick(Utc::now() + TimeDelta::seconds(9));
    assert_eq!(slide(&app), Some((item, 1)));
    show(&app, item, 0);
    cmd(&app, Command::ClearSlide, None);
    app.automation_tick(Utc::now() + TimeDelta::seconds(9));
    assert_eq!(slide(&app), Some((item, 0)), "a cleared slide layer stops it");
}

#[test]
fn auto_advance_waits_for_the_room_and_a_message_does_not_restart_it() {
    let app = test_app();
    let item = advancing(&app, 3, true);
    let _room = app.screen_guard("room");
    show(&app, item, 0);
    app.automation_tick(Utc::now() + TimeDelta::seconds(20));
    assert_eq!(slide(&app), Some((item, 0)), "nothing is due before the room commits");
    let v = version(&app);
    app.room_event(RoomEvent::Committed { version: v });
    let t = Utc::now();
    let custom = BTreeMap::from([("Message".into(), "Hi".into())]);
    cmd(&app, Command::ShowMessage { template: crate::automation::messages::CUSTOM_ID, values: custom, seen_clear: epoch(&app) }, None);
    app.automation_tick(t + TimeDelta::seconds(9));
    assert_eq!(slide(&app), Some((item, 1)));
}

#[test]
fn next_after_auto_next_is_stale() {
    let app = test_app();
    let item = advancing(&app, 3, true);
    show(&app, item, 0);
    let v = version(&app);
    app.automation_tick(Utc::now() + TimeDelta::seconds(9));
    assert!(!cmd(&app, Command::Next, Some(v)).ok);
}

#[test]
fn auto_set_pauses_and_resumes() {
    let app = test_app();
    let item = advancing(&app, 3, true);
    show(&app, item, 0);
    act(&app, "auto/set", serde_json::json!({ "on": false })).unwrap();
    app.automation_tick(Utc::now() + TimeDelta::seconds(9));
    assert_eq!(slide(&app), Some((item, 0)));
    assert!(app.timers().auto.is_some_and(|a| a.paused));
    act(&app, "auto/set", serde_json::json!({ "on": true })).unwrap();
    app.automation_tick(Utc::now() + TimeDelta::seconds(9));
    assert_eq!(slide(&app), Some((item, 1)));
}

#[test]
fn preservice_end_first_item_only_when_untouched() {
    let app = test_app();
    let first = text_item(&app, "Song", 1);
    let lp = advancing(&app, 2, true);
    let service = app.data.lock().unwrap().service().unwrap().id;
    let soon = (Local::now() + TimeDelta::minutes(30)).format("%H:%M").to_string();
    if Local::now().date_naive() != (Local::now() + TimeDelta::minutes(30)).date_naive() { return } // near midnight
    act(&app, "automation/starts-at", serde_json::json!({ "service": service, "at": soon })).unwrap();
    let p = PreService { loop_title: Some("Announcements".into()), end: PreServiceEnd::FirstItem, ..PreService::default() };
    act(&app, "automation/preservice/save", serde_json::to_value(p).unwrap()).unwrap();
    let r = run_macro(&app, PRE_SERVICE_MACRO_ID, Some(version(&app)), None);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(slide(&app), Some((lp, 0)));
    assert!(keys(&app).contains(&slots::timer_key(PRE_SERVICE_TIMER_ID)));
    // The loop turning by itself is not someone taking over.
    app.automation_tick(Utc::now() + TimeDelta::seconds(9));
    assert_eq!(slide(&app), Some((lp, 1)));
    app.automation_tick(Utc::now() + TimeDelta::minutes(31));
    assert_eq!(slide(&app), Some((first, 0)));
    assert!(!keys(&app).contains(&slots::timer_key(PRE_SERVICE_TIMER_ID)));

    // Again, but the operator takes an item by hand first: only the countdown hides.
    run_macro(&app, PRE_SERVICE_MACRO_ID, Some(version(&app)), None);
    show(&app, first, 0);
    app.automation_tick(Utc::now() + TimeDelta::minutes(31));
    assert_eq!(slide(&app), Some((first, 0)));
    assert_eq!(app.live.lock().unwrap().notice, Some(strings::countdown_ended_nothing_else()));
}

#[test]
fn adoption_remaps_auto_run_without_restarting() {
    let app = test_app();
    let item = advancing(&app, 3, true);
    show(&app, item, 2);
    let due = app.automation.lock().unwrap().auto.run.as_ref().unwrap().due;
    app.automation.lock().unwrap().on_adopt(item, &[Some(0), Some(1), Some(0)]);
    let run = app.automation.lock().unwrap().auto.run.clone().unwrap();
    assert_eq!((run.slide, run.due), (0, due));
}

/// N3: a video and an audio timer read what plays, on every screen's readout.
#[test]
fn video_and_audio_timers_follow_what_plays() {
    use crate::live::{AudioLayer, Inst, PlayStatus};
    let app = test_app();
    let v = act(&app, "automation/timer/save", serde_json::json!({ "name": "Video", "kind": { "type": "video_left" }, "on_stage": true, "on_remote": true })).unwrap();
    let a = act(&app, "automation/timer/save", serde_json::json!({ "name": "Audio", "kind": { "type": "audio_left" } })).unwrap();
    let (vid, aid): (Uuid, Uuid) = (serde_json::from_value(v["id"].clone()).unwrap(), serde_json::from_value(a["id"].clone()).unwrap());
    assert_eq!(snapshot(&app, vid).state, TimerState::Stopped);
    {
        let mut live = app.live.lock().unwrap();
        live.frame.media = Some(Inst { inst: 5, v: MediaLayer::Video { asset: "v.mp4".into(), end: crate::model::EndAction::Stop, paused: false } });
        live.media_status = Some(PlayStatus { inst: 5, state: PlayState::Playing, position: 30.2, duration: 90.0 });
        live.frame.audio = Some(Inst { inst: 6, v: AudioLayer { asset: "a.mp3".into(), title: "A".into(), looped: false, paused: true } });
        live.audio_status = Some(PlayStatus { inst: 6, state: PlayState::Paused, position: 5.0, duration: 65.0 });
    }
    app.automation_changed();
    let s = snapshot(&app, vid);
    assert_eq!((s.state, crate::timers::text(s.value_ms)), (TimerState::Running, "1:00".to_string()));
    let s = snapshot(&app, aid);
    assert_eq!((s.state, crate::timers::text(s.value_ms)), (TimerState::Paused, "1:00".to_string()));
    // Start does nothing to it, and an old status of another video is not read.
    act(&app, "timer/start", serde_json::json!({ "id": vid })).unwrap();
    app.live.lock().unwrap().media_status.as_mut().unwrap().inst = 4;
    app.automation_changed();
    assert_eq!(snapshot(&app, vid).state, TimerState::Stopped);
}

/// N13: the walk-in music plays apart from the slides, and Clear audio or Clear All stops it.
#[test]
fn walk_in_music_keeps_playing_through_slides_and_stops_on_clear_audio() {
    use crate::automation::music::{MusicList, MusicState, Track};
    let app = test_app();
    let tracks: Vec<Track> = (1..=2).map(|i| Track { id: Uuid::from_u128(i), asset: format!("{i}.mp3"), title: format!("Song {i}"), duration: None }).collect();
    act(&app, "automation/music/save", serde_json::to_value(MusicList { tracks, ..MusicList::default() }).unwrap()).unwrap();
    assert!(act(&app, "automation/music/dance", serde_json::json!({})).is_err());
    act(&app, "automation/music/play", serde_json::json!({})).unwrap();
    let state = |app: &App| app.timers().music.as_ref().map(|m| m.state);
    assert_eq!(state(&app), Some(MusicState::Playing));
    let item = text_item(&app, "Welcome", 3);
    show(&app, item, 0);
    cmd(&app, Command::Next, Some(version(&app)));
    cmd(&app, Command::ClearSlide, None);
    assert_eq!(state(&app), Some(MusicState::Playing), "slides and Clear text leave it playing");
    // A song that ends goes on to the next one.
    let inst = app.automation.lock().unwrap().music.inst;
    app.music_report(crate::video::PlayerReport { inst, player: 1, state: PlayState::Ended, position: 1.0, duration: 1.0, error: None });
    assert_eq!(app.timers().music.as_ref().unwrap().title, "Song 2");
    cmd(&app, Command::ClearAudio, None);
    assert_eq!(state(&app), Some(MusicState::Stopped));
    act(&app, "automation/music/play", serde_json::json!({})).unwrap();
    cmd(&app, Command::ClearAll, None);
    assert_eq!(state(&app), Some(MusicState::Stopped), "Clear All stops it too");
    // A macro step starts it.
    let m = act(&app, "automation/macro/save", serde_json::json!({ "name": "Walk-in", "steps": [{ "step": "music", "play": true }] })).unwrap();
    let out = act(&app, "automation/run", serde_json::json!({ "id": m["id"] })).unwrap();
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(state(&app), Some(MusicState::Playing));
}

/// N17: an entry fires once when its time comes on a chosen day, from the ticker, and runs its step.
#[test]
fn calendar_entry_starts_the_countdown_when_its_time_comes() {
    use crate::automation::calendar::{CalendarEntry, EVERY_DAY, next_due};
    let app = test_app();
    let e = serde_json::json!({ "name": "Countdown", "at": "10:45", "days": EVERY_DAY, "run": { "type": "step", "step": { "step": "timer_start", "timer": SERMON_ID } } });
    let saved = act(&app, "automation/calendar/save", e.clone()).unwrap();
    let id: Uuid = serde_json::from_value(saved["id"].clone()).unwrap();
    assert!(act(&app, "automation/calendar/save", serde_json::json!({ "name": "None", "at": "10:45", "days": 0, "run": e["run"] })).is_err(), "no days");
    let entry: CalendarEntry = app.data.lock().unwrap().automation.calendar[0].clone();
    // The next 10:45 on this computer's clock, whatever its time zone.
    let due = next_due(&entry, Utc::now(), &Local).unwrap();
    app.automation_tick(due - TimeDelta::seconds(30));
    assert_eq!(snapshot(&app, SERMON_ID).state, TimerState::Stopped);
    assert!(!app.automation_tick(due - TimeDelta::seconds(3)), "due soon: the ticker stays awake");
    app.automation_tick(due + TimeDelta::milliseconds(200));
    assert_eq!(snapshot(&app, SERMON_ID).state, TimerState::Running, "it ran at its time");
    act(&app, "timer/reset", serde_json::json!({ "id": SERMON_ID })).unwrap();
    app.automation_tick(due + TimeDelta::seconds(1));
    assert_eq!(snapshot(&app, SERMON_ID).state, TimerState::Stopped, "once only");
    // Try it now runs it at once.
    let r = act(&app, "automation/calendar/run", serde_json::json!({ "id": id })).unwrap();
    assert_eq!(r["toast"], crate::automation::strings::calendar_ran("Countdown"));
    assert_eq!(snapshot(&app, SERMON_ID).state, TimerState::Running);
    act(&app, "automation/calendar/delete", serde_json::json!({ "id": id })).unwrap();
    assert!(app.data.lock().unwrap().automation.calendar.is_empty());
}
