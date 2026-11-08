//! N32: File → Import and File → Export's newer items. The work is done by the `transfer/*` actions
//! (`crate::transfer`); here are the pickers, the clipboard and the toasts.
use super::{file_input::Filter, files, window::Operator};
use crate::ui::dialogs::{AddTab, add, settings::paste_text, song_editor};
use crate::ui::AddState;
use serde_json::{Value, json};
use slint::ComponentHandle;
use std::{path::PathBuf, rc::Rc};

/// A File → Import or Export item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer { SongFromClipboard, AnnouncementFromClipboard, PptxWords, ThemesIn, Words, Slides, Playlist, ThemesOut }

/// The item of a `menu("…")` id.
pub fn action(id: &str) -> Option<Transfer> {
    Some(match id {
        "import-clipboard-song" => Transfer::SongFromClipboard,
        "import-clipboard-text" => Transfer::AnnouncementFromClipboard,
        "import-pptx-words" => Transfer::PptxWords,
        "import-themes" => Transfer::ThemesIn,
        "export-words" => Transfer::Words,
        "export-slides" => Transfer::Slides,
        "export-playlist" => Transfer::Playlist,
        "export-themes" => Transfer::ThemesOut,
        _ => return None,
    })
}

mod tr {
    use crate::tr;
    pub fn saved_to(path: &str) -> String { tr!("Saved to {path}", path = path) }
    pub fn pictures_saved(n: u64, path: &str) -> String { crate::trn!("Saved {n} picture in {path}", "Saved {n} pictures in {path}", n, path = path) }
    pub fn pptx_added(title: &str, n: u64) -> String {
        crate::trn!("Added “{title}” with {n} slide of words. Pictures and layout are not copied: add it as pictures to keep them.",
            "Added “{title}” with {n} slides of words. Pictures and layout are not copied: add it as pictures to keep them.", n, title = title)
    }
    pub fn themes_added(n: u64) -> String { crate::trn!("Added {n} theme.", "Added {n} themes.", n) }
    pub fn words_file(service: &str) -> String { tr!("{service} words", service = service) }
    pub fn themes_file() -> String { tr!("Themes") }
}

impl Operator {
    pub(super) fn transfer(self: &Rc<Self>, t: Transfer) {
        match t {
            Transfer::SongFromClipboard => match clipboard(self) {
                Some(text) => song_editor::open_new_song(self, crate::transfer::song_from_text(&text), false),
                None => self.toast(crate::transfer::strings::clipboard_empty(), true),
            },
            Transfer::AnnouncementFromClipboard => match clipboard(self) {
                Some(text) => {
                    let (title, body) = crate::transfer::split_title(&text);
                    add::open_add(self, AddTab::Text);
                    let st = self.ui.global::<AddState>();
                    st.set_text_title(title.into());
                    st.set_text_body(body.into());
                }
                None => self.toast(crate::transfer::strings::clipboard_empty(), true),
            },
            Transfer::PptxWords => {
                let me = Rc::downgrade(self);
                files::pick(self, Filter::Pptx, false, move |paths| {
                    let (Some(op), Some(path)) = (me.upgrade(), paths.into_iter().next()) else { return };
                    op.run_transfer("transfer/pptx-words", path, |v| tr::pptx_added(v["title"].as_str().unwrap_or_default(), v["slides"].as_u64().unwrap_or(0)));
                });
            }
            Transfer::ThemesIn => {
                let me = Rc::downgrade(self);
                files::pick(self, Filter::ThemePack, false, move |paths| {
                    let (Some(op), Some(path)) = (me.upgrade(), paths.into_iter().next()) else { return };
                    op.run_transfer("transfer/themes-in", path, |v| tr::themes_added(v["count"].as_u64().unwrap_or(0)));
                });
            }
            Transfer::Words => self.save_as(Filter::Text, &format!("{}.txt", tr::words_file(&self.service_name())), "transfer/words"),
            Transfer::Playlist => self.save_as(Filter::Playlist, &format!("{}.{}", self.service_name(), crate::transfer::PLAYLIST_EXT), "transfer/playlist"),
            Transfer::ThemesOut => self.save_as(Filter::ThemePack, &format!("{}.{}", tr::themes_file(), crate::transfer::THEME_EXT), "transfer/themes-out"),
            Transfer::Slides => {
                let start = dirs::picture_dir().or_else(dirs::home_dir).unwrap_or_default();
                let me = Rc::downgrade(self);
                files::pick_folder(self, &start, move |folder| {
                    let Some(op) = me.upgrade() else { return };
                    op.run_transfer("transfer/slides", folder, |v| tr::pictures_saved(v["count"].as_u64().unwrap_or(0), v["path"].as_str().unwrap_or_default()));
                });
            }
        }
    }

    fn service_name(&self) -> String {
        let name = self.state.borrow().snap.as_ref().map(|s| s.service_name.clone()).unwrap_or_default();
        crate::transfer::safe_name(&name)
    }

