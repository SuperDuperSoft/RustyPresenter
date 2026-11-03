//! Service bundles (spec §11): one service and everything it needs, in a zip, for moving it to another computer.
#[path = "strings/bundle.rs"]
pub mod strings;
use crate::{
    import::CANCELLED,
    model::{AssetId, Background, Data, Deck, ItemKind, MediaFile, Service, Song},
    sha256::{Digest, Sha256, hex},
    store::Store,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

/// The file extension, without its dot.
pub const EXT: &str = "rpservice";
pub const FORMAT: u32 = 1;
const CONTENTS: &str = "bundle.json";
const ASSETS: &str = "assets/";
/// The most of `bundle.json` that is read. A service's words come nowhere near it.
const MAX_CONTENTS: u64 = 64 << 20;

/// `bundle.json`: the service, and the library entries it uses.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Contents {
    pub format: u32,
    pub service: Service,
    #[serde(default)]
    pub songs: Vec<Song>,
    #[serde(default)]
    pub media: Vec<MediaFile>,
    #[serde(default)]
    pub decks: Vec<Deck>,
}

/// What opening a bundle did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Opened {
    pub service: Uuid,
    pub name: String,
    pub songs_added: usize,
    /// Titles of songs that differ from the library's song of that name: added beside it.
    pub songs_differ: Vec<String>,
    pub media_added: usize,
    pub decks_added: usize,
}

/// The service's contents and the assets to carry, or one line per thing that is missing. An original file or
/// thumbnail that has gone is left out: the service plays without them.
pub fn gather(data: &Data, service: Uuid, has_asset: &dyn Fn(&str) -> bool) -> Result<(Contents, Vec<AssetId>), Vec<String>> {
    let Some(service) = data.services.iter().find(|s| s.id == service) else { return Err(vec![strings::SERVICE_GONE.into()]) };
    let (mut songs, mut media, mut decks) = (Vec::<Song>::new(), Vec::<MediaFile>::new(), Vec::<Deck>::new());
    let mut assets = BTreeSet::new();
    let mut missing: Vec<String> = vec![];
    let mut need = |id: &AssetId, what: String, missing: &mut Vec<String>| if has_asset(id) { assets.insert(id.clone()); } else { missing.push(what) };
    for item in &service.items {
        match &item.kind {
            ItemKind::Song { song_id } => match data.songs.iter().find(|s| s.id == *song_id) {
                Some(s) => if !songs.iter().any(|x| x.id == s.id) { songs.push(s.clone()) },
                None => missing.push(strings::SONG_GONE.into()),
            },
            ItemKind::Media { media_id, .. } => match data.media.iter().find(|m| m.id == *media_id) {
                Some(m) => if !media.iter().any(|x| x.id == m.id) {
                    need(&m.asset, strings::file_gone(&m.name), &mut missing);
                    let mut m = m.clone();
                    m.original_asset = m.original_asset.filter(|a| has_asset(a));
                    m.thumb = m.thumb.filter(|a| has_asset(a));
                    for extra in m.original_asset.iter().chain(&m.thumb) { need(extra, String::new(), &mut missing) }
                    media.push(m);
                },
                None => missing.push(strings::MEDIA_GONE.into()),
            },
            ItemKind::Deck { deck_id } => match data.decks.iter().find(|d| d.id == *deck_id) {
                Some(d) => if !decks.iter().any(|x| x.id == d.id) {
                    for slide in &d.slides { need(slide, strings::file_gone(&d.title), &mut missing) }
                    decks.push(d.clone());
                },
                None => missing.push(strings::DECK_GONE.into()),
            },
            ItemKind::Scripture { .. } | ItemKind::Text { .. } => {}
        }
        if let Some(Background::Image { asset } | Background::Video { asset }) = &item.background {
            need(asset, strings::BACKGROUND_GONE.into(), &mut missing);
        }
    }
    if !missing.is_empty() { return Err(missing) }
    Ok((Contents { format: FORMAT, service: service.clone(), songs, media, decks }, assets.into_iter().collect()))
}

/// Copies until the end, watching for Cancel. Sends each chunk to `seen`.
fn copy(from: &mut impl Read, to: &mut impl Write, cancel: &CancellationToken, seen: &mut dyn FnMut(&[u8])) -> Result<(), String> {
    let mut buf = vec![0u8; 1 << 16];
    loop {
        if cancel.is_cancelled() { return Err(CANCELLED.into()) }
        let n = from.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 { return Ok(()) }
        seen(&buf[..n]);
        to.write_all(&buf[..n]).map_err(|e| e.to_string())?;
    }
}

