use super::*;
use crate::ui::dialogs::settings::testing::{find_any as find};
use crate::{
    app::{App, test_app},
    stage::{StageElement, presets::{BAND, CLASSIC, SPEAKER}},
    ui::{dialogs::ask, operator::{menu::{MenuAction, action}, window::testing::{open as open_window, refresh}}},
};
use slint::{Model, platform::PointerEventButton};
use std::sync::Arc;

fn ours(name: &str) -> StageLayout {
    let el = presets::new_box(ElementKind::ItemTitle, Uuid::new_v4());
    StageLayout { id: Uuid::new_v4(), name: name.into(), background: crate::stage::BACKGROUND.into(), font: String::new(), elements: vec![Lenient::Known(el)] }
}

fn rect(ed: &Editor, id: Uuid) -> PctRect {
    ed.current().elements.iter().filter_map(Lenient::known).find(|e| e.id == id).unwrap().rect
}

/// An editor on a fresh empty layout with one box at `r`, chosen.
fn with_box(r: PctRect) -> (Editor, Uuid) {
    let mut ed = Editor::new(vec![], CLASSIC);
    ed.new_layout();
    ed.add_box(0).unwrap();
    let id = ed.sel.unwrap();
    ed.edit(|l, _| if let Lenient::Known(e) = &mut l.elements[0] { e.rect = r });
    (ed, id)
}

// ---- the editor, no window ----

#[test]
fn opens_on_the_stage_default_and_lists_builtins_first() {
    let o = ours("Ours");
    let ed = Editor::new(vec![o.clone()], o.id);
    assert_eq!(ed.layout, o.id);
    let names: Vec<&str> = ed.all().iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Classic", "Words only", "Band", "Speaker", "Clock and countdown", "Slides and notes", "Ours"]);
    assert!(!ed.locked());
    assert!(Editor::new(vec![], Uuid::new_v4()).locked(), "a default that is gone opens Classic");
}

#[test]
fn builtins_are_locked_and_duplicate_to_edit() {
    let mut ed = Editor::new(vec![], BAND);
    assert!(ed.locked());
    ed.add_box(0).unwrap();
    ed.rename("Mine");
    assert!(!ed.changed(), "nothing changes a built-in");
    let id = ed.duplicate();
    assert_eq!(ed.layout, id);
    assert_eq!(ed.current().name, "Band copy");
    assert!(!ed.locked());
    let band = presets::by_id(BAND).unwrap();
    assert_eq!(ed.current().elements.len(), band.elements.len());
    let ids = |l: &StageLayout| -> Vec<Uuid> { l.elements.iter().filter_map(geom::element_id).collect() };
    assert!(ids(ed.current()).iter().all(|i| !ids(band).contains(i)), "the copy's boxes have new ids");
    ed.select_layout(2);
    assert_eq!(ed.duplicate(), ed.layout);
    assert_eq!(ed.current().name, "Band copy 2", "names stay unique");
}

#[test]
fn new_rename_delete_and_save_body() {
    let saved = ours("Old");
    let mut ed = Editor::new(vec![saved.clone()], CLASSIC);
    ed.new_layout();
    assert_eq!(ed.current().name, "New stage layout");
    assert!(ed.current().elements.is_empty());
    ed.rename("  Ours  ");
    assert_eq!(ed.current().name, "Ours");
    ed.select_layout(6);
    assert_eq!(ed.current().name, "Old");
    ed.delete();
    assert_eq!(ed.layout, CLASSIC);
    let body = ed.save_body();
    assert_eq!(body["layouts"].as_array().unwrap().len(), 1);
    assert_eq!(body["layouts"][0]["name"], "Ours");
    assert_eq!(body["deleted"], serde_json::json!([saved.id]));
    assert!(ed.changed());
    ed.saved();
    assert!(!ed.changed());
}

#[test]
fn add_box_selects_it_and_stops_at_24() {
    let mut ed = Editor::new(vec![], CLASSIC);
    ed.new_layout();
    ed.add_box(11).unwrap();
    let e = ed.selected_box().unwrap();
    assert_eq!(e.kind, ElementKind::Text { text: "Your words here".into() });
    assert_eq!(e.rect, geom::new_box_rect());
    for _ in 1..24 { ed.add_box(7).unwrap() }
    assert_eq!(ed.add_box(7), Err("A stage layout can have at most 24 boxes.".into()));
    assert_eq!(ed.current().elements.len(), 24);
    ed.remove_box();
    assert_eq!(ed.current().elements.len(), 23);
    assert_eq!(ed.sel, None);
}

