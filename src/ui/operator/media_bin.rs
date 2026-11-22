//! The bottom library pane: saved songs, media and video inputs. Media folders may exist before they contain a file.
use super::{add_item::add_item, file_input::Filter as FileFilter, files, strings as tr, window::Operator};
use crate::{
    app::Imported,
    model::{AssetId, Background, MediaFile, MediaKind, MediaStatus},
    ui::{AddRow, BinFolder, BinRow, dialogs::{add, add_strings, ask, song_editor::open_song_editor}, thumbs::{ThumbJob, ThumbKey, Thumbs}},
};
use serde_json::{Value, json};
use slint::{Image, Model, ModelRc};
use std::{collections::{HashMap, VecDeque}, path::PathBuf, rc::{Rc, Weak}};
use uuid::Uuid;

/// Thumbnails are drawn at the largest tile's size, twice over for high-DPI screens.
const THUMB: (u32, u32) = (420, 236);
const SONGS: &str = "songs";

/// The library's choices between updates.
#[derive(Default)]
pub struct BinState {
    /// The active tab; empty counts as "all".
    pub(super) tab: String,
    /// The folder shown; empty is "All media".
    folder: String,
    pub(super) query: String,
    song_query: String,
    songs: Vec<AddRow>,
    /// The snapshot's song rows `songs` was made from: the same ones again are not made into rows again.
    songs_from: Option<std::sync::Arc<Vec<crate::ui::operator::view::SongRow>>>,
    pub(super) picked: Option<Uuid>,
    images: HashMap<AssetId, Image>,
    on_screen: Option<AssetId>,
}

/// What a tab shows.
pub fn tab_matches(tab: &str, kind: MediaKind) -> bool {
    match tab {
        "video" => kind == MediaKind::Video,
        "image" => kind == MediaKind::Image,
        "audio" => kind == MediaKind::Audio,
        _ => true,
    }
}

/// The files to show, newest first: right tab, right folder, and the name holds the search words in any order.
pub fn shown<'a>(media: &'a [MediaFile], tab: &str, folder: &str, query: &str) -> Vec<&'a MediaFile> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    media.iter().rev()
        .filter(|m| tab_matches(tab, m.kind))
        .filter(|m| folder.is_empty() || m.folder == folder)
        .filter(|m| { let name = m.name.to_lowercase(); words.iter().all(|w| name.contains(w.as_str())) })
        .collect()
}

/// The folder list: "All media", then every named folder A–Z with its file count. A folder named only on a file still
/// shows, so no file can be stranded.
pub fn folder_counts(media: &[MediaFile], listed: &[String]) -> Vec<(String, usize)> {
    let mut names: Vec<String> = listed.iter().chain(media.iter().map(|m| &m.folder)).filter(|n| !n.is_empty()).cloned().collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    let mut out = vec![(String::new(), media.len())];
    out.extend(names.into_iter().map(|n| { let c = media.iter().filter(|m| m.folder == n).count(); (n, c) }));
    out
}

/// The small label on a tile.
fn tag(m: &MediaFile) -> String {
    match (&m.status, m.kind) {
        (MediaStatus::Converting { percent }, _) => tr::converting_short(*percent),
        (MediaStatus::Failed { .. }, _) => tr::media_failed().into(),
        (_, MediaKind::Video) => tr::media_video().into(),
        (_, MediaKind::Image) => tr::media_picture().into(),
        (_, MediaKind::Audio) => tr::media_audio().into(),
    }
}

/// The picture a tile shows: a picture itself, a video's grabbed frame; audio has none.
pub fn tile_asset(m: &MediaFile) -> Option<AssetId> {
    match m.kind {
        MediaKind::Image => Some(m.asset.clone()),
        MediaKind::Video => m.thumb.clone(),
        MediaKind::Audio => None,
    }
}