/// `done` of `total` as a percentage below 100, so only the very end says finished.
fn percent(done: u64, total: u64) -> u8 { (done.saturating_mul(99) / total.max(1)).min(99) as u8 }

/// Writes the zip beside `dest`, then moves it into place, so a failed save leaves any older file as it was. The
/// assets are stored as they are: pictures and video do not compress.
pub fn write(store: &Store, contents: &Contents, assets: &[AssetId], dest: &Path, cancel: &CancellationToken, progress: &dyn Fn(u8)) -> Result<(), String> {
    let mut part = dest.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let r = write_zip(store, contents, assets, &part, cancel, progress).and_then(|()| fs::rename(&part, dest).map_err(|e| e.to_string()));
    if r.is_err() { let _ = fs::remove_file(&part); } else { progress(100) }
    r
}

fn write_zip(store: &Store, contents: &Contents, assets: &[AssetId], part: &Path, cancel: &CancellationToken, progress: &dyn Fn(u8)) -> Result<(), String> {
    let text = |e: zip::result::ZipError| e.to_string();
    let total = assets.iter().try_fold(0u64, |n, a| fs::metadata(store.asset_path(a)).map(|m| n + m.len())).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(fs::File::create(part).map_err(|e| e.to_string())?);
    let json = serde_json::to_vec_pretty(contents).map_err(|e| e.to_string())?;
    zip.start_file(CONTENTS, SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)).map_err(text)?;
    zip.write_all(&json).map_err(|e| e.to_string())?;
    let mut done = 0;
    for a in assets {
        let mut file = fs::File::open(store.asset_path(a)).map_err(|e| e.to_string())?;
        zip.start_file(format!("{ASSETS}{a}"), SimpleFileOptions::default().compression_method(CompressionMethod::Stored).large_file(true)).map_err(text)?;
        copy(&mut file, &mut zip, cancel, &mut |b| { done += b.len() as u64; progress(percent(done, total)) })?;
    }
    if cancel.is_cancelled() { return Err(CANCELLED.into()) }
    zip.finish().map_err(text)?.sync_all().map_err(|e| e.to_string())
}

/// `<64 hex>.<ext>`, as `Store::publish` names them. An asset id becomes a path, so a file from elsewhere must not
/// get to choose one that leaves the store.
fn is_asset_id(id: &str) -> bool {
    let Some((hash, ext)) = id.split_once('.') else { return false };
    hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && (1..=8).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Every asset the contents name.
fn asset_ids(c: &Contents) -> BTreeSet<AssetId> {
    let mut set: BTreeSet<AssetId> = c.decks.iter().flat_map(|d| d.slides.iter().cloned()).collect();
    for m in &c.media {
        set.insert(m.asset.clone());
        set.extend(m.original_asset.iter().chain(&m.thumb).cloned());
    }
    for i in &c.service.items {
        if let Some(Background::Image { asset } | Background::Video { asset }) = &i.background { set.insert(asset.clone()); }
    }
    set
}

/// Reads and checks a bundle, and puts its assets in the store. Files are staged, and published only when all of
/// them have matched their names. Nothing is in the library yet: see `merge`.
pub fn read(store: &Store, path: &Path, cancel: &CancellationToken, progress: &dyn Fn(u8)) -> Result<Contents, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|_| strings::NOT_A_SERVICE_FILE.to_string())?;
    let contents = read_contents(&mut zip)?;
    let ids = asset_ids(&contents);
    if !ids.iter().all(|id| is_asset_id(id)) { return Err(strings::DAMAGED.into()) }
    let mut sizes = BTreeMap::new();
    for id in ids.iter().filter(|id| !store.asset_path(id).exists()) {
        sizes.insert(id.clone(), zip.by_name(&format!("{ASSETS}{id}")).map_err(|_| strings::DAMAGED.to_string())?.size());
    }
    let stage = store.tmp().join(format!("bundle-{}", Uuid::new_v4()));
    fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let r = unpack(store, &mut zip, &sizes, &stage, cancel, progress);
    let _ = fs::remove_dir_all(&stage);
    r.map(|()| { progress(100); contents })
}

fn read_contents(zip: &mut ZipArchive<fs::File>) -> Result<Contents, String> {
    let not_ours = |_| strings::NOT_A_SERVICE_FILE.to_string();
    let mut json = vec![];
    zip.by_name(CONTENTS).map_err(|_| strings::NOT_A_SERVICE_FILE.to_string())?.take(MAX_CONTENTS).read_to_end(&mut json).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_slice(&json).map_err(not_ours)?;
    if value.get("format").and_then(|f| f.as_u64()).is_some_and(|f| f > FORMAT as u64) { return Err(strings::NEWER_FILE.into()) }
    serde_json::from_value(value).map_err(not_ours)
}