#[test]
fn drag_moves_box_snapped_to_grid_or_guides() {
    let (mut ed, id) = with_box(PctRect::new(10.0, 10.0, 20.0, 10.0));
    ed.press((15.0, 15.0), (1.0, 1.0), false);
    ed.drag_to((19.2, 26.4));
    assert_eq!(rect(&ed, id), PctRect::new(15.0, 22.5, 20.0, 10.0), "grid: 14.2 → 15, 21.4 → 22.5");
    // Near the screen's middle: the box's centre snaps to 50 % with a guide.
    ed.drag_to((15.0 + 29.6, 15.0));
    assert!((rect(&ed, id).x - 40.0).abs() < 1e-4, "{:?}", rect(&ed, id));
    assert!(ed.guides.contains(&Guide::V(50.0)));
    ed.release();
    assert!(ed.guides.is_empty());
    ed.undo();
    assert_eq!(rect(&ed, id), PctRect::new(10.0, 10.0, 20.0, 10.0), "one drag is one undo step");
    ed.redo();
    assert!((rect(&ed, id).x - 40.0).abs() < 1e-4);
}

#[test]
fn alt_drag_moves_freely() {
    let (mut ed, id) = with_box(PctRect::new(10.0, 10.0, 20.0, 10.0));
    ed.press((15.0, 15.0), (1.0, 1.0), true);
    ed.drag_to((15.7, 15.0));
    assert!((rect(&ed, id).x - 10.7).abs() < 1e-4);
    assert!(ed.guides.is_empty());
}

#[test]
fn handle_resizes_and_respects_the_minimum() {
    let (mut ed, id) = with_box(PctRect::new(10.0, 10.0, 20.0, 20.0));
    // The bottom-right corner, within the handle's reach.
    ed.press((30.4, 29.7), (1.0, 1.0), false);
    ed.drag_to((40.4, 34.7));
    assert_eq!(rect(&ed, id), PctRect::new(10.0, 10.0, 30.0, 25.0));
    // The top-left corner past the opposite edges: never below 5 %.
    ed.release();
    ed.press((10.0, 10.0), (1.0, 1.0), false);
    ed.drag_to((80.0, 80.0));
    assert_eq!(rect(&ed, id), PctRect::new(35.0, 30.0, 5.0, 5.0));
}

#[test]
fn clicking_empty_screen_chooses_nothing_and_locked_layouts_never_move() {
    let (mut ed, _) = with_box(PctRect::new(10.0, 10.0, 20.0, 20.0));
    ed.press((80.0, 80.0), (1.0, 1.0), false);
    assert_eq!(ed.sel, None);
    let mut ed = Editor::new(vec![], CLASSIC);
    let before = ed.draft.clone();
    ed.press((50.0, 30.0), (1.0, 1.0), false);
    ed.drag_to((60.0, 40.0));
    ed.release();
    assert_eq!(ed.draft, before);
}

#[test]
fn keys_nudge_duplicate_tab_and_undo() {
    let (mut ed, id) = with_box(PctRect::new(10.0, 10.0, 20.0, 20.0));
    ed.nudge(Nudge::Right, false);
    assert_eq!(rect(&ed, id).x, 12.5);
    ed.nudge(Nudge::Down, true);
    assert_eq!(rect(&ed, id).h, 22.5);
    ed.duplicate_box().unwrap();
    let copy = ed.sel.unwrap();
    assert_ne!(copy, id);
    assert_eq!(rect(&ed, copy), PctRect::new(15.0, 12.5, 20.0, 22.5));
    ed.next_box(false);
    assert_eq!(ed.sel, Some(id), "Tab wraps to the first box");
    ed.undo();
    assert_eq!(ed.current().elements.len(), 1, "undo removes the copy");
    assert_eq!(ed.sel, Some(id));
}

