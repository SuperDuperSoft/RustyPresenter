//! The Automation dialog, Show message and the live column's automation strip, on the testing backend.
use super::*;
use crate::{
    app::{App, test_app},
    automation::messages::NURSERY_ID,
    timers::SERMON_ID,
    ui::{AutoStrip, ShowMessageDlg, operator::window::testing::{find, open as open_window, refresh}},
};
use slint::{Model, platform::PointerEventButton};
use std::sync::Arc;

fn opened(app: &Arc<App>) -> Rc<Operator> {
    let op = open_window(app);
    op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
    slint::platform::update_timers_and_animations();
    refresh(&op);
    op
}

fn settle() { slint::platform::update_timers_and_animations(); }

fn click(op: &Operator, id: &str) {
    settle();
    find(op, id).unwrap_or_else(|| panic!("{id} not found")).mock_single_click(PointerEventButton::Left);
    settle();
}

fn g(op: &Operator) -> AutomationDlg<'_> { op.ui.global::<AutomationDlg>() }

#[test]
fn gear_button_opens_the_dialog_on_timers_with_the_built_ins() {
    let app = test_app();
    let op = opened(&app);
    click(&op, "AutomationStrip::automation-btn");
    assert_eq!(op.dialog(), Some(DialogKind::Automation));
    let names: Vec<String> = g(&op).get_rows().iter().map(|r| r.title.to_string()).collect();
    assert_eq!(names, ["Service start", "Sermon", "Stopwatch"]);
}

#[test]
fn new_timer_saves_through_the_form() {
    let app = test_app();
    let op = opened(&app);
    open(&op);
    g(&op).invoke_new_one();
    g(&op).set_t_name("Offering".into());
    g(&op).set_t_kind(1);
    g(&op).set_t_minutes("4".into());
    g(&op).set_t_seconds("30".into());
    g(&op).invoke_save();
    let t = app.data.lock().unwrap().automation.timers.iter().find(|t| t.name == "Offering").cloned().expect("saved");
    assert_eq!(t.kind, Lenient::Known(TimerKind::Duration { secs: 270 }));
    assert_eq!(g(&op).get_error(), "");
    // An empty name is refused with the core's words, and the form stays.
    g(&op).invoke_new_one();
    g(&op).invoke_save();
    assert_eq!(g(&op).get_error(), crate::automation::strings::name_length());
}

#[test]
fn macro_steps_are_added_moved_and_saved() {
    let app = test_app();
    let op = opened(&app);
    open(&op);
    g(&op).set_tab(2);
    g(&op).invoke_tab_changed(2);
    g(&op).invoke_new_one();
    g(&op).set_k_name("Sermon".into());
    let kind = |k: StepKind| STEP_KINDS.iter().position(|x| *x == k).unwrap() as i32;
    g(&op).set_step_kind(kind(StepKind::TimerStart));
    g(&op).invoke_step_kind_changed();
    g(&op).set_step_target(1); // Sermon
    g(&op).invoke_step_add();
    g(&op).set_step_kind(kind(StepKind::MessageShow));
    g(&op).invoke_step_kind_changed();
    g(&op).set_step_text("Number=12".into());
    g(&op).invoke_step_add();
    g(&op).invoke_step_move(1, -1);
    assert_eq!(g(&op).get_k_steps().row_count(), 2);
    g(&op).invoke_save();
    assert_eq!(g(&op).get_error(), "");
    let m = app.data.lock().unwrap().automation.macros.iter().find(|m| m.name == "Sermon").cloned().expect("saved");
    assert!(matches!(m.steps[0].known(), Some(MacroStep::MessageShow { template, .. }) if *template == NURSERY_ID));
    assert_eq!(m.steps[1].known(), Some(&MacroStep::TimerStart { timer: SERMON_ID }));
    // The strip shows its button.
    refresh(&op);
    let names: Vec<String> = op.ui.global::<AutoStrip>().get_macros().iter().flat_map(|r| r.buttons.iter().map(|b| b.name.to_string()).collect::<Vec<_>>()).collect();
    assert_eq!(names, ["Sermon"]);
}

#[test]
fn show_message_needs_its_box_then_shows_and_the_button_hides_it() {
    let app = test_app();
    let op = opened(&app);
    click(&op, "AutomationStrip::message-btn");
    assert_eq!(op.dialog(), Some(DialogKind::ShowMessage));
    let m = op.ui.global::<ShowMessageDlg>();
    assert_eq!(m.get_fields().row_count(), 1);
    m.invoke_show();
    assert_eq!(m.get_error(), crate::automation::strings::fill_in("Number"));
    m.invoke_field_edited(0, "27".into());
    assert_eq!(m.get_preview(), "Parent of child #27, please come to the nursery.");
    m.invoke_show();
    assert_eq!(op.dialog(), None);
    op.apply_live(crate::app::LiveUpdate::new(app.live_view()));
    assert!(op.ui.global::<AutoStrip>().get_message_on());
    click(&op, "AutomationStrip::message-btn");
    op.apply_live(crate::app::LiveUpdate::new(app.live_view()));
    assert!(!op.ui.global::<AutoStrip>().get_message_on());
}

