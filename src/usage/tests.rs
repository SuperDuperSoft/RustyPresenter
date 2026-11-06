use super::*;
use crate::{
    app::{App, test_app},
    live::{Command, Envelope, RoomEvent},
    model::{ItemKind, ServiceItem, Song},
};
use chrono::TimeZone;

fn at(h: u32, m: u32) -> DateTime<Utc> { Utc.with_ymd_and_hms(2026, 11, 1, h, m, 0).unwrap() }
fn day(y: i32, m: u32, d: u32) -> NaiveDate { NaiveDate::from_ymd_opt(y, m, d).unwrap() }

fn entry(song: Uuid, service: Uuid, when: DateTime<Utc>) -> UsageEntry {
    UsageEntry {
        id: Uuid::new_v4(), at: when, local_date: when.date_naive(), song_id: song, title: "Song".into(), ccli: "123".into(),
        writers: String::new(), copyright: String::new(), service_id: service, service_name: "Sunday".into(), streamed: false,
    }
}

fn named(title: &str, ccli: &str, date: NaiveDate) -> UsageEntry {
    let mut e = entry(Uuid::new_v4(), Uuid::new_v4(), Utc.from_utc_datetime(&date.and_hms_opt(10, 0, 0).unwrap()));
    e.title = title.into();
    e.ccli = ccli.into();
    e
}

// ---- the usage session ----

#[test]
fn logs_once_per_song_per_session() {
    let (song, other, service) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let log = vec![entry(song, service, at(9, 40))];
    assert!(!should_log(&log, song, service, at(9, 50)), "same song, same session");
    assert!(should_log(&log, other, service, at(9, 50)), "another song");
    assert!(should_log(&log, song, Uuid::new_v4(), at(9, 50)), "another service");
    assert!(should_log(&[], song, service, at(9, 50)), "nothing logged yet");
}

#[test]
fn same_service_reused_after_gap_logs_again() {
    let (a, b, service) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    // 9:00 service: a at 9:05, b at 9:40. 11:00 service starts with a at 11:20.
    let log = vec![entry(a, service, at(9, 5)), entry(b, service, at(9, 40))];
    assert!(should_log(&log, a, service, at(11, 20)));
    assert!(should_log(&log, b, service, at(11, 20)));
}

#[test]
fn reprise_within_session_not_logged() {
    let (a, b, c, service) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    // Each song is within 45 minutes of the next, so the run reaches back to 9:00.
    let log = vec![entry(a, service, at(9, 0)), entry(b, service, at(9, 40)), entry(c, service, at(10, 20))];
    assert!(!should_log(&log, a, service, at(11, 0)), "a reprise of the opener, no gap over 45 minutes");
    // After a sermon longer than 45 minutes with no song, the session is over.
    assert!(should_log(&log, a, service, at(11, 6)));
}

#[test]
fn session_survives_restart() {
    let root = std::env::temp_dir().join(format!("rp-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let (song, service) = (Uuid::new_v4(), Uuid::new_v4());
    let now = Utc::now();
    {
        let usage = Usage::open(&root);
        usage.hit(entry(song, service, now - chrono::Duration::minutes(10)));
        usage.flush();
    }
    let usage = Usage::open(&root);
    usage.hit(entry(song, service, now));
    usage.flush();
    assert_eq!(usage.log().entries.len(), 1, "the session comes from the saved log");
}

// ---- what counts (through the app) ----

fn app_with_song(ccli: &str) -> (Arc<App>, Uuid, Uuid) {
    let app = test_app();
    let song = Song { id: Uuid::new_v4(), title: "Amazing Grace".into(), writers: "John Newton".into(), ccli: ccli.into(),
        copyright: "Public Domain".into(), lyrics: "Verse 1\nAmazing grace\n\nVerse 2\nTwas grace".into(), arrangement: vec![] };
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id }, background: None, notes: String::new(), theme: None };
    let item_id = item.id;
    let service = {
        let mut d = app.data.lock().unwrap();
        d.songs.push(song);
        let s = d.service_mut().unwrap();
        s.items.push(item);
        s.id
    };
    (app, item_id, service)
}

