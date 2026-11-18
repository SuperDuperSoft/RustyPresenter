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
    assert_eq!(s.layers[2].shows, Shows::Text { text: "Welcome".into(), look: Default::default() });
    ed.move_layer(false);
    assert_eq!((ed.layer, ed.current().unwrap().layers[1].shows.clone()), (Some(1), Shows::Text { text: "Welcome".into(), look: Default::default() }));
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
    assert_eq!(ed.current_layer().unwrap().shows, Shows::Text { text: "Welcome home".into(), look: Default::default() });
    ed.set_colour("#ff0000");
    assert_eq!(ed.current_layer().unwrap().shows, Shows::Text { text: "Welcome home".into(), look: Default::default() }, "a colour means nothing to text");
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
    super::super::settings::open(&op, crate::ui::dialogs::SettingsTab::StreamScenes);
    assert_eq!(crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>().get_stream_own_count(), 0);
    crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>().invoke_edit_own_scenes();
    assert_eq!(op.dialog(), Some(DialogKind::StreamScenes), "opened at once in the operator window");
    assert_eq!(crate::ui::dialogs::settings::shown(&op), Some(crate::ui::dialogs::SettingsTab::StreamScenes), "Settings stays open beside it");
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
    assert_eq!(saved[0].layers[2].shows, Shows::Text { text: "Welcome!".into(), look: Default::default() });
    assert_eq!(saved[0].instead_of, Some(Scene::ALL[0]));
    assert_eq!(op.dialog(), None, "Your scenes closed");
    assert_eq!(crate::ui::dialogs::settings::shown(&op), Some(crate::ui::dialogs::SettingsTab::StreamScenes), "Settings is still open");
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
    assert_eq!(op.dialog(), None);
    assert!(app.data.lock().unwrap().settings.record.own_scenes.is_empty(), "nothing saved");
}

#[test]
fn settings_changes_wait_for_their_own_save_while_your_scenes_open() {
    let app = test_app();
    let op = open_window(&app);
    refresh(&op);
    super::super::settings::open(&op, crate::ui::dialogs::SettingsTab::StreamScenes);
    crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>().set_stream_words_top(1);
    crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>().invoke_edit_own_scenes();
    assert_eq!(op.dialog(), Some(DialogKind::StreamScenes), "opened at once: Settings keeps its changes");
    dlg(&op).invoke_add_scene();
    dlg(&op).invoke_save();
    assert_eq!(app.data.lock().unwrap().settings.record.own_scenes.len(), 1);
    refresh(&op);
    crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>().invoke_save();
    let s = app.data.lock().unwrap().settings.record.clone();
    assert!(s.stream_layout.words_top, "the Settings change saved");
    assert_eq!(s.own_scenes.len(), 1, "and the scene saved meanwhile is kept");
}

// ---- the scene extras (design 2026-11-17-scene-extras-design.md) ----

#[test]
fn a_layer_is_dragged_on_the_picture_and_resized_by_its_corner() {
    let mut ed = two();
    // The camera in the bottom right corner (a quarter as wide): pressing on it chooses and moves it.
    ed.choose_layer(1);
    assert_eq!(ed.layer, Some(0), "the room chosen");
    let (x, y, w, h) = { ed.choose_layer(0); ed.box_pct().unwrap() };
    ed.choose_layer(1);
    ed.press(x + w / 2.0, y + h / 2.0);
    assert_eq!(ed.layer, Some(1), "the camera in front there is chosen");
    assert!(ed.drag_to(x + w / 2.0 - 10.0, y + h / 2.0 - 10.0));
    let Place::Box { x: bx, y: by, w: bw, h: bh } = ed.current_layer().unwrap().place else { panic!("a box") };
    assert_eq!((bx, by), ((x - 10.0).round() as u8, (y - 10.0).round() as u8), "moved up and left");
    assert_eq!((bw, bh), (w.round() as u8, h.round() as u8), "the same size");
    ed.release();
    assert!(!ed.drag_to(0.0, 0.0), "nothing dragged once let go");
    // Its corner square: a press there resizes it, kept inside the picture.
    let (x, y, w, h) = ed.box_pct().unwrap();
    ed.press(x + w, y + h);
    ed.drag_to(x + w + 50.0, y + h + 2.0);
    let Place::Box { x: nx, w: nw, h: nh, .. } = ed.current_layer().unwrap().place else { panic!("a box") };
    assert_eq!((nx, u32::from(nx) + u32::from(nw)), (bx, 100), "as wide as the picture allows");
    assert_eq!(nh, (h + 2.0).round() as u8);
    // A press where nothing is (no layer covers it) chooses nothing new and drags nothing.
    let mut empty = Editor::new(vec![OwnScene { layers: vec![], ..OwnScene::new("x".into()) }]);
    empty.press(50.0, 50.0);
    assert!(!empty.drag_to(60.0, 60.0));
}

#[test]
fn the_tall_picture_has_its_own_places() {
    let mut ed = two();
    ed.set_view(true);
    assert!(!ed.editing_tall(), "the wide picture fitted across until asked");
    assert_eq!(ed.view_size(), (360, 640));
    ed.arrange_tall(true);
    assert!(ed.editing_tall());
    ed.set_place(Place::choices().iter().position(|p| *p == Place::LeftHalf).unwrap());
    let l = ed.current_layer().unwrap();
    assert_eq!((l.tall, l.place), (Some(Place::LeftHalf), Place::Corner { corner: Corner::BottomRight, size: CornerSize::Medium }), "the wide place is kept");
    ed.set_place(Place::choices().len() - 1);
    assert_eq!(ed.current_layer().unwrap().tall, Some(Place::Box { x: 0, y: 0, w: 50, h: 100 }), "a box starts where it was in the tall picture");
    ed.arrange_tall(false);
    assert_eq!((ed.current().unwrap().tall, ed.current_layer().unwrap().tall), (false, None));
}