#[test]
fn settings_change_the_box_and_typing_is_one_undo_step() {
    let (mut ed, _) = with_box(PctRect::new(10.0, 10.0, 20.0, 20.0));
    for v in [10, 11, 12] { ed.set_size(v) }
    ed.set_color("#f5c66b");
    ed.set_color("blurple");
    ed.set_align(0);
    let e = ed.selected_box().unwrap().clone();
    assert_eq!((e.style.max_size, e.style.color.as_deref(), e.style.align), (Some(12.0), Some("#f5c66b"), Some(Align::Left)));
    ed.undo();
    ed.undo();
    assert_eq!(ed.selected_box().unwrap().style.max_size, Some(12.0), "the colour was one step");
    ed.undo();
    assert_eq!(ed.selected_box().unwrap().style.max_size, None, "the slider drag was one step");
    ed.set_kind(7);
    assert_eq!(ed.selected_box().unwrap().kind, ElementKind::Clock { seconds: false, h24: false });
}

#[test]
fn unknown_box_is_listed_and_can_be_removed_but_not_moved() {
    let mut l = ours("Ours");
    let uid = Uuid::new_v4();
    l.elements.push(Lenient::Unknown(serde_json::json!({ "id": uid, "rect": { "x": 50, "y": 50, "w": 20, "h": 20 }, "kind": { "type": "hologram" } })));
    let mut ed = Editor::new(vec![l.clone()], l.id);
    let rows = box_rows(&ed);
    assert_eq!(rows.len(), 2);
    assert!(rows[1].unknown);
    ed.press((60.0, 60.0), (1.0, 1.0), false);
    assert_eq!(ed.sel, Some(uid));
    ed.drag_to((70.0, 70.0));
    ed.release();
    assert_eq!(ed.draft, vec![l]);
    ed.remove_box();
    assert_eq!(ed.current().elements.len(), 1);
}

#[test]
fn a_locked_box_cannot_be_moved_resized_nudged_or_removed() {
    let (mut ed, id) = with_box(PctRect::new(10.0, 10.0, 20.0, 20.0));
    ed.set_locked(true);
    assert!(ed.selected_box().unwrap().locked);
    ed.press((15.0, 15.0), (1.0, 1.0), false);
    ed.drag_to((50.0, 50.0));
    ed.release();
    ed.press((30.0, 30.0), (1.0, 1.0), false); // the corner handle
    ed.drag_to((60.0, 60.0));
    ed.release();
    ed.nudge(Nudge::Right, false);
    ed.remove_box();
    assert_eq!(rect(&ed, id), PctRect::new(10.0, 10.0, 20.0, 20.0));
    assert_eq!(ed.sel, Some(id), "still chosen, so it can be unlocked");
    // Its settings still change, and a copy is not locked.
    ed.set_size(12);
    assert_eq!(ed.selected_box().unwrap().style.max_size, Some(12.0));
    ed.duplicate_box().unwrap();
    assert!(!ed.selected_box().unwrap().locked);
    ed.select_box(id);
    ed.set_locked(false);
    ed.nudge(Nudge::Right, false);
    assert_eq!(rect(&ed, id).x, 12.5);
    ed.undo();
    ed.undo();
    assert!(ed.selected_box().unwrap().locked, "lock and unlock are undo steps");
}

#[test]
fn hide_chords_and_shapes_are_box_settings() {
    let (mut ed, _) = with_box(PctRect::new(10.0, 10.0, 20.0, 20.0));
    ed.set_hidden(true);
    assert!(ed.selected_box().unwrap().hidden);
    ed.set_kind(1);
    ed.set_chords(true);
    assert_eq!(ed.selected_box().unwrap().kind, ElementKind::NextSlide { pictures: false, chords: true });
    ed.set_kind(13);
    ed.set_shape(2);
    assert_eq!(ed.selected_box().unwrap().kind, ElementKind::Shape { shape: ShapeKind::Ellipse });
    ed.set_color("#ff0000");
    assert_eq!((ed.selected_box().unwrap().style.fill.as_deref(), ed.selected_box().unwrap().style.color.as_deref()), (Some("#ff0000"), None), "a shape's colour is its fill");
    ed.set_shape(3);
    ed.set_color("#00ff00");
    assert_eq!(ed.selected_box().unwrap().style.color.as_deref(), Some("#00ff00"), "a line's colour is its text colour");
    let rows = box_rows(&ed);
    assert_eq!((rows[0].shape, rows[0].hidden, rows[0].locked), (3, true, false));
}