#[test]
fn running_timer_gets_a_chip_that_pauses() {
    let app = test_app();
    let op = opened(&app);
    assert_eq!(op.ui.global::<AutoStrip>().get_timers().row_count(), 0, "stopped timers have no chip");
    crate::ui::ops::Ops::inline(app.clone()).call::<Value>("timer/start", json!({ "id": SERMON_ID }), |r| { r.unwrap(); });
    op.apply_timers();
    let chips = op.ui.global::<AutoStrip>().get_timers();
    assert_eq!(chips.row_count(), 1);
    assert!(chips.row_data(0).unwrap().running);
    op.ui.global::<AutoStrip>().invoke_timer_toggle(SERMON_ID.to_string().into());
    assert_eq!(app.timers().timers.iter().find(|t| t.id == SERMON_ID).unwrap().state, TimerState::Paused);
}

#[test]
fn video_and_audio_timers_save_from_the_kind_list() {
    let app = test_app();
    let op = opened(&app);
    open(&op);
    for (kind, want) in [(3, TimerKind::VideoLeft), (4, TimerKind::AudioLeft)] {
        g(&op).invoke_new_one();
        g(&op).set_t_name(format!("Left {kind}").into());
        g(&op).set_t_kind(kind);
        g(&op).invoke_save();
        assert_eq!(g(&op).get_error(), "");
        let t = app.data.lock().unwrap().automation.timers.iter().find(|t| t.name == format!("Left {kind}")).cloned().expect("saved");
        assert_eq!(t.kind, Lenient::Known(want));
        assert_eq!(g(&op).get_t_kind(), kind, "the form opens again on its kind");
    }
    let subs: Vec<String> = g(&op).get_rows().iter().map(|r| r.sub.to_string()).collect();
    assert!(subs.contains(&"Time left in the video".to_string()) && subs.contains(&"Time left in the audio".to_string()), "{subs:?}");
}

#[test]
fn calendar_entries_are_made_on_their_tab() {
    let app = test_app();
    let op = opened(&app);
    open(&op);
    goto(&op, CALENDAR_TAB, -1);
    g(&op).invoke_new_one();
    assert_eq!(g(&op).get_c_day_names().row_count(), 7);
    assert_eq!(g(&op).get_c_days().row_data(0), Some(true), "a new entry is on Sundays");
    g(&op).set_c_name("Sunday countdown".into());
    g(&op).set_c_at("10:45".into());
    g(&op).invoke_c_day_toggle(3); // and Wednesdays
    let pick = |k: StepKind| {
        let macros = app.data.lock().unwrap().automation.macros.len();
        (macros + STEP_KINDS.iter().position(|x| *x == k).unwrap()) as i32
    };
    g(&op).set_c_what(pick(StepKind::TimerRestart));
    g(&op).invoke_c_what_changed();
    g(&op).set_step_target(1); // Sermon
    g(&op).invoke_save();
    assert_eq!(g(&op).get_error(), "");
    let e = app.data.lock().unwrap().automation.calendar[0].clone();
    assert_eq!((e.name.as_str(), e.at, e.days), ("Sunday countdown", TimeOfDay::new(10, 45), 0b1001));
    assert_eq!(e.run.known(), Some(&crate::automation::calendar::CalendarRun::Step { step: MacroStep::TimerRestart { timer: SERMON_ID } }));
    assert_eq!(g(&op).get_rows().row_data(0).unwrap().sub, "Sun, Wed at 10:45 AM · Restart timer: Sermon");
    assert!(g(&op).get_c_next().starts_with("Next: "), "{}", g(&op).get_c_next());
    // It opens again with what was saved.
    g(&op).invoke_pick(0);
    assert_eq!(g(&op).get_c_what(), pick(StepKind::TimerRestart));
    assert_eq!(g(&op).get_step_target(), 1);
    // Try it now.
    g(&op).invoke_calendar_run();
    assert_eq!(g(&op).get_note(), crate::automation::strings::calendar_ran("Sunday countdown"));
    assert_eq!(app.timers().timers.iter().find(|t| t.id == SERMON_ID).unwrap().state, TimerState::Running);
    // No days is refused with the core's words.
    g(&op).invoke_c_day_toggle(0);
    g(&op).invoke_c_day_toggle(3);
    g(&op).invoke_save();
    assert_eq!(g(&op).get_error(), crate::automation::strings::calendar_no_days());
    g(&op).invoke_delete();
    assert!(app.data.lock().unwrap().automation.calendar.is_empty());
}

#[test]
fn days_read_plainly() {
    assert_eq!(days_label(0x7f), "Every day");
    assert_eq!(days_label(0b011_1110), "Weekdays");
    assert_eq!(days_label(0b100_0001), "Sun, Sat");
}

#[test]
fn pre_service_tab_saves_a_stage_layout_and_the_menu_runs_it() {
    let app = test_app();
    let op = opened(&app);
    open(&op);
    goto(&op, 3, -1);
    let options: Vec<String> = g(&op).get_p_stage_options().iter().map(|s| s.to_string()).collect();
    assert_eq!(options[0], "Leave it as it is");
    assert!(options.len() > 2, "the built-in layouts are there: {options:?}");
    g(&op).set_p_stage(2);
    g(&op).invoke_preservice_save();
    let want = crate::stage::presets::builtins()[1].id;
    let data = app.data.lock().unwrap();
    assert_eq!(data.automation.pre_service.as_ref().and_then(|p| p.stage_layout), Some(want));
    let m = data.automation.macros.first().unwrap();
    assert_eq!(m.steps.last(), Some(&Lenient::Known(MacroStep::StageLayout { layout: want })));
}