#[test]
fn crops_keys_and_words_are_set() {
    let mut ed = two();
    ed.set_crop(10, 20, 95, 0);
    assert_eq!(ed.current_layer().unwrap().crop, Crop { left: 10, top: 20, right: 89, bottom: 0 }, "something always left");
    ed.set_key_kind(1);
    ed.set_key_colour("0047BB");
    ed.set_key_amounts(55, 120);
    assert_eq!(ed.current_layer().unwrap().key, Key::Colour { colour: "#0047bb".into(), closeness: 55, soft: 100 });
    ed.set_key_kind(2);
    assert!(matches!(ed.current_layer().unwrap().key, Key::Dark { .. }));
    let clock = Shows::choices().iter().position(|s| matches!(s, Shows::Clock { .. })).unwrap();
    ed.set_shows(clock);
    ed.set_clock(true, true);
    ed.set_look_size(80);
    ed.set_look_colour("#FF0000");
    ed.set_look_colour("nope");
    assert_eq!(ed.current_layer().unwrap().shows, Shows::Clock { seconds: true, h24: true, look: own_scenes::TextLook { size: 80, colour: "#ff0000".into() } });
    ed.set_look_colour("");
    assert_eq!(ed.current_layer().unwrap().shows.look().unwrap().colour, "", "empty: the church's colour");
    let countdown = Shows::choices().iter().position(|s| matches!(s, Shows::Countdown { .. })).unwrap();
    ed.set_shows(countdown);
    assert_eq!(ed.current_layer().unwrap().shows.look().unwrap().size, 80, "the look carries over");
    ed.set_countdown_to("10:30");
    ed.set_countdown_to("half past");
    assert!(matches!(ed.current_layer().unwrap().shows, Shows::Countdown { to: Some(t), .. } if t == TimeOfDay::new(10, 30)));
    ed.set_countdown_to("");
    assert!(matches!(ed.current_layer().unwrap().shows, Shows::Countdown { to: None, .. }), "empty: the service's start");
    let web = Shows::choices().iter().position(|s| matches!(s, Shows::Web { .. })).unwrap();
    ed.set_shows(web);
    ed.set_web("  https://example.org  ");
    assert_eq!(ed.current_layer().unwrap().shows, Shows::Web { address: "https://example.org".into() });
    let video = Shows::choices().iter().position(|s| matches!(s, Shows::Video { .. })).unwrap();
    ed.set_shows(video);
    ed.set_video("C:/loop.mp4");
    assert_eq!(ed.current_layer().unwrap().shows, Shows::Video { path: "C:/loop.mp4".into() });
}

#[test]
fn each_scene_can_change_in_its_own_way_and_the_default_is_kept() {
    let mut ed = two();
    ed.set_transition(4);
    ed.set_stinger("C:/sting.webm");
    assert_eq!(ed.current().unwrap().transition, Some(Transition::Stinger { path: "C:/sting.webm".into() }));
    ed.set_transition(0);
    assert_eq!(ed.current().unwrap().transition, None, "like the other scenes");
    let mut saved = Editor::with_default(vec![], Transition::Fade);
    saved.set_default_transition(2);
    assert!(saved.changed());
    assert_eq!(saved.default_transition, Transition::Slide);
    saved.set_default_stinger("x.webm");
    assert_eq!(saved.default_transition, Transition::Slide, "a stinger's video only for a stinger");
}

#[test]
fn the_picture_is_dragged_and_the_way_scenes_change_is_saved() {
    let app = test_app();
    let op = open_window(&app);
    refresh(&op);
    open(&op);
    let g = dlg(&op);
    g.invoke_add_scene();
    assert!(g.get_has_box(), "the chosen layer's frame on the picture");
    let (x, y, w, h) = (g.get_box_left(), g.get_box_top(), g.get_box_width(), g.get_box_height());
    g.invoke_press(x + w / 2.0, y + h / 2.0);
    g.invoke_drag(x + w / 2.0 - 20.0, y + h / 2.0);
    g.invoke_release();
    assert!(g.get_is_box() && (g.get_box_left() - (x - 20.0)).abs() < 1.0, "moved left: {} from {x}", g.get_box_left());
    g.invoke_view_changed(true);
    assert!(g.get_tall_view() && !g.get_scene_tall() && !g.get_has_box(), "the tall picture is the wide one fitted, nothing to drag");
    assert_eq!(g.get_picture().size().width, 360);
    g.invoke_arrange_tall(true);
    assert!(g.get_has_box());
    g.invoke_default_transition_changed(2);
    g.invoke_transition_changed(1);
    assert_eq!(g.get_transition_names().row_data(0).unwrap(), "Like the other scenes (Slide)");
    g.invoke_save();
    let s = app.data.lock().unwrap().settings.record.clone();
    assert_eq!((s.scene_transition, s.own_scenes[0].transition.clone(), s.own_scenes[0].tall), (Transition::Slide, Some(Transition::Cut), true));
}