fn send(app: &App, cmd: Command) {
    // Next and Prev must name the version they were pressed on.
    let base = Some(app.live.lock().unwrap().version);
    let out = app.command(Envelope { id: Uuid::new_v4().to_string(), base, cmd });
    assert!(out.ok, "{:?}", out.error);
}

fn commit(app: &App) {
    let version = app.live.lock().unwrap().version;
    app.room_event(RoomEvent::Committed { version });
}

fn logged(app: &App) -> Vec<UsageEntry> {
    app.usage.flush();
    app.usage.log().entries.clone()
}

#[test]
fn refresh_and_jumps_do_not_log_twice() {
    let (app, item, service) = app_with_song("22025");
    send(&app, Command::Show { item_id: item, slide: 0 });
    send(&app, Command::Next);
    send(&app, Command::Show { item_id: item, slide: 0 });
    send(&app, Command::Show { item_id: item, slide: 0 });
    let log = logged(&app);
    assert_eq!(log.len(), 1);
    let e = &log[0];
    assert_eq!((e.title.as_str(), e.ccli.as_str(), e.writers.as_str(), e.service_id), ("Amazing Grace", "22025", "John Newton", service));
    assert_eq!(e.service_name, "Sunday Service");
}

#[test]
fn with_a_room_only_a_committed_frame_counts() {
    let (app, item, _) = app_with_song("22025");
    app.room_conns.store(1, std::sync::atomic::Ordering::Relaxed);
    send(&app, Command::Show { item_id: item, slide: 0 });
    assert!(logged(&app).is_empty(), "asked for, not yet on the room screen");
    commit(&app);
    assert_eq!(logged(&app).len(), 1);
}

#[test]
fn does_not_log_when_words_hidden_everywhere() {
    let (app, item, _) = app_with_song("22025");
    app.room_conns.store(1, std::sync::atomic::Ordering::Relaxed);
    send(&app, Command::Show { item_id: item, slide: 0 });
    send(&app, Command::ClearSlide);
    commit(&app);
    assert!(logged(&app).is_empty(), "the words were cleared before the room showed them");
}

#[test]
fn does_not_log_non_song_items() {
    let (app, _, _) = app_with_song("22025");
    let text = ServiceItem { id: Uuid::new_v4(), kind: serde_json::from_value(serde_json::json!({ "kind": "text", "title": "Welcome", "body": "Hello" })).unwrap(),
        background: None, notes: String::new(), theme: None };
    let id = text.id;
    app.data.lock().unwrap().service_mut().unwrap().items.push(text);
    send(&app, Command::Show { item_id: id, slide: 0 });
    assert!(logged(&app).is_empty());
}

#[test]
fn does_not_log_when_turned_off() {
    let (app, item, _) = app_with_song("22025");
    app.usage.edit(|l| l.enabled = false).unwrap();
    send(&app, Command::Show { item_id: item, slide: 0 });
    assert!(logged(&app).is_empty());
}

#[test]
fn streamed_is_false_without_a_livestream_output() {
    let (app, item, _) = app_with_song("22025");
    send(&app, Command::Show { item_id: item, slide: 0 });
    assert!(!logged(&app)[0].streamed);
}

#[test]
fn usage_write_happens_off_the_command_path() {
    let (app, item, _) = app_with_song("22025");
    // Holding the log blocks the writer thread, not the command.
    let held = app.usage.log();
    send(&app, Command::Show { item_id: item, slide: 0 });
    assert!(held.entries.is_empty());
    drop(held);
    assert_eq!(logged(&app).len(), 1);
    let saved: UsageLog = serde_json::from_slice(&std::fs::read(app.store.root.join("usage.json")).unwrap()).unwrap();
    assert_eq!(saved.entries.len(), 1, "written to usage.json");
}

