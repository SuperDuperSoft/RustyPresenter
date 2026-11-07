use super::*;
use crate::{
    app::{App, test_app},
    live::{Command, Envelope},
    model::{ItemKind, ServiceItem},
};
use serde_json::json;
use std::sync::Arc;

fn song(title: &str) -> Song { Song { id: Uuid::new_v4(), title: title.into(), lyrics: "Verse 1\nla".into(), ..Default::default() } }

fn text_item(title: &str, body: &str) -> ServiceItem {
    ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: body.into() }, background: None, notes: String::new(), theme: None }
}

/// Runs `f` on `d` and keeps it in `h`, as `App::edit` does.
fn edit(h: &mut History, d: &mut Data, f: impl FnOnce(&mut Data)) {
    let before = d.clone();
    f(d);
    h.record(&before, d);
}

#[test]
fn undo_and_redo_a_song_change() {
    let (mut h, mut d) = (History::default(), Data::default());
    let s = song("Grace");
    edit(&mut h, &mut d, |d| d.songs.push(s.clone()));
    edit(&mut h, &mut d, |d| d.songs[0].title = "Amazing Grace".into());
    assert_eq!(h.undo(&mut d).unwrap(), "Undone: changes to the song “Amazing Grace”");
    assert_eq!(d.songs[0].title, "Grace");
    assert_eq!(h.undo(&mut d).unwrap(), "Undone: the new song “Grace”");
    assert!(d.songs.is_empty());
    assert_eq!(h.undo(&mut d), Err("Nothing to undo.".into()));
    assert_eq!(h.redo(&mut d).unwrap(), "Redone: the new song “Grace”");
    assert_eq!(h.redo(&mut d).unwrap(), "Redone: changes to the song “Amazing Grace”");
    assert_eq!(d.songs[0].title, "Amazing Grace");
    assert_eq!(h.redo(&mut d), Err("Nothing to redo.".into()));
}

#[test]
fn settings_and_opening_another_service_are_not_steps() {
    let (mut h, mut d) = (History::default(), Data::default());
    edit(&mut h, &mut d, |d| d.settings.lines_per_slide = 2);
    let other = Service::new("Evening");
    d.services.push(other.clone());
    h = History::default();
    edit(&mut h, &mut d, |d| d.current_service = Some(other.id));
    assert!(!h.can_undo());
}

#[test]
fn undo_never_touches_settings() {
    let (mut h, mut d) = (History::default(), Data::default());
    edit(&mut h, &mut d, |d| { d.songs.push(song("A")); d.settings.lines_per_slide = 3; });
    d.settings.lines_per_slide = 5;
    h.undo(&mut d).unwrap();
    assert!(d.songs.is_empty());
    assert_eq!(d.settings.lines_per_slide, 5);
}

#[test]
fn a_new_edit_clears_redo() {
    let (mut h, mut d) = (History::default(), Data::default());
    edit(&mut h, &mut d, |d| d.songs.push(song("A")));
    h.undo(&mut d).unwrap();
    assert!(h.can_redo());
    edit(&mut h, &mut d, |d| d.songs.push(song("B")));
    assert!(!h.can_redo());
}

#[test]
fn only_the_last_steps_are_kept() {
    let (mut h, mut d) = (History::default(), Data::default());
    for i in 0..LIMIT + 5 { edit(&mut h, &mut d, |d| d.songs.push(song(&i.to_string()))) }
    let mut n = 0;
    while h.undo(&mut d).is_ok() { n += 1 }
    assert_eq!(n, LIMIT);
    assert_eq!(d.songs.len(), 5, "the five oldest stay");
}

#[test]
fn a_later_change_to_another_entry_does_not_block_undo() {
    let (mut h, mut d) = (History::default(), Data::default());
    let (a, b) = (song("A"), song("B"));
    d.songs = vec![a.clone(), b.clone()];
    edit(&mut h, &mut d, |d| d.songs[0].lyrics = "new".into());
    // Not through History: as if made by a part of the app Undo does not know.
    d.songs[1].title = "B2".into();
    h.undo(&mut d).unwrap();
    assert_eq!(d.songs[0].lyrics, a.lyrics);
    assert_eq!(d.songs[1].title, "B2", "kept");
}

