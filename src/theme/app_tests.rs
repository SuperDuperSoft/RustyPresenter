//! FEAT-1 tests of `App` (actions, read-only mode, live overlays and themes), kept apart from `app.rs`.
use std::sync::Arc;

use uuid::Uuid;

use crate::app::{App, test_app};
use crate::live::{Command, Envelope};
use crate::model::{ItemKind, ServiceItem};
use crate::theme::strings;

pub(super) fn text_item(body: &str) -> ServiceItem {
    ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "t".into(), body: body.into(), layout: Default::default() }, background: None, notes: String::new(), theme: None }
}

pub(super) fn cmd(app: &App, cmd: Command) -> crate::live::Outcome {
    let base = Some(app.live().version);
    app.command(Envelope { id: Uuid::new_v4().to_string(), base, cmd })
}

pub(super) async fn act(app: &Arc<App>, action: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
    app.do_action(action, body.to_string().as_bytes()).await.map_err(|e| e.message)
}

#[tokio::test]
async fn read_only_rejects_edits() {
    let app = test_app();
    app.set_read_only(true);
    let err = act(&app, "song/save", serde_json::json!({ "id": Uuid::nil(), "title": "A", "lyrics": "" })).await.unwrap_err();
    assert_eq!(err, strings::newer_data());
    assert!(app.data().songs.is_empty(), "nothing changed in memory either");
    assert_eq!(act(&app, "backup/restore", serde_json::json!({ "name": "backup-x" })).await.unwrap_err(), strings::newer_data());
    assert!(!app.store.data_path().exists(), "nothing written");
    assert_eq!(app.state_json()["read_only"], true);
}

#[tokio::test]
async fn read_only_keeps_live_control() {
    let app = test_app();
    let item = text_item("one\n\ntwo");
    let id = item.id;
    app.data().services[0].items.push(item);
    app.set_read_only(true);
    assert!(cmd(&app, Command::Show { item_id: id, slide: 0 }).ok);
    assert!(cmd(&app, Command::Next).ok);
    assert!(cmd(&app, Command::ClearAll).ok);
    assert!(!app.store.data_path().exists());
}

// ---- themes (spec §5.1) ----

use crate::model::{Background, Theme};
use crate::theme::{STANDARD_ID, SavedTheme};
use serde_json::json;

fn theme_json(name: &str, size: u32) -> serde_json::Value {
    json!({ "id": Uuid::nil(), "name": name, "style": Theme { size, ..Theme::default() } })
}

async fn new_theme(app: &Arc<App>, name: &str, size: u32) -> Uuid {
    let v = act(app, "theme/save", theme_json(name, size)).await.unwrap();
    serde_json::from_value(v["id"].clone()).unwrap()
}

fn live_size(app: &App) -> u32 {
    match &app.live().frame.slide {
        Some(crate::live::SlideLayer::Text { theme: Some(t), .. }) => t.size,
        s => panic!("{s:?}"),
    }
}

#[tokio::test]
async fn theme_save_requires_unique_name() {
    let app = test_app();
    new_theme(&app, "Christmas", 90).await;
    assert_eq!(act(&app, "theme/save", theme_json(" christmas ", 80)).await.unwrap_err(), strings::theme_exists("christmas"));
    assert_eq!(act(&app, "theme/save", theme_json("  ", 80)).await.unwrap_err(), strings::theme_name_required());
    assert_eq!(app.data().themes.len(), 1);
}

#[tokio::test]
async fn theme_name_church_default_reserved() {
    let app = test_app();
    for name in ["Church default", "standard"] {
        assert_eq!(act(&app, "theme/save", theme_json(name, 80)).await.unwrap_err(), strings::theme_name_reserved());
    }
}

#[tokio::test]
async fn theme_save_rejects_standard_id() {
    let app = test_app();
    let body = json!({ "id": STANDARD_ID, "name": "Mine", "style": Theme::default() });
    assert_eq!(act(&app, "theme/save", body).await.unwrap_err(), strings::standard_save_elsewhere());
}

