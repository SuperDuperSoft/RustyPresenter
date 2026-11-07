use super::*;
use crate::model::{Background, BlankKind, DateOrder, ItemKind, Paper, Service, ServiceItem};
use chrono::{NaiveDate, Weekday};
use uuid::Uuid;

fn d(y: i32, m: u32, day: u32) -> NaiveDate { NaiveDate::from_ymd_opt(y, m, day).unwrap() }

fn dated(name: &str, date: Option<NaiveDate>) -> Service { Service { date, ..Service::new(name) } }

fn item(kind: ItemKind) -> ServiceItem { ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None } }

fn song() -> ItemKind { ItemKind::Song { song_id: Uuid::new_v4(), arrangement: None } }
fn text(t: &str) -> ItemKind { ItemKind::Text { title: t.into(), body: "b".into() } }
fn reading() -> ItemKind {
    ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses: vec![], per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() }
}

// Sunday November 8, 2026 is a Sunday.
const SUN: (i32, u32, u32) = (2026, 11, 8);
fn sun() -> NaiveDate { d(SUN.0, SUN.1, SUN.2) }

#[test]
fn week_source_prefers_latest_past_date() {
    let s = [dated("A", Some(d(2026, 10, 25))), dated("B", Some(d(2026, 11, 1))), dated("C", None)];
    assert_eq!(week_source(&s, Some(s[2].id), d(2026, 11, 4)), Some(s[1].id));
}

#[test]
fn week_source_ignores_future_dated_services() {
    let s = [dated("A", Some(d(2026, 11, 1))), dated("Christmas", Some(d(2026, 12, 24)))];
    assert_eq!(week_source(&s, Some(s[1].id), d(2026, 11, 4)), Some(s[0].id));
}

#[test]
fn week_source_future_only_falls_back_to_current() {
    let s = [dated("A", None), dated("Christmas", Some(d(2026, 12, 24)))];
    assert_eq!(week_source(&s, Some(s[0].id), d(2026, 11, 4)), Some(s[0].id));
}

#[test]
fn week_source_ties_take_later_service() {
    let s = [dated("A", Some(d(2026, 11, 1))), dated("B", Some(d(2026, 11, 1)))];
    assert_eq!(week_source(&s, None, d(2026, 11, 4)), Some(s[1].id));
}

#[test]
fn week_source_falls_back_to_current_when_undated() {
    let s = [dated("A", None), dated("B", None)];
    assert_eq!(week_source(&s, Some(s[1].id), sun()), Some(s[1].id));
    assert_eq!(week_source(&[], None, sun()), None);
}

#[test]
fn next_week_adds_seven_days() {
    assert_eq!(next_week_date(Some(d(2026, 11, 1)), d(2026, 11, 4)), sun());
}

#[test]
fn next_week_skips_to_future_same_weekday() {
    // Last used in September; the next Sunday from Wednesday Nov 4 is Nov 8.
    assert_eq!(next_week_date(Some(d(2026, 9, 6)), d(2026, 11, 4)), sun());
}

#[test]
fn next_week_undated_is_next_sunday() {
    assert_eq!(next_week_date(None, d(2026, 11, 4)), sun());
}

#[test]
fn next_week_on_sunday_undated_is_today() {
    assert_eq!(next_week_date(None, sun()), sun());
}

#[test]
fn dated_name_replaces_us_date() {
    assert_eq!(dated_name("Service 11/1/2026", Some(d(2026, 11, 1)), sun(), DateOrder::Mdy), "Service 11/8/2026");
}

#[test]
fn dated_name_replaces_uk_date_matching_old() {
    assert_eq!(dated_name("Service 8/11/2026", Some(sun()), d(2026, 11, 15), DateOrder::Dmy), "Service 15/11/2026");
}

#[test]
fn dated_name_keeps_non_matching_date_text() {
    // "Psalm 23" is not a date, and "1/1/2020" is not this service's date.
    assert_eq!(dated_name("Psalm 23", Some(d(2026, 11, 1)), sun(), DateOrder::Mdy), "Psalm 23 11/8/2026");
    assert_eq!(dated_name("Reunion 1/1/2020", Some(d(2026, 11, 1)), sun(), DateOrder::Mdy), "Reunion 1/1/2020 11/8/2026");
}

