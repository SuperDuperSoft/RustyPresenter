//! N32: more ways in and out. Song words as plain text (for the bulletin), slides as pictures, a service as a file
//! list (playlist), theme packs, a song or announcement from text on the clipboard, and a PowerPoint's words as an
//! editable announcement. The `transfer/*` actions are here; `ui::operator::transfer` is the menu side.
use crate::{
    app::{ActionError, App},
    model::{AssetId, Data, ItemKind, MediaFile, Service, Song, SlideContent},
    theme::SavedTheme,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{io::{Read, Write}, path::{Path, PathBuf}, sync::Arc};
use uuid::Uuid;

pub mod strings {
    use crate::tr;
    pub fn no_service() -> String { tr!("No service is open.") }
    pub fn no_songs() -> String { tr!("This service has no songs.") }
    pub fn no_themes() -> String { tr!("There are no saved themes to export. Make one in Settings → Themes.") }
    pub fn not_a_theme_pack() -> String { tr!("This is not a RustyPresenter theme pack.") }
    pub fn newer_pack() -> String { tr!("This theme pack was made by a newer RustyPresenter. Update to open it.") }
    pub fn clipboard_empty() -> String { tr!("The clipboard has no text. Copy the words first.") }
    pub fn not_saved(e: &str) -> String { tr!("Could not save: {error}", error = e) }
    pub fn copy_suffix(name: &str) -> String { tr!("{name} (imported)", name = name) }
}

/// The theme pack's extension, without its dot.
pub const THEME_EXT: &str = "rpthemes";
pub const PLAYLIST_EXT: &str = "m3u8";
const THEME_FORMAT: u32 = 1;
const THEMES_JSON: &str = "themes.json";
const ASSETS: &str = "assets/";
const MAX_JSON: u64 = 16 << 20;

// ---------- From the clipboard ----------

/// Clipboard text split into a title and the rest: the first line is a title when it is short and more follows.
pub fn split_title(text: &str) -> (String, String) {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim();
    match text.split_once('\n') {
        Some((first, rest)) if first.trim().chars().count() <= 60 && !rest.trim().is_empty() => (first.trim().to_string(), rest.trim().to_string()),
        _ => (String::new(), text.to_string()),
    }
}

/// A song from clipboard text, for the song editor to check before saving: a SongSelect-style text with headings
/// and credits as `song/import` reads it, else the first line as the title and the rest as the words.
pub fn song_from_text(text: &str) -> Song {
    if let Ok(s) = crate::songselect::import_txt(text) { return s }
    let (title, lyrics) = split_title(text);
    Song { id: Uuid::nil(), title, lyrics, ..Default::default() }
}

// ---------- Words as text ----------

/// One song's words for a bulletin: the title, the words with their headings, then the credits.
pub fn song_text(song: &Song, church_ccli: &str) -> String {
    let mut out = vec![song.title.trim().to_string(), String::new(), song.lyrics.trim().to_string()];
    let credits: Vec<String> = [song.writers.trim().to_string(), song.copyright.trim().to_string(),
        if song.ccli.trim().is_empty() { String::new() } else { format!("CCLI Song # {}", song.ccli.trim()) },
        if church_ccli.trim().is_empty() || song.ccli.trim().is_empty() { String::new() } else { format!("CCLI License # {}", church_ccli.trim()) }]
        .into_iter().filter(|c| !c.is_empty()).collect();
    if !credits.is_empty() { out.push(String::new()); out.extend(credits) }
    out.join("\n")
}

/// Every song of a service, in service order (a song used twice once), as plain text.
pub fn service_words(data: &Data, service: &Service) -> Option<String> {
    let mut seen = vec![];
    let songs: Vec<String> = service.items.iter().filter_map(|i| match i.kind { ItemKind::Song { song_id, .. } => Some(song_id), _ => None })
        .filter(|id| { let new = !seen.contains(id); seen.push(*id); new })
        .filter_map(|id| data.songs.iter().find(|s| s.id == id)).map(|s| song_text(s, &data.settings.church_ccli)).collect();
    (!songs.is_empty()).then(|| songs.join("\n\n\n") + "\n")
}

// ---------- Playlist ----------

/// The service as an M3U playlist: every picture, video and sound file in order (with its full path), and the
/// other items as comments, so the list reads as the service order.
pub fn playlist(data: &Data, service: &Service, asset_path: &dyn Fn(&str) -> PathBuf) -> String {
    let mut out = vec!["#EXTM3U".to_string(), format!("#PLAYLIST:{}", service.name)];
    let media = |id: Uuid| data.media.iter().find(|m| m.id == id);
    let file = |out: &mut Vec<String>, name: &str, secs: Option<f64>, asset: &str| {
        out.push(format!("#EXTINF:{},{name}", secs.map_or(-1, |s| s.round() as i64)));
        out.push(asset_path(asset).display().to_string());
    };
    for item in &service.items {
        match &item.kind {
            ItemKind::Media { media_id, .. } => if let Some(m) = media(*media_id) { file(&mut out, &m.name, m.duration, &m.asset) },
            ItemKind::Deck { deck_id } => if let Some(d) = data.decks.iter().find(|d| d.id == *deck_id) {
                for (i, a) in d.slides.iter().enumerate() { file(&mut out, &format!("{} {}", d.title, i + 1), None, a) }
            },
            ItemKind::Header { title, .. } => out.push(format!("# —— {title} ——")),
            _ => {
                let (title, _, slides) = crate::slides::build_item(data, item);
                out.push(format!("# {title}"));
                // Media inside other items (a song's video slide) is listed too.
                for s in slides {
                    if let SlideContent::Video { asset, .. } | SlideContent::Still { asset } | SlideContent::Audio { asset, .. } = &s.content {
                        let m: Option<&MediaFile> = data.media.iter().find(|m| m.asset == *asset);
                        file(&mut out, m.map_or(asset.as_str(), |m| m.name.as_str()), m.and_then(|m| m.duration), asset);
                    }
                }
            }
        }
    }
    out.join("\n") + "\n"
}

// ---------- Slides as pictures ----------

/// A file name part with nothing a file system refuses.
pub fn safe_name(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    let t = t.trim().trim_end_matches('.').chars().take(60).collect::<String>();
    if t.is_empty() { "slide".into() } else { t }
}

/// Draws every slide of the service (sound-only slides left out) at `size` and saves each as a PNG in `folder`,
/// named in service order ("03-02 Amazing Grace.png"). Slow: call off the UI thread. Gives the number saved.
pub fn export_slides(app: &App, service: Uuid, folder: &Path, size: (u32, u32)) -> Result<usize, String> {
    let data = app.data.lock().unwrap().clone();
    let s = data.services.iter().find(|s| s.id == service).ok_or_else(strings::no_service)?;
    std::fs::create_dir_all(folder).map_err(|e| strings::not_saved(&e.to_string()))?;
    let renderer = crate::ui::thumbs::TextRenderer::new();
    let mut n = 0;
    for (i, item) in s.items.iter().enumerate() {
        let (title, kind, slides) = crate::slides::build_item(&data, item);
        let settings = crate::ui::thumbs::item_settings(&data, item);
        let passage = crate::model::passage(kind, &slides);
        for (j, slide) in slides.iter().enumerate() {
            if matches!(slide.content, SlideContent::Audio { .. }) { continue }
            let (_, job) = crate::ui::thumbs::thumb(slide, kind, &passage, item.background.as_ref(), &settings, &data.media, size);
            let px = crate::ui::thumbs::compose(&job, size, &renderer, &app.store);
            let img = image::RgbaImage::from_raw(px.width(), px.height(), px.as_bytes().to_vec()).ok_or_else(|| strings::not_saved("image"))?;
            let path = folder.join(format!("{:02}-{:02} {}.png", i + 1, j + 1, safe_name(&title)));
            img.save(&path).map_err(|e| strings::not_saved(&e.to_string()))?;
            n += 1;
        }
    }
    Ok(n)
}

// ---------- Theme packs ----------

#[derive(Serialize, Deserialize)]
struct Pack { format: u32, themes: Vec<SavedTheme> }

fn is_asset_id(id: &str) -> bool {
    let (hash, ext) = id.split_once('.').unwrap_or((id, ""));
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) && !ext.is_empty() && ext.len() <= 8 && ext.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Writes `themes` and their background files into a theme pack (a zip).
pub fn write_pack(themes: &[SavedTheme], asset_path: &dyn Fn(&str) -> PathBuf, dest: &Path) -> Result<(), String> {
    let tmp = dest.with_extension("part");
    let r = (|| -> Result<(), String> {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&tmp).map_err(|e| e.to_string())?);
        let o = zip::write::SimpleFileOptions::default();
        z.start_file(THEMES_JSON, o).map_err(|e| e.to_string())?;
        z.write_all(&serde_json::to_vec_pretty(&Pack { format: THEME_FORMAT, themes: themes.to_vec() }).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let mut done: Vec<&AssetId> = vec![];
        for a in themes.iter().filter_map(|t| t.background.as_ref()?.asset()) {
            if done.contains(&a) { continue }
            done.push(a);
            let Ok(mut f) = std::fs::File::open(asset_path(a)) else { continue }; // a missing file: the theme comes without it
            z.start_file(format!("{ASSETS}{a}"), o.compression_method(zip::CompressionMethod::Stored)).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, &mut z).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
        Ok(())
    })();
    match r {
        Ok(()) => std::fs::rename(&tmp, dest).map_err(|e| strings::not_saved(&e.to_string())),
        Err(e) => { let _ = std::fs::remove_file(&tmp); Err(strings::not_saved(&e)) }
    }
}