#[tokio::test]
async fn theme_save_returns_empty_screen_views() {
    let app = test_app();
    let v = act(&app, "theme/save", theme_json("Youth", 500)).await.unwrap();
    assert_eq!(v["screen_views"], json!([]));
    assert_eq!(app.data().themes[0].style.size, 200, "clamped like Settings");
    assert!(app.store.load().unwrap().themes.len() == 1, "saved");
}

#[tokio::test]
async fn theme_delete_clears_references_and_counts() {
    let app = test_app();
    let x = new_theme(&app, "X", 90).await;
    {
        let mut d = app.data();
        d.services[0].theme = Some(x);
        d.services[0].items = vec![text_item("a"), text_item("b")];
        d.services[0].items[0].theme = Some(x);
    }
    let v = act(&app, "theme/delete", json!({ "id": x })).await.unwrap();
    assert_eq!(v, json!({ "items": 1, "services": 1, "was_default": false }));
    let d = app.data();
    assert!(d.themes.is_empty() && d.services[0].theme.is_none() && d.services[0].items[0].theme.is_none());
    drop(d);
    assert_eq!(act(&app, "theme/delete", json!({ "id": STANDARD_ID })).await.unwrap_err(), strings::builtin_no_delete());
    assert_eq!(act(&app, "theme/delete", json!({ "id": x })).await.unwrap_err(), strings::theme_gone());
}

#[tokio::test]
async fn delete_starred_theme_returns_to_standard() {
    let app = test_app();
    let x = new_theme(&app, "X", 90).await;
    act(&app, "theme/make-default", json!({ "id": x })).await.unwrap();
    let v = act(&app, "theme/delete", json!({ "id": x })).await.unwrap();
    assert_eq!(v["was_default"], true);
    let d = app.data();
    assert_eq!((d.default_theme, crate::theme::church_default(&d).id), (None, STANDARD_ID));
}

#[tokio::test]
async fn make_default_keeps_previous_default_as_theme() {
    let app = test_app();
    let christmas = new_theme(&app, "Christmas", 120).await;
    act(&app, "theme/make-default", json!({ "id": christmas })).await.unwrap();
    assert_eq!(app.live_view().theme.size, 120);
    act(&app, "theme/make-default", json!({ "id": STANDARD_ID })).await.unwrap();
    let d = app.data();
    assert_eq!(d.default_theme, None, "Standard is stored as no default");
    assert_eq!(d.themes[0].style.size, 120, "Christmas unchanged");
    drop(d);
    assert_eq!(app.live_view().theme, Theme::default(), "the original look is back");
    assert_eq!(act(&app, "theme/make-default", json!({ "id": Uuid::new_v4() })).await.unwrap_err(), strings::theme_gone());
}

#[tokio::test]
async fn editing_starred_theme_changes_default() {
    let app = test_app();
    let x = new_theme(&app, "X", 90).await;
    act(&app, "theme/make-default", json!({ "id": x })).await.unwrap();
    act(&app, "theme/save", json!({ "id": x, "name": "X", "style": Theme { size: 100, ..Theme::default() } })).await.unwrap();
    assert_eq!(app.live_view().theme.size, 100, "the star is a pointer");
    let rows = act(&app, "theme/list", json!({})).await.unwrap();
    assert_eq!(rows["rows"][0]["id"], json!(STANDARD_ID));
    let mine = rows["rows"].as_array().unwrap().iter().find(|r| r["id"] == json!(x)).unwrap();
    assert_eq!(mine["is_default"], true);
    assert_eq!(rows["rows"][1]["name"], strings::lower_third_name(), "built-ins next");
}

