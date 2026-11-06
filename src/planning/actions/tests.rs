use crate::{app::test_app, model::*, planning::strings as tr};
use serde_json::{Value, json};
use uuid::Uuid;

async fn act(app: &std::sync::Arc<crate::app::App>, action: &str, body: Value) -> Result<Value, String> {
    app.do_action(action, body.to_string().as_bytes()).await.map_err(|e| e.message)
}

fn item(kind: ItemKind) -> ServiceItem { ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new() } }

#[tokio::test]
async fn copy_week_action_selects_new_service_and_reports_blanks() {
    let app = test_app();
    let src = {
        let mut d = app.data.lock().unwrap();
        d.settings.date_order = Some(DateOrder::Mdy);
        let s = &mut d.services[0];
        s.name = "Service 1/4/2026".into();
        s.date = chrono::NaiveDate::from_ymd_opt(2026, 1, 4);
        s.items = vec![item(ItemKind::Song { song_id: Uuid::new_v4() }), item(ItemKind::Text { title: "Hi".into(), body: "x".into() })];
        s.id
    };
    let r = act(&app, "service/copy-week", json!({ "date": "2026-11-08", "blanks": true })).await.unwrap();
    assert_eq!((r["name"].as_str(), r["date"].as_str(), r["blanks"].as_u64()), (Some("Service 11/8/2026"), Some("2026-11-08"), Some(1)));
    let d = app.data.lock().unwrap();
    let new = d.service().unwrap();
    assert_ne!(new.id, src, "the new service is open");
    assert_eq!(new.name, "Service 11/8/2026");
    assert!(matches!(new.items[0].kind, ItemKind::Blank { .. }));
}

#[tokio::test]
async fn copy_week_without_a_date_goes_a_week_on_or_to_the_next_same_weekday() {
    let app = test_app();
    app.data.lock().unwrap().services[0].date = chrono::NaiveDate::from_ymd_opt(2020, 1, 5); // a Sunday
    let r = act(&app, "service/copy-week", json!({})).await.unwrap();
    let date = chrono::NaiveDate::parse_from_str(r["date"].as_str().unwrap(), "%Y-%m-%d").unwrap();
    use chrono::Datelike;
    assert_eq!(date.weekday(), chrono::Weekday::Sun);
    assert!(date >= chrono::Local::now().date_naive());
}

#[tokio::test]
async fn set_date_sets_and_clears() {
    let app = test_app();
    let id = app.data.lock().unwrap().services[0].id;
    act(&app, "service/set-date", json!({ "id": id, "date": "2026-11-08" })).await.unwrap();
    assert_eq!(app.data.lock().unwrap().services[0].date, chrono::NaiveDate::from_ymd_opt(2026, 11, 8));
    assert_eq!(act(&app, "service/set-date", json!({ "id": id, "date": "2026-13-01" })).await.unwrap_err(), tr::BAD_DATE);
    act(&app, "service/set-date", json!({ "id": id, "date": null })).await.unwrap();
    assert_eq!(app.data.lock().unwrap().services[0].date, None);
    assert_eq!(act(&app, "service/set-date", json!({ "id": Uuid::new_v4(), "date": null })).await.unwrap_err(), tr::SERVICE_GONE);
}

#[tokio::test]
async fn fill_replaces_blank_in_place_and_refuses_non_blank() {
    let app = test_app();
    let mut blank = item(ItemKind::Blank { label: "Song".into(), want: BlankKind::Song });
    blank.notes = "upbeat".into();
    let (bid, other) = (blank.id, item(ItemKind::Text { title: "Hi".into(), body: "x".into() }));
    let oid = other.id;
    app.data.lock().unwrap().services[0].items = vec![other, blank];
    let song = Uuid::new_v4();
    act(&app, "service/fill", json!({ "id": bid, "item": { "kind": "song", "song_id": song } })).await.unwrap();
    {
        let d = app.data.lock().unwrap();
        let it = &d.services[0].items[1];
        assert_eq!(it.id, bid);
        assert!(matches!(it.kind, ItemKind::Song { song_id } if song_id == song));
        assert_eq!(it.notes, "upbeat");
    }
    let again = act(&app, "service/fill", json!({ "id": bid, "item": { "kind": "song", "song_id": song } })).await.unwrap_err();
    assert_eq!(again, tr::NOT_A_BLANK, "two quick fills cannot overwrite a real item");
    assert_eq!(act(&app, "service/fill", json!({ "id": oid, "item": { "kind": "song", "song_id": song } })).await.unwrap_err(), tr::NOT_A_BLANK);
    assert_eq!(act(&app, "service/fill", json!({ "id": Uuid::new_v4(), "item": { "kind": "song", "song_id": song } })).await.unwrap_err(), tr::BLANK_GONE);
}