#[test]
fn dated_name_replaces_iso_date() {
    assert_eq!(dated_name("Service 2026-11-01", Some(d(2026, 11, 1)), sun(), DateOrder::Ymd), "Service 2026-11-08");
    // Undated (pre-FEAT-6): a date in the computer's order or ISO goes.
    assert_eq!(dated_name("Service 2026-11-01", None, sun(), DateOrder::Mdy), "Service 11/8/2026");
    assert_eq!(dated_name("Service 11/1/26", None, sun(), DateOrder::Mdy), "Service 11/8/2026");
}

#[test]
fn dated_name_appends_when_no_date() {
    assert_eq!(dated_name("Sunday Morning", None, d(2026, 11, 15), DateOrder::Dmy), "Sunday Morning 15/11/2026");
    assert_eq!(dated_name("11/1/2026", Some(d(2026, 11, 1)), sun(), DateOrder::Mdy), "Service 11/8/2026");
}

#[test]
fn dated_name_strips_en_dash() {
    assert_eq!(dated_name("Easter – 11/1/2026", Some(d(2026, 11, 1)), sun(), DateOrder::Mdy), "Easter 11/8/2026");
    assert_eq!(dated_name("Easter - 11/1/2026", Some(d(2026, 11, 1)), sun(), DateOrder::Mdy), "Easter 11/8/2026");
}

#[test]
fn region_defaults_us_gb_jp_unknown() {
    assert_eq!(region_defaults(Some("en-US")), (DateOrder::Mdy, Paper::Letter));
    assert_eq!(region_defaults(Some("en_GB.UTF-8")), (DateOrder::Dmy, Paper::A4));
    assert_eq!(region_defaults(Some("ja-JP")), (DateOrder::Ymd, Paper::A4));
    assert_eq!(region_defaults(Some("zh-Hans-CN")), (DateOrder::Ymd, Paper::A4));
    assert_eq!(region_defaults(Some("es-MX")), (DateOrder::Dmy, Paper::Letter));
    assert_eq!(region_defaults(Some("C")), (DateOrder::Ymd, Paper::Letter));
    assert_eq!(region_defaults(None), (DateOrder::Ymd, Paper::Letter));
}

#[test]
fn dates_format_and_parse_in_each_order() {
    assert_eq!(format_date(sun(), DateOrder::Mdy), "11/8/2026");
    assert_eq!(format_date(sun(), DateOrder::Dmy), "8/11/2026");
    assert_eq!(format_date(sun(), DateOrder::Ymd), "2026-11-08");
    assert_eq!(format_day_date(sun(), DateOrder::Mdy), "Sun 11/8/2026");
    assert_eq!(format_date_long(sun(), DateOrder::Mdy), "Sunday, November 8, 2026");
    assert_eq!(format_date_long(sun(), DateOrder::Dmy), "Sunday 8 November 2026");
    assert_eq!(parse_date("11/8/2026", DateOrder::Mdy), Some(sun()));
    assert_eq!(parse_date("8/11/2026", DateOrder::Dmy), Some(sun()));
    assert_eq!(parse_date(" 2026-11-08 ", DateOrder::Mdy), Some(sun()), "ISO always works");
    assert_eq!(parse_date("31/2/2026", DateOrder::Dmy), None);
    assert_eq!(parse_date("next sunday", DateOrder::Mdy), None);
}

#[test]
fn default_date_next_weekday_includes_today() {
    assert_eq!(default_date(Some(Weekday::Sun), sun()), sun());
    assert_eq!(default_date(Some(Weekday::Sun), d(2026, 11, 9)), d(2026, 11, 15));
    assert_eq!(default_date(Some(Weekday::Wed), sun()), d(2026, 11, 11));
}

#[test]
fn default_date_any_day_is_today() {
    assert_eq!(default_date(None, d(2026, 11, 4)), d(2026, 11, 4));
}

#[test]
fn copy_items_new_ids_keep_all_fields() {
    let mut a = item(song());
    a.notes = "Key of G".into();
    a.background = Some(Background::Color { color: "#123456".into() });
    let (out, n) = copy_items(std::slice::from_ref(&a), CopyMode::LastWeek { blanks: false });
    assert_eq!(n, 0);
    assert_ne!(out[0].id, a.id);
    assert_eq!(out[0].notes, "Key of G");
    assert_eq!(out[0].background, a.background);
    assert!(matches!((&out[0].kind, &a.kind), (ItemKind::Song { song_id: x, .. }, ItemKind::Song { song_id: y, .. }) if x == y));
}