#[tokio::test]
async fn theme_save_standard_writes_only_theme_and_background() {
    let app = test_app();
    app.data().settings.lines_per_slide = 5;
    let bg = Background::Gradient { from: "#0b1a33".into(), to: "#000000".into(), angle: 180, dim: 90 };
    act(&app, "theme/save-standard", json!({ "style": Theme { size: 10, ..Theme::default() }, "background": bg })).await.unwrap();
    let d = app.data();
    assert_eq!(d.settings.theme.size, 24);
    assert_eq!(d.settings.background.dim(), 80, "Darken at most 80 %");
    assert_eq!(d.settings.lines_per_slide, 5);
    assert!(d.themes.is_empty());
}

#[tokio::test]
async fn service_theme_rejects_unknown() {
    let app = test_app();
    let sid = app.data().services[0].id;
    assert_eq!(act(&app, "service/theme", json!({ "service_id": sid, "theme": Uuid::new_v4() })).await.unwrap_err(), strings::theme_gone());
    assert_eq!(act(&app, "service/theme", json!({ "service_id": Uuid::new_v4(), "theme": null })).await.unwrap_err(), strings::service_gone());
}

#[tokio::test]
async fn service_theme_reports_items_with_own_background() {
    let app = test_app();
    let plain = new_theme(&app, "Plain", 90).await;
    let with_bg = SavedTheme { id: Uuid::nil(), name: "Night".into(), style: Theme::default(), background: Some(Background::Color { color: "#0b1a33".into() }) };
    let v = act(&app, "theme/save", serde_json::to_value(&with_bg).unwrap()).await.unwrap();
    let night: Uuid = serde_json::from_value(v["id"].clone()).unwrap();
    let sid = {
        let mut d = app.data();
        let mut own = text_item("a");
        own.background = Some(Background::Color { color: "#ff0000".into() });
        d.services[0].items = vec![own, text_item("b")];
        d.services[0].id
    };
    assert_eq!(act(&app, "service/theme", json!({ "service_id": sid, "theme": plain })).await.unwrap()["own_background"], 0);
    assert_eq!(act(&app, "service/theme", json!({ "service_id": sid, "theme": night })).await.unwrap()["own_background"], 1);
    assert_eq!(app.data().services[0].theme, Some(night));
}

#[tokio::test]
async fn clear_item_backgrounds_only_that_service() {
    let app = test_app();
    let red = Some(Background::Color { color: "#ff0000".into() });
    let (sid, other) = {
        let mut d = app.data();
        let mut a = text_item("a");
        a.background = red.clone();
        d.services[0].items = vec![a.clone()];
        let mut s = crate::model::Service::new("Evening");
        s.items = vec![a];
        let other = s.id;
        d.services.push(s);
        (d.services[0].id, other)
    };
    assert_eq!(act(&app, "service/clear-item-backgrounds", json!({ "service_id": sid })).await.unwrap()["cleared"], 1);
    let d = app.data();
    assert_eq!(d.services[0].items[0].background, None);
    assert_eq!(d.services.iter().find(|s| s.id == other).unwrap().items[0].background, red);
}

#[tokio::test]
async fn service_update_without_theme_key_keeps_theme() {
    let app = test_app();
    let x = new_theme(&app, "X", 90).await;
    let mut item = text_item("old");
    item.theme = Some(x);
    app.data().services[0].items = vec![item.clone()];
    let patch = json!({ "id": item.id, "kind": "text", "title": "t", "body": "new", "background": null, "notes": "" });
    act(&app, "service/update", patch).await.unwrap();
    let d = app.data();
    assert_eq!(d.services[0].items[0].theme, Some(x));
    assert!(matches!(&d.services[0].items[0].kind, ItemKind::Text { body, .. } if body == "new"));
}