#[tokio::test]
async fn templates_save_make_rename_and_delete() {
    let app = test_app();
    let sid = {
        let mut d = app.data.lock().unwrap();
        let s = &mut d.services[0];
        s.items = vec![item(ItemKind::Text { title: "Welcome".into(), body: "x".into() }), item(ItemKind::Song { song_id: Uuid::new_v4() })];
        s.date = chrono::NaiveDate::from_ymd_opt(2026, 11, 4); // a Wednesday
        s.id
    };
    assert_eq!(act(&app, "template/from-service", json!({ "service": sid, "name": " " })).await.unwrap_err(), tr::TEMPLATE_NEEDS_NAME);
    let t = act(&app, "template/from-service", json!({ "service": sid, "name": "Midweek" })).await.unwrap();
    let tid: Uuid = serde_json::from_value(t["id"].clone()).unwrap();
    assert_eq!(app.data.lock().unwrap().templates[0].day, Some(chrono::Weekday::Wed));

    let r = act(&app, "service/from-template", json!({ "template": tid, "date": "2026-11-11" })).await.unwrap();
    assert_eq!(r["blanks"], 1);
    {
        let d = app.data.lock().unwrap();
        let s = d.service().unwrap();
        assert_eq!((s.template, s.date), (Some(tid), chrono::NaiveDate::from_ymd_opt(2026, 11, 11)));
        assert!(s.name.starts_with("Midweek "), "{}", s.name);
    }
    act(&app, "template/rename", json!({ "id": tid, "name": "Wednesday" })).await.unwrap();
    act(&app, "template/day", json!({ "id": tid, "day": null })).await.unwrap();
    {
        let d = app.data.lock().unwrap();
        assert_eq!((d.templates[0].name.as_str(), d.templates[0].day), ("Wednesday", None));
    }
    act(&app, "template/delete", json!({ "id": tid })).await.unwrap();
    assert!(app.data.lock().unwrap().templates.is_empty());
    assert_eq!(act(&app, "service/from-template", json!({ "template": tid })).await.unwrap_err(), tr::TEMPLATE_GONE);
    assert_eq!(act(&app, "template/save", json!({ "id": Uuid::new_v4(), "name": "X", "items": [] })).await.unwrap_err(), tr::TEMPLATE_NEEDS_PARTS);
}

#[tokio::test]
async fn library_deletes_are_blocked_by_templates() {
    let app = test_app();
    let (song, media, deck) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    {
        let mut d = app.data.lock().unwrap();
        d.songs.push(Song { id: song, title: "S".into(), lyrics: "x".into(), ..Default::default() });
        d.media.push(MediaFile { id: media, name: "M".into(), kind: MediaKind::Image, asset: "a.png".into(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() });
        d.decks.push(Deck { id: deck, title: "D".into(), slides: vec![] });
        d.templates.push(ServiceTemplate { id: Uuid::new_v4(), name: "Sunday".into(), day: None, items: vec![
            item(ItemKind::Song { song_id: song }), item(ItemKind::Media { media_id: media, autoplay: true, end: EndAction::Stop, looped: false }), item(ItemKind::Deck { deck_id: deck }),
        ] });
    }
    let want = tr::in_template("Sunday");
    assert_eq!(act(&app, "song/delete", json!({ "id": song })).await.unwrap_err(), want);
    assert_eq!(act(&app, "media/delete", json!({ "id": media })).await.unwrap_err(), want);
    assert_eq!(act(&app, "deck/delete", json!({ "id": deck })).await.unwrap_err(), want);
}

#[tokio::test]
async fn new_service_is_dated_today() {
    let app = test_app();
    act(&app, "service/new", json!({ "name": "Easter" })).await.unwrap();
    assert_eq!(app.data.lock().unwrap().service().unwrap().date, Some(chrono::Local::now().date_naive()));
}

#[tokio::test]
async fn show_blank_says_choose_first_and_next_steps_over_it() {
    let app = test_app();
    let blank = item(ItemKind::Blank { label: "Song".into(), want: BlankKind::Song });
    let (a, b) = (item(ItemKind::Text { title: "A".into(), body: "a".into() }), item(ItemKind::Text { title: "B".into(), body: "b".into() }));
    let (bid, aid, cid) = (blank.id, a.id, b.id);
    app.data.lock().unwrap().services[0].items = vec![a, blank, b];
    let cmd = |c: crate::live::Command, base: Option<u64>| crate::live::Envelope { id: Uuid::new_v4().to_string(), base, cmd: c };
    let out = app.command(cmd(crate::live::Command::Show { item_id: bid, slide: 0 }, Some(0)));
    assert_eq!(out.error.as_deref(), Some(tr::BLANK_CHOOSE_FIRST));
    let v = app.live_view().version;
    assert!(app.command(cmd(crate::live::Command::Show { item_id: aid, slide: 0 }, Some(v))).ok);
    let v = app.live_view().version;
    assert!(app.command(cmd(crate::live::Command::Next, Some(v))).ok);
    assert_eq!(app.live_view().item.map(|i| i.item_id), Some(cid), "Next steps over the blank");
}

#[tokio::test]
async fn print_write_saves_options_and_writes_the_page() {
    let app = test_app();
    let id = app.data.lock().unwrap().services[0].id;
    let dest = app.store.root.join("order.html");
    let r = act(&app, "print/write", json!({ "service": id, "what": "order", "notes": false, "scripture": true, "song_per_page": true, "paper": "a4", "path": dest })).await.unwrap();
    assert_eq!(r["path"].as_str().map(std::path::PathBuf::from), Some(dest.clone()));
    assert!(std::fs::read_to_string(&dest).unwrap().contains("size: A4"));
    let p = app.data.lock().unwrap().settings.print.clone();
    assert_eq!(p, PrintSettings { what: PrintWhat::Order, notes: false, scripture: true, song_per_page: true, paper: Some(Paper::A4) });
    let r = act(&app, "print/write", json!({ "service": id, "what": "both", "notes": true, "scripture": true, "song_per_page": false, "paper": null })).await.unwrap();
    let opened = std::path::PathBuf::from(r["path"].as_str().unwrap());
    assert!(opened.starts_with(crate::report_page::reports_dir(&app.store.root)));
    assert!(crate::report_page::testing::OPENED.lock().unwrap().contains(&opened));
}