#[test]
fn copy_last_week_drops_a_custom_song_order_and_copy_this_service_keeps_it() {
    let a = item(ItemKind::Song { song_id: Uuid::new_v4(), arrangement: Some(vec!["Chorus".into()]) });
    let (out, _) = copy_items(std::slice::from_ref(&a), CopyMode::LastWeek { blanks: false });
    assert!(matches!(out[0].kind, ItemKind::Song { arrangement: None, .. }));
    let (out, _) = copy_items(std::slice::from_ref(&a), CopyMode::ThisService);
    assert!(matches!(&out[0].kind, ItemKind::Song { arrangement: Some(o), .. } if o == &["Chorus"]));
}

#[test]
fn copy_items_blanks_keep_notes_and_background() {
    let mut a = item(song());
    a.notes = "Key of G".into();
    a.background = Some(Background::Color { color: "#123456".into() });
    let items = [a.clone(), item(reading()), item(text("Welcome")), item(ItemKind::Header { title: "Worship".into(), color: String::new() })];
    let (out, n) = copy_items(&items, CopyMode::LastWeek { blanks: true });
    assert_eq!(n, 2);
    assert!(matches!(&out[0].kind, ItemKind::Blank { label, want: BlankKind::Song } if label == "Song"));
    assert_eq!((out[0].notes.as_str(), &out[0].background), ("Key of G", &a.background));
    assert!(matches!(&out[1].kind, ItemKind::Blank { label, want: BlankKind::Scripture } if label == "Bible reading"));
    assert!(matches!(&out[2].kind, ItemKind::Text { .. }));
    assert!(matches!(&out[3].kind, ItemKind::Header { .. }));
    let (_, n) = copy_items(&items, CopyMode::ThisService);
    assert_eq!(n, 0, "Copy this service never blanks");
}

#[test]
fn copy_items_keeps_existing_blanks() {
    let b = item(ItemKind::Blank { label: "Offering song".into(), want: BlankKind::Song });
    let (out, n) = copy_items(&[b], CopyMode::LastWeek { blanks: false });
    assert_eq!(n, 1);
    assert!(matches!(&out[0].kind, ItemKind::Blank { label, .. } if label == "Offering song"));
}

#[test]
fn copy_week_names_dates_and_keeps_template() {
    let mut s = dated("Service 11/1/2026", Some(d(2026, 11, 1)));
    s.template = Some(Uuid::new_v4());
    s.items.push(item(song()));
    let c = copy_week(&s, sun(), false, DateOrder::Mdy);
    assert_eq!((c.name.as_str(), c.date, c.template), ("Service 11/8/2026", Some(sun()), s.template));
    assert_ne!(c.id, s.id);
    assert_eq!(c.items.len(), 1);
}

#[test]
fn template_from_service_keep_and_blank() {
    let mut s = dated("Sunday Service 11/1/2026", Some(d(2026, 11, 1)));
    s.items = vec![item(text("Welcome")), item(song()), item(reading())];
    let name = template_name(&s, DateOrder::Mdy);
    assert_eq!(name, "Sunday Service");
    let t = template_from_service(&s, &name);
    assert_eq!(t.day, Some(Weekday::Sun));
    assert!(matches!(&t.items[0].kind, ItemKind::Text { title, .. } if title == "Welcome"));
    assert_ne!(t.items[0].id, s.items[0].id);
    assert_eq!(blank_count(&t.items), 2);
}

#[test]
fn template_blank_labels_default_by_kind() {
    let labels: Vec<String> = [BlankKind::Song, BlankKind::Scripture, BlankKind::Media, BlankKind::Deck, BlankKind::Text, BlankKind::Any]
        .into_iter().map(strings::blank_label).collect();
    assert_eq!(labels, ["Song", "Bible reading", "Media", "Slides", "Announcement", "Something"]);
    assert_eq!(blank_title("", BlankKind::Song), "Song (blank)");
    assert_eq!(blank_title("Closing song", BlankKind::Song), "Closing song (blank)");
}

#[test]
fn template_day_comes_from_the_service_or_sunday() {
    let t = template_from_service(&dated("Midweek", Some(d(2026, 11, 4))), "Midweek");
    assert_eq!(t.day, Some(Weekday::Wed));
    assert_eq!(template_from_service(&dated("X", None), "X").day, Some(Weekday::Sun));
    assert_eq!(day_label(Some(Weekday::Sun)), "Sundays");
    assert_eq!(day_label(None), "Any day");
}