// ---- the report ----

#[test]
fn report_groups_by_ccli_and_excludes_no_ccli_by_default() {
    let mut old = named("Amazing Grace (old title)", "22025", day(2026, 9, 1));
    old.at -= chrono::Duration::days(1);
    let mut streamed = named("Amazing Grace", "22025", day(2026, 10, 4));
    streamed.streamed = true;
    let log = vec![
        old, streamed, named("Be Thou My Vision", "30639", day(2026, 10, 4)),
        named("Our own song", "", day(2026, 10, 4)), named("Too late", "1", day(2026, 12, 1)),
    ];
    let rows = report(&log, day(2026, 9, 1), day(2026, 10, 31), false);
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].title.as_str(), rows[0].ccli.as_str(), rows[0].shown, rows[0].streamed, rows[0].last_used),
        ("Amazing Grace", "22025", 2, 1, day(2026, 10, 4)), "grouped by number, named by the newest entry");
    assert_eq!(rows[1].title, "Be Thou My Vision");
    let all = report(&log, day(2026, 9, 1), day(2026, 10, 31), true);
    assert_eq!(all.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["Amazing Grace", "Be Thou My Vision", "Our own song"]);
}

#[test]
fn csv_quotes_commas_and_quotes() {
    let rows = vec![ReportRow { title: "Holy, Holy, Holy".into(), ccli: "1156".into(), writers: "Reginald \"Reg\" Heber".into(),
        shown: 3, streamed: 1, last_used: day(2026, 10, 4) },
        ReportRow { title: "=cmd()".into(), ccli: "2".into(), writers: String::new(), shown: 1, streamed: 0, last_used: day(2026, 10, 5) }];
    let csv = export::to_csv(&rows, "998877");
    assert_eq!(csv, "\u{feff}CCLI License #,998877\r\n\r\nTitle,CCLI Song #,Writers,Times shown,Times streamed,Last used\r\n\
\"Holy, Holy, Holy\",1156,\"Reginald \"\"Reg\"\" Heber\",3,1,2026-10-04\r\n'=cmd(),2,,1,0,2026-10-05\r\n");
    assert!(!export::to_csv(&rows, "").contains("License"), "no licence line without a number");
}

#[test]
fn html_page_escapes_and_names_the_range() {
    let rows = vec![ReportRow { title: "<b>Grace</b> & Peace".into(), ccli: "1".into(), writers: String::new(), shown: 1, streamed: 0, last_used: day(2026, 10, 4) }];
    let html = export::to_html(&rows, "998877", (day(2026, 8, 1), day(2026, 10, 31)));
    assert!(html.contains("&lt;b&gt;Grace&lt;/b&gt; &amp; Peace"));
    assert!(html.contains("2026-08-01 to 2026-10-31"));
    assert!(html.contains("CCLI license #: 998877"));
    assert!(export::to_html(&[], "", (day(2026, 8, 1), day(2026, 10, 31))).contains("Add your church&#39;s CCLI license number"), "no licence: the note, escaped");
    assert_eq!(export::file_name(day(2026, 8, 1), day(2026, 10, 31), "csv"), "song-use-2026-08-01-to-2026-10-31.csv");
}

#[test]
fn mark_reported_moves_default_from_date() {
    let today = day(2026, 11, 6);
    assert_eq!(default_from(None, today), day(2026, 8, 6), "three months ago");
    assert_eq!(default_from(Some(day(2026, 10, 31)), today), day(2026, 11, 1), "the day after the last report");
    assert_eq!(Quick::SinceLastReport.range(Some(day(2026, 10, 31)), today), (day(2026, 11, 1), today));
    assert_eq!(Quick::Last6Months.range(None, today), (day(2026, 5, 6), today));
    assert_eq!(Quick::ThisYear.range(None, today), (day(2026, 1, 1), today));
}

