use super::*;
use crate::{
    app::test_app,
    stream::compose::{Corner, CornerSize},
    ui::{SettingsDlg, dialogs::ask, operator::window::testing::{open as open_window, refresh}},
};
use slint::Model;

fn two() -> Editor {
    let mut ed = Editor::new(vec![]);
    ed.add();
    ed.add();
    ed
}

// ---- the editor, no window ----

#[test]
fn a_new_scene_is_the_room_with_the_camera_in_the_corner_and_named_in_turn() {
    let ed = two();
    let names: Vec<&str> = ed.scenes.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Scene 1", "Scene 2"]);
    assert_eq!(ed.scene, Some(1), "the new one is chosen");
    let s = ed.current().unwrap();
    assert_eq!(s.layers[0].shows, Shows::Room);
    assert_eq!(s.layers[1].place, Place::Corner { corner: Corner::BottomRight, size: CornerSize::Medium });
    assert_eq!((ed.layer, ed.layer_row()), (Some(1), Some(0)), "its front layer is chosen, at the top of the list");
    assert!(ed.changed());
    assert!(!Editor::new(vec![]).changed());
}

#[test]
fn scenes_are_copied_renamed_moved_and_deleted() {
    let mut ed = two();
    ed.choose(0);
    ed.set_instead(2);
    ed.copy();
    let (a, b) = (&ed.scenes[0], &ed.scenes[1]);
    assert_eq!((b.name.as_str(), b.layers.clone(), b.instead_of), ("Scene 1 copy", a.layers.clone(), None), "a copy does not stand in");
    assert_ne!(a.id, b.id);
    assert_eq!(ed.scene, Some(1));
    ed.rename("  Welcome  ");
    ed.rename("   ");
    assert_eq!(ed.scenes[1].name, "Welcome", "an empty name is not taken");
    ed.move_scene(true);
    assert_eq!((ed.scenes[0].name.as_str(), ed.scene), ("Welcome", Some(0)));
    ed.move_scene(true);
    assert_eq!(ed.scene, Some(0), "already first");
    ed.choose(2);
    ed.delete();
    assert_eq!((ed.scenes.len(), ed.scene), (2, Some(1)), "the one before is chosen");
    ed.delete();
    ed.delete();
    assert_eq!((ed.scene, ed.layer), (None, None));
}

#[test]
fn only_one_scene_stands_in_for_each_built_in_one() {
    let mut ed = two();
    let words = 1 + Scene::ALL.iter().position(|s| *s == Scene::CameraWords).unwrap();
    ed.choose(0);
    ed.set_instead(words);
    ed.choose(1);
    ed.set_instead(words);
    assert_eq!((ed.scenes[0].instead_of, ed.scenes[1].instead_of), (None, Some(Scene::CameraWords)));
    ed.set_instead(0);
    assert_eq!(ed.scenes[1].instead_of, None, "Never");
}

#[test]
fn layers_are_added_in_front_moved_and_removed() {
    let mut ed = two();
    ed.add_layer();
    let s = ed.current().unwrap();
    assert_eq!((s.layers.len(), ed.layer_row()), (3, Some(0)), "in front, chosen");
    assert_eq!(s.layers[2].shows, Shows::Text { text: "Welcome".into() });
    ed.move_layer(false);
    assert_eq!((ed.layer, ed.current().unwrap().layers[1].shows.clone()), (Some(1), Shows::Text { text: "Welcome".into() }));
    ed.move_layer(false);
    ed.move_layer(false);
    assert_eq!(ed.layer, Some(0), "at the back it stays");
    ed.move_layer(true);
    assert_eq!(ed.layer, Some(1));
    ed.choose_layer(0);
    assert_eq!(ed.layer, Some(2), "the top row is the front layer");
    ed.remove_layer();
    assert_eq!((ed.current().unwrap().layers.len(), ed.layer), (2, Some(1)));
}

