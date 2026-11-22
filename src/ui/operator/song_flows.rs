//! FEAT-4 in the operator window: section keys, the section bar, Edit words, the change banner and Update screen.
use super::window::{Operator, testing::{find, open, refresh}};
use crate::{
    app::{App, LiveUpdate, test_app},
    live::Command,
    model::{ItemKind, ServiceItem, Song},
    sections::SectionKind,
    ui::{SongEditorState, dialogs::DialogKind},
};
use slint::{ComponentHandle, Model, SharedString, platform::{PointerEventButton, WindowEvent}};
use std::{rc::Rc, sync::Arc};
use uuid::Uuid;

const LYRICS: &str = "Verse 1\nv1\nChorus\nc\nVerse 2\nv2";

/// A song in the library and in the service; returns (song id, item id).
fn add_song(app: &App, lyrics: &str, order: &[&str]) -> (Uuid, Uuid) {
    let song = Song { id: Uuid::new_v4(), title: "Grace".into(), lyrics: lyrics.into(), arrangement: order.iter().map(|s| s.to_string()).collect(), ..Default::default() };
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None };
    let mut d = app.data();
    d.songs.push(song.clone());
    d.services[0].items.push(item.clone());
    (song.id, item.id)
}

fn add_text(app: &App) -> Uuid {
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Hello".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
    app.data().services[0].items.push(item.clone());
    item.id
}

fn opened(app: &Arc<App>) -> Rc<Operator> {
    let op = open(app);
    op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
    slint::platform::update_timers_and_animations();
    refresh(&op);
    op
}

fn live(op: &Operator) { op.apply_live(LiveUpdate::new(op.app().live_view())); }

fn show(op: &Operator, item_id: Uuid, slide: usize) {
    op.ops().cmd(Command::Show { item_id, slide }, None, |o| assert!(o.ok, "{o:?}"));
    live(op);
}

fn press(op: &Operator, text: &str) {
    let text: SharedString = text.into();
    op.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    op.window().dispatch_event(WindowEvent::KeyReleased { text });
    live(op);
}

fn cursor(app: &App) -> Option<(Uuid, usize)> { app.live_view().cursor.map(|c| (c.item_id, c.slide)) }

fn toasts(op: &Operator) -> Vec<String> { op.toast_log().into_iter().map(|(t, _)| t).collect() }

fn click(op: &Operator, id: &str) {
    find(op, id).unwrap_or_else(|| panic!("{id}")).mock_single_click(PointerEventButton::Left);
    refresh(op);
    live(op);
}

fn select(op: &Operator, item_id: Uuid) {
    op.state.borrow_mut().selected = Some(item_id);
    refresh(op);
    live(op);
}

fn set_lyrics(app: &App, song_id: Uuid, lyrics: &str) {
    app.edit(|d| { d.songs.iter_mut().find(|s| s.id == song_id).unwrap().lyrics = lyrics.into(); Ok(()) }).unwrap();
}

#[test]
fn theme_section_tokens_match_rust_palette() {
    crate::ui::ui_test_init();
    let w = crate::ui::OperatorWindow::new().unwrap();
    let t = w.global::<crate::ui::Theme>();
    for k in SectionKind::ALL {
        let h = k.default_color().trim_start_matches('#');
        let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap();
        assert_eq!(t.invoke_section_color(k.id().into()), slint::Color::from_rgb_u8(c(0), c(2), c(4)), "{k:?}");
    }
}

#[test]
fn c_key_jumps_live_song_to_chorus_and_toasts() {
    let app = test_app();
    let (_, id) = add_song(&app, LYRICS, &[]);
    let op = opened(&app);
    show(&op, id, 0);
    press(&op, "c");
    assert_eq!(cursor(&app), Some((id, 1)));
    press(&op, "2");
    assert_eq!(cursor(&app), Some((id, 2)));
    assert_eq!(toasts(&op), ["Jumped to Chorus (C)", "Jumped to Verse 2 (2)"]);
}

#[test]
fn failed_jump_shows_error_not_jump_toast() {
    let app = test_app();
    let (_, id) = add_song(&app, LYRICS, &[]);
    let op = opened(&app);
    show(&op, id, 0);
    press(&op, "t");
    assert_eq!(cursor(&app), Some((id, 0)));
    assert_eq!(toasts(&op), ["This song has no Tag."]);
}