#[test]
fn prune_keeps_three_years() {
    let today = day(2026, 11, 6);
    let mut log = UsageLog { entries: vec![named("Old", "1", day(2023, 11, 5)), named("Kept", "2", day(2023, 11, 6))], ..UsageLog::default() };
    assert!(after_load(&mut log, today));
    assert_eq!(log.entries.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Kept"]);
    assert!(!after_load(&mut log, today), "nothing more to drop");
}

#[test]
fn usage_dates_serialise_as_naive_date_and_rfc3339() {
    let mut log = UsageLog { reported_through: Some(day(2026, 10, 31)), ..UsageLog::default() };
    log.entries.push(entry(Uuid::nil(), Uuid::nil(), at(9, 40)));
    let v = serde_json::to_value(&log).unwrap();
    assert_eq!(v["reported_through"], "2026-10-31");
    assert_eq!(v["entries"][0]["at"], "2026-11-01T09:40:00Z");
    assert_eq!(v["entries"][0]["local_date"], "2026-11-01");
    assert_eq!(serde_json::from_value::<UsageLog>(v).unwrap(), log);
    let empty: UsageLog = serde_json::from_str("{}").unwrap();
    assert!(empty.enabled, "on by default");
}

// ---- actions ----

async fn act(app: &Arc<App>, action: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
    app.do_action(action, body.to_string().as_bytes()).await.map_err(|e| e.message)
}

#[tokio::test]
async fn report_action_checks_dates_and_lists_entries() {
    let (app, item, _) = app_with_song("22025");
    app.data.lock().unwrap().settings.church_ccli = "998877".into();
    send(&app, Command::Show { item_id: item, slide: 0 });
    app.usage.flush();
    let v = act(&app, "usage/report", serde_json::json!({})).await.unwrap();
    assert_eq!(v["to"], export::day(today()));
    assert_eq!(v["from"], export::day(default_from(None, today())));
    assert_eq!(v["license"], "998877");
    assert_eq!(v["rows"][0]["shown"], 1);
    assert_eq!(v["entries"][0]["song"], "Amazing Grace");
    let bad = act(&app, "usage/report", serde_json::json!({ "from": "2026-11-02", "to": "2026-11-01" })).await;
    assert_eq!(bad.unwrap_err(), strings::FROM_AFTER_TO);
    let bad = act(&app, "usage/report", serde_json::json!({ "from": "11/02/2026" })).await;
    assert_eq!(bad.unwrap_err(), strings::BAD_DATE);
}

#[tokio::test]
async fn remove_mark_and_export_actions() {
    let (app, item, _) = app_with_song("22025");
    send(&app, Command::Show { item_id: item, slide: 0 });
    let id = logged(&app)[0].id;
    let path = app.store.root.join("out").join("r.csv");
    let v = act(&app, "usage/export", serde_json::json!({ "format": "csv", "path": path })).await.unwrap();
    assert_eq!(v["path"], serde_json::json!(path));
    assert!(std::fs::read_to_string(&path).unwrap().contains("Amazing Grace,22025"));
    let v = act(&app, "usage/export", serde_json::json!({ "format": "html", "from": "2026-01-01", "to": "2026-01-31" })).await.unwrap();
    assert!(v["path"].as_str().unwrap().ends_with("song-use-2026-01-01-to-2026-01-31.html"));
    assert!(app.store.root.join("reports").join("song-use-2026-01-01-to-2026-01-31.html").exists());
    act(&app, "usage/mark-reported", serde_json::json!({ "through": "2026-10-31" })).await.unwrap();
    assert_eq!(app.usage.log().reported_through, Some(day(2026, 10, 31)));
    act(&app, "usage/remove", serde_json::json!({ "id": id })).await.unwrap();
    assert!(app.usage.log().entries.is_empty());
    act(&app, "usage/set", serde_json::json!({ "enabled": false })).await.unwrap();
    assert!(!app.usage.enabled());
}
