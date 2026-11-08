//! Media safety (N23) and Listen (N21). Every file added goes into the app's own `assets` folder as a copy
//! (`Store::import_file`, `Store::publish`), so moving or deleting the original never breaks a slide. "Clean up
//! unused media" takes out the library's files that no service or template uses, and deletes the files in that
//! folder that nothing uses any more. Called from `App::do_action`.
use crate::app::{ActionError, App};
use crate::live::{Frame, Live, MediaLayer, SlideLayer};
use crate::model::{AssetId, Data, ItemKind, SlideContent};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

/// A file this new may belong to an import that hasn't reached the library yet, so it is never deleted.
const YOUNG: Duration = Duration::from_secs(3600);
/// How many names the question lists before "and N more".
pub const LISTED: usize = 12;

pub fn handles(action: &str) -> bool { matches!(action, "media/unused" | "media/cleanup" | "sound/listen") }

/// A file in the `assets` folder.
#[derive(Clone, Debug, PartialEq)]
pub struct OnDisk { pub id: AssetId, pub bytes: u64, pub young: bool }

/// What "Clean up unused media" would take out.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Unused {
    /// Library entries no service or template uses: (id, name).
    pub entries: Vec<(Uuid, String)>,
    /// Files nothing would use once those are gone.
    pub files: Vec<AssetId>,
    pub bytes: u64,
}

/// The files a frame shows or plays.
fn frame_assets(f: &Frame, set: &mut BTreeSet<AssetId>) {
    if let Some(b) = &f.background { set.extend(b.v.asset().cloned()) }
    match f.media.as_ref().map(|m| &m.v) {
        Some(MediaLayer::Video { asset, .. } | MediaLayer::Still { asset }) => { set.insert(asset.clone()); }
        None => {}
    }
    if let Some(SlideLayer::Image { asset, .. }) = &f.slide { set.insert(asset.clone()); }
    if let Some(SlideLayer::Design { design, .. }) = &f.slide { set.extend(design.assets()); }
    if let Some(a) = &f.audio { set.insert(a.v.asset.clone()); }
    set.extend(crate::theme::assets::slot_assets(&f.overlays));
}

/// Everything the live engine shows or could still show: its frames and the slides of its items. Kept even when the
/// library no longer has them, so cleaning up never takes a picture off the screen.
pub fn on_air(live: &Live) -> BTreeSet<AssetId> {
    let mut set = BTreeSet::new();
    for f in live.frames() { frame_assets(f, &mut set) }
    for item in live.items() {
        set.extend(item.background.asset().cloned());
        for s in &item.slides {
            match &s.content {
                SlideContent::Image { asset, .. } | SlideContent::Video { asset, .. } | SlideContent::Still { asset } | SlideContent::Audio { asset, .. } => { set.insert(asset.clone()); }
                SlideContent::Design { design, .. } => set.extend(design.assets()),
                SlideContent::Text { .. } => {}
            }
        }
    }
    set
}

/// The library entries no service or template uses (and not on air), and the files nothing uses once they are gone.
/// A file younger than an hour is left, as an import may be about to add it.
pub fn find(data: &Data, on_air: &BTreeSet<AssetId>, disk: &[OnDisk]) -> Unused {
    let items = data.services.iter().flat_map(|s| &s.items).chain(data.templates.iter().flat_map(|t| &t.items));
    let used: BTreeSet<Uuid> = items.filter_map(|i| match i.kind { ItemKind::Media { media_id, .. } => Some(media_id), _ => None }).collect();
    let gone = |m: &crate::model::MediaFile| !used.contains(&m.id) && !on_air.contains(&m.asset);
    let entries: Vec<(Uuid, String)> = data.media.iter().filter(|m| gone(m)).map(|m| (m.id, m.name.clone())).collect();
    let mut after = data.clone();
    after.media.retain(|m| !gone(m));
    let (files, bytes) = loose_files(&after, on_air, disk);
    Unused { entries, files, bytes }
}

/// The files (and their total size) that nothing in `data` uses and that are not on air.
pub fn loose_files(data: &Data, on_air: &BTreeSet<AssetId>, disk: &[OnDisk]) -> (Vec<AssetId>, u64) {
    let keep = data.referenced_assets();
    let files: Vec<&OnDisk> = disk.iter().filter(|f| !f.young && !keep.contains(&f.id) && !on_air.contains(&f.id)).collect();
    (files.iter().map(|f| f.id.clone()).collect(), files.iter().map(|f| f.bytes).sum())
}

