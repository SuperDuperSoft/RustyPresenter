//! The Check button (Preflight, spec §13) and the stage message (spec §12.2).
use super::{strings as tr, window::Operator};
use crate::{preflight, ui::dialogs::ask};
use std::rc::Rc;

/// N2: how often the service is checked by itself, for the status pill.
pub const EVERY: std::time::Duration = std::time::Duration::from_secs(30);

pub(super) fn wire(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ui.on_check_service(move || if let Some(op) = me.upgrade() { check(&op); recheck(&op) });
    let me = Rc::downgrade(op);
    op.ui.on_stage_message(move || if let Some(op) = me.upgrade() { stage_message(&op) });
}

/// Starts the quiet check: once now, then every `EVERY`, while the window is open.
pub(super) fn start_watch(op: &Rc<Operator>) {
    recheck(op);
    let me = Rc::downgrade(op);
    op.check_timer.start(slint::TimerMode::Repeated, EVERY, move || if let Some(op) = me.upgrade() { recheck(&op) });
}

/// Runs the check off the UI thread and puts the answer on the status pill.
pub(super) fn recheck(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ops().preflight(move |problems| if let Some(op) = me.upgrade() { show_pill(&op, &problems) });
}

fn show_pill(op: &Operator, problems: &[String]) {
    let p = pill(problems);
    op.ui.set_check_state(p.state.into());
    op.ui.set_check_label(p.label.into());
    op.ui.set_check_tip(p.tip.into());
}

