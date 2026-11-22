//! The slide editor, the slide menu and the order on the Slides panel, driven in the window.
use super::*;
use crate::{
    app::{App, LiveUpdate, test_app},
    live::Command,
    model::ServiceItem,
    ui::{SongEditorState, dialogs::{DialogKind, add::tests::{click, opened, press, settle, type_text}}, operator::window::testing::{find, find_all, refresh}},
};
use slint::{ComponentHandle, Model, platform::Key};
use std::sync::Arc;

const LYRICS: &str = "Verse 1\n[G]Amazing [C]grace\nhow sweet the sound\n\nChorus\nMy chains are gone\n\nVerse 2\n'Twas grace";

/// A song in the library and in the service, four lines a slide; returns (song id, item id).
fn add_song(app: &App, lyrics: &str, order: &[&str]) -> (Uuid, Uuid) {
    let song = Song { id: Uuid::new_v4(), title: "Grace".into(), lyrics: lyrics.into(), arrangement: order.iter().map(|s| s.to_string()).collect(),
        lines_per_slide: 4, ..Default::default() };
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None };
    let mut d = app.data();
    d.songs.push(song.clone());
    d.services[0].items.push(item.clone());
    (song.id, item.id)
}

fn ready(app: &Arc<App>, item: Uuid) -> Rc<Operator> {
    let op = opened(app);
    op.state.borrow_mut().selected = Some(item);
    op.render_slides();
    settle(&op);
    op
}

fn lyrics(app: &App, id: Uuid) -> String { app.data().songs.iter().find(|s| s.id == id).unwrap().lyrics.clone() }
fn song_order(app: &App, id: Uuid) -> Vec<String> { app.data().songs.iter().find(|s| s.id == id).unwrap().arrangement.clone() }
fn item_order(app: &App, id: Uuid) -> Option<Vec<String>> {
    app.data().services[0].items.iter().find(|i| i.id == id).and_then(|i| match &i.kind { ItemKind::Song { arrangement, .. } => arrangement.clone(), _ => None })
}
fn labels(op: &Operator) -> Vec<String> { op.ui.get_slide_rows().iter().flat_map(|r| r.cells.iter().map(|c| c.label.to_string()).collect::<Vec<_>>()).collect() }
fn act(op: &Operator, i: i32, a: &str) {
    op.ui.slide_edit_all().invoke_act(i, a.into());
    refresh(op);
    settle(op);
}
fn last_toast(op: &Operator) -> String { op.toast_log().last().map(|t| t.0.clone()).unwrap_or_default() }

#[test]
fn a_slide_opens_in_place_and_enter_saves_its_words_with_the_chords_kept() {
    let app = test_app();
    let (song, item) = add_song(&app, LYRICS, &[]);
    let op = ready(&app, item);
    let g = op.ui.slide_edit_all();
    assert!(op.ui.get_slide_rows().iter().flat_map(|r| r.cells.iter().collect::<Vec<_>>()).all(|c| c.song), "song slides can be edited");
    act(&op, 0, "edit");
    assert!(g.get_open());
    assert_eq!(g.get_heading(), "Verse 1");
    assert_eq!(g.get_start_text(), "Amazing grace\nhow sweet the sound", "the words, without chords");
    assert!(g.get_has_chords() && !g.get_chords_mode());
    assert_eq!(g.get_chord_view(), "G       C\nAmazing grace\nhow sweet the sound");
    assert!(find(&op, "SlideEditor::editor-save").is_some(), "the editor is over the slides, its buttons in view");
    // The words box has the keyboard: type, then Enter saves.
    type_text(&op, "Oh ");
    press(&op, Key::Return);
    refresh(&op);
    assert!(!g.get_open(), "Enter saved and closed it");
    assert_eq!(lyrics(&app, song), LYRICS.replace("[G]Amazing", "Oh [G]Amazing"), "the chord stays over its word");
    assert_eq!(last_toast(&op), "Slide saved");
    // Edit → Undo takes it back in one step.
    app.undo(false).unwrap();
    assert_eq!(lyrics(&app, song), LYRICS);
}

#[test]
fn esc_cancels_and_chords_can_be_edited_too() {
    let app = test_app();
    let (song, item) = add_song(&app, LYRICS, &[]);
    let op = ready(&app, item);
    let g = op.ui.slide_edit_all();
    act(&op, 0, "edit");
    type_text(&op, "typed ");
    press(&op, Key::Escape);
    assert!(!g.get_open());
    assert_eq!(lyrics(&app, song), LYRICS, "nothing saved");
    act(&op, 0, "edit");
    let text = g.invoke_chords_toggled(true, "Amazing grace\nhow sweet the sound".into());
    assert_eq!(text, "[G]Amazing [C]grace\nhow sweet the sound", "the chords as marks");
    g.invoke_save("[D]Amazing [C]grace\nhow sweet the sound".into(), false);
    refresh(&op);
    assert!(lyrics(&app, song).starts_with("Verse 1\n[D]Amazing [C]grace"));
    act(&op, 0, "edit");
    assert!(g.get_chords_mode(), "the next slide opens the same way");
    assert_eq!(g.get_start_text(), "[D]Amazing [C]grace\nhow sweet the sound");
}