#[tokio::test]
async fn item_theme_sets_clears_and_refuses_decks() {
    let app = test_app();
    let x = new_theme(&app, "X", 90).await;
    let item = text_item("a");
    let deck = ServiceItem { kind: ItemKind::Deck { deck_id: Uuid::new_v4() }, ..text_item("") };
    app.data().services[0].items = vec![item.clone(), deck.clone()];
    act(&app, "item/theme", json!({ "item_id": item.id, "theme": x })).await.unwrap();
    assert_eq!(app.data().services[0].items[0].theme, Some(x));
    act(&app, "item/theme", json!({ "item_id": item.id, "theme": null })).await.unwrap();
    assert_eq!(app.data().services[0].items[0].theme, None);
    assert_eq!(act(&app, "item/theme", json!({ "item_id": deck.id, "theme": x })).await.unwrap_err(), strings::item_no_theme());
    assert_eq!(act(&app, "item/theme", json!({ "item_id": Uuid::new_v4(), "theme": x })).await.unwrap_err(), strings::item_gone());
}

// ---- theme snapshot on the live item (spec §5.3) ----

#[tokio::test]
async fn live_item_snapshots_resolved_theme() {
    let app = test_app();
    let x = new_theme(&app, "X", 90).await;
    let mut item = text_item("one\n\ntwo");
    item.theme = Some(x);
    let id = item.id;
    app.data().services[0].items = vec![item];
    assert!(cmd(&app, Command::Show { item_id: id, slide: 0 }).ok);
    assert_eq!(app.live().item.as_ref().unwrap().theme.size, 90);
    assert_eq!(live_size(&app), 90);
}

#[tokio::test]
async fn theme_edit_does_not_change_frame_and_next_keeps_snapshot() {
    let app = test_app();
    let x = new_theme(&app, "X", 90).await;
    let mut item = text_item("one\n\ntwo");
    item.theme = Some(x);
    let id = item.id;
    app.data().services[0].items = vec![item];
    cmd(&app, Command::Show { item_id: id, slide: 0 });
    let frame = app.live().frame.clone();
    act(&app, "theme/save", json!({ "id": x, "name": "X", "style": Theme { size: 150, ..Theme::default() } })).await.unwrap();
    assert_eq!(app.live().frame, frame, "the room is not changed");
    cmd(&app, Command::Next);
    assert_eq!(live_size(&app), 90, "next within the item keeps the snapshot");
    cmd(&app, Command::Show { item_id: id, slide: 0 });
    assert_eq!(live_size(&app), 150, "clicking a slide of the live item takes the new theme");
}

#[test]
fn old_live_state_json_loads() {
    let old = r#"{"type":"text","text":"a","footer":null,"reference":null,"scripture":false}"#;
    let s: crate::live::SlideLayer = serde_json::from_str(old).unwrap();
    assert!(matches!(s, crate::live::SlideLayer::Text { theme: None, .. }));
    let item = r##"{"item_id":"6f7b3c1e-1a2b-4c3d-8e9f-001122334466","title":"t","kind":"song","slides":[],"background":{"type":"color","color":"#000"}}"##;
    let item: crate::live::LiveItem = serde_json::from_str(item).unwrap();
    assert_eq!(item.theme, Theme::default());
}

// ---- backgrounds, overlays and the logo (spec §5.1, §5.2) ----

use crate::theme::overlay::{ImageSource, Overlay, OverlaySource, Placement};

fn picture_file(app: &App, name: &str) -> String {
    let p = app.store.asset_path(name);
    image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255])).save_with_format(&p, image::ImageFormat::Png).unwrap();
    name.to_string()
}

fn wifi() -> serde_json::Value {
    json!({ "id": Uuid::nil(), "name": " Wi-Fi ", "source": { "type": "text", "text": "Wi-Fi: GraceGuest" } })
}

async fn new_overlay(app: &Arc<App>, body: serde_json::Value) -> Uuid {
    serde_json::from_value(act(app, "overlay/save", body).await.unwrap()["id"].clone()).unwrap()
}