#[test]
fn a_step_changed_again_since_is_refused_and_dropped() {
    let (mut h, mut d) = (History::default(), Data::default());
    d.songs = vec![song("A")];
    edit(&mut h, &mut d, |d| d.songs.push(song("B")));
    edit(&mut h, &mut d, |d| d.songs[0].title = "A2".into());
    d.songs[0].title = "A3".into();
    assert_eq!(h.undo(&mut d), Err("Can't undo changes to the song “A2”: it was changed again since.".into()));
    assert_eq!(d.songs[0].title, "A3", "nothing changed");
    assert_eq!(h.undo(&mut d).unwrap(), "Undone: the new song “B”", "the step under it still works");
    assert_eq!(d.songs.len(), 1);
}

#[test]
fn changes_the_app_makes_by_itself_do_not_block_undo() {
    // An import, then what the app found out about the file: Undo still takes the import back.
    let (mut h, mut d) = (History::default(), Data::default());
    edit(&mut h, &mut d, |d| d.songs.push(song("A")));
    let before = d.clone();
    d.songs[0].ccli = "123".into();
    h.carry(&before, &d);
    assert_eq!(h.undo(&mut d).unwrap(), "Undone: the new song “A”");
    assert!(d.songs.is_empty());
    assert_eq!(h.redo(&mut d).unwrap(), "Redone: the new song “A”");
    assert_eq!(d.songs[0].ccli, "123", "redo brings back what was found too");
}

#[test]
fn a_change_the_app_made_to_an_edited_entry_goes_with_its_undo() {
    // Undo goes back to the entry as the edit found it: what was found after the edit goes too.
    let (mut h, mut d) = (History::default(), Data::default());
    d.songs.push(song("A"));
    edit(&mut h, &mut d, |d| d.songs[0].title = "A2".into());
    let before = d.clone();
    d.songs[0].ccli = "123".into();
    h.carry(&before, &d);
    h.undo(&mut d).unwrap();
    assert_eq!((d.songs[0].title.as_str(), d.songs[0].ccli.as_str()), ("A", ""));
    h.redo(&mut d).unwrap();
    assert_eq!((d.songs[0].title.as_str(), d.songs[0].ccli.as_str()), ("A2", "123"));
}

#[test]
fn service_steps_say_what_happened() {
    let (mut h, mut d) = (History::default(), Data::default());
    let (a, b) = (text_item("Welcome", "hi"), text_item("Offering", "give"));
    edit(&mut h, &mut d, |d| d.services[0].items.push(a.clone()));
    edit(&mut h, &mut d, |d| d.services[0].items.push(b.clone()));
    edit(&mut h, &mut d, |d| d.services[0].items.swap(0, 1));
    edit(&mut h, &mut d, |d| d.services[0].items.retain(|i| i.id != a.id));
    edit(&mut h, &mut d, |d| d.services[0].name = "Easter".into());
    assert_eq!(h.undo(&mut d).unwrap(), "Undone: changes to “Easter”");
    assert_eq!(d.services[0].name, "Sunday Service");
    assert_eq!(h.undo(&mut d).unwrap(), "Undone: removing “Welcome”");
    assert_eq!(h.undo(&mut d).unwrap(), "Undone: moving “Offering”");
    assert_eq!(h.undo(&mut d).unwrap(), "Undone: adding “Offering”");
    assert_eq!(d.services[0].items, vec![a]);
}

#[test]
fn undoing_a_new_service_goes_back_to_the_one_open_before() {
    let (mut h, mut d) = (History::default(), Data::default());
    let first = d.current_service;
    let new = Service::new("Evening");
    edit(&mut h, &mut d, |d| { d.services.push(new.clone()); d.current_service = Some(new.id); });
    assert_eq!(h.undo(&mut d).unwrap(), "Undone: the new service “Evening”");
    assert_eq!((d.services.len(), d.current_service), (1, first));
    h.redo(&mut d).unwrap();
    assert_eq!(d.current_service, Some(new.id));
}

#[test]
fn undoing_a_deleted_service_keeps_the_one_open_now() {
    let (mut h, mut d) = (History::default(), Data::default());
    let (a, b) = (Service::new("A"), Service::new("B"));
    d.services = vec![a.clone(), b.clone()];
    d.current_service = Some(b.id);
    edit(&mut h, &mut d, |d| d.services.retain(|s| s.id != a.id));
    assert_eq!(h.undo(&mut d).unwrap(), "Undone: deleting the service “A”");
    assert_eq!(d.services.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["A", "B"], "back in its place");
    assert_eq!(d.current_service, Some(b.id), "the open one stays open");
}