#[test]
fn stray_typing_on_non_song_or_nothing_live_changes_nothing() {
    let app = test_app();
    let id = add_text(&app);
    let op = opened(&app);
    press(&op, "c");
    assert_eq!(cursor(&app), None);
    show(&op, id, 0);
    let v = app.live_view().version;
    press(&op, "c");
    assert_eq!(app.live_view().version, v);
    assert_eq!(toasts(&op), ["Nothing is live yet.", "Section keys work on songs."]);
}

#[test]
fn jump_keys_off_in_settings_do_nothing() {
    let app = test_app();
    let (_, id) = add_song(&app, LYRICS, &[]);
    app.data().settings.section_keys = false;
    let op = opened(&app);
    show(&op, id, 0);
    press(&op, "c");
    assert_eq!(cursor(&app), Some((id, 0)));
    assert!(toasts(&op).is_empty());
}

#[test]
fn c_key_with_preview_on_other_song_still_acts_on_live() {
    let app = test_app();
    let (_, a) = add_song(&app, LYRICS, &[]);
    let (_, b) = add_song(&app, LYRICS, &[]);
    let op = opened(&app);
    show(&op, a, 0);
    select(&op, b);
    press(&op, "c");
    assert_eq!(cursor(&app), Some((a, 1)));
}

#[test]
fn section_bar_marks_the_live_section_and_jumps_on_the_live_song() {
    let app = test_app();
    let (_, id) = add_song(&app, LYRICS, &["Verse 1", "Chorus", "Verse 2", "Chorus"]);
    let op = opened(&app);
    select(&op, id);
    let chips: Vec<_> = op.ui.get_slide_jumps().iter().collect();
    assert_eq!(chips.iter().map(|c| c.short.to_string()).collect::<Vec<_>>(), ["V1", "C", "V2", "C"]);
    assert!(chips.iter().all(|c| !c.live && c.tooltip == "Click to show this part"));
    // Not live: a chip takes that part live.
    op.ui.invoke_slides_pick_section(3);
    live(&op);
    assert_eq!(cursor(&app), Some((id, 3)));
    let chips: Vec<_> = op.ui.get_slide_jumps().iter().collect();
    assert_eq!(chips.iter().map(|c| c.live).collect::<Vec<_>>(), [false, false, false, true]);
    assert_eq!(chips[2].tooltip, "Verse 2 — press 2");
    // Live: a chip jumps there.
    op.ui.invoke_slides_pick_section(1);
    live(&op);
    assert_eq!(cursor(&app), Some((id, 1)));
    assert_eq!(op.ui.get_slide_rows().iter().map(|r| r.cells.row_count()).sum::<usize>(), 4, "the grid keeps its slides");
}

#[test]
fn thumb_captions_take_the_section_colour() {
    let app = test_app();
    let (_, id) = add_song(&app, LYRICS, &[]);
    let op = opened(&app);
    select(&op, id);
    let cells: Vec<_> = op.ui.get_slide_rows().iter().flat_map(|r| r.cells.iter().collect::<Vec<_>>()).collect();
    assert_eq!(cells.iter().map(|c| c.section.to_string()).collect::<Vec<_>>(), ["verse", "chorus", "verse"]);
}

#[test]
fn live_column_shows_the_section_and_edit_words_opens_the_editor_there() {
    let app = test_app();
    let (song_id, id) = add_song(&app, LYRICS, &[]);
    let op = opened(&app);
    show(&op, id, 2);
    let s = op.ui.get_live_song();
    assert_eq!((s.line.as_str(), s.kind.as_str(), s.edit), ("Verse 2 · slide 1 of 1", "verse", true));
    click(&op, "LiveColumn::live-edit-words");
    assert!(op.ui.slide_edit_all().get_open(), "the live slide opens in the Slides panel's editor");
    click(&op, "SlideEditor::editor-song");
    assert_eq!(op.dialog(), Some(DialogKind::SongEditor { song_id: Some(song_id), add_after: false }));
    let st = op.ui.global::<SongEditorState>();
    assert_eq!(st.get_caret() as usize, LYRICS.find("v2").unwrap(), "the caret at Verse 2's first line");
    assert!(st.get_live_song(), "the editor warns the song is on screen");
}

