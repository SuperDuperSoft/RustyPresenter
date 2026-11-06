//! The Add dialog's Video & media and Slides tabs (`addMedia()` and `addDeck()`, `operator.js:447-500`): the media
//! and slides libraries, newest first, with a small picture of each, and their Upload / Import buttons.
use super::{AddTab, add::AddCtx, add::search_key, add_strings as tr, confirm};
use crate::{
    model::{Background, MediaFile, MediaKind, MediaStatus},
    ui::{
        AddRow, AddState,
        operator::{file_input::Filter, files, strings::kind_name, view::OperatorSnapshot, window::Operator},
        thumbs::{ThumbJob, ThumbKey, thumb_size},
    },
};
use serde_json::{Value, json};
use slint::{ComponentHandle, Image, Model};
use std::rc::Rc;
use uuid::Uuid;

/// `.thumbmini`'s width in logical pixels.
const THUMB_W: f32 = 96.0;

/// The rows, the search placeholder and the pictures to draw (row id, asset) of the Media or Slides tab.
pub(crate) fn rows(snap: &OperatorSnapshot, tab: AddTab) -> (Vec<AddRow>, String, Vec<(String, String)>) {
    match tab {
        AddTab::Media => {
            let media = &snap.data.media;
            let rows = media.iter().rev().map(|m| media_row(m, snap.player_missing)).collect();
            let pictures = media.iter().rev().filter(|m| m.kind == MediaKind::Image).map(|m| (m.id.to_string(), m.asset.clone())).collect();
            (rows, tr::search_files(media.len()), pictures)
        }
        AddTab::Deck => {
            let decks = &snap.data.decks;
            let rows = decks.iter().rev().map(|d| AddRow {
                id: d.id.to_string().into(), title: d.title.as_str().into(), subtitle: tr::slide_count(d.slides.len()).into(),
                can_delete: true, key: search_key(d), ..Default::default()
            }).collect();
            let pictures = decks.iter().rev().filter_map(|d| Some((d.id.to_string(), d.slides.first()?.clone()))).collect();
            (rows, tr::SEARCH_SLIDES.into(), pictures)
        }
        _ => (Vec::new(), String::new(), Vec::new()),
    }
}

/// Asks for each row's picture, 96 px wide, covering 16:9 (`object-fit: cover`). Each is set on its row by id when
/// it arrives, since the list may have been rebuilt by then. Called once the rows are in the model.
pub(crate) fn request_pictures(op: &Operator, ctx: &AddCtx, pictures: Vec<(String, String)>) {
    let size = thumb_size(THUMB_W, op.window().scale_factor());
    let thumbs = ctx.thumbs(op);
    thumbs.cancel_pending();
    for (id, asset) in pictures {
        let job = ThumbJob { background: Some(Background::Image { asset }), media: None, picture: None, text: None };
        let key = ThumbKey { render: None, layers: serde_json::to_string(&job).unwrap_or_default(), size };
        let all = ctx.all.clone();
        thumbs.request(key, job, move |px| {
            let Some(i) = all.iter().position(|r| r.id == id.as_str()) else { return };
            let Some(mut row) = all.row_data(i) else { return };
            row.thumb = Image::from_rgba8(px);
            row.thumb_shown = true;
            all.set_row_data(i, row);
        });
    }
}

/// One media file: its kind (or the missing-player note), its status chip and its buttons.
fn media_row(m: &MediaFile, player_missing: bool) -> AddRow {
    let kind = match m.kind { MediaKind::Video => "video", MediaKind::Image => "image", MediaKind::Audio => "audio" };
    let subtitle = if player_missing && m.kind != MediaKind::Image { tr::PLAYER_MISSING.to_string() } else { kind_name(kind).to_string() };
    let (chip, state, tip) = status_chip(&m.status);
    AddRow {
        id: m.id.to_string().into(), title: m.name.as_str().into(), subtitle: subtitle.into(),
        thumb_icon: if m.kind == MediaKind::Image { "" } else { kind }.into(),
        chip: chip.into(), chip_state: state.into(), chip_tip: tip.into(),
        can_convert: matches!(m.status, MediaStatus::Unconverted { .. } | MediaStatus::Failed { .. }),
        can_delete: true, delete_tip: tr::DELETE_FROM_LIBRARY.into(), key: search_key(m), ..Default::default()
    }
}