#[test]
fn service_from_template_fresh_ids_and_template_id() {
    let mut s = dated("Sunday", None);
    s.items = vec![item(text("Welcome")), item(song())];
    let t = template_from_service(&s, "Sunday");
    let made = service_from_template(&t, "Sunday 11/8/2026", sun());
    assert_eq!((made.template, made.date, made.name.as_str()), (Some(t.id), Some(sun()), "Sunday 11/8/2026"));
    assert!(made.items.iter().zip(&t.items).all(|(a, b)| a.id != b.id));
    assert_eq!(blank_count(&made.items), 1);
}

#[test]
fn fill_replaces_blank_keeping_id_notes_and_background() {
    let mut b = item(ItemKind::Blank { label: "Song".into(), want: BlankKind::Song });
    b.notes = "Something upbeat".into();
    b.background = Some(Background::Color { color: "#000011".into() });
    let mut chosen = item(song());
    chosen.notes = "Key of D".into();
    let f = fill(&b, chosen.clone());
    assert_eq!(f.id, b.id);
    assert!(matches!(f.kind, ItemKind::Song { .. }));
    assert_eq!(f.notes, "Something upbeat\n\nKey of D");
    assert_eq!(f.background, b.background, "the blank's background when the choice has none");
    chosen.background = Some(Background::Color { color: "#ffffff".into() });
    chosen.notes.clear();
    let f = fill(&b, chosen.clone());
    assert_eq!((f.background, f.notes.as_str()), (chosen.background, "Something upbeat"));
}

#[test]
fn services_sort_by_date_newest_first_undated_last() {
    let s = [dated("Old", None), dated("A", Some(d(2026, 11, 1))), dated("Older", None), dated("B", Some(sun()))];
    let names: Vec<&str> = sorted(&s).iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["B", "A", "Old", "Older"]);
    assert_eq!(find_on_date(&s, sun()), Some(s[3].id));
    assert_eq!(find_on_date(&s, d(2020, 1, 1)), None);
}

#[test]
fn header_colours_step_round() {
    assert_eq!(header_color(""), HEADER_COLORS[0]);
    assert_eq!(header_color("#3ECF8E"), HEADER_COLORS[1]);
    assert_eq!(next_header_color(""), HEADER_COLORS[0]);
    assert_eq!(next_header_color(HEADER_COLORS[0]), HEADER_COLORS[1]);
    assert_eq!(next_header_color(HEADER_COLORS[5]), HEADER_COLORS[0]);
}

#[test]
fn old_data_loads_with_no_dates_and_no_templates_and_blanks_round_trip() {
    let old = r#"{"format":1,"services":[{"id":"6f7b3c1e-1a2b-4c3d-8e9f-001122334455","name":"Sunday","items":[]}]}"#;
    let data: crate::model::Data = serde_json::from_str(old).unwrap();
    assert_eq!(data.services[0].date, None);
    assert!(data.templates.is_empty());
    assert_eq!(data.settings.print, crate::model::PrintSettings::default());
    let json = serde_json::to_string(&data).unwrap();
    assert!(!json.contains("\"date\"") && !json.contains("templates"), "nothing new is written for old data");

    let mut s = dated("X", Some(sun()));
    s.items.push(item(ItemKind::Blank { label: "Song".into(), want: BlankKind::Song }));
    s.items.push(item(ItemKind::Header { title: "Worship".into(), color: "#4aa3ff".into() }));
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains(r#""date":"2026-11-08""#), "{json}");
    let back: Service = serde_json::from_str(&json).unwrap();
    assert_eq!(back.date, Some(sun()));
    assert!(matches!(&back.items[0].kind, ItemKind::Blank { want: BlankKind::Song, .. }));
    assert!(matches!(&back.items[1].kind, ItemKind::Header { title, .. } if title == "Worship"));
    let t: crate::model::ServiceTemplate = serde_json::from_str(r#"{"id":"6f7b3c1e-1a2b-4c3d-8e9f-001122334455","name":"T","items":[]}"#).unwrap();
    assert_eq!(t.day, Some(Weekday::Sun), "a template with no day is for Sundays");
}