#[test]
fn edit_words_caret_follows_item_override() {
    let app = test_app();
    let (song_id, id) = add_song(&app, LYRICS, &[]);
    if let Some(it) = app.data().services[0].items.iter_mut().find(|i| i.id == id) {
        it.kind = ItemKind::Song { song_id, arrangement: Some(vec!["Verse 2".into(), "Chorus".into()]) };
    }
    let op = opened(&app);
    show(&op, id, 0);
    assert_eq!(op.ui.get_live_song().line.as_str(), "Verse 2 · slide 1 of 1", "this service starts with Verse 2");
    click(&op, "LiveColumn::live-edit-words");
    click(&op, "SlideEditor::editor-song");
    assert_eq!(op.ui.global::<SongEditorState>().get_caret() as usize, LYRICS.find("v2").unwrap());
}

#[test]
fn edit_words_hidden_for_non_song() {
    let app = test_app();
    let id = add_text(&app);
    let op = opened(&app);
    show(&op, id, 0);
    assert!(!op.ui.get_live_song().edit);
    assert!(find(&op, "LiveColumn::live-edit-words").is_none());
}

#[test]
fn save_changes_words_on_screen_shows_banner_and_update_screen_shows_them() {
    let app = test_app();
    let (song_id, id) = add_song(&app, LYRICS, &[]);
    let op = opened(&app);
    show(&op, id, 1);
    let frame = app.live_view().frame;
    set_lyrics(&app, song_id, &LYRICS.replace("\nc\n", "\nc fixed\n"));
    live(&op);
    let s = op.ui.get_live_song();
    assert!(s.banner.starts_with("You changed the words that are on screen."), "{}", s.banner);
    assert!(s.update);
    assert_eq!(app.live_view().frame, frame, "the room is unchanged");
    click(&op, "LiveColumn::live-update-screen");
    assert_eq!(cursor(&app), Some((id, 1)));
    assert!(op.ui.get_live_song().banner.is_empty());
    assert!(matches!(&app.live_view().frame.slide, Some(crate::live::SlideLayer::Text { text, .. }) if text == "c fixed"));
}

#[test]
fn save_unrelated_section_shows_no_banner() {
    let app = test_app();
    let (song_id, id) = add_song(&app, LYRICS, &[]);
    let op = opened(&app);
    show(&op, id, 1);
    let v = app.live_view().version;
    set_lyrics(&app, song_id, &LYRICS.replace("v2", "v2 fixed"));
    live(&op);
    assert!(op.ui.get_live_song().banner.is_empty());
    assert_eq!(app.live_view().version, v + 1, "taken in quietly");
}

#[test]
fn save_and_show_now_updates_live() {
    let app = test_app();
    let (song_id, id) = add_song(&app, LYRICS, &[]);
    let op = opened(&app);
    show(&op, id, 1);
    crate::ui::dialogs::song_editor::open_song_editor(&op, Some(song_id), false);
    let st = op.ui.global::<SongEditorState>();
    assert!(st.get_live_song());
    st.set_lyrics(LYRICS.replace("\nc\n", "\nc fixed\n").into());
    st.invoke_save_and_show();
    live(&op);
    assert_eq!(op.dialog(), None);
    assert!(matches!(&app.live_view().frame.slide, Some(crate::live::SlideLayer::Text { text, .. }) if text == "c fixed"));
    assert_eq!(app.live_view().changed, None);
}

#[test]
fn editor_for_another_song_has_no_live_banner() {
    let app = test_app();
    let (_, id) = add_song(&app, LYRICS, &[]);
    let (other, _) = add_song(&app, LYRICS, &[]);
    let op = opened(&app);
    show(&op, id, 0);
    crate::ui::dialogs::song_editor::open_song_editor(&op, Some(other), false);
    assert!(!op.ui.global::<SongEditorState>().get_live_song());
    assert!(find(&op, "SongEditor::save-and-show").is_none());
}