/// Reads a theme pack: its themes, with their background files put in the store.
pub fn read_pack(store: &crate::store::Store, path: &Path) -> Result<Vec<SavedTheme>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| strings::not_a_theme_pack())?;
    let mut json = vec![];
    zip.by_name(THEMES_JSON).map_err(|_| strings::not_a_theme_pack())?.take(MAX_JSON).read_to_end(&mut json).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_slice(&json).map_err(|_| strings::not_a_theme_pack())?;
    if v.get("format").and_then(Value::as_u64).is_some_and(|f| f > u64::from(THEME_FORMAT)) { return Err(strings::newer_pack()) }
    let mut pack: Pack = serde_json::from_value(v).map_err(|_| strings::not_a_theme_pack())?;
    std::fs::create_dir_all(store.tmp()).map_err(|e| e.to_string())?;
    for t in &mut pack.themes {
        let Some(bg) = t.background.as_mut() else { continue };
        let Some(a) = bg.asset().cloned() else { continue };
        if !is_asset_id(&a) { t.background = None; continue }
        if store.asset_path(&a).exists() { continue }
        let Ok(mut entry) = zip.by_name(&format!("{ASSETS}{a}")) else { t.background = None; continue };
        let tmp = store.tmp().join(format!("theme-{}", Uuid::new_v4()));
        let copied = std::fs::File::create(&tmp).map_err(|e| e.to_string()).and_then(|mut out| std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string()));
        let ext = a.split_once('.').map_or("bin", |(_, e)| e);
        let stored = copied.and_then(|_| store.import_file(&tmp, ext).map_err(|e| e.to_string()));
        let _ = std::fs::remove_file(&tmp);
        // Stored under its real hash: a damaged file gets a new name, so the theme points at what was stored.
        let stored = stored?;
        if stored != a { if let crate::model::Background::Image { asset, .. } | crate::model::Background::Video { asset, .. } = bg { *asset = stored } }
    }
    Ok(pack.themes)
}

