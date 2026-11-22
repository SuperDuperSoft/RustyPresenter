//! Every dialog of the operator window at the smallest window a church may have (1024 × 768, November 17, 2026
//! Settings reorganisation): the card inside the window, its title's X and every footer button inside the card, and
//! nothing past its sides. Also the dialogs Settings now lists separately (Your scenes, SongSelect, the Go Live
//! helper), which the 1280 × 720 check in `src/ui/operator/fit.rs` does not open.
use crate::{
    app::test_app,
    model::{ItemKind, ServiceItem},
    ui::{
        dialogs::{AddTab, add, automation, find_verse, getting_started, go_live_helper, item_edit, music, overlays, phone, print, quick_search, screens, services, show_message, slide_breaks, song_editor, songselect, stage_layouts, stream_scenes, translation_editor, usage},
        operator::window::{Operator, testing::{find, open, refresh}},
    },
};
use i_slint_backend_testing::ElementHandle;
use std::rc::Rc;
use uuid::Uuid;

fn bounds(e: &ElementHandle) -> (f32, f32, f32, f32) {
    let (p, s) = (e.absolute_position(), e.size());
    (p.x, p.y, p.x + s.width, p.y + s.height)
}

fn assert_fits(op: &Operator, name: &str, (w, h): (f32, f32)) {
    let card = find(op, "DialogShell::card").unwrap_or_else(|| panic!("{name} is open"));
    let (l, t, r, b) = bounds(&card);
    assert!(l >= -0.5 && t >= -0.5 && r <= w + 0.5 && b <= h + 0.5, "{name}'s card {l},{t}–{r},{b} is outside the {w}×{h} window");
    let close = bounds(&find(op, "DialogShell::close-btn").expect("the X"));
    assert!(close.2 <= r + 0.5 && close.3 <= b + 0.5, "{name}'s X is inside its card");
    if let Some(line) = find(op, "DialogFoot::foot-line") {
        let line_y = line.absolute_position().y;
        let buttons: Vec<ElementHandle> = card.query_descendants().match_descendants().match_type_name("Btn").find_all()
            .into_iter().filter(|e| e.absolute_position().y > line_y && e.size().height > 0.0).collect();
        for btn in buttons {
            let (bl, _, br, bb) = bounds(&btn);
            assert!(bb <= b + 0.5 && bl >= l - 0.5 && br <= r + 0.5, "{name}'s footer button {:?} ({bl}–{br}, ends {bb}) is outside its card {l}–{r}, {b} at {w}×{h}",
                btn.accessible_label());
        }
    }
}

#[test]
fn every_dialog_fits_a_1024_by_768_window() {
    let app = test_app();
    let op = open(&app);
    refresh(&op);
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Good morning".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
    op.app().data().services[0].items.push(item.clone());
    refresh(&op);
    let (item, service) = (item.id, op.app().data().services[0].id);
    let dialogs: Vec<(&str, Box<dyn Fn(&Rc<Operator>)>)> = vec![
        ("Add song", Box::new(|op| add::open_add(op, AddTab::Song))),
        ("Add scripture", Box::new(|op| add::open_add(op, AddTab::Scripture))),
        ("Add media", Box::new(|op| add::open_add(op, AddTab::Media))),
        ("Add deck", Box::new(|op| add::open_add(op, AddTab::Deck))),
        ("Add text", Box::new(|op| add::open_add(op, AddTab::Text))),
        ("Song editor", Box::new(|op| song_editor::open_song_editor(op, None, false))),
        ("Item edit", Box::new(move |op| item_edit::open(op, item))),
        ("Services", Box::new(services::open)),
        ("Print", Box::new(move |op| print::open(op, service))),
        ("Screens", Box::new(screens::open)),
        ("Remote", Box::new(phone::open)),
        ("Usage", Box::new(usage::open)),
        ("Stage layouts", Box::new(stage_layouts::open)),
        ("Overlays", Box::new(overlays::open)),
        ("Automation", Box::new(automation::open)),
        ("Walk-in music", Box::new(music::open)),
        ("Show message", Box::new(show_message::open)),
        ("Find verse", Box::new(|op| find_verse::open(op, ""))),
        ("Slide breaks", Box::new(move |op| slide_breaks::open(op, item))),
        ("Quick search", Box::new(quick_search::open)),
        ("Translations", Box::new(|op| translation_editor::open(op, None))),
        ("Your scenes", Box::new(stream_scenes::open)),
        ("SongSelect", Box::new(songselect::open)),
        ("Go Live helper", Box::new(go_live_helper::open)),
        ("Getting started", Box::new(getting_started::open)),
    ];
    for size in [(1024.0, 768.0), (1100.0, 700.0)] {
        op.window().set_size(slint::LogicalSize::new(size.0, size.1));
        for (name, show) in &dialogs {
            show(&op);
            op.window().set_size(slint::LogicalSize::new(size.0, size.1));
            slint::platform::update_timers_and_animations();
            assert!(op.dialog().is_some(), "{name} opened");
            assert_fits(&op, name, size);
            op.close_dialog();
        }
    }
}