/// What this file becomes as a background; `None` for audio and for files not ready to play.
pub fn as_background(m: &MediaFile) -> Option<Background> {
    if m.status != MediaStatus::Ready { return None }
    match m.kind {
        MediaKind::Image => Some(Background::Image { asset: m.asset.clone(), dim: 0 }),
        MediaKind::Video => Some(Background::Video { asset: m.asset.clone(), dim: 0 }),
        MediaKind::Audio => None,
    }
}

impl Operator {
    fn bin_media(&self) -> Option<Vec<MediaFile>> {
        self.state.borrow().snap.as_ref().map(|s| s.data.media.clone())
    }

    pub(super) fn picked(&self) -> Option<MediaFile> {
        let id = self.bin.borrow().picked?;
        self.bin_media()?.into_iter().find(|m| m.id == id)
    }

    /// The asset the room's current item uses as its background, for the "ON SCREEN" mark.
    fn bin_on_screen(&self) -> Option<AssetId> {
        let live = self.live_item()?;
        let st = self.state.borrow();
        let item = st.snap.as_ref()?.items.iter().find(|i| i.id == live)?;
        match item.item.background.as_ref()? {
            Background::Image { asset, .. } | Background::Video { asset, .. } => Some(asset.clone()),
            Background::Camera { device, .. } => Some(super::video_inputs::on_screen_mark(device)),
            Background::Color { .. } | Background::Gradient { .. } => None,
        }
    }

    /// Redraws the folders, the tiles and the side panel from the current snapshot.
    pub(super) fn render_media(&self) {
        if self.bin.borrow().tab == SONGS {
            let Some(snap) = self.state.borrow().snap.clone() else { return };
            {
                let mut bin = self.bin.borrow_mut();
                if !bin.songs_from.as_ref().is_some_and(|from| std::sync::Arc::ptr_eq(from, &snap.songs)) {
                    bin.songs = add::add_rows(&snap.songs);
                    bin.songs_from = Some(snap.songs.clone());
                }
            }
            return self.render_songs();
        }
        let Some((media, listed)) = self.state.borrow().snap.as_ref().map(|s| (s.data.media.clone(), s.data.media_folders.clone())) else { return };
        let on_screen = self.bin_on_screen();
        {
            let mut b = self.bin.borrow_mut();
            // A folder that is gone falls back to everything.
            if !b.folder.is_empty() && !folder_counts(&media, &listed).iter().any(|(n, _)| *n == b.folder) { b.folder.clear(); }
            if b.tab != super::video_inputs::TAB && b.picked.is_some_and(|id| !media.iter().any(|m| m.id == id)) { b.picked = None; }
            b.on_screen = on_screen.clone();
        }
        // N11: the Video inputs tab lists cameras, not files.
        if self.bin.borrow().tab == super::video_inputs::TAB {
            let rows = self.input_rows(on_screen.as_deref());
            let folders: Vec<BinFolder> = folder_counts(&media, &listed).into_iter().map(|(name, count)| BinFolder {
                label: if name.is_empty() { tr::all_media() } else { name.clone() }.into(), on: false, count: count as i32, name: name.as_str().into(),
            }).collect();
            self.bin_rows.set_vec(rows);
            self.bin_folders.set_vec(folders);
            return self.render_input_pick();
        }
        let (rows, folders, wanted) = {
            let b = self.bin.borrow();
            let files = shown(&media, &b.tab, &b.folder, &b.query);
            let rows: Vec<BinRow> = files.iter().map(|m| BinRow {
                id: m.id.to_string().into(), name: m.name.as_str().into(),
                kind: match m.kind { MediaKind::Video => "video", MediaKind::Image => "image", MediaKind::Audio => "audio" }.into(),
                tag: tag(m).into(), image: tile_asset(m).and_then(|a| b.images.get(&a).cloned()).unwrap_or_default(),
                picked: b.picked == Some(m.id), on_screen: on_screen.as_ref() == Some(&m.asset),
            }).collect();
            let folders: Vec<BinFolder> = folder_counts(&media, &listed).into_iter().map(|(name, count)| BinFolder {
                label: if name.is_empty() { tr::all_media() } else { name.clone() }.into(),
                on: name == b.folder, count: count as i32, name: name.as_str().into(),
            }).collect();
            let mut wanted: Vec<AssetId> = files.iter().filter_map(|m| tile_asset(m)).filter(|a| !b.images.contains_key(a)).collect();
            wanted.dedup();
            (rows, folders, wanted)
        };
        self.bin_rows.set_vec(rows);
        self.bin_folders.set_vec(folders);
        self.render_pick();
        for asset in wanted { self.draw_tile(asset); }
    }