#[test]
fn picker_lists_every_layout_with_the_one_on_stage() {
    let o = ours("Ours");
    assert_eq!(picker(&[o.clone()], o.id), (vec!["Classic".into(), "Words only".into(), "Band".into(), "Speaker".into(), "Clock and countdown".into(), "Slides and notes".into(), "Ours".into()], 6));
    assert_eq!(picker(&[], Uuid::new_v4()).1, 0);
}

// ---- in the operator window ----

fn opened(app: &Arc<App>) -> Rc<Operator> {
    let op = open_window(app);
    op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
    slint::platform::update_timers_and_animations();
    refresh(&op);
    op
}

fn settle() { slint::platform::update_timers_and_animations(); }

fn click(op: &Operator, id: &str) {
    settle();
    find(op, id).unwrap_or_else(|| panic!("{id} not found")).mock_single_click(PointerEventButton::Left);
    settle();
}

fn asked(op: &Operator) -> Option<String> { op.ui.get_ask().row_data(0).map(|a| a.text.to_string()) }

fn g(op: &Operator) -> StageLayoutsDlg<'_> { op.ui.global::<StageLayoutsDlg>() }

#[test]
fn window_menu_opens_the_editor() {
    assert_eq!(action("stage-layouts"), Some(MenuAction::StageLayouts));
    let app = test_app();
    let op = opened(&app);
    op.ui.invoke_menu("stage-layouts".into());
    assert_eq!(op.dialog(), Some(DialogKind::StageLayouts));
    assert_eq!(op.ui.get_dialog_title(), "Stage layouts");
    assert_eq!(g(&op).get_layouts().row_count(), 6);
    assert!(g(&op).get_locked());
    assert!(find(&op, "StageLayoutsDialog::canvas").is_some());
}

#[test]
fn duplicate_edit_and_save_reaches_the_data() {
    let app = test_app();
    let op = opened(&app);
    open(&op);
    click(&op, "LayoutList::duplicate-btn");
    assert_eq!(g(&op).get_layouts().row_count(), 7);
    assert!(!g(&op).get_locked());
    g(&op).invoke_add_box(7);
    assert_eq!(g(&op).get_boxes().row_count(), 10, "Classic's nine boxes and the clock");
    click(&op, "StageLayoutsDialog::save");
    assert_eq!(op.dialog(), None);
    let d = app.data.lock().unwrap();
    assert_eq!(d.stage_layouts.len(), 1);
    assert_eq!(d.stage_layouts[0].name, "Classic copy");
    assert_eq!(d.stage_layouts[0].elements.len(), 10);
}

#[test]
fn lock_hide_and_grid_controls_in_the_window() {
    let app = test_app();
    let op = opened(&app);
    open(&op);
    g(&op).invoke_new_layout();
    g(&op).invoke_add_box(13);
    settle();
    assert!(g(&op).get_box_is_shape());
    click(&op, "BoxPanel::lock-check");
    assert!(g(&op).get_box_locked());
    assert!(g(&op).get_boxes().row_data(0).unwrap().locked);
    click(&op, "BoxPanel::hide-check");
    assert!(g(&op).get_boxes().row_data(0).unwrap().hidden);
    assert!(!g(&op).get_show_grid());
    find(&op, "StageLayoutsDialog::grid-check").unwrap().invoke_accessible_default_action();
    settle();
    assert!(g(&op).get_show_grid());
    assert!(find(&op, "StageCanvas::ruler-top").is_some());
}

#[test]
fn save_error_keeps_dialog_and_changes() {
    let app = test_app();
    let op = opened(&app);
    open(&op);
    g(&op).invoke_new_layout();
    g(&op).invoke_rename_layout();
    ask::answer(&op, true, "classic".into());
    click(&op, "StageLayoutsDialog::save");
    assert_eq!(op.dialog(), Some(DialogKind::StageLayouts));
    assert_eq!(g(&op).get_error(), "There is already a stage layout called “classic”.");
    assert!(app.data.lock().unwrap().stage_layouts.is_empty());
    assert_eq!(g(&op).get_layouts().row_data(6).unwrap().name, "classic");
}