/// Adds imported themes beside the church's: one that is already here is left alone, one with a taken id or name
/// comes in as a copy. Gives how many were added.
pub fn merge_themes(data: &mut Data, themes: Vec<SavedTheme>) -> usize {
    let mut added = 0;
    for mut t in themes {
        if data.themes.iter().any(|x| x.style == t.style && x.background == t.background && x.name == t.name) { continue }
        if data.themes.iter().any(|x| x.id == t.id) || t.id == crate::theme::STANDARD_ID { t.id = Uuid::new_v4() }
        if data.themes.iter().any(|x| x.name == t.name) { t.name = strings::copy_suffix(&t.name) }
        data.themes.push(t);
        added += 1;
    }
    added
}

// ---------- Actions ----------

pub fn handles(action: &str) -> bool { action.starts_with("transfer/") }

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ActionError> {
    serde_json::from_slice(body).map_err(|e| ActionError::bad(crate::tr!("Bad request: {error}", error = e)))
}

#[derive(Deserialize)]
struct To { path: PathBuf }

fn write_text(path: &Path, text: &str) -> Result<(), ActionError> {
    std::fs::write(path, text.as_bytes()).map_err(|e| ActionError::bad(strings::not_saved(&e.to_string())))
}

fn open_service(app: &App) -> Result<(Data, Service), ActionError> {
    let d = app.data.lock().unwrap().clone();
    let s = d.service().cloned().ok_or_else(|| ActionError::bad(strings::no_service()))?;
    Ok((d, s))
}