/// The files in the `assets` folder (no folders, no half-written `.part` files).
fn on_disk(app: &App) -> Vec<OnDisk> {
    let Ok(dir) = std::fs::read_dir(app.store.assets()) else { return vec![] };
    let now = SystemTime::now();
    dir.flatten().filter_map(|e| {
        let meta = e.metadata().ok().filter(|m| m.is_file())?;
        let id = e.file_name().to_str()?.to_string();
        if id.ends_with(".part") { return None }
        let young = meta.modified().ok().and_then(|t| now.duration_since(t).ok()).is_none_or(|age| age < YOUNG);
        Some(OnDisk { id, bytes: meta.len(), young })
    }).collect()
}

fn look(app: &App) -> Unused {
    let disk = on_disk(app);
    let data = app.data.lock().unwrap();
    let live = app.live.lock().unwrap();
    find(&data, &on_air(&live), &disk)
}

/// The question before cleaning up: what goes, the first `LISTED` names, and the space it frees.
pub fn question(u: &Unused) -> String {
    let mut lines = vec![];
    if !u.entries.is_empty() {
        lines.push(crate::trn!("Take this file out of the media library? No service or template uses it.",
            "Take these {n} files out of the media library? No service or template uses them.", u.entries.len()));
        lines.extend(u.entries.iter().take(LISTED).map(|(_, name)| format!("•  {name}")));
        let more = u.entries.len().saturating_sub(LISTED);
        if more > 0 { lines.push(crate::trn!("…and {n} more", "…and {n} more", more)) }
        lines.push(String::new());
    }
    lines.push(crate::tr!("This frees {size} on this computer. Backups keep their own copies, but this can't be undone.", size = size(u.bytes)));
    lines.join("\n")
}

/// Bytes as the operator reads them: "350 KB", "12.4 MB", "1.2 GB".
pub fn size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 { crate::tr!("{n} GB", n = format!("{:.1}", b / 1e9)) }
    else if b >= 1e6 { crate::tr!("{n} MB", n = format!("{:.1}", b / 1e6)) }
    else { crate::tr!("{n} KB", n = format!("{}", (b / 1e3).ceil() as u64)) }
}

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ActionError> {
    serde_json::from_slice(body).map_err(|e| ActionError::bad(crate::tr!("Bad request: {error}", error = e)))
}

pub async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        // What a clean-up would take out, and the question to ask about it.
        "media/unused" => {
            let u = look(app);
            let nothing = u.entries.is_empty() && u.files.is_empty();
            Ok(json!({ "entries": u.entries.len(), "files": u.files.len(), "bytes": u.bytes, "nothing": nothing, "question": question(&u) }))
        }
        // Takes out the entries the question named (`ids`) that are still unused, then every file nothing uses.
        // Undo can't bring a deleted file back, so Edit → Undo forgets what it had.
        "media/cleanup" => {
            #[derive(Deserialize)] struct C { ids: Vec<Uuid> }
            let C { ids } = parse(body)?;
            let removed = {
                let u = look(app);
                let take: BTreeSet<Uuid> = u.entries.iter().map(|(id, _)| *id).filter(|id| ids.contains(id)).collect();
                app.edit_untracked(|d| { d.media.retain(|m| !take.contains(&m.id)); Ok(take.len()) }).map_err(ActionError::bad)?
            };
            app.history.lock().unwrap().clear();
            app.broadcast_data();
            // Looked at again with the library as it now is (entries the question didn't name keep their files).
            let loose = {
                let disk = on_disk(app);
                let data = app.data.lock().unwrap();
                let live = app.live.lock().unwrap();
                loose_files(&data, &on_air(&live), &disk).0
            };
            let (mut files, mut bytes) = (0, 0);
            for id in &loose {
                let path = app.store.asset_path(id);
                let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                if std::fs::remove_file(&path).is_ok() { files += 1; bytes += len }
            }
            Ok(json!({ "removed": removed, "files": files, "bytes": bytes, "toast": done(removed, bytes) }))
        }
        // Listen (N21): the room's sound goes to the operator's headphones only, or back to the main output.
        "sound/listen" => {
            #[derive(Deserialize)] struct L { on: bool }
            let L { on } = parse(body)?;
            if on && app.data.lock().unwrap().settings.media.listen_device.is_empty() {
                return Err(ActionError::bad(crate::tr!("Choose your headphones in Settings → Media first.")));
            }
            app.listening.store(on, Ordering::Relaxed);
            app.broadcast_data();
            Ok(json!({ "on": on }))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}