#[test]
fn cancel_asks_only_when_changed() {
    let app = test_app();
    let op = opened(&app);
    open(&op);
    click(&op, "StageLayoutsDialog::cancel");
    assert_eq!(op.dialog(), None, "nothing changed: closes at once");
    open(&op);
    g(&op).invoke_new_layout();
    click(&op, "StageLayoutsDialog::cancel");
    assert_eq!(asked(&op).as_deref(), Some("Discard your changes to the stage layouts?"));
    ask::answer(&op, true, String::new());
    assert_eq!(op.dialog(), None);
    assert!(app.data.lock().unwrap().stage_layouts.is_empty());
}

#[test]
fn delete_asks_with_what_uses_the_layout() {
    let app = test_app();
    let o = ours("Band 2");
    {
        let mut d = app.data.lock().unwrap();
        d.stage_layouts.push(o.clone());
        d.settings.stage.layout = o.id;
    }
    let op = opened(&app);
    open(&op);
    assert_eq!(g(&op).get_layouts().row_data(6).unwrap().on_stage, true);
    click(&op, "LayoutList::delete-btn");
    assert_eq!(asked(&op).as_deref(), Some("Delete “Band 2”? It is used by the stage default. The default will use Classic."));
    ask::answer(&op, true, String::new());
    assert_eq!(g(&op).get_layouts().row_count(), 6);
    click(&op, "StageLayoutsDialog::save");
    let d = app.data.lock().unwrap();
    assert!(d.stage_layouts.is_empty());
    assert_eq!(d.settings.stage.layout, CLASSIC);
}

#[test]
fn use_on_stage_saves_then_sets_the_default() {
    let app = test_app();
    let op = opened(&app);
    open(&op);
    g(&op).invoke_new_layout();
    click(&op, "LayoutList::use-btn");
    let d = app.data.lock().unwrap();
    assert_eq!(d.stage_layouts.len(), 1);
    assert_eq!(d.settings.stage.layout, d.stage_layouts[0].id);
    drop(d);
    assert_eq!(op.dialog(), Some(DialogKind::StageLayouts), "stays open");
    assert!(g(&op).get_layouts().row_data(6).unwrap().on_stage);
}

#[test]
fn top_bar_picker_sets_the_stage_layout() {
    let app = test_app();
    app.data.lock().unwrap().settings.screens.stage.on = true;
    let op = opened(&app);
    g(&op).invoke_fill_picker();
    assert_eq!(g(&op).get_picker_names().row_count(), 6);
    assert_eq!(g(&op).get_picker_current(), 0);
    g(&op).invoke_pick(3);
    assert_eq!(app.data.lock().unwrap().settings.stage.layout, SPEAKER);
    refresh(&op);
    g(&op).invoke_fill_picker();
    assert_eq!(g(&op).get_picker_current(), 3);
    g(&op).invoke_edit_layouts();
    assert_eq!(op.dialog(), Some(DialogKind::StageLayouts));
}

#[test]
fn settings_video_tab_picks_the_stage_layout() {
    let app = test_app();
    let op = opened(&app);
    crate::ui::dialogs::settings::open(&op, crate::ui::dialogs::SettingsTab::Video);
    let s = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    assert_eq!(s.get_stage_layout_names().row_count(), 6);
    assert_eq!(s.get_stage_layout(), 0);
    s.set_stage_layout(2);
    let mut settings = app.data.lock().unwrap().settings.clone();
    read_settings(&s, &mut settings);
    assert_eq!(settings.stage.layout, BAND);
}

#[tokio::test]
async fn saved_layout_as_default_is_the_active_layout() {
    let el: StageElement = presets::new_box(ElementKind::ItemTitle, Uuid::new_v4());
    let mut ed = Editor::new(vec![], CLASSIC);
    ed.new_layout();
    ed.edit(|l, _| l.elements.push(Lenient::Known(el)));
    let body = ed.save_body();
    let app = test_app();
    app.do_action("stage/layouts/replace", body.to_string().as_bytes()).await.unwrap();
    let id = ed.layout;
    app.do_action("stage/layout/default", serde_json::json!({ "id": id }).to_string().as_bytes()).await.unwrap();
    let d = app.data.lock().unwrap();
    assert_eq!(crate::stage::active_layout(&d.stage_layouts, None, None, Some(d.settings.stage.layout)).id, id);
}