#[tokio::test]
async fn background_add_rename_remove() {
    let app = test_app();
    assert_eq!(act(&app, "background/add", json!({ "name": " ", "background": { "type": "color", "color": "#000" } })).await.unwrap_err(), strings::background_name_required());
    let bg = Background::Gradient { from: "#0b1a33".into(), to: "#000000".into(), angle: 180, dim: 30 };
    let v = act(&app, "background/add", json!({ "name": "Midnight", "background": bg })).await.unwrap();
    let id = v["id"].clone();
    act(&app, "background/rename", json!({ "id": id, "name": "Night" })).await.unwrap();
    assert_eq!(app.data().backgrounds[0].name, "Night");
    assert_eq!(act(&app, "background/rename", json!({ "id": id, "name": "" })).await.unwrap_err(), strings::background_name_required());
    app.data().settings.background = bg.clone();
    act(&app, "background/remove", json!({ "id": id })).await.unwrap();
    assert_eq!(app.data().settings.background, bg, "what uses it keeps it");
    assert_eq!(act(&app, "background/remove", json!({ "id": id })).await.unwrap_err(), strings::background_gone());
}

#[tokio::test]
async fn overlay_save_validates() {
    let app = test_app();
    let mut nameless = wifi();
    nameless["name"] = json!("");
    assert_eq!(act(&app, "overlay/save", nameless).await.unwrap_err(), strings::overlay_name_required());
    let blank = json!({ "id": Uuid::nil(), "name": "S", "source": { "type": "text", "text": "  " } });
    assert_eq!(act(&app, "overlay/save", blank).await.unwrap_err(), strings::overlay_text_required());
    let gone = json!({ "id": Uuid::nil(), "name": "P", "source": { "type": "image", "source": { "kind": "asset", "asset": "nope.png" } } });
    assert_eq!(act(&app, "overlay/save", gone).await.unwrap_err(), strings::picture_gone());
    let id = new_overlay(&app, wifi()).await;
    let d = app.data();
    assert_eq!((d.overlays[0].id, d.overlays[0].name.as_str()), (id, "Wi-Fi"));
}

#[tokio::test]
async fn overlay_on_from_the_operator_lists_lit_chips_and_f6_clears() {
    let app = test_app();
    let id = new_overlay(&app, wifi()).await;
    let seen = app.live_view().clear_epoch;
    let cmd = crate::live::Command::Overlay { id, on: true, seen_clear: Some(seen) };
    assert!(app.command(Envelope { id: "c1".into(), base: None, cmd }).ok);
    let chips = act(&app, "overlay/list", json!({})).await.unwrap();
    assert_eq!(chips["chips"][0]["on"], true);
    assert_eq!(chips["chips"][0]["kind"], "text");
    let off: Envelope = serde_json::from_value(json!({ "id": "c1b", "cmd": "overlay", "overlay": id, "on": false })).unwrap();
    assert_eq!(off.cmd, Command::Overlay { id, on: false, seen_clear: None }, "the JSON the operator and phone send");
    let f6: Envelope = serde_json::from_value(json!({ "id": "c2", "cmd": "clear_overlays" })).unwrap();
    assert!(app.command(f6).ok);
    assert!(app.live_view().frame.overlays.is_empty());
    assert_eq!(app.live_view().clear_epoch, seen + 1);
    let late = crate::live::Command::Overlay { id, on: true, seen_clear: Some(seen) };
    assert_eq!(app.command(Envelope { id: "c3".into(), base: None, cmd: late }).error.as_deref(), Some(strings::screen_cleared().as_str()));
}

#[tokio::test]
async fn overlay_delete_keeps_slot_on_screen_and_move_reorders() {
    let app = test_app();
    let a = new_overlay(&app, wifi()).await;
    let b = new_overlay(&app, json!({ "id": Uuid::nil(), "name": "Clock", "source": { "type": "clock" } })).await;
    act(&app, "overlay/move", json!({ "id": b, "to": 0 })).await.unwrap();
    assert_eq!(app.data().overlays.iter().map(|o| o.id).collect::<Vec<_>>(), [b, a]);
    let seen = app.live_view().clear_epoch;
    app.command(Envelope { id: "1".into(), base: None, cmd: Command::Overlay { id: a, on: true, seen_clear: Some(seen) } });
    act(&app, "overlay/delete", json!({ "id": a })).await.unwrap();
    assert_eq!(app.live_view().frame.overlays.len(), 1, "still on screen");
    let chips = act(&app, "overlay/list", json!({})).await.unwrap();
    assert_eq!(chips["chips"][1]["deleted"], true);
    assert_eq!(act(&app, "overlay/delete", json!({ "id": a })).await.unwrap_err(), strings::overlay_gone());
}