#[test]
fn a_layer_is_changed_by_its_lists_and_fields() {
    let mut ed = two();
    let text = Shows::choices().iter().position(|s| matches!(s, Shows::Text { .. })).unwrap();
    ed.set_shows(text);
    ed.set_text("Welcome home");
    assert_eq!(ed.current_layer().unwrap().shows, Shows::Text { text: "Welcome home".into() });
    ed.set_colour("#ff0000");
    assert_eq!(ed.current_layer().unwrap().shows, Shows::Text { text: "Welcome home".into() }, "a colour means nothing to text");
    let colour = Shows::choices().iter().position(|s| matches!(s, Shows::Colour { .. })).unwrap();
    ed.set_shows(colour);
    ed.set_colour("12AB34");
    ed.set_colour("nope");
    assert_eq!(ed.current_layer().unwrap().shows, Shows::Colour { colour: "#12ab34".into() });
    ed.set_place(Place::choices().len() - 1);
    assert_eq!(ed.current_layer().unwrap().place, Place::Corner { corner: Corner::BottomRight, size: CornerSize::Medium }.as_box(), "a box starts where it was");
    ed.set_box(90, 10, 50, 200);
    assert_eq!(ed.current_layer().unwrap().place, Place::Box { x: 90, y: 10, w: 10, h: 90 }, "kept inside the picture");
    ed.set_see_through(150);
    assert_eq!(ed.current_layer().unwrap().see_through, 100);
}

// ---- in the window ----

fn dlg(op: &Operator) -> StreamScenesDlg<'_> { op.ui.global::<StreamScenesDlg>() }

#[test]
fn your_scenes_are_made_in_their_own_dialog_from_settings_and_saved() {
    let app = test_app();
    let op = open_window(&app);
    refresh(&op);
    super::super::settings::open(&op, SettingsTab::Media);
    assert_eq!(op.ui.global::<SettingsDlg>().get_stream_own_count(), 0);
    op.ui.global::<SettingsDlg>().invoke_edit_own_scenes();
    assert_eq!(op.dialog(), Some(DialogKind::StreamScenes), "nothing to save first: opened at once");
    assert_eq!(op.ui.get_dialog_title(), "Your livestream scenes");
    let g = dlg(&op);
    assert_eq!(g.get_scenes().row_count(), 0);
    g.invoke_add_scene();
    assert_eq!(g.get_scenes().row_data(0).unwrap(), "Scene 1");
    assert_eq!(g.get_layers().row_count(), 2);
    assert_eq!(g.get_layers().row_data(0).unwrap(), "The camera on air · Bottom right, medium", "the front layer at the top");
    assert!(g.get_picture().size().width > 0, "the picture is drawn");
    g.invoke_add_layer();
    assert!(g.get_is_text() && g.get_is_box());
    g.invoke_text_edited("Welcome!".into());
    g.invoke_see_through_changed(25);
    assert_eq!(g.get_layers().row_data(0).unwrap(), "Your words: Welcome! · A box you set · 25% see-through");
    g.invoke_instead_changed(1);
    assert!(g.get_changed());
    g.invoke_save();
    let saved = app.data.lock().unwrap().settings.record.own_scenes.clone();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].layers[2].shows, Shows::Text { text: "Welcome!".into() });
    assert_eq!(saved[0].instead_of, Some(Scene::ALL[0]));
    assert_eq!(op.dialog(), Some(DialogKind::Settings(SettingsTab::Media)), "back to Settings");
}

#[test]
fn leaving_with_changes_asks_first() {
    let app = test_app();
    let op = open_window(&app);
    refresh(&op);
    open(&op);
    dlg(&op).invoke_add_scene();
    dlg(&op).invoke_cancel();
    assert_eq!(op.dialog(), Some(DialogKind::StreamScenes), "asked first");
    ask::answer(&op, false, String::new());
    assert_eq!(op.dialog(), Some(DialogKind::StreamScenes), "kept editing");
    dlg(&op).invoke_cancel();
    ask::answer(&op, true, String::new());
    assert!(matches!(op.dialog(), Some(DialogKind::Settings(_))));
    assert!(app.data.lock().unwrap().settings.record.own_scenes.is_empty(), "nothing saved");
}

#[test]
fn settings_with_changes_are_saved_before_your_scenes_open() {
    let app = test_app();
    let op = open_window(&app);
    refresh(&op);
    super::super::settings::open(&op, SettingsTab::Media);
    op.ui.global::<SettingsDlg>().set_stream_words_top(1);
    op.ui.global::<SettingsDlg>().invoke_edit_own_scenes();
    assert!(matches!(op.dialog(), Some(DialogKind::Settings(_))), "asked first");
    ask::answer(&op, true, String::new());
    assert!(app.data.lock().unwrap().settings.record.stream_layout.words_top, "the other change saved");
    assert_eq!(op.dialog(), Some(DialogKind::StreamScenes));
}