fn unpack(store: &Store, zip: &mut ZipArchive<fs::File>, sizes: &BTreeMap<AssetId, u64>, stage: &Path, cancel: &CancellationToken, progress: &dyn Fn(u8)) -> Result<(), String> {
    let total: u64 = sizes.values().sum();
    let mut done = 0;
    for id in sizes.keys() {
        let mut entry = zip.by_name(&format!("{ASSETS}{id}")).map_err(|_| strings::DAMAGED.to_string())?;
        let mut out = fs::File::create(stage.join(id)).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        copy(&mut entry, &mut out, cancel, &mut |b| { hasher.update(b); done += b.len() as u64; progress(percent(done, total)) })
            .map_err(|e| if e == CANCELLED { e } else { strings::DAMAGED.to_string() })?;
        let (hash, _) = id.split_once('.').unwrap_or_default();
        if hex(&hasher.finalize()) != hash { return Err(strings::DAMAGED.into()) }
        out.sync_all().map_err(|e| e.to_string())?;
    }
    for id in sizes.keys() {
        let (hash, ext) = id.split_once('.').unwrap_or_default();
        store.publish(&stage.join(id), hash, ext).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The library song this one is: found by id, CCLI # or title, with the same words in the same order. Second
/// value: whether a song by that id, CCLI # or title exists at all.
fn matching_song(library: &[Song], song: &Song) -> (Option<Uuid>, bool) {
    let ccli = song.ccli.trim();
    let title = song.title.trim().to_lowercase();
    let mut same_name = library.iter().filter(|s| s.id == song.id || (!ccli.is_empty() && s.ccli.trim() == ccli) || s.title.trim().to_lowercase() == title).peekable();
    let named = same_name.peek().is_some();
    (same_name.find(|s| s.lyrics == song.lyrics && s.arrangement == song.arrangement).map(|s| s.id), named)
}

/// Adds the service to the library and opens it. What this computer already has is reused. A song that differs
/// from the library's comes in beside it, so the service shows what was saved and the library's song is untouched.
pub fn merge(data: &mut Data, contents: Contents) -> Opened {
    let Contents { mut service, songs, media, decks, .. } = contents;
    let mut opened = Opened::default();
    let mut ids: HashMap<Uuid, Uuid> = HashMap::new();
    for mut song in songs {
        let old = song.id;
        let (found, named) = matching_song(&data.songs, &song);
        if let Some(id) = found { ids.insert(old, id); continue }
        if named { opened.songs_differ.push(song.title.clone()) } else { opened.songs_added += 1 }
        if data.songs.iter().any(|s| s.id == old) { song.id = Uuid::new_v4() }
        ids.insert(old, song.id);
        data.songs.push(song);
    }
    for mut m in media {
        let old = m.id;
        if let Some(have) = data.media.iter().find(|x| x.asset == m.asset) { ids.insert(old, have.id); continue }
        if data.media.iter().any(|x| x.id == old) { m.id = Uuid::new_v4() }
        ids.insert(old, m.id);
        data.media.push(m);
        opened.media_added += 1;
    }
    for mut d in decks {
        let old = d.id;
        if let Some(have) = data.decks.iter().find(|x| x.slides == d.slides) { ids.insert(old, have.id); continue }
        if data.decks.iter().any(|x| x.id == old) { d.id = Uuid::new_v4() }
        ids.insert(old, d.id);
        data.decks.push(d);
        opened.decks_added += 1;
    }
    let mapped = |id: &mut Uuid| if let Some(new) = ids.get(id) { *id = *new };
    for item in &mut service.items {
        item.id = Uuid::new_v4();
        match &mut item.kind {
            ItemKind::Song { song_id: id } | ItemKind::Media { media_id: id, .. } | ItemKind::Deck { deck_id: id } => mapped(id),
            ItemKind::Scripture { .. } | ItemKind::Text { .. } => {}
        }
    }
    service.id = Uuid::new_v4();
    service.name = free_name(data, &service.name);
    (opened.service, opened.name) = (service.id, service.name.clone());
    data.current_service = Some(service.id);
    data.services.push(service);
    opened
}

/// `name`, or `name (2)` and so on when a service has it.
fn free_name(data: &Data, name: &str) -> String {
    let taken = |n: &str| data.services.iter().any(|s| s.name == n);
    if !taken(name) { return name.into() }
    (2..).map(|n| format!("{name} ({n})")).find(|n| !taken(n)).unwrap_or_default()
}

#[cfg(test)]
#[path = "bundle_tests.rs"]
mod tests;