#[tokio::test]
async fn logo_choose_and_remove_save_immediately() {
    let app = test_app();
    assert_eq!(act(&app, "settings/logo", json!({ "asset": "missing.png" })).await.unwrap_err(), strings::picture_gone());
    let logo = picture_file(&app, "logo.png");
    assert_eq!(act(&app, "settings/logo", json!({ "asset": logo })).await.unwrap()["toast"], strings::logo_saved());
    assert_eq!(app.store.load().unwrap().settings.logo.as_deref(), Some("logo.png"));
    assert_eq!(act(&app, "settings/logo", json!({ "asset": null })).await.unwrap()["toast"], strings::logo_removed());
    assert_eq!(app.store.load().unwrap().settings.logo, None);
}

#[test]
fn picture_overlay_type_round_trips() {
    let o = Overlay { id: Uuid::new_v4(), name: "Bug".into(), source: OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 80 }, place: Placement::default() };
    let back: Overlay = serde_json::from_value(serde_json::to_value(&o).unwrap()).unwrap();
    assert_eq!(back, o);
}

// ---- read-only mode keeps the live state, settings merge by key (spec §4.7, §9.1) ----

#[test]
fn read_only_still_writes_live_state() {
    use crate::recovery::state;
    let app = test_app();
    let item = text_item("one");
    let id = item.id;
    app.data().services[0].items.push(item);
    app.set_read_only(true);
    app.set_state_writer(state::StateWriter::start(app.store.root.clone()));
    let version = cmd(&app, Command::Show { item_id: id, slide: 0 }).version;
    app.room_event(crate::live::RoomEvent::Committed { version });
    app.stop_state_writer();
    assert!(state::load(&app.store.root).is_some(), "live-state.json is written");
    assert!(!app.store.data_path().exists(), "data.json is not");
}

#[tokio::test]
async fn settings_save_keeps_logo_set_meanwhile() {
    let app = test_app();
    let mut form = serde_json::to_value(&app.data().settings).unwrap();
    let logo = picture_file(&app, "logo.png");
    act(&app, "settings/logo", json!({ "asset": logo })).await.unwrap();
    form.as_object_mut().unwrap().remove("logo");
    form["lines_per_slide"] = json!(4);
    act(&app, "settings", form).await.unwrap();
    let d = app.data();
    assert_eq!(d.settings.logo.as_deref(), Some("logo.png"), "a form without the key keeps it");
    assert_eq!(d.settings.lines_per_slide, 4);
}

#[tokio::test]
async fn settings_save_keeps_theme_background_changed_meanwhile() {
    let app = test_app();
    let bg = Background::Gradient { from: "#0b1a33".into(), to: "#000000".into(), angle: 180, dim: 0 };
    act(&app, "theme/save-standard", json!({ "style": Theme { size: 99, ..Theme::default() }, "background": bg })).await.unwrap();
    act(&app, "settings", json!({ "lines_per_slide": 3 })).await.unwrap();
    let d = app.data();
    assert_eq!((d.settings.theme.size, &d.settings.background, d.settings.lines_per_slide), (99, &bg, 3));
}

#[tokio::test]
async fn settings_save_null_clears_optional_field_and_rejects_non_objects() {
    let app = test_app();
    let logo = picture_file(&app, "logo.png");
    act(&app, "settings/logo", json!({ "asset": logo })).await.unwrap();
    act(&app, "settings", json!({ "logo": null })).await.unwrap();
    assert_eq!(app.data().settings.logo, None);
    assert_eq!(act(&app, "settings", json!([1])).await.unwrap_err(), strings::expected_object());
}