/// `transfer/words`, `transfer/playlist`, `transfer/slides` (the open service), `transfer/themes-out`,
/// `transfer/themes-in` and `transfer/pptx-words` (an announcement at the end of the open service).
pub async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        "transfer/words" => {
            let To { path } = parse(body)?;
            let (d, s) = open_service(app)?;
            write_text(&path, &service_words(&d, &s).ok_or_else(|| ActionError::bad(strings::no_songs()))?)?;
            Ok(json!({ "path": path }))
        }
        "transfer/playlist" => {
            let To { path } = parse(body)?;
            let (d, s) = open_service(app)?;
            write_text(&path, &playlist(&d, &s, &|a| app.store.asset_path(a)))?;
            Ok(json!({ "path": path }))
        }
        "transfer/slides" => {
            let To { path } = parse(body)?;
            let (_, s) = open_service(app)?;
            let a = app.clone();
            let folder = path.join(safe_name(&s.name));
            let f2 = folder.clone();
            let n = tokio::task::spawn_blocking(move || export_slides(&a, s.id, &f2, (1920, 1080))).await
                .map_err(|_| ActionError { status: 500, message: strings::not_saved("") })?.map_err(ActionError::bad)?;
            Ok(json!({ "path": folder, "count": n }))
        }
        "transfer/themes-out" => {
            let To { path } = parse(body)?;
            let themes = app.data.lock().unwrap().themes.clone();
            if themes.is_empty() { return Err(ActionError::bad(strings::no_themes())) }
            let a = app.clone();
            let p = path.clone();
            tokio::task::spawn_blocking(move || write_pack(&themes, &|x| a.store.asset_path(x), &p)).await
                .map_err(|_| ActionError { status: 500, message: strings::not_saved("") })?.map_err(ActionError::bad)?;
            Ok(json!({ "path": path }))
        }
        "transfer/themes-in" => {
            let To { path } = parse(body)?;
            let a = app.clone();
            let themes = tokio::task::spawn_blocking(move || read_pack(&a.store, &path)).await
                .map_err(|_| ActionError { status: 500, message: strings::not_a_theme_pack() })?.map_err(ActionError::bad)?;
            let n = app.edit(|d| Ok(merge_themes(d, themes))).map_err(ActionError::bad)?;
            app.broadcast_data();
            Ok(json!({ "count": n }))
        }
        "transfer/pptx-words" => {
            let To { path } = parse(body)?;
            let p = path.clone();
            let slides = tokio::task::spawn_blocking(move || crate::import::pptx_text::slide_texts(&p)).await
                .map_err(|_| ActionError { status: 500, message: strings::not_saved("") })?.map_err(ActionError::bad)?;
            let (body, empty) = crate::import::pptx_text::announcement_body(&slides);
            let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let item = crate::model::ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.clone(), body, layout: Default::default() }, background: None, notes: String::new(), theme: None };
            let id = item.id;
            app.edit(|d| { d.service_mut().ok_or_else(strings::no_service)?.items.push(item); Ok(()) }).map_err(ActionError::bad)?;
            Ok(json!({ "id": id, "title": title, "slides": slides.len() - empty, "empty": empty }))
        }
        _ => Err(ActionError { status: 404, message: crate::tr!("Unknown action.") }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;
    use crate::model::{Deck, MediaKind, MediaStatus, ServiceItem};

    fn song(title: &str, ccli: &str) -> Song {
        Song { id: Uuid::new_v4(), title: title.into(), writers: "John Newton".into(), ccli: ccli.into(), copyright: "Public Domain".into(), lyrics: "Verse 1\nAmazing grace".into(), ..Default::default() }
    }

    fn item(kind: ItemKind) -> ServiceItem { ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None } }

    async fn act(app: &Arc<App>, action: &str, body: Value) -> Result<Value, ActionError> { app.do_action(action, body.to_string().as_bytes()).await }

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("v1-screens-transfer-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn clipboard_text_becomes_a_title_and_words() {
        assert_eq!(split_title("Welcome!\r\n\r\nCoffee after church\n"), ("Welcome!".into(), "Coffee after church".into()));
        assert_eq!(split_title("Just one line"), (String::new(), "Just one line".into()), "no title to take");
        let long = "A very long first line that is clearly part of the words and not a title at all, really\nmore";
        assert_eq!(split_title(long).0, "");
        let s = song_from_text("How Great Thou Art\n\nO Lord my God\nwhen I in awesome wonder");
        assert_eq!((s.title.as_str(), s.lyrics.as_str()), ("How Great Thou Art", "O Lord my God\nwhen I in awesome wonder"));
        let ss = song_from_text("Amazing Grace\n\nVerse 1\nAmazing grace how sweet\n\nJohn Newton\nCCLI Song # 22025\n");
        assert_eq!((ss.title.as_str(), ss.ccli.as_str(), ss.writers.as_str()), ("Amazing Grace", "22025", "John Newton"), "a SongSelect text keeps its credits");
    }

    #[test]
    fn a_service_s_words_for_the_bulletin() {
        let mut d = Data::default();
        d.settings.church_ccli = "1234".into();
        let (a, b) = (song("Amazing Grace", "22025"), Song { ccli: String::new(), ..song("Be Thou My Vision", "") });
        d.songs = vec![a.clone(), b.clone()];
        let s = &mut d.services[0];
        s.items = vec![item(ItemKind::Song { song_id: b.id, arrangement: None }), item(ItemKind::Text { title: "x".into(), body: "y".into(), layout: Default::default() }),
            item(ItemKind::Song { song_id: a.id, arrangement: None }), item(ItemKind::Song { song_id: b.id, arrangement: None })];
        let text = service_words(&d, &d.services[0]).unwrap();
        assert_eq!(text, "Be Thou My Vision\n\nVerse 1\nAmazing grace\n\nJohn Newton\nPublic Domain\n\n\nAmazing Grace\n\nVerse 1\nAmazing grace\n\nJohn Newton\nPublic Domain\nCCLI Song # 22025\nCCLI License # 1234\n");
        d.services[0].items.clear();
        assert!(service_words(&d, &d.services[0]).is_none());
    }

    #[test]
    fn a_playlist_lists_the_files_in_service_order() {
        let mut d = Data::default();
        let video = MediaFile { id: Uuid::new_v4(), name: "Walk-in.mp4".into(), kind: MediaKind::Video, asset: "v.mp4".into(), status: MediaStatus::Ready, original_asset: None, duration: Some(61.4), thumb: None, size: None, folder: String::new() };
        d.media.push(video.clone());
        d.decks.push(Deck { id: Uuid::new_v4(), title: "Notices".into(), slides: vec!["n1.png".into(), "n2.png".into()], notes: vec![], transitions: vec![], advance_ms: vec![] });
        let deck = d.decks[0].id;
        d.services[0].name = "Sunday".into();
        d.services[0].items = vec![item(ItemKind::Header { title: "Worship".into(), color: String::new() }), item(ItemKind::Media { media_id: video.id, autoplay: true, end: Default::default(), looped: false }),
            item(ItemKind::Text { title: "Welcome".into(), body: "Hi".into(), layout: Default::default() }), item(ItemKind::Deck { deck_id: deck })];
        let list = playlist(&d, &d.services[0], &|a| PathBuf::from("/media").join(a));
        let lines: Vec<&str> = list.lines().collect();
        assert_eq!(lines, ["#EXTM3U", "#PLAYLIST:Sunday", "# —— Worship ——", "#EXTINF:61,Walk-in.mp4", "/media/v.mp4", "# Welcome",
            "#EXTINF:-1,Notices 1", "/media/n1.png", "#EXTINF:-1,Notices 2", "/media/n2.png"]);
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(safe_name("What a Friend: We Have / in Jesus?"), "What a Friend_ We Have _ in Jesus_");
        assert_eq!(safe_name("  ..."), "slide");
    }

    #[test]
    fn a_theme_pack_goes_out_and_comes_back() {
        let app = test_app();
        let d = dir();
        let pic = d.join("bg.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 10, 10, 255])).save(&pic).unwrap();
        let asset = app.store.import_file(&pic, "png").unwrap();
        let theme = SavedTheme { id: Uuid::new_v4(), name: "Easter".into(), style: Default::default(), background: Some(crate::model::Background::Image { asset: asset.clone(), dim: 20 }) };
        let pack = d.join("themes.rpthemes");
        write_pack(std::slice::from_ref(&theme), &|a| app.store.asset_path(a), &pack).unwrap();
        // Another computer: a fresh store.
        let other = test_app();
        let back = read_pack(&other.store, &pack).unwrap();
        assert_eq!(back, [theme.clone()]);
        assert!(other.store.asset_path(&asset).exists(), "the background came along");
        let mut data = Data::default();
        assert_eq!(merge_themes(&mut data, back.clone()), 1);
        assert_eq!(merge_themes(&mut data, back.clone()), 0, "the same theme twice is one");
        let mut changed = back[0].clone();
        changed.style.size = 99;
        assert_eq!(merge_themes(&mut data, vec![changed]), 1);
        assert_eq!(data.themes[1].name, "Easter (imported)");
        assert_ne!(data.themes[1].id, data.themes[0].id);
        std::fs::write(d.join("x.rpthemes"), b"nope").unwrap();
        assert_eq!(read_pack(&other.store, &d.join("x.rpthemes")).unwrap_err(), strings::not_a_theme_pack());
        let _ = std::fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn the_actions_write_their_files() {
        let app = test_app();
        let d = dir();
        let s = song("Amazing Grace", "22025");
        app.edit(|data| { data.songs.push(s.clone()); data.services[0].items.push(item(ItemKind::Song { song_id: s.id, arrangement: None })); Ok(()) }).unwrap();
        act(&app, "transfer/words", json!({ "path": d.join("words.txt") })).await.unwrap();
        assert!(std::fs::read_to_string(d.join("words.txt")).unwrap().starts_with("Amazing Grace\n\nVerse 1"));
        act(&app, "transfer/playlist", json!({ "path": d.join("list.m3u8") })).await.unwrap();
        assert!(std::fs::read_to_string(d.join("list.m3u8")).unwrap().contains("# Amazing Grace"));
        let v = act(&app, "transfer/slides", json!({ "path": d })).await.unwrap();
        assert_eq!(v["count"], 1);
        let folder = PathBuf::from(v["path"].as_str().unwrap());
        let png = std::fs::read_dir(&folder).unwrap().next().unwrap().unwrap().path();
        assert_eq!(png.file_name().unwrap().to_string_lossy(), "01-01 Amazing Grace.png");
        assert_eq!(image::open(&png).unwrap().width(), 1920);
        assert_eq!(act(&app, "transfer/themes-out", json!({ "path": d.join("t.rpthemes") })).await.unwrap_err().message, strings::no_themes());
        let _ = std::fs::remove_dir_all(d);
    }
}