/// `statusChip()` (`operator.js:458-464`): the text, its colour and the reason as a tooltip.
pub fn status_chip(s: &MediaStatus) -> (String, &'static str, String) {
    match s {
        MediaStatus::Ready => (tr::READY.into(), "ok", String::new()),
        MediaStatus::Converting { percent } => (tr::converting_percent(*percent), "warn", String::new()),
        MediaStatus::Unconverted { reason } => (tr::NEEDS_CONVERTER.into(), "warn", reason.clone()),
        MediaStatus::Failed { reason } => (tr::FAILED.into(), "bad", reason.clone()),
    }
}

/// Convert (`operator.js:482`): `media/convert`, then "Converting…".
fn convert(op: &Rc<Operator>, id: Uuid) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("media/convert", json!({ "id": id }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r { Ok(_) => op.toast(tr::CONVERTING, false), Err(e) => op.toast(e, true) }
    });
}

/// The trash button: asks first, then `media/delete` or `deck/delete`. The core refuses one still in a service.
fn delete(op: &Rc<Operator>, tab: AddTab, id: Uuid) {
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let (question, action) = match tab {
        AddTab::Media => match snap.data.media.iter().find(|m| m.id == id) {
            Some(m) => (tr::delete_from_library(&m.name), "media/delete"),
            None => return,
        },
        AddTab::Deck => match snap.data.decks.iter().find(|d| d.id == id) {
            Some(d) => (tr::delete_deck(&d.title), "deck/delete"),
            None => return,
        },
        _ => return,
    };
    let me = Rc::downgrade(op);
    confirm(op, &question, tr::OK, tr::CANCEL, true, move |yes| {
        let Some(op) = me.upgrade().filter(|_| yes) else { return };
        let me = Rc::downgrade(&op);
        op.ops().call::<Value>(action, json!({ "id": id }), move |r| {
            if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(e, true) }
        });
    });
}

/// Upload files, Import PowerPoint and Make slides from pictures go through the file picker.
fn pick_then(op: &Rc<Operator>, filter: Filter, multi: bool, then: fn(&Rc<Operator>, Vec<std::path::PathBuf>)) {
    let me = Rc::downgrade(op);
    files::pick(op, filter, multi, move |paths| if let Some(op) = me.upgrade() { then(&op, paths) });
}

