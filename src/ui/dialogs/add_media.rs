//! The Add dialog's Video & media and Slides tabs (`addMedia()` and `addDeck()`, `operator.js:447-500`): the media
//! and slides libraries, newest first, with a small picture of each, and their Upload / Import buttons.
use super::{AddTab, add::AddCtx, add::search_key, add_strings as tr, confirm, settings};
use crate::{
    import::{deck::{ConverterStatus, Lo}, strings::LO_SNAP},
    model::{Background, MediaFile, MediaKind, MediaStatus},
    platform::tools::{OS, install_command},
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
            let pictures = media.iter().rev().filter_map(|m| Some((m.id.to_string(), row_picture(m)?))).collect();
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

/// What a media row shows: the small saved picture, or a whole picture until `reprobe` has made that.
fn row_picture(m: &MediaFile) -> Option<String> {
    m.thumb.clone().or_else(|| (m.kind == MediaKind::Image).then(|| m.asset.clone()))
}

/// One media file: its kind (or the missing-player note), its status chip and its buttons.
fn media_row(m: &MediaFile, player_missing: bool) -> AddRow {
    let kind = match m.kind { MediaKind::Video => "video", MediaKind::Image => "image", MediaKind::Audio => "audio" };
    let subtitle = if player_missing && m.kind != MediaKind::Image { tr::PLAYER_MISSING.to_string() } else { kind_name(kind).to_string() };
    let (chip, state, tip) = status_chip(&m.status);
    AddRow {
        id: m.id.to_string().into(), title: m.name.as_str().into(), subtitle: subtitle.into(),
        thumb_icon: if row_picture(m).is_some() { "" } else { kind }.into(),
        chip: chip.into(), chip_state: state.into(), chip_tip: tip.into(),
        can_try_again: matches!(m.status, MediaStatus::Failed { .. }),
        can_delete: true, delete_tip: tr::DELETE_FROM_LIBRARY.into(), key: search_key(m), ..Default::default()
    }
}

/// The text, its colour and a tooltip. `Converting` and `Unconverted` only come from an old `data.json`, which
/// `upgrade_media` cleans, so they show as ready.
pub fn status_chip(s: &MediaStatus) -> (String, &'static str, String) {
    match s {
        MediaStatus::Failed { .. } => (tr::FAILED.into(), "bad", tr::CANT_BE_PLAYED.into()),
        _ => (tr::READY.into(), "ok", String::new()),
    }
}

/// Try again: `media/reprobe`, which looks at the file afresh.
fn try_again(op: &Rc<Operator>, id: Uuid) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("media/reprobe", json!({ "id": id }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r { Ok(_) => op.toast(tr::CHECKING_AGAIN, false), Err(e) => op.toast(e, true) }
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

/// The Slides tab's help and the command in it: empty when PowerPoint or a usable LibreOffice is here. A PDF needs
/// neither, but a volunteer with a `.pptx` needs to be told before they try.
pub(crate) fn deck_help(status: Option<ConverterStatus>) -> (&'static str, &'static str) {
    match status {
        Some(ConverterStatus { libreoffice: Lo::Snap, .. }) => (LO_SNAP, ""),
        Some(ConverterStatus { powerpoint: false, libreoffice: Lo::None, .. }) => (tr::INSTALL_LIBREOFFICE, install_command(OS).unwrap_or("")),
        _ => ("", ""),
    }
}

/// Looks at this computer, shows the help, and hands the operator the status.
pub(crate) fn show_deck_help(op: &Rc<Operator>, then: impl FnOnce(&Rc<Operator>, Option<ConverterStatus>) + 'static) {
    let me = Rc::downgrade(op);
    op.ops().converter_status(move |status| {
        let Some(op) = me.upgrade() else { return };
        let (help, command) = deck_help(status);
        let st = op.ui.global::<AddState>();
        st.set_deck_help(help.into());
        st.set_install_command(command.into());
        then(&op, status);
    });
}

/// Upload files, Import presentation and Make slides from pictures go through the file picker.
fn pick_then(op: &Rc<Operator>, filter: Filter, multi: bool, then: fn(&Rc<Operator>, Vec<std::path::PathBuf>)) {
    let me = Rc::downgrade(op);
    files::pick(op, filter, multi, move |paths| if let Some(op) = me.upgrade() { then(&op, paths) });
}

/// The two tabs' callbacks on `AddState`.
pub(crate) fn wire(op: &Rc<Operator>, ctx: &Rc<AddCtx>) {
    let st = op.ui.global::<AddState>();
    let c = ctx.clone();
    st.on_try_again(move |id| if let (Some(op), Ok(id)) = (c.op.upgrade(), Uuid::parse_str(&id)) { try_again(&op, id) });
    let c = ctx.clone();
    st.on_delete_row(move |id| {
        let (Some(op), Ok(id)) = (c.op.upgrade(), Uuid::parse_str(&id)) else { return };
        if let Some(super::DialogKind::Add(tab)) = op.dialog() { delete(&op, tab, id) }
    });
    let c = ctx.clone();
    st.on_upload_media(move || if let Some(op) = c.op.upgrade() { pick_then(&op, Filter::Media, true, files::add_files) });
    let c = ctx.clone();
    st.on_import_deck(move || if let Some(op) = c.op.upgrade() { pick_then(&op, Filter::PowerPoint, true, files::add_files) });
    let c = ctx.clone();
    st.on_copy_install(move || {
        let Some(op) = c.op.upgrade() else { return };
        let command = op.ui.global::<AddState>().get_install_command();
        settings::copy(&op, &command, tr::COPIED);
    });
    let c = ctx.clone();
    st.on_check_libreoffice(move || {
        let Some(op) = c.op.upgrade() else { return };
        show_deck_help(&op, |op, status| {
            let found = status.is_some_and(|s| s.libreoffice == Lo::Ok);
            op.toast(if found { tr::LIBREOFFICE_FOUND } else { tr::LIBREOFFICE_NOT_FOUND }, false);
        });
    });
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
        let m = MediaFile { id: Uuid::new_v4(), name: name.into(), kind, asset: format!("{name}.bin"), status, original_asset: None, duration: None, thumb: None, size: None };
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
        media(app, "clip.mp4", MediaKind::Video, MediaStatus::Ready);
        media(app, "song.wma", MediaKind::Audio, MediaStatus::Ready);
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
            ("Failed".into(), "bad".into(), "This file can't be played.".into()),
            ("Ready".into(), "ok".into(), String::new()),
            ("Ready".into(), "ok".into(), String::new()),
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
    fn media_row_shows_thumb_when_set() {
        let app = test_app();
        let (old, new) = (media(&app, "old.mp4", MediaKind::Video, MediaStatus::Ready), media(&app, "new.mp4", MediaKind::Video, MediaStatus::Ready));
        media(&app, "song.mp3", MediaKind::Audio, MediaStatus::Ready);
        let frame = "frame.png";
        image::RgbaImage::from_pixel(16, 9, image::Rgba([0, 255, 0, 255])).save(app.store.asset_path(frame)).unwrap();
        app.data.lock().unwrap().media.iter_mut().find(|m| m.id == new).unwrap().thumb = Some(frame.into());
        let op = opened(&app);
        open_add(&op, AddTab::Media);
        settle(&op);
        let rows = shown(&op);
        assert_eq!(titles(&op), ["song.mp3", "new.mp4", "old.mp4"]);
        assert!(rows[1].thumb_shown && rows[1].thumb_icon.is_empty(), "the saved frame");
        assert!(!rows[2].thumb_shown && rows[2].thumb_icon == "video", "not probed yet: the icon, as before");
        assert!(!rows[0].thumb_shown && rows[0].thumb_icon == "audio");
        assert_ne!(old, new);
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
    fn try_again_button_only_for_failed() {
        let app = test_app();
        four(&app);
        let op = opened(&app);
        open_add(&op, AddTab::Media);
        settle(&op);
        assert_eq!(find_all(&op, "AddListRow::try-again-btn").len(), 1);
        assert_eq!(find_all(&op, "AddListRow::convert-btn").len(), 0);
        let can: Vec<bool> = shown(&op).iter().map(|r| r.can_try_again).collect();
        assert_eq!(can, [true, false, false, false]);
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

    /// A `Converters` that finds a fake LibreOffice only while `here` is set.
    fn converters(app: &App, here: Arc<std::sync::atomic::AtomicBool>) {
        let dir = app.store.root.clone();
        let soffice = dir.join("soffice");
        std::fs::write(&soffice, "").unwrap();
        let find = Box::new(move || here.load(std::sync::atomic::Ordering::SeqCst).then(|| crate::platform::tools::Soffice::new(soffice.clone())));
        app.set_converters(Arc::new(crate::import::deck::Converters::new(find, None, None, dir)));
    }

    #[test]
    fn deck_help_by_what_this_computer_has() {
        let status = |powerpoint, libreoffice| Some(ConverterStatus { powerpoint, libreoffice, pdfium: true });
        assert_eq!(deck_help(status(true, Lo::None)), ("", ""), "PowerPoint is enough");
        assert_eq!(deck_help(status(false, Lo::Ok)), ("", ""));
        assert_eq!(deck_help(status(false, Lo::None)).0, tr::INSTALL_LIBREOFFICE);
        assert_eq!(deck_help(status(false, Lo::None)).1, install_command(OS).unwrap());
        assert_eq!(deck_help(status(false, Lo::Snap)), (LO_SNAP, ""));
        assert_eq!(deck_help(None), ("", ""), "still starting");
    }

    #[test]
    fn slides_tab_shows_install_hint_copy_and_check_again() {
        let app = test_app();
        let here = Arc::new(std::sync::atomic::AtomicBool::new(false));
        converters(&app, here.clone());
        let op = opened(&app);
        open_add(&op, AddTab::Deck);
        settle(&op);
        let st = op.ui.global::<AddState>();
        assert_eq!(st.get_deck_help(), tr::INSTALL_LIBREOFFICE);
        assert_eq!(st.get_install_command(), install_command(OS).unwrap());
        assert_eq!(find_all(&op, "AddDeckTab::copy").len(), 1);
        click(&op, "AddDeckTab::check");
        settle(&op);
        assert_eq!(last_toast(&op), Some((tr::LIBREOFFICE_NOT_FOUND.into(), false)));
        here.store(true, std::sync::atomic::Ordering::SeqCst);
        click(&op, "AddDeckTab::check");
        settle(&op);
        assert_eq!(last_toast(&op), Some((tr::LIBREOFFICE_FOUND.into(), false)));
        assert_eq!(st.get_deck_help(), "", "the help goes away");
        assert!(find_all(&op, "AddDeckTab::check").is_empty());
    }

    #[test]
    fn slides_tab_button_says_import_presentation() {
        let app = test_app();
        let op = opened(&app);
        open_add(&op, AddTab::Deck);
        settle(&op);
        let on_screen = |text: &str| i_slint_backend_testing::ElementHandle::find_by_accessible_label(&op.ui, text).count();
        assert_eq!((on_screen("Import presentation"), on_screen("Import PowerPoint")), (1, 0));
        assert_eq!(op.ui.global::<AddState>().get_deck_help(), "", "no converters yet: nothing to say");
    }
}