    /// Moves the "ON SCREEN" mark when the live item changed, without rebuilding the tiles.
    pub(super) fn mark_media(&self) {
        if self.bin.borrow().tab == SONGS { return }
        let now = self.bin_on_screen();
        if self.bin.borrow().on_screen == now { return }
        self.bin.borrow_mut().on_screen = now;
        self.render_media();
    }

    fn render_songs(&self) {
        let bin = self.bin.borrow();
        let query = bin.song_query.trim().to_lowercase();
        let rows: Vec<AddRow> = bin.songs.iter().filter(|song| song.key.contains(query.as_str())).cloned().collect();
        self.ui.set_bin_song_placeholder(add_strings::search_songs(bin.songs.len()).into());
        let empty = if query.is_empty() { add_strings::nothing_yet() } else { add_strings::nothing_matches() };
        self.ui.set_bin_song_empty(empty.into());
        self.ui.set_bin_songs(ModelRc::new(slint::VecModel::from(rows)));
    }

    fn render_pick(&self) {
        if self.bin.borrow().tab == super::video_inputs::TAB { return self.render_input_pick() }
        let m = self.picked();
        let ui = &self.ui;
        ui.set_bin_has_pick(m.is_some());
        ui.set_bin_pick_name(m.as_ref().map_or("", |m| m.name.as_str()).into());
        ui.set_bin_can_use(m.as_ref().is_some_and(|m| as_background(m).is_some()));
        ui.set_bin_pick_hint(match &m {
            None => tr::bin_hint_none(),
            Some(m) if m.kind == MediaKind::Audio => tr::bin_hint_audio(),
            Some(m) if m.status != MediaStatus::Ready => tr::bin_hint_not_ready(),
            Some(_) => tr::bin_hint_use(),
        }.into());
        ui.set_bin_can_delete_folder(!self.bin.borrow().folder.is_empty());
    }

    /// Asks for one picture's thumbnail. Marked as asked for, so a redraw before it arrives does not ask again.
    fn draw_tile(&self, asset: AssetId) {
        self.bin.borrow_mut().images.insert(asset.clone(), Image::default());
        let job = ThumbJob { background: Some(Background::Image { asset: asset.clone(), dim: 0 }), media: None, picture: None, text: None };
        let key = ThumbKey { render: None, layers: serde_json::to_string(&job).unwrap_or_default(), size: THUMB };
        let me = self.me.clone();
        self.bin_thumbs.request(key, job, move |px| {
            let Some(op) = me.upgrade() else { return };
            let image = Image::from_rgba8(px);
            op.bin.borrow_mut().images.insert(asset.clone(), image.clone());
            let Some(media) = op.bin_media() else { return };
            let ids: Vec<String> = media.iter().filter(|m| tile_asset(m).as_ref() == Some(&asset)).map(|m| m.id.to_string()).collect();
            for i in 0..op.bin_rows.row_count() {
                let Some(mut row) = op.bin_rows.row_data(i) else { continue };
                if ids.iter().any(|id| id == row.id.as_str()) {
                    row.image = image.clone();
                    op.bin_rows.set_row_data(i, row);
                }
            }
        });
    }