/// What the status pill says for these problems (N2).
#[derive(Debug, PartialEq)]
pub struct CheckPill { pub state: &'static str, pub label: String, pub tip: String }

/// At most this many problems in the pill's tooltip; clicking it shows the full report.
const TIP_LINES: usize = 3;

pub fn pill(problems: &[String]) -> CheckPill {
    if problems.is_empty() {
        return CheckPill { state: "on", label: crate::tr!("Ready"), tip: crate::tr!("Nothing missing for this service.") };
    }
    let mut tip: Vec<String> = problems.iter().take(TIP_LINES).cloned().collect();
    if problems.len() > TIP_LINES { tip.push(crate::trn!("…and {n} more", "…and {n} more", problems.len() - TIP_LINES)) }
    tip.push(crate::tr!("Click to see everything."));
    CheckPill { state: "warn", label: crate::trn!("{n} problem", "{n} problems", problems.len()), tip: tip.join("\n") }
}

fn check(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ops().preflight(move |problems| if let Some(op) = me.upgrade() { ask::notice(&op, &preflight::report(&problems)) });
}

fn stage_message(op: &Rc<Operator>) {
    let current = op.app().live_view().stage_message;
    let me = Rc::downgrade(op);
    ask::prompt(op, &tr::stage_message_ask(), &current, move |text| {
        let (Some(op), Some(text)) = (me.upgrade(), text) else { return };
        let me = Rc::downgrade(&op);
        op.ops().call::<serde_json::Value>("stage/message", serde_json::json!({ "text": text }), move |r| {
            if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(e, true) }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        preflight::strings as pf,
        ui::{operator::window::testing::{find, open, refresh}},
    };
    use slint::{Model, platform::PointerEventButton};
    use std::sync::Arc;

    fn click(op: &Operator, id: &str) {
        // `narrow` changes at the top of an event loop turn, which moves the buttons: let it, then look for the button.
        op.window().dispatch_event(slint::platform::WindowEvent::PointerMoved { position: slint::LogicalPosition::new(0.0, 0.0) });
        find(op, id).unwrap_or_else(|| panic!("{id}")).mock_single_click(PointerEventButton::Left);
    }
    fn asked(op: &Operator) -> Option<crate::ui::AskRow> { op.ui.get_ask().row_data(0) }
    fn stage_screen(app: &App) { app.data.lock().unwrap().settings.screens.assign("stage", crate::model::ScreenTarget::Monitor("stage".into())); }

    fn opened() -> (Arc<App>, Rc<Operator>) {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        (app, op)
    }

    #[test]
    fn check_button_reports_what_is_wrong_and_changes_nothing() {
        let (app, op) = opened();
        let version = app.live_view().version;
        click(&op, "ServiceColumn::check-btn");
        let row = asked(&op).expect("a report");
        assert!(row.text.contains(&pf::no_room()), "{}", row.text);
        assert!(row.text.contains(&pf::empty_service()), "{}", row.text);
        assert_eq!(row.cancel_label, "", "a report, not a question");
        assert_eq!(app.live_view().version, version);
    }

    #[test]
    fn pill_says_ready_or_counts_problems_and_lists_a_few() {
        let ok = pill(&[]);
        assert_eq!((ok.state, ok.label.as_str()), ("on", "Ready"));
        let one = pill(&["Room screen is not connected".into()]);
        assert_eq!((one.state, one.label.as_str()), ("warn", "1 problem"));
        assert!(one.tip.starts_with("Room screen is not connected"), "{}", one.tip);
        let five: Vec<String> = (1..=5).map(|i| format!("p{i}")).collect();
        let many = pill(&five);
        assert_eq!(many.label, "5 problems");
        assert!(many.tip.contains("p3") && !many.tip.contains("p4") && many.tip.contains("2 more"), "{}", many.tip);
    }

    #[test]
    fn status_pill_shows_the_quiet_check_and_opens_the_report() {
        let (_app, op) = opened();
        recheck(&op);
        assert_eq!(op.ui.get_check_state(), "warn", "a test app has no room screen");
        click(&op, "TopBar::status-check");
        let row = asked(&op).expect("a report");
        assert!(row.text.contains(&pf::no_room()), "{}", row.text);
    }

    #[test]
    fn stage_message_button_only_shows_with_a_stage_screen() {
        let (app, op) = opened();
        assert!(find(&op, "TopBar::message-btn").is_none());
        stage_screen(&app);
        refresh(&op);
        assert!(find(&op, "TopBar::message-btn").is_some());
    }

    #[test]
    fn stage_message_asks_then_sends_and_starts_from_the_current_text() {
        let (app, op) = opened();
        stage_screen(&app);
        refresh(&op);
        click(&op, "TopBar::message-btn");
        let row = asked(&op).expect("a question");
        assert!(row.prompt);
        assert_eq!(row.text, tr::stage_message_ask());
        assert_eq!(row.value, "");
        ask::answer(&op, true, "Wrap up".into());
        assert_eq!(app.live_view().stage_message, "Wrap up");
        click(&op, "TopBar::message-btn");
        assert_eq!(asked(&op).unwrap().value, "Wrap up");
    }

    #[test]
    fn top_bar_fits_with_the_message_button_on_small_laptops() {
        let (app, op) = opened();
        stage_screen(&app);
        refresh(&op);
        for w in [1024.0, 1250.0, 1251.0, 1280.0, 1366.0, 1440.0] {
            op.window().set_size(slint::LogicalSize::new(w, 768.0));
            slint::platform::update_timers_and_animations();
            let e = find(&op, "TopBar::settings-btn").unwrap();
            let right = e.absolute_position().x + e.size().width;
            eprintln!("width {w}: settings ends at {right}");
            assert!(right <= w, "at {w} the Settings button ends at {right}");
        }
    }

    #[test]
    fn cancelling_the_stage_message_leaves_it() {
        let (app, op) = opened();
        stage_screen(&app);
        refresh(&op);
        op.ops().call::<serde_json::Value>("stage/message", serde_json::json!({ "text": "Wrap up" }), |_| {});
        click(&op, "TopBar::message-btn");
        assert!(asked(&op).is_some());
        ask::answer(&op, false, String::new());
        assert_eq!(app.live_view().stage_message, "Wrap up");
    }
}
