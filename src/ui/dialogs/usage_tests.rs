use super::*;
use crate::ui::dialogs::settings::testing::{find_any as find, find_all_any as find_all};
use crate::{
    app::{App, test_app},
    ui::{
        dialogs::{SettingsTab, ask, settings},
        operator::{files::testing::will_pick, window::testing::{open as open_window, refresh}},
    },
    usage::UsageEntry,
};
use chrono::{Days, Utc};
use slint::{Model, platform::PointerEventButton};
use std::sync::Arc;
use uuid::Uuid;

fn opened(app: &Arc<App>) -> Rc<Operator> {
    let op = open_window(app);
    op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
    slint::platform::update_timers_and_animations();
    refresh(&op);
    op
}

fn g(op: &Operator) -> UsageDlg<'_> { op.ui.global::<UsageDlg>() }

fn settle() { slint::platform::update_timers_and_animations(); }

fn click(op: &Operator, id: &str) {
    settle();
    find(op, id).unwrap_or_else(|| panic!("{id} not found")).mock_single_click(PointerEventButton::Left);
    settle();
}

fn toasts(op: &Operator) -> Vec<String> {
    find_all(op, "ToastStack::toast").iter().map(|t| t.accessible_label().unwrap_or_default().to_string()).collect()
}

/// The question over the operator window, or over Settings.
fn asked(op: &Operator) -> Option<String> {
    let ask = slint::Model::row_data(&op.ui.get_ask(), 0).or_else(|| slint::Model::row_data(&crate::ui::dialogs::settings_window::win(op).get_ask(), 0));
    ask.map(|a| a.text.to_string())
}

/// A logged use today of a song with this CCLI number.
fn log_today(app: &App, title: &str, ccli: &str) -> Uuid {
    let e = UsageEntry {
        id: Uuid::new_v4(), at: Utc::now(), local_date: today(), song_id: Uuid::new_v4(), title: title.into(), ccli: ccli.into(),
        writers: String::new(), copyright: String::new(), service_id: Uuid::new_v4(), service_name: "Sunday Service".into(), streamed: false,
    };
    let id = e.id;
    app.usage.edit(|l| l.entries.push(e)).unwrap();
    id
}

fn open_report(app: &Arc<App>) -> Rc<Operator> {
    let op = opened(app);
    op.ui.invoke_usage_report();
    settle();
    assert_eq!(op.dialog(), Some(DialogKind::Usage));
    op
}

#[test]
fn window_callback_opens_the_report_with_its_title() {
    let app = test_app();
    log_today(&app, "Amazing Grace", "22025");
    log_today(&app, "Our own song", "");
    let op = open_report(&app);
    assert_eq!(op.ui.get_dialog_title(), strings::title());
    assert_eq!(g(&op).get_license_line(), strings::no_license());
    assert_eq!(g(&op).get_rows().row_count(), 1, "no CCLI number, not listed by default");
    assert_eq!(g(&op).get_rows().row_data(0).unwrap().title, "Amazing Grace");
    assert_eq!(g(&op).get_entries().row_count(), 2, "Details lists every use");
    click(&op, "UsageDialog::include");
    assert_eq!(g(&op).get_rows().row_count(), 2);
}

#[test]
fn usage_dialog_defaults_since_last_report() {
    let app = test_app();
    let through = today().checked_sub_days(Days::new(10)).unwrap();
    app.usage.edit(|l| l.reported_through = Some(through)).unwrap();
    let op = open_report(&app);
    assert_eq!(g(&op).get_from(), export::day(through.checked_add_days(Days::new(1)).unwrap()));
    assert_eq!(g(&op).get_to(), export::day(today()));
    assert!(g(&op).get_has_since_last());
    assert_eq!(g(&op).get_mark_label(), strings::mark_reported(&export::day(today())));
    click(&op, "UsageDialog::this-year");
    assert!(g(&op).get_from().ends_with("-01-01"));
}

#[test]
fn bad_dates_say_so() {
    let app = test_app();
    let op = open_report(&app);
    g(&op).set_from("2026-13-40".into());
    g(&op).invoke_dates_edited();
    assert_eq!(g(&op).get_error(), strings::bad_date());
    g(&op).set_from("2099-01-01".into());
    g(&op).invoke_dates_edited();
    assert_eq!(g(&op).get_error(), strings::from_after_to());
}

#[test]
fn usage_remove_entry_asks_then_removes() {
    let app = test_app();
    log_today(&app, "Amazing Grace", "22025");
    let op = open_report(&app);
    click(&op, "UsageDialog::details-btn");
    click(&op, "UsageDialog::remove-btn");
    assert_eq!(asked(&op).as_deref(), Some(strings::remove_confirm().as_str()));
    ask::answer(&op, true, String::new());
    settle();
    assert!(app.usage.log().entries.is_empty());
    assert_eq!(g(&op).get_entries().row_count(), 0);
    assert!(toasts(&op).contains(&strings::removed()));
}

#[test]
fn mark_reported_asks_and_moves_the_next_start() {
    let app = test_app();
    let op = open_report(&app);
    click(&op, "UsageDialog::mark-reported");
    let to = export::day(today());
    assert_eq!(asked(&op), Some(strings::mark_confirm(&to)));
    ask::answer(&op, true, String::new());
    settle();
    assert_eq!(app.usage.log().reported_through, Some(today()));
    assert!(g(&op).get_has_since_last());
}

#[test]
fn csv_is_saved_where_picked() {
    let app = test_app();
    log_today(&app, "Amazing Grace", "22025");
    let op = open_report(&app);
    let path = app.store.root.join("picked.csv");
    will_pick(vec![path.clone()]);
    click(&op, "UsageDialog::export-csv");
    assert!(std::fs::read_to_string(&path).unwrap().contains("Amazing Grace,22025"));
    assert!(toasts(&op).iter().any(|t| t.starts_with("Saved ")));
}

#[test]
fn keep_log_checkbox_turns_logging_off() {
    let app = test_app();
    let op = open_report(&app);
    assert!(g(&op).get_keep_log());
    click(&op, "UsageDialog::keep-log");
    assert!(!app.usage.enabled());
}

#[test]
fn settings_link_opens_the_report_at_once_without_changes() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Songs);
    settle();
    click(&op, "SongsPage::usage-report");
    assert_eq!(asked(&op), None);
    assert_eq!(op.dialog(), Some(DialogKind::Usage));
}

#[test]
fn settings_edits_survive_opening_usage_report() {
    let app = test_app();
    let op = opened(&app);
    settings::open(&op, SettingsTab::Songs);
    settle();
    crate::ui::dialogs::settings_window::win(&op).global::<crate::ui::SettingsDlg>().set_ccli("998877".into());
    click(&op, "SongsPage::usage-report");
    assert_eq!(asked(&op).as_deref(), Some(strings::save_first().as_str()));
    ask::answer_settings(&op, true, String::new());
    settle();
    assert_eq!(app.data().settings.church_ccli, "998877");
    assert_eq!(op.dialog(), Some(DialogKind::Usage));
    assert_eq!(g(&op).get_license_line(), strings::license_line("998877"));
}