    /// The chosen file becomes the selected service item's background.
    fn use_picked(self: &Rc<Self>) {
        if self.bin.borrow().tab == super::video_inputs::TAB { return self.use_input() }
        let Some(m) = self.picked() else { return };
        let Some(background) = as_background(&m) else { return };
        let (selected, snap) = { let st = self.state.borrow(); (st.selected, st.snap.clone()) };
        let Some(item) = selected.and_then(|id| snap?.items.iter().find(|i| i.id == id).map(|i| i.item.clone())) else {
            return self.toast(tr::pick_item_first(), false);
        };
        let mut item = item;
        item.background = Some(background);
        let me = Rc::downgrade(self);
        self.ops.call::<Value>("service/update", json!(item), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(_) => op.toast(tr::background_set(&m.name), false),
                Err(e) => op.toast(e, true),
            }
        });
    }

    fn add_picked(self: &Rc<Self>) {
        let Some(m) = self.picked() else { return };
        let me = Rc::downgrade(self);
        add_item(self, json!({ "kind": "media", "media_id": m.id }), false, move |r| {
            if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(e, true) }
        });
    }

    fn folder_action(self: &Rc<Self>, action: &'static str, body: Value) {
        let me = Rc::downgrade(self);
        self.ops.call::<Value>(action, body, move |r| {
            if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(e, true) }
        });
    }

    fn move_picked(self: &Rc<Self>) {
        let Some(m) = self.picked() else { return };
        let me = Rc::downgrade(self);
        ask::prompt(self, &tr::move_to_folder(), &m.folder, move |name| {
            let (Some(op), Some(name)) = (me.upgrade(), name) else { return };
            op.folder_action("media/folder", json!({ "id": m.id, "folder": name.trim() }));
        });
    }

    fn new_folder(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        ask::prompt(self, &tr::new_folder(), "", move |name| {
            let (Some(op), Some(name)) = (me.upgrade(), name) else { return };
            op.folder_action("media/folder-new", json!({ "name": name.trim() }));
        });
    }

    fn delete_folder(self: &Rc<Self>) {
        let name = self.bin.borrow().folder.clone();
        if name.is_empty() { return }
        let me = Rc::downgrade(self);
        ask::confirm(self, &tr::delete_folder(&name), &tr::delete_folder_button(), &tr::cancel_action(), true, move |ok| {
            let Some(op) = me.upgrade() else { return };
            if ok { op.folder_action("media/folder-delete", json!({ "name": name })); }
        });
    }

    /// Import: the files go into the library and the folder being shown, not into the service.
    fn import_media(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        // On the Songs tab: lyrics files (SongSelect .txt and the like), as Add to service's Import does.
        if self.bin.borrow().tab == "songs" {
            return files::pick(self, FileFilter::Lyrics, true, move |paths| if let Some(op) = me.upgrade() { files::add_files(&op, paths) });
        }
        let folder = self.bin.borrow().folder.clone();
        files::pick(self, FileFilter::Media, true, move |paths| {
            if let Some(op) = me.upgrade() { import_next(&op, paths.into(), folder) }
        });
    }
}

/// One file at a time, so the Uploading cards stay in order; a failure toasts and the next file goes on.
fn import_next(op: &Rc<Operator>, mut rest: VecDeque<PathBuf>, folder: String) {
    let Some(path) = rest.pop_front() else { return };
    let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();
    let job = op.client_job(tr::uploading(&name));
    let progress = job.progress();
    let purpose = files::route(&path).purpose();
    let me = Rc::downgrade(op);
    op.ops.import(path, purpose, progress, move |r| {
        drop(job);
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(Imported::Media(m)) => {
                if !folder.is_empty() { op.folder_action("media/folder", json!({ "id": m.id, "folder": folder })); }
                op.toast(tr::media_added(&name), false);
            }
            Ok(_) => {}
            Err(e) => op.toast(tr::file_failed(&name, &e), true),
        }
        import_next(&op, rest, folder);
    });
}