#[test]
fn a_repeated_part_says_so_and_changes_everywhere() {
    let app = test_app();
    let (song, item) = add_song(&app, LYRICS, &["Verse 1", "Chorus", "Verse 2", "Chorus"]);
    let op = ready(&app, item);
    let g = op.ui.slide_edit_all();
    act(&op, 3, "edit");
    assert_eq!(g.get_heading(), "Chorus");
    assert_eq!(g.get_note(), "Chorus plays 2 times in this song. Your change shows every time.");
    g.invoke_save("My chains are gone!".into(), false);
    refresh(&op);
    assert!(lyrics(&app, song).contains("Chorus\nMy chains are gone!\n"));
    // Deleting a slide of a repeated part asks first.
    act(&op, 1, "delete");
    assert_eq!(op.ui.get_ask().row_data(0).map(|a| a.text.to_string()).as_deref(), Some("Chorus plays 2 times in this song. Delete this slide every time it plays?"));
    press(&op, Key::Escape);
    settle(&op);
    assert!(lyrics(&app, song).contains("Chorus"), "Cancel keeps it");
}

#[test]
fn the_slide_menu_splits_joins_adds_copies_deletes_and_renames() {
    let app = test_app();
    let (song, item) = add_song(&app, LYRICS, &[]);
    let op = ready(&app, item);
    let g = op.ui.slide_edit_all();
    assert_eq!(labels(&op), ["Verse 1", "Chorus", "Verse 2"]);
    // Split here: the editor opens; the cursor's line starts the new slide.
    act(&op, 0, "split");
    assert!(g.get_open());
    g.invoke_split_at("Amazing grace\nhow sweet the sound".into(), 14);
    refresh(&op);
    assert_eq!(labels(&op), ["Verse 1", "Verse 1", "Chorus", "Verse 2"]);
    assert!(lyrics(&app, song).starts_with("Verse 1\n[G]Amazing [C]grace\n\nhow sweet the sound"));
    // Join it back.
    act(&op, 0, "join");
    assert_eq!(lyrics(&app, song), LYRICS);
    // Add a slide after the chorus.
    act(&op, 1, "add");
    assert!(g.get_adding());
    g.invoke_save("A new line".into(), false);
    refresh(&op);
    assert!(lyrics(&app, song).contains("My chains are gone\n\nA new line\n\nVerse 2"));
    assert_eq!(last_toast(&op), "Slide added");
    act(&op, 2, "delete");
    assert_eq!(lyrics(&app, song), LYRICS);
    act(&op, 1, "duplicate");
    assert!(lyrics(&app, song).contains("My chains are gone\n\nMy chains are gone\n\nVerse 2"));
    act(&op, 2, "delete");
    // Change section: the chooser lists the names, each in its colour.
    act(&op, 2, "section");
    assert!(g.get_choosing());
    let names: Vec<String> = g.get_choices().iter().map(|c| c.name.to_string()).collect();
    assert_eq!(names[..3], ["Verse 3".to_string(), "Chorus 2".into(), "Pre-Chorus".into()]);
    find_all(&op, "SectionChooser::choice")[4].mock_single_click(slint::platform::PointerEventButton::Left);
    refresh(&op);
    assert!(lyrics(&app, song).ends_with("Tag\n'Twas grace"), "{}", lyrics(&app, song));
    assert_eq!(labels(&op), ["Verse 1", "Chorus", "Tag"]);
    assert_eq!(last_toast(&op), "This part is now Tag");
    // Every step is one Undo.
    for _ in 0..7 { app.undo(false).unwrap(); }
    assert_eq!(lyrics(&app, song), LYRICS);
}