/// The two tabs' callbacks on `AddState`.
pub(crate) fn wire(op: &Rc<Operator>, ctx: &Rc<AddCtx>) {
    let st = op.ui.global::<AddState>();
    let c = ctx.clone();
    st.on_convert(move |id| if let (Some(op), Ok(id)) = (c.op.upgrade(), Uuid::parse_str(&id)) { convert(&op, id) });
    let c = ctx.clone();
    st.on_delete_row(move |id| {
        let (Some(op), Ok(id)) = (c.op.upgrade(), Uuid::parse_str(&id)) else { return };
        if let Some(super::DialogKind::Add(tab)) = op.dialog() { delete(&op, tab, id) }
    });
    let c = ctx.clone();
    st.on_upload_media(move || if let Some(op) = c.op.upgrade() { pick_then(&op, Filter::Media, true, files::add_files) });
    let c = ctx.clone();
    st.on_import_powerpoint(move || if let Some(op) = c.op.upgrade() { pick_then(&op, Filter::PowerPoint, true, files::add_files) });
    let c = ctx.clone();
    st.on_pictures_to_slides(move || if let Some(op) = c.op.upgrade() { pick_then(&op, Filter::Pictures, true, files::images_to_deck) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        model::{Deck, ItemKind, ServiceItem},
        ui::{
            dialogs::{DialogKind, add::{open_add, tests::*}},
            operator::window::testing::{find_all, refresh},
        },
    };
    use slint::platform::Key;
    use std::sync::Arc;

    fn media(app: &App, name: &str, kind: MediaKind, status: MediaStatus) -> Uuid {
        let m = MediaFile { id: Uuid::new_v4(), name: name.into(), kind, asset: format!("{name}.bin"), status, original_asset: None, folder: String::new() };
        app.data.lock().unwrap().media.push(m.clone());
        m.id
    }

    fn library(app: &App) -> Vec<String> { app.data.lock().unwrap().media.iter().map(|m| m.name.clone()).collect() }

    fn question(op: &Operator) -> Option<String> { op.ui.get_ask().row_data(0).map(|a| a.text.to_string()) }

    fn chips(op: &Operator) -> Vec<(String, String, String)> {
        find_all(op, "AddListRow::chip").iter().map(|c| (
            c.accessible_label().unwrap_or_default().to_string(),
            c.accessible_value().unwrap_or_default().to_string(),
            c.accessible_description().unwrap_or_default().to_string(),
        )).collect()
    }

    fn four(app: &App) {
        media(app, "pic.png", MediaKind::Image, MediaStatus::Ready);
        media(app, "clip.mp4", MediaKind::Video, MediaStatus::Converting { percent: 40 });
        media(app, "song.wma", MediaKind::Audio, MediaStatus::Unconverted { reason: "No converter found.".into() });
        media(app, "old.avi", MediaKind::Video, MediaStatus::Failed { reason: "Broken file.".into() });
    }

    #[test]
    fn media_rows_newest_first_with_status_chips() {
        let app = test_app();
        four(&app);
        let op = opened(&app);
        open_add(&op, AddTab::Media);
        settle(&op);
        assert_eq!(titles(&op), ["old.avi", "song.wma", "clip.mp4", "pic.png"]);
        let st = op.ui.global::<AddState>();
        assert_eq!(st.get_placeholder(), "Search 4 files…");
        let subtitles: Vec<String> = shown(&op).iter().map(|r| r.subtitle.to_string()).collect();
        assert_eq!(subtitles, ["Video", "Audio", "Video", "Picture"]);
        assert_eq!(chips(&op), [
            ("Failed".into(), "bad".into(), "Broken file.".into()),
            ("Needs converter".into(), "warn".into(), "No converter found.".into()),
            ("Converting 40%".into(), "warn".into(), String::new()),
            ("Ready".into(), "ok".into(), String::new()),
        ]);
        let rows = shown(&op);
        assert!(rows[3].thumb_shown, "a picture gets its thumbnail");
        assert_eq!((rows[0].thumb_icon.as_str(), rows[1].thumb_icon.as_str()), ("video", "audio"), "video and audio show an icon");
        assert!(rows.iter().all(|r| r.icon.is_empty() && r.can_delete && r.delete_tip == "Delete from library"));
        // Search still matches any field.
        st.invoke_search_edited("WMA".into());
        assert_eq!(titles(&op), ["song.wma"]);
    }

    #[test]
    fn media_row_says_player_missing() {
        let app = test_app();
        four(&app);
        let op = opened(&app);
        open_add(&op, AddTab::Media);
        op.apply_snapshot(Arc::new(crate::ui::operator::view::OperatorSnapshot::read(&app).with_player_missing(true)));
        let subtitles: Vec<String> = shown(&op).iter().map(|r| r.subtitle.to_string()).collect();
        let missing = "The video player is missing — reinstall RustyPresenter";
        assert_eq!(subtitles, [missing, missing, missing, "Picture"]);
    }

    #[test]
    fn delete_media_asks_then_calls_core() {
        let app = test_app();
        media(&app, "pic.png", MediaKind::Image, MediaStatus::Ready);
        let op = opened(&app);
        open_add(&op, AddTab::Media);
        settle(&op);
        click(&op, "AddListRow::delete-btn");
        assert_eq!(question(&op).as_deref(), Some("Delete “pic.png” from the library?"));
        press(&op, char::from(Key::Escape));
        settle(&op);
        assert_eq!(library(&app), ["pic.png"], "No: nothing is deleted");
        assert_eq!(op.dialog(), Some(DialogKind::Add(AddTab::Media)), "and the dialog stays");

        click(&op, "AddListRow::delete-btn");
        press(&op, char::from(Key::Return));
        settle(&op);
        assert!(library(&app).is_empty(), "Yes: deleted");
        refresh(&op);
        assert!(titles(&op).is_empty());
        assert_eq!(op.ui.global::<AddState>().get_empty(), "Nothing here yet.");

        // One still in a service: the core refuses, in its own words.
        let id = media(&app, "clip.mp4", MediaKind::Video, MediaStatus::Ready);
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Media { media_id: id, autoplay: true, end: Default::default(), looped: false }, background: None, notes: String::new() };
        app.data.lock().unwrap().services[0].items.push(item);
        refresh(&op);
        settle(&op);
        click(&op, "AddListRow::delete-btn");
        press(&op, char::from(Key::Return));
        settle(&op);
        assert_eq!(last_toast(&op), Some(("This is in a service. Remove it from the service first.".into(), true)));
        assert_eq!(library(&app), ["clip.mp4"]);
    }

    #[test]
    fn convert_button_only_for_unconverted_or_failed() {
        let app = test_app();
        four(&app);
        let op = opened(&app);
        open_add(&op, AddTab::Media);
        settle(&op);
        let can: Vec<bool> = shown(&op).iter().map(|r| r.can_convert).collect();
        assert_eq!(can, [true, true, false, false], "failed and unconverted only");
        assert_eq!(find_all(&op, "AddListRow::convert-btn").len(), 2);
    }

    #[test]
    fn add_media_adds_with_web_defaults_and_closes() {
        let app = test_app();
        let id = media(&app, "clip.mp4", MediaKind::Video, MediaStatus::Ready);
        let op = opened(&app);
        open_add(&op, AddTab::Media);
        settle(&op);
        click(&op, "AddListRow::add-btn");
        assert!(matches!(items(&app)[..], [ItemKind::Media { media_id, autoplay: true, looped: false, .. }] if media_id == id));
        assert_eq!(op.dialog(), None);
        assert_eq!(last_toast(&op), Some(("Added to the service".into(), false)));
    }

    #[test]
    fn deck_rows_show_slide_count() {
        let app = test_app();
        let old = Deck { id: Uuid::new_v4(), title: "Easter".into(), slides: vec!["a.png".into()] };
        let new = Deck { id: Uuid::new_v4(), title: "Missions".into(), slides: vec!["b.png".into(), "c.png".into(), "d.png".into()] };
        app.data.lock().unwrap().decks.extend([old, new.clone()]);
        let op = opened(&app);
        open_add(&op, AddTab::Deck);
        settle(&op);
        assert_eq!(op.ui.global::<AddState>().get_placeholder(), "Search slides…");
        let rows: Vec<(String, String)> = shown(&op).iter().map(|r| (r.title.to_string(), r.subtitle.to_string())).collect();
        assert_eq!(rows, [("Missions".into(), "3 slides".into()), ("Easter".into(), "1 slide".into())], "newest first");
        assert!(shown(&op).iter().all(|r| r.thumb_shown), "the first slide's picture");
        click(&op, "AddListRow::delete-btn");
        assert_eq!(question(&op).as_deref(), Some("Delete “Missions”?"));
        press(&op, char::from(Key::Escape));
        settle(&op);
        click(&op, "AddListRow::add-btn");
        assert!(matches!(items(&app)[..], [ItemKind::Deck { deck_id }] if deck_id == new.id));
        assert_eq!(op.dialog(), None);
    }
}
