use super::*;
use crate::{
    app::{App, test_app},
    ui::{dialogs::ask, operator::{menu::{MenuAction, action}, window::testing::{find, open as open_window, refresh}}},
};
use slint::{Model, platform::PointerEventButton};
use std::sync::Arc;

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

fn g(op: &Operator) -> DesignDlg<'_> { op.ui.global::<DesignDlg>() }

fn designs(app: &App) -> Vec<(Uuid, String, Vec<Design>)> {
    let d = app.data.lock().unwrap();
    d.service().unwrap().items.iter().filter_map(|i| match &i.kind {
        ItemKind::Design { title, slides } => Some((i.id, title.clone(), slides.clone())),
        _ => None,
    }).collect()
}

#[test]
fn the_add_menu_opens_the_editor_and_save_adds_the_item() {
    assert_eq!(action("add-design"), Some(MenuAction::AddDesign));
    let app = test_app();
    let op = opened(&app);
    op.ui.invoke_menu("add-design".into());
    assert_eq!(op.dialog(), Some(DialogKind::Design(None)));
    assert_eq!(op.ui.get_dialog_title(), "Slide editor");
    assert!(find(&op, "DesignEditorDialog::canvas").is_some());
    assert_eq!(g(&op).get_slides().row_count(), 1);
    click(&op, "DesignEditorDialog::add-text-btn");
    assert_eq!((g(&op).get_objects().row_count(), g(&op).get_chosen(), g(&op).get_kind()), (1, 1, 0));
    assert_eq!(g(&op).get_text(), "Your words here");
    g(&op).invoke_text_edited("Point one".into());
    g(&op).invoke_add_shape(1);
    g(&op).invoke_build_on(true);
    assert_eq!(g(&op).get_build_note(), "Appears on Next 1 of 1");
    g(&op).invoke_title_edited("Sermon".into());
    click(&op, "DesignEditorDialog::save");
    assert_eq!(op.dialog(), None);
    let saved = designs(&app);
    assert_eq!(saved.len(), 1);
    let (id, title, slides) = &saved[0];
    assert_eq!((title.as_str(), slides.len(), slides[0].objects.len(), slides[0].steps()), ("Sermon", 1, 2, 1));
    // Edit the selected item opens it again in the editor.
    refresh(&op);
    crate::ui::dialogs::item_edit::open(&op, *id);
    assert_eq!(op.dialog(), Some(DialogKind::Design(Some(*id))));
    assert_eq!(g(&op).get_title(), "Sermon");
    g(&op).invoke_add_slide(true);
    click(&op, "DesignEditorDialog::save");
    assert_eq!(designs(&app)[0].2.len(), 2, "updated in place");
    assert_eq!(designs(&app).len(), 1);
}

#[test]
fn the_slide_is_drawn_as_the_room_draws_it() {
    let app = test_app();
    let op = opened(&app);
    open(&op, None);
    let blank = g(&op).get_picture().to_rgba8().unwrap();
    g(&op).invoke_add_shape(0);
    let drawn = g(&op).get_picture().to_rgba8().unwrap();
    assert_eq!((drawn.width(), drawn.height()), PICTURE);
    let px = |b: &slint::SharedPixelBuffer<slint::Rgba8Pixel>, x: u32, y: u32| b.as_slice()[(y * b.width() + x) as usize];
    assert_ne!(px(&blank, 480, 270), px(&drawn, 480, 270), "the new rectangle shows in the middle");
}

#[test]
fn ctrl_alt_c_and_v_copy_a_style_to_the_next_slide() {
    let app = test_app();
    let op = opened(&app);
    open(&op, None);
    g(&op).invoke_command("v".into(), true, false);
    g(&op).invoke_add_text();
    g(&op).invoke_size_changed(150);
    g(&op).invoke_bold_changed(true);
    g(&op).invoke_command("c".into(), true, false);
    assert!(g(&op).get_style_copied());
    g(&op).invoke_add_slide(false);
    g(&op).invoke_add_text();
    g(&op).invoke_press(1.0, 1.0, 0.1, 0.1, false, false);
    g(&op).invoke_release();
    assert_eq!(g(&op).get_chosen(), 0);
    g(&op).invoke_command("v".into(), true, false);
    g(&op).invoke_select_slide(1);
    g(&op).invoke_next(false);
    assert_eq!((g(&op).get_size(), g(&op).get_bold()), (150, true));
    g(&op).invoke_command("z".into(), false, false);
    assert_eq!(g(&op).get_size(), 72, "Ctrl+Z takes the paste back");
}

#[test]
fn cancel_asks_only_when_changed() {
    let app = test_app();
    let op = opened(&app);
    open(&op, None);
    click(&op, "DesignEditorDialog::cancel");
    assert_eq!(op.dialog(), None, "nothing changed: closes at once");
    open(&op, None);
    g(&op).invoke_add_text();
    click(&op, "DesignEditorDialog::cancel");
    assert_eq!(op.ui.get_ask().row_data(0).map(|a| a.text.to_string()).as_deref(), Some("Throw away the changes to these slides?"));
    ask::answer(&op, true, String::new());
    assert_eq!(op.dialog(), None);
    assert!(designs(&app).is_empty());
}

#[test]
fn the_pen_draws_a_line() {
    let app = test_app();
    let op = opened(&app);
    open(&op, None);
    g(&op).invoke_toggle_pen();
    assert!(g(&op).get_pen());
    g(&op).invoke_press(10.0, 10.0, 0.5, 0.5, false, false);
    g(&op).invoke_drag(40.0, 30.0);
    g(&op).invoke_drag(60.0, 60.0);
    g(&op).invoke_release();
    assert_eq!((g(&op).get_objects().row_count(), g(&op).get_kind()), (1, 4));
}

#[test]
fn a_web_page_gets_its_address_typed_in() {
    let app = test_app();
    let op = opened(&app);
    open(&op, None);
    click(&op, "DesignEditorDialog::add-web-btn");
    assert_eq!(g(&op).get_kind(), 5);
    assert!(find(&op, "Inspector::web-url").is_some());
    g(&op).invoke_text_edited("https://church.example".into());
    click(&op, "DesignEditorDialog::save");
    assert_eq!(designs(&app)[0].2[0].objects[0].kind, ObjKind::Web { url: "https://church.example".into() });
}

#[test]
fn live_edit_save_and_show_now_puts_the_change_on_screen() {
    use crate::{app::LiveUpdate, live::{Command, SlideLayer}};
    let app = test_app();
    let op = opened(&app);
    open(&op, None);
    g(&op).invoke_add_text();
    g(&op).invoke_text_edited("Old point".into());
    click(&op, "DesignEditorDialog::save");
    let id = designs(&app)[0].0;
    refresh(&op);
    op.ops().cmd(Command::Show { item_id: id, slide: 0 }, None, |o| assert!(o.ok, "{o:?}"));
    op.apply_live(LiveUpdate::new(app.live_view()));
    open(&op, Some(id));
    assert!(g(&op).get_live());
    g(&op).invoke_select_slide(0);
    g(&op).invoke_next(false);
    g(&op).invoke_text_edited("New point".into());
    click(&op, "DesignEditorDialog::save-and-show");
    assert_eq!(op.dialog(), None);
    let words = match &app.live_view().frame.slide { Some(SlideLayer::Design { design, .. }) => design.words(), _ => String::new() };
    assert_eq!(words, "New point");
    open(&op, None);
    assert!(!g(&op).get_live(), "new slides are not on screen");
    assert!(find(&op, "DesignEditorDialog::save-and-show").is_none());
}