#[test]
fn moving_a_part_changes_this_services_order_only_until_saved_as_usual() {
    let app = test_app();
    let (song, item) = add_song(&app, LYRICS, &[]);
    let op = ready(&app, item);
    let g = op.ui.slide_edit_all();
    assert!(g.get_order_shown());
    assert!(find(&op, "OrderBar::order-strip").is_some(), "the order is on the panel");
    act(&op, 2, "earlier");
    assert_eq!(labels(&op), ["Verse 1", "Verse 2", "Chorus"]);
    assert_eq!(item_order(&app, item), Some(vec!["Verse 1".into(), "Verse 2".into(), "Chorus".into()]));
    assert!(song_order(&app, song).is_empty(), "the song's usual order is untouched");
    assert!(g.get_order_own());
    // A part dragged in from + Chorus, a chip duplicated, a chip removed.
    g.invoke_order_insert("Chorus".into(), 1);
    refresh(&op);
    assert_eq!(labels(&op), ["Verse 1", "Chorus", "Verse 2", "Chorus"]);
    g.invoke_order_duplicate(3);
    refresh(&op);
    assert_eq!(labels(&op), ["Verse 1", "Chorus", "Verse 2", "Chorus", "Chorus"]);
    g.invoke_order_remove(4);
    refresh(&op);
    // A slide dragged onto another: its part plays there.
    g.invoke_dropped(2, 0);
    refresh(&op);
    assert_eq!(labels(&op), ["Verse 2", "Verse 1", "Chorus", "Chorus"]);
    // Save as the usual order: the song plays this way everywhere; this service follows it again.
    g.invoke_order_usual();
    refresh(&op);
    assert_eq!(song_order(&app, song), ["Verse 2", "Verse 1", "Chorus", "Chorus"]);
    assert_eq!(item_order(&app, item), None);
    assert!(!g.get_order_own());
    // A pattern, then back to the song's order.
    g.invoke_order_pattern(0);
    refresh(&op);
    assert_eq!(labels(&op), ["Verse 2", "Chorus", "Verse 1", "Chorus", "Chorus"]);
    g.invoke_order_back();
    refresh(&op);
    assert_eq!(labels(&op), ["Verse 2", "Verse 1", "Chorus", "Chorus"]);
    app.undo(false).unwrap();
    refresh(&op);
    assert_eq!(labels(&op), ["Verse 2", "Chorus", "Verse 1", "Chorus", "Chorus"], "Undo brings the pattern back");
}

fn show(op: &Operator, item_id: Uuid, slide: usize) {
    op.ops().cmd(Command::Show { item_id, slide }, None, |o| assert!(o.ok, "{o:?}"));
    op.apply_live(LiveUpdate::new(op.app().live_view()));
}

#[test]
fn rearranging_while_live_keeps_the_live_part_live() {
    let app = test_app();
    let (_, item) = add_song(&app, LYRICS, &["Verse 1", "Chorus", "Verse 2", "Chorus"]);
    let op = ready(&app, item);
    show(&op, item, 3);
    let frame = app.live_view().frame;
    // The live (second) chorus is dragged to just after Verse 1.
    op.ui.slide_edit_all().invoke_order_move(3, 1);
    op.apply_live(LiveUpdate::new(app.live_view()));
    let v = app.live_view();
    assert_eq!(v.frame, frame, "the room never changes");
    assert_eq!(v.cursor.map(|c| c.slide), Some(1), "the chorus that was live is still live, where it went");
    op.ops().cmd(Command::Next, Some(app.live_view().version), |o| assert!(o.ok, "{o:?}"));
    assert_eq!(app.live_view().cursor.map(|c| c.slide), Some(2), "Next follows the new order");
}

#[test]
fn edit_words_in_the_live_panel_opens_the_live_slide_in_place() {
    let app = test_app();
    let (song, item) = add_song(&app, LYRICS, &[]);
    let other = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Hi".into(), body: "x".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
    app.data().services[0].items.insert(0, other.clone());
    let op = ready(&app, other.id);
    show(&op, item, 2);
    click(&op, "LiveColumn::live-edit-words");
    let g = op.ui.slide_edit_all();
    assert_eq!(op.state.borrow().selected, Some(item), "the live song's slides are shown");
    assert!(g.get_open() && g.get_live());
    assert_eq!(g.get_heading(), "Verse 2");
    assert!(find(&op, "SlideEditor::editor-save-show").is_some(), "Save and show now");
    // Edit the whole song…: the song editor, the cursor at this slide.
    click(&op, "SlideEditor::editor-song");
    assert_eq!(op.dialog(), Some(DialogKind::SongEditor { song_id: Some(song), add_after: false }));
    assert_eq!(op.ui.global::<SongEditorState>().get_caret() as usize, LYRICS.find("'Twas").unwrap());
}

#[test]
fn a_slide_of_chords_alone_opens_with_its_chords() {
    let app = test_app();
    let (song, item) = add_song(&app, "Intro\n[G] [D]\n\nVerse 1\n[G]Amazing grace", &[]);
    let op = ready(&app, item);
    let g = op.ui.slide_edit_all();
    act(&op, 0, "edit");
    assert!(g.get_chords_mode());
    assert_eq!(g.get_start_text(), "[G] [D]");
    assert_eq!(g.get_hint(), "This slide is chords only. Type the chords in square brackets, like [G] [D].");
    g.invoke_save("[G] [D] [C]".into(), false);
    refresh(&op);
    assert!(lyrics(&app, song).starts_with("Intro\n[G] [D] [C]\n\nVerse 1"));
}

#[test]
fn other_items_have_no_slide_editing() {
    let app = test_app();
    let other = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Hi".into(), body: "x".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
    app.data().services[0].items.push(other.clone());
    let op = ready(&app, other.id);
    assert!(!op.ui.slide_edit_all().get_order_shown());
    assert!(op.ui.get_slide_rows().iter().flat_map(|r| r.cells.iter().collect::<Vec<_>>()).all(|c| !c.song));
    act(&op, 0, "edit");
    assert!(!op.ui.slide_edit_all().get_open());
}