/// The toast after a clean-up.
pub fn done(removed: usize, bytes: u64) -> String {
    crate::trn!("Took {n} file out of the media library and freed {size}.", "Took {n} files out of the media library and freed {size}.",
        removed, size = size(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;
    use crate::live::{Command, Envelope, Inst};
    use crate::model::{EndAction, MediaFile, MediaKind, MediaStatus, ServiceItem, ServiceTemplate};

    fn file(name: &str, asset: &str) -> MediaFile {
        MediaFile { id: Uuid::new_v4(), name: name.into(), kind: MediaKind::Video, asset: asset.into(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: Some(format!("{asset}.jpg")), size: None, folder: String::new() }
    }

    fn item(media_id: Uuid) -> ServiceItem {
        ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Media { media_id, autoplay: true, end: EndAction::Stop, looped: false }, background: None, notes: String::new(), theme: None }
    }

    fn disk(ids: &[&str]) -> Vec<OnDisk> { ids.iter().map(|id| OnDisk { id: id.to_string(), bytes: 1000, young: false }).collect() }

    #[test]
    fn a_file_no_service_uses_goes_with_its_thumbnail() {
        let mut d = Data::default();
        let (used, spare) = (file("Intro", "a.mp4"), file("Old promo", "b.mp4"));
        d.services[0].items.push(item(used.id));
        d.media = vec![used, spare.clone()];
        let u = find(&d, &BTreeSet::new(), &disk(&["a.mp4", "a.mp4.jpg", "b.mp4", "b.mp4.jpg", "stray.png"]));
        assert_eq!(u.entries, [(spare.id, "Old promo".to_string())]);
        assert_eq!(u.files, ["b.mp4", "b.mp4.jpg", "stray.png"], "a file left behind by an earlier delete goes too");
        assert_eq!(u.bytes, 3000);
    }

    #[test]
    fn a_template_keeps_its_file() {
        let mut d = Data::default();
        let f = file("Welcome", "w.mp4");
        d.templates.push(ServiceTemplate { id: Uuid::new_v4(), name: "Sunday".into(), day: None, items: vec![item(f.id)] });
        d.media = vec![f];
        assert_eq!(find(&d, &BTreeSet::new(), &disk(&["w.mp4"])), Unused::default());
    }

    #[test]
    fn a_file_on_air_stays_even_out_of_every_service() {
        let mut d = Data::default();
        let f = file("Countdown", "c.mp4");
        d.media = vec![f];
        let on_air = BTreeSet::from(["c.mp4".to_string()]);
        let u = find(&d, &on_air, &disk(&["c.mp4", "c.mp4.jpg"]));
        assert!(u.entries.is_empty());
        assert!(u.files.is_empty(), "its thumbnail is the library's");
    }

    #[test]
    fn files_the_rest_of_the_app_uses_stay() {
        let mut d = Data::default();
        d.settings.logo = Some("logo.png".into());
        d.decks.push(crate::model::Deck { id: Uuid::new_v4(), title: "Talk".into(), slides: vec!["s1.png".into()], notes: vec![], transitions: vec![], advance_ms: vec![] });
        // The same picture as a library file and a background: the entry goes, the file stays.
        let pic = file("Sunset", "sun.jpg");
        d.backgrounds.push(crate::model::BackgroundEntry::new("Sunset", crate::model::Background::Image { asset: "sun.jpg".into(), dim: 0 }, MediaStatus::Ready, chrono::Utc::now()));
        d.media = vec![pic.clone()];
        let u = find(&d, &BTreeSet::new(), &disk(&["logo.png", "s1.png", "sun.jpg", "sun.jpg.jpg"]));
        assert_eq!(u.entries, [(pic.id, "Sunset".to_string())]);
        assert_eq!(u.files, ["sun.jpg.jpg"]);
    }

    #[test]
    fn a_young_file_is_never_deleted() {
        let mut files = disk(&["new.mp4"]);
        files[0].young = true;
        assert!(find(&Data::default(), &BTreeSet::new(), &files).files.is_empty());
    }

    #[test]
    fn the_question_lists_names_and_the_space() {
        let entries: Vec<(Uuid, String)> = (0..14).map(|i| (Uuid::new_v4(), format!("Clip {i}"))).collect();
        let q = question(&Unused { entries, files: vec![], bytes: 12_400_000 });
        assert!(q.starts_with("Take these 14 files out of the media library?"), "{q}");
        assert!(q.contains("•  Clip 0") && q.contains("•  Clip 11") && !q.contains("Clip 12"), "{q}");
        assert!(q.contains("…and 2 more"), "{q}");
        assert!(q.contains("This frees 12.4 MB"), "{q}");
        let only_files = question(&Unused { entries: vec![], files: vec!["x".into()], bytes: 2_000 });
        assert!(only_files.starts_with("This frees 2 KB"), "{only_files}");
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(size(350_000), "350 KB");
        assert_eq!(size(12_400_000), "12.4 MB");
        assert_eq!(size(1_230_000_000), "1.2 GB");
    }

    fn put(app: &App, name: &str, bytes: &[u8]) -> AssetId {
        let tmp = app.store.tmp().join(format!("{name}.src"));
        std::fs::write(&tmp, bytes).unwrap();
        let id = app.store.import_file(&tmp, "mp4").unwrap();
        // Old enough to be cleaned up.
        let old = std::fs::File::options().write(true).open(app.store.asset_path(&id)).unwrap();
        old.set_modified(SystemTime::now() - Duration::from_secs(7200)).unwrap();
        id
    }

    #[tokio::test]
    async fn cleaning_up_takes_out_only_what_was_asked_and_frees_the_files() {
        let app = test_app();
        let (keep, spare, other) = (put(&app, "keep", b"keep"), put(&app, "spare", b"spare!"), put(&app, "other", b"other"));
        let (mut k, mut s, mut o) = (file("Keep", &keep), file("Spare", &spare), file("Other", &other));
        (k.thumb, s.thumb, o.thumb) = (None, None, None);
        app.data.lock().unwrap().services[0].items.push(item(k.id));
        app.data.lock().unwrap().media = vec![k.clone(), s.clone(), o.clone()];
        let asked = app.do_action("media/unused", b"{}").await.unwrap();
        assert_eq!((asked["entries"].as_u64(), asked["files"].as_u64(), asked["nothing"].as_bool()), (Some(2), Some(2), Some(false)));
        assert!(asked["question"].as_str().unwrap().contains("•  Spare"));
        // The operator saw only "Spare" (say "Other" was added meanwhile): "Other" stays, and so does its file.
        let done = app.do_action("media/cleanup", json!({ "ids": [s.id] }).to_string().as_bytes()).await.unwrap();
        assert_eq!((done["removed"].as_u64(), done["files"].as_u64(), done["bytes"].as_u64()), (Some(1), Some(1), Some(6)));
        assert_eq!(done["toast"], "Took 1 file out of the media library and freed 1 KB.");
        let names: Vec<String> = app.data.lock().unwrap().media.iter().map(|m| m.name.clone()).collect();
        assert_eq!(names, ["Keep", "Other"]);
        assert!(app.store.asset_path(&keep).exists() && app.store.asset_path(&other).exists());
        assert!(!app.store.asset_path(&spare).exists());
        assert!(!app.history.lock().unwrap().can_undo(), "Undo can't bring a deleted file back");
    }

    #[tokio::test]
    async fn a_file_on_the_screen_is_not_cleaned_up() {
        let app = test_app();
        let clip = put(&app, "clip", b"clip");
        let f = file("Clip", &clip);
        app.data.lock().unwrap().media.push(f.clone());
        let it = item(f.id);
        app.data.lock().unwrap().services[0].items.push(it.clone());
        assert!(app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::Show { item_id: it.id, slide: 0 } }).ok);
        // Taken out of the service while it plays.
        app.data.lock().unwrap().services[0].items.clear();
        assert!(matches!(app.live.lock().unwrap().frame.media, Some(Inst { v: MediaLayer::Video { .. }, .. })));
        let asked = app.do_action("media/unused", b"{}").await.unwrap();
        assert_eq!(asked["nothing"], true, "{asked}");
    }

    #[tokio::test]
    async fn moving_or_deleting_the_original_never_breaks_a_slide() {
        let app = test_app();
        let dir = crate::store::test_dir();
        let original = dir.join("sermon-bumper.png");
        image::RgbImage::from_pixel(8, 8, image::Rgb([200, 30, 30])).save(&original).unwrap();
        let v = app.do_action("import/path", json!({ "path": original, "purpose": "media" }).to_string().as_bytes()).await.unwrap();
        let asset = v["media"]["asset"].as_str().unwrap().to_string();
        std::fs::remove_file(&original).unwrap();
        let copy = app.store.asset_path(&asset);
        assert!(copy.starts_with(app.store.assets()), "the copy is in the app's own media folder");
        assert!(image::open(&copy).is_ok(), "the copy still opens with the original gone");
    }

    #[tokio::test]
    async fn listen_needs_headphones_chosen_and_is_never_saved() {
        let app = test_app();
        let err = app.do_action("sound/listen", br#"{"on":true}"#).await.unwrap_err();
        assert_eq!(err.message, "Choose your headphones in Settings → Media first.");
        app.data.lock().unwrap().settings.media.listen_device = "wasapi/{phones}".into();
        app.data.lock().unwrap().settings.audio_device = "wasapi/{board}".into();
        app.do_action("sound/listen", br#"{"on":true}"#).await.unwrap();
        assert_eq!(app.sound_out().device, "wasapi/{phones}");
        assert!(!serde_json::to_string(&app.data.lock().unwrap().settings).unwrap().contains("listening"));
        app.do_action("sound/listen", br#"{"on":false}"#).await.unwrap();
        assert_eq!(app.sound_out().device, "wasapi/{board}");
    }
}