pub(super) fn wire(op: &Rc<Operator>) {
    let ui = &op.ui;
    ui.set_bin_rows(ModelRc::from(op.bin_rows.clone()));
    ui.set_bin_folders(ModelRc::from(op.bin_folders.clone()));
    super::video_inputs::wire(op);
    let on = |f: fn(&Rc<Operator>)| { let me: Weak<Operator> = Rc::downgrade(op); move || if let Some(op) = me.upgrade() { f(&op) } };
    ui.on_bin_use_picked(on(Operator::use_picked));
    ui.on_bin_add_picked(on(Operator::add_picked));
    ui.on_bin_move_picked(on(Operator::move_picked));
    ui.on_bin_new_folder(on(Operator::new_folder));
    ui.on_bin_delete_folder(on(Operator::delete_folder));
    ui.on_bin_import(on(Operator::import_media));
    let me = Rc::downgrade(op);
    ui.on_bin_set_tab(move |t| if let Some(op) = me.upgrade() {
        op.ui.set_bin_tab(t.clone());
        op.bin.borrow_mut().tab = t.to_string();
        let mut state = op.state.borrow_mut();
        if t == SONGS { state.list = super::window::ListFocus::Songs; }
        else if state.list == super::window::ListFocus::Songs { state.list = super::window::ListFocus::Live; }
        drop(state);
        op.render_media();
        if t == super::video_inputs::TAB { op.inputs_opened() }
    });
    let me = Rc::downgrade(op);
    ui.on_bin_set_folder(move |f| if let Some(op) = me.upgrade() { op.bin.borrow_mut().folder = f.into(); op.render_media(); });
    let me = Rc::downgrade(op);
    ui.on_bin_search(move |q| if let Some(op) = me.upgrade() { op.bin.borrow_mut().query = q.into(); op.render_media(); });
    let me = Rc::downgrade(op);
    ui.on_bin_pick(move |id| if let Some(op) = me.upgrade() {
        op.bin.borrow_mut().picked = id.parse().ok();
        op.state.borrow_mut().list = super::window::ListFocus::Media;
        op.render_media();
    });
    ui.on_bin_new_song(on(|op| open_song_editor(op, None, false)));
    let me = Rc::downgrade(op);
    ui.on_bin_song_search(move |query| if let Some(op) = me.upgrade() {
        op.bin.borrow_mut().song_query = query.into();
        op.render_songs();
    });
    let me = Rc::downgrade(op);
    ui.on_bin_edit_song(move |id| if let (Some(op), Ok(id)) = (me.upgrade(), Uuid::parse_str(&id)) {
        open_song_editor(&op, Some(id), false);
    });
    let me = Rc::downgrade(op);
    ui.on_bin_add_song(move |id| if let (Some(op), Ok(id)) = (me.upgrade(), Uuid::parse_str(&id)) {
        let me = Rc::downgrade(&op);
        add_item(&op, json!({ "kind": "song", "song_id": id }), false, move |result| {
            if let (Some(op), Err(error)) = (me.upgrade(), result) { op.toast(error, true); }
        });
    });
}

