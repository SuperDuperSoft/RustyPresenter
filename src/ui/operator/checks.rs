//! The Check button (Preflight, spec §13) and the stage message (spec §12.2).
use super::{strings as tr, window::Operator};
use crate::{preflight, ui::dialogs::ask};
use std::rc::Rc;

pub(super) fn wire(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ui.on_check_service(move || if let Some(op) = me.upgrade() { check(&op) });
    let me = Rc::downgrade(op);
    op.ui.on_stage_message(move || if let Some(op) = me.upgrade() { stage_message(&op) });
}

fn check(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ops().preflight(move |problems| if let Some(op) = me.upgrade() { ask::notice(&op, &preflight::report(&problems)) });
}

fn stage_message(op: &Rc<Operator>) {
    let current = op.app().live_view().stage_message;
    let me = Rc::downgrade(op);
    ask::prompt(op, tr::STAGE_MESSAGE_ASK, &current, move |text| {
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
    fn stage_screen(app: &App) { app.data.lock().unwrap().settings.displays.stage = Some("stage".into()); }

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
        assert!(row.text.contains(pf::NO_ROOM), "{}", row.text);
        assert!(row.text.contains(pf::EMPTY_SERVICE), "{}", row.text);
        assert_eq!(row.cancel_label, "", "a report, not a question");
        assert_eq!(app.live_view().version, version);
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
        assert_eq!(row.text, tr::STAGE_MESSAGE_ASK);
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