    /// The save dialog, then `action` writes the file there.
    fn save_as(self: &Rc<Self>, filter: Filter, name: &str, action: &'static str) {
        let me = Rc::downgrade(self);
        files::pick_save(self, filter, name, move |path| {
            if let Some(op) = me.upgrade() { op.run_transfer(action, path, |v| tr::saved_to(v["path"].as_str().unwrap_or_default())) }
        });
    }

    fn run_transfer(self: &Rc<Self>, action: &'static str, path: PathBuf, done: impl FnOnce(&Value) -> String + 'static) {
        let me = Rc::downgrade(self);
        self.ops().call::<Value>(action, json!({ "path": path }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(v) => op.toast(done(&v), false),
                Err(e) => op.toast(e, true),
            }
        });
    }
}

/// The clipboard's text, when it has some.
fn clipboard(_: &Operator) -> Option<String> { paste_text().filter(|t| !t.trim().is_empty()) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, model::ItemKind, ui::{SongEditorState, dialogs::{DialogKind, services::testing::*, settings::testing::CLIPBOARD}}};

    fn op() -> (std::sync::Arc<crate::app::App>, Rc<Operator>) { let app = test_app(); let op = opened(&app); (app, op) }

    #[test]
    fn every_menu_id_has_an_item() {
        for id in ["import-clipboard-song", "import-clipboard-text", "import-pptx-words", "import-themes", "export-words", "export-slides", "export-playlist", "export-themes"] {
            assert!(action(id).is_some(), "{id}");
            assert!(include_str!("../../../ui/operator.slint").contains(&format!("root.menu(\"{id}\")")), "{id} is in the menu bar");
        }
    }

    #[test]
    fn a_song_from_the_clipboard_opens_in_the_editor() {
        let (_app, op) = op();
        CLIPBOARD.with(|c| *c.borrow_mut() = Some("10,000 Reasons\n\nBless the Lord O my soul".into()));
        op.transfer(Transfer::SongFromClipboard);
        assert!(matches!(op.dialog(), Some(DialogKind::SongEditor { song_id: None, .. })));
        let st = op.ui.global::<SongEditorState>();
        assert_eq!((st.get_title().to_string(), st.get_lyrics().to_string()), ("10,000 Reasons".to_string(), "Bless the Lord O my soul".to_string()));
        CLIPBOARD.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn an_announcement_from_the_clipboard_opens_in_add() {
        let (_app, op) = op();
        CLIPBOARD.with(|c| *c.borrow_mut() = Some("Potluck\nSunday after church".into()));
        op.transfer(Transfer::AnnouncementFromClipboard);
        assert_eq!(op.dialog(), Some(DialogKind::Add(AddTab::Text)));
        let st = op.ui.global::<AddState>();
        assert_eq!((st.get_text_title().to_string(), st.get_text_body().to_string()), ("Potluck".to_string(), "Sunday after church".to_string()));
        CLIPBOARD.with(|c| *c.borrow_mut() = Some("  ".into()));
        op.close_dialog();
        op.transfer(Transfer::AnnouncementFromClipboard);
        assert_eq!(op.dialog(), None, "nothing to paste");
        assert_eq!(op.toast_log().last().unwrap().0, crate::transfer::strings::clipboard_empty());
        CLIPBOARD.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn powerpoint_words_become_an_announcement() {
        use std::io::Write;
        let (app, op) = op();
        let dir = std::env::temp_dir().join(format!("v1-screens-ppt-ui-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Notices.pptx");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        z.start_file("ppt/slides/slide1.xml", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(b"<p:sld><a:p><a:r><a:t>Youth night Friday</a:t></a:r></a:p></p:sld>").unwrap();
        z.finish().unwrap();
        crate::ui::operator::files::testing::will_pick(vec![path]);
        op.transfer(Transfer::PptxWords);
        settle(&op);
        let items: Vec<ItemKind> = app.data.lock().unwrap().service().unwrap().items.iter().map(|i| i.kind.clone()).collect();
        assert!(matches!(&items[..], [ItemKind::Text { title, body }] if title == "Notices" && body == "Youth night Friday"), "{items:?}");
        assert!(op.toast_log().last().unwrap().0.contains("with 1 slide of words"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn export_words_saves_where_picked() {
        let (app, op) = op();
        let song = crate::model::Song { id: uuid::Uuid::new_v4(), title: "Amazing Grace".into(), lyrics: "Verse 1\nAmazing grace".into(), ..Default::default() };
        app.edit(|d| { d.songs.push(song.clone()); d.services[0].items.push(crate::model::ServiceItem { id: uuid::Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None }); Ok(()) }).unwrap();
        let file = std::env::temp_dir().join(format!("v1-screens-words-{}.txt", uuid::Uuid::new_v4()));
        crate::ui::operator::files::testing::will_pick(vec![file.clone()]);
        op.transfer(Transfer::Words);
        settle(&op);
        assert!(std::fs::read_to_string(&file).unwrap().starts_with("Amazing Grace"));
        assert!(op.toast_log().last().unwrap().0.starts_with("Saved to"));
        let _ = std::fs::remove_file(file);
    }
}