#[test]
fn a_service_added_since_blocks_undoing_a_delete() {
    let (mut h, mut d) = (History::default(), Data::default());
    let a = Service::new("A");
    d.services.push(a.clone());
    edit(&mut h, &mut d, |d| d.services.retain(|s| s.id != a.id));
    d.services.push(Service::new("C"));
    assert!(h.undo(&mut d).is_err(), "the list is not as the delete left it");
    assert_eq!(d.services.len(), 2);
}

#[test]
fn every_library_list_is_covered() {
    let (mut h, mut d) = (History::default(), Data::default());
    edit(&mut h, &mut d, |d| {
        d.media_folders.push("Kids".into());
        d.decks.push(Deck { id: Uuid::new_v4(), title: "Talk".into(), slides: vec![] });
        d.default_theme = Some(Uuid::new_v4());
    });
    assert_eq!(h.undo(&mut d).unwrap(), "Undone: the last change");
    assert!(d.media_folders.is_empty() && d.decks.is_empty() && d.default_theme.is_none());
}

// ---- in the app ----

fn act(app: &Arc<App>, action: &str, body: serde_json::Value) -> serde_json::Value {
    tokio::runtime::Runtime::new().unwrap().block_on(app.do_action(action, body.to_string().as_bytes())).unwrap_or_else(|e| panic!("{action}: {}", e.message))
}

#[test]
fn the_app_undoes_an_edit_and_saves_it() {
    let app = test_app();
    act(&app, "song/save", json!({ "id": Uuid::nil(), "title": "Grace", "lyrics": "Verse 1\nla" }));
    assert!(app.history.lock().unwrap().can_undo());
    assert_eq!(app.undo(false).unwrap(), "Undone: the new song “Grace”");
    assert!(app.data.lock().unwrap().songs.is_empty());
    assert!(app.store.load().unwrap().songs.is_empty(), "saved");
    assert_eq!(app.undo(true).unwrap(), "Redone: the new song “Grace”");
    assert_eq!(app.store.load().unwrap().songs.len(), 1);
}

#[test]
fn a_settings_save_is_not_undone() {
    let app = test_app();
    act(&app, "settings", json!({ "lines_per_slide": 3 }));
    assert!(!app.history.lock().unwrap().can_undo());
    assert_eq!(app.undo(false), Err("Nothing to undo.".into()));
}

#[test]
fn what_the_app_finds_by_itself_is_not_a_step() {
    let app = test_app();
    app.edit_untracked(|d| { d.media_folders.push("Found".into()); Ok(()) }).unwrap();
    assert!(!app.history.lock().unwrap().can_undo());
}

#[test]
fn read_only_data_is_never_undone() {
    let app = test_app();
    act(&app, "song/save", json!({ "id": Uuid::nil(), "title": "Grace", "lyrics": "Verse 1\nla" }));
    app.set_read_only(true);
    assert!(app.undo(false).is_err());
    assert_eq!(app.data.lock().unwrap().songs.len(), 1);
}

fn show(app: &App, item_id: Uuid) {
    let out = app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::Show { item_id, slide: 0 } });
    assert!(out.ok, "{out:?}");
}

/// What the room shows.
fn on_screen(app: &App) -> String { format!("{:?}", app.live_view().frame) }

#[test]
fn undo_never_changes_the_live_screen() {
    let app = test_app();
    let item = text_item("Welcome", "Good morning");
    app.edit(|d| { d.services[0].items.push(item.clone()); Ok(()) }).unwrap();
    app.edit(|d| { if let ItemKind::Text { body, .. } = &mut d.services[0].items[0].kind { *body = "Good evening".into() } Ok(()) }).unwrap();
    show(&app, item.id);
    let shown = on_screen(&app);
    assert!(shown.contains("Good evening"), "{shown}");
    // Undo takes the words back: the screen keeps them and says the item changed.
    app.undo(false).unwrap();
    assert_eq!(on_screen(&app), shown);
    assert!(app.live_view().changed.is_some(), "the changed banner is up");
    // Undo again removes the live item from the service: still on screen.
    app.undo(false).unwrap();
    assert!(app.data.lock().unwrap().services[0].items.is_empty());
    assert_eq!(on_screen(&app), shown);
    // Redo puts it all back; the screen matches the service again.
    app.undo(true).unwrap();
    app.undo(true).unwrap();
    assert_eq!(on_screen(&app), shown);
    assert!(app.live_view().changed.is_none());
}