/// Keeps the thumbnails' worker to this file's type alias.
pub(super) fn new_thumbs(root: std::path::PathBuf, inline: bool) -> Thumbs { if inline { Thumbs::inline(root) } else { Thumbs::new(root) } }

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, kind: MediaKind, folder: &str) -> MediaFile {
        MediaFile { id: Uuid::new_v4(), name: name.into(), kind, asset: format!("{name}.bin"), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: folder.into() }
    }

    fn sample() -> Vec<MediaFile> {
        vec![file("Blue Waves", MediaKind::Video, "Loops"), file("Mountain Dawn", MediaKind::Image, "Stills"), file("Prelude", MediaKind::Audio, ""), file("Soft Waves", MediaKind::Video, "Loops")]
    }

    fn names(f: Vec<&MediaFile>) -> Vec<String> { f.into_iter().map(|m| m.name.clone()).collect() }

    #[test]
    fn newest_first_and_tabs_filter_by_kind() {
        let m = sample();
        assert_eq!(names(shown(&m, "", "", "")), ["Soft Waves", "Prelude", "Mountain Dawn", "Blue Waves"]);
        assert_eq!(names(shown(&m, "video", "", "")), ["Soft Waves", "Blue Waves"]);
        assert_eq!(names(shown(&m, "image", "", "")), ["Mountain Dawn"]);
        assert_eq!(names(shown(&m, "audio", "", "")), ["Prelude"]);
    }

    #[test]
    fn folder_and_search_narrow_together() {
        let m = sample();
        assert_eq!(names(shown(&m, "all", "Stills", "")), ["Mountain Dawn"]);
        assert_eq!(names(shown(&m, "all", "Loops", "blue")), ["Blue Waves"]);
        assert_eq!(names(shown(&m, "all", "", "waves soft")), ["Soft Waves"]);
        assert!(shown(&m, "audio", "Loops", "").is_empty());
    }

    #[test]
    fn folders_list_all_media_first_then_a_to_z_with_counts() {
        let m = sample();
        let got = folder_counts(&m, &["Empty".to_string(), "Loops".to_string()]);
        assert_eq!(got, vec![(String::new(), 4), ("Empty".into(), 0), ("Loops".into(), 2), ("Stills".into(), 1)]);
    }

    #[test]
    fn only_ready_pictures_and_videos_can_be_backgrounds() {
        let v = file("v", MediaKind::Video, "");
        assert_eq!(as_background(&v), Some(Background::Video { asset: "v.bin".into(), dim: 0 }));
        assert_eq!(as_background(&file("a", MediaKind::Audio, "")), None);
        let mut busy = file("i", MediaKind::Image, "");
        busy.status = MediaStatus::Converting { percent: 10 };
        assert_eq!(as_background(&busy), None);
    }

    #[test]
    fn the_bottom_songs_tab_searches_and_uses_the_saved_library() {
        use crate::{app::test_app, model::{ItemKind, Song}, ui::{SongEditorState, dialogs::DialogKind}};
        use super::super::window::testing::{find, open, refresh};
        use slint::{ComponentHandle, platform::PointerEventButton};

        let app = test_app();
        let grace = Uuid::new_v4();
        app.data.lock().unwrap().songs = vec![
            Song { id: grace, title: "Amazing Grace".into(), writers: "John Newton".into(), lyrics: "Verse 1\nAmazing grace".into(), ..Default::default() },
            Song { id: Uuid::new_v4(), title: "Abide with Me".into(), ..Default::default() },
        ];
        let op = open(&app);
        op.window().set_size(slint::LogicalSize::new(1440.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        let click = |id| {
            find(&op, id).unwrap_or_else(|| panic!("{id} not found")).mock_single_click(PointerEventButton::Left);
            slint::platform::update_timers_and_animations();
        };
        find(&op, "MediaBin::songs-tab").unwrap().invoke_accessible_default_action();
        slint::platform::update_timers_and_animations();
        let titles = || op.ui.get_bin_songs().iter().map(|row| row.title.to_string()).collect::<Vec<_>>();
        assert_eq!(titles(), ["Abide with Me", "Amazing Grace"]);
        let search = find(&op, "MediaBin::song-search-field").unwrap();
        search.set_accessible_value("newton");
        slint::platform::update_timers_and_animations();
        assert_eq!(titles(), ["Amazing Grace"]);

        click("MediaBin::song-add");
        assert!(matches!(app.data.lock().unwrap().service().unwrap().items.first().map(|item| &item.kind),
            Some(ItemKind::Song { song_id, .. }) if *song_id == grace), "{:?}", op.toast_log());
        assert!(app.live_view().cursor.is_none());
        click("MediaBin::song-edit");
        assert_eq!(op.dialog(), Some(DialogKind::SongEditor { song_id: Some(grace), add_after: false }));
        op.close_dialog();
        click("MediaBin::new-song-button");
        let editor = op.ui.global::<SongEditorState>();
        editor.set_title("Evening Hymn".into());
        editor.set_lyrics("Verse 1\nThe day is done".into());
        click("SongEditor::save");
        refresh(&op);
        search.set_accessible_value("");
        assert_eq!(titles(), ["Abide with Me", "Amazing Grace", "Evening Hymn"]);
        assert_eq!(app.data.lock().unwrap().service().unwrap().items.len(), 1);
    }
}
