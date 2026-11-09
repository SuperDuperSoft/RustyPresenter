//! Data folder: `data.json`, content-addressed `assets/`, and complete backups
//! that carry their own copies of every asset they reference (review R2-04).
use crate::model::{AssetId, Data};
use crate::theme::migrate;
use serde::{Deserialize, Serialize};
use crate::sha256::{Digest, Sha256, hex};
use std::{fs, io::{self, Write}, path::{Path, PathBuf}};

pub struct Store {
    pub root: PathBuf,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BackupInfo {
    pub name: String,
    pub created: String,
    pub assets: usize,
    pub songs: usize,
    pub services: usize,
}

#[derive(Serialize, Deserialize)]
struct Manifest { created: String, assets: Vec<AssetId> }

const KEEP_BACKUPS: usize = 10;

/// The data format this build reads and writes (`Data.format`): the only format number in the code (FEAT-1 spec
/// §4.7). Older data is brought up to it once, at load, by the steps in `upgrade_format`; newer data opens read-only.
pub const SUPPORTED_FORMAT: u32 = 2;

/// What `Store::load_full` read.
#[derive(Debug)]
pub struct Loaded {
    pub data: Data,
    /// Saved by a newer RustyPresenter: show it, never save over it.
    pub read_only: bool,
}

impl Store {
    pub fn open(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(root.join("assets"))?;
        fs::create_dir_all(root.join("tmp"))?;
        fs::create_dir_all(root.join("backups").join("assets"))?;
        Ok(Self { root })
    }

    pub fn data_path(&self) -> PathBuf { self.root.join("data.json") }
    pub fn assets(&self) -> PathBuf { self.root.join("assets") }
    pub fn tmp(&self) -> PathBuf { self.root.join("tmp") }
    pub fn asset_path(&self, id: &str) -> PathBuf { self.assets().join(id) }

    pub fn load(&self) -> Result<Data, String> { self.load_full().map(|l| l.data) }

    /// Reads `data.json`: older data is brought up to `SUPPORTED_FORMAT` and saved at once (after a "before-update"
    /// backup), newer data opens read-only, then `Data::after_load` runs (its changes saved unless read-only).
    pub fn load_full(&self) -> Result<Loaded, String> {
        let unreadable = |e: &dyn std::fmt::Display| crate::tr!(
            "Your saved data at {path} could not be read ({reason}). Nothing was overwritten. Restore a backup from the backups folder or move the file aside.",
            path = self.data_path().display(), reason = e);
        let text = match fs::read_to_string(self.data_path()) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Loaded { data: Data::default(), read_only: false }),
            Err(e) => return Err(crate::tr!("Cannot read {path}: {reason}", path = self.data_path().display(), reason = e)),
        };
        let found = migrate::read_format(&text).map_err(|e| unreadable(&e))?;
        let step = migrate::step(found, SUPPORTED_FORMAT);
        let mut data = match serde_json::from_str::<Data>(&text) {
            Ok(d) => d.upgrade(),
            // A newer build's data may not parse here; it still opens (read-only, empty) instead of refusing to start.
            Err(_) if step == migrate::Step::Newer => Data::default(),
            Err(e) => return Err(unreadable(&e)),
        };
        let read_only = step == migrate::Step::Newer;
        if step == migrate::Step::Migrate {
            data.format = found;
            self.upgrade_format(&mut data, true);
            self.save(&data).map_err(|e| crate::tr!("Could not save: {reason}", reason = e))?;
        }
        if data.after_load() && !read_only { self.save(&data).map_err(|e| crate::tr!("Could not save: {reason}", reason = e))?; }
        Ok(Loaded { data, read_only })
    }

    /// The format steps, in order, from `data.format` up to `SUPPORTED_FORMAT`. With `backup`, each step first takes
    /// a "before-update" backup; when that can't be made (an asset is missing), `data.json` is copied aside instead.
    fn upgrade_format(&self, data: &mut Data, backup: bool) {
        while data.format < SUPPORTED_FORMAT {
            if backup && let Err(e) = self.backup_labeled(data, migrate::BEFORE_UPDATE) {
                eprintln!("Could not back up before the update ({e}); keeping a copy of data.json instead");
                let _ = fs::copy(self.data_path(), self.root.join(migrate::before_update_copy_name(&now_stamp())));
            }
            match data.format {
                0 | 1 => migrate::migrate_1_to_2(data, chrono::Utc::now()),
                _ => data.format = SUPPORTED_FORMAT,
            }
        }
    }

    /// Online Bible text is left out (Nov 5 spec §5).
    pub fn save(&self, data: &Data) -> io::Result<()> {
        let data = crate::scripture::online::without_online_text(data, &self.root);
        let json = serde_json::to_vec_pretty(&*data).map_err(io::Error::other)?;
        write_atomic(&self.data_path(), &json)
    }

    /// Move a finished temp file into the asset store under its hash.
    pub fn publish(&self, tmp: &Path, sha: &str, ext: &str) -> io::Result<AssetId> {
        let id = format!("{sha}.{}", sanitize_ext(ext));
        let dest = self.asset_path(&id);
        if dest.exists() { let _ = fs::remove_file(tmp); } else { fs::rename(tmp, &dest)?; }
        Ok(id)
    }

    /// Hash and publish an existing file (copying it).
    pub fn import_file(&self, src: &Path, ext: &str) -> io::Result<AssetId> {
        let tmp = self.tmp().join(format!("{}.part", uuid::Uuid::new_v4()));
        let mut hasher = Sha256::new();
        let mut input = fs::File::open(src)?;
        let mut out = fs::File::create(&tmp)?;
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = io::Read::read(&mut input, &mut buf)?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n])?;
        }
        out.sync_all()?;
        drop(out);
        self.publish(&tmp, &hex(&hasher.finalize()), ext)
    }

    /// A complete restore point: data snapshot + every referenced asset's bytes.
    /// The `COMPLETE` marker is written last; only marked backups are offered.
    pub fn backup(&self, data: &Data) -> io::Result<String> { self.backup_named(data, None) }

    /// A backup named `backup-<stamp>-<label>`. The newest one of each label is kept apart from the usual
    /// `KEEP_BACKUPS`, so app updates never push out the church's own backups.
    pub fn backup_labeled(&self, data: &Data, label: &str) -> io::Result<String> { self.backup_named(data, Some(label)) }

    fn backup_named(&self, data: &Data, label: Option<&str>) -> io::Result<String> {
        let created = now_stamp();
        let name = match label { Some(l) => format!("backup-{created}-{l}"), None => format!("backup-{created}") };
        let dir = self.root.join("backups").join(&name);
        let partial = self.root.join("backups").join(format!("{name}.partial"));
        fs::create_dir_all(&partial)?;
        let assets: Vec<AssetId> = data.referenced_assets().into_iter().collect();
        let store = self.root.join("backups").join("assets");
        for a in &assets {
            let src = self.asset_path(a);
            let dst = store.join(a);
            if dst.exists() { continue; }
            if !src.exists() { return Err(io::Error::other(format!("asset {a} is missing; backup not completed"))); }
            let tmp = store.join(format!("{a}.part"));
            if fs::hard_link(&src, &tmp).is_err() { fs::copy(&src, &tmp)?; }
            fs::rename(&tmp, &dst)?;
        }
        let data = crate::scripture::online::without_online_text(data, &self.root);
        fs::write(partial.join("data.json"), serde_json::to_vec_pretty(&*data).map_err(io::Error::other)?)?;
        fs::write(partial.join("manifest.json"), serde_json::to_vec_pretty(&Manifest { created: created.clone(), assets }).map_err(io::Error::other)?)?;
        fs::write(partial.join("COMPLETE"), b"ok")?;
        fs::rename(&partial, &dir)?;
        self.prune_backups()?;
        Ok(name)
    }

    pub fn backups(&self) -> Vec<BackupInfo> {
        let mut list = vec![];
        let Ok(rd) = fs::read_dir(self.root.join("backups")) else { return list };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with("backup-") || name.ends_with(".partial") || !p.join("COMPLETE").exists() { continue; }
            let manifest: Option<Manifest> = fs::read(p.join("manifest.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
            let data: Option<Data> = fs::read(p.join("data.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
            if let (Some(m), Some(d)) = (manifest, data) {
                list.push(BackupInfo { name, created: m.created, assets: m.assets.len(), songs: d.songs.len(), services: d.services.len() });
            }
        }
        list.sort_by(|a, b| b.name.cmp(&a.name));
        list
    }

    /// Restore a backup using only the backup set (works into an empty data folder).
    pub fn restore(&self, name: &str) -> Result<Data, String> {
        if name.contains(['/', '\\', '.']) || !name.starts_with("backup-") { return Err(crate::tr!("Unknown backup.")); }
        let dir = self.root.join("backups").join(name);
        if !dir.join("COMPLETE").exists() { return Err(crate::tr!("That backup is incomplete and cannot be restored.")); }
        let manifest: Manifest = serde_json::from_slice(&fs::read(dir.join("manifest.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let mut data: Data = serde_json::from_slice::<Data>(&fs::read(dir.join("data.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?.upgrade();
        if data.format > SUPPORTED_FORMAT { return Err(crate::theme::strings::newer_data().into()); }
        // An older backup comes back in this build's format; the data it replaces is copied aside below.
        self.upgrade_format(&mut data, false);
        data.after_load();
        let store = self.root.join("backups").join("assets");
        for a in &manifest.assets {
            let dst = self.asset_path(a);
            if dst.exists() { continue; }
            let src = store.join(a);
            fs::copy(&src, &dst).map_err(|e| format!("Backup asset {a} is missing: {e}"))?;
        }
        // Keep the data we are replacing, just in case.
        if self.data_path().exists() { let _ = fs::copy(self.data_path(), self.root.join(format!("data.before-restore-{}.json", now_stamp()))); }
        self.save(&data).map_err(|e| e.to_string())?;
        Ok(data)
    }

    fn prune_backups(&self) -> io::Result<()> {
        let all = self.backups();
        let kept = kept_backups(&all);
        for old in all.iter().filter(|b| !kept.contains(&b.name)) {
            let _ = fs::remove_dir_all(self.root.join("backups").join(&old.name));
        }
        // Drop backup asset copies no retained backup references.
        let mut keep = std::collections::BTreeSet::new();
        for b in all.iter().filter(|b| kept.contains(&b.name)) {
            if let Ok(bytes) = fs::read(self.root.join("backups").join(&b.name).join("manifest.json")) {
                if let Ok(m) = serde_json::from_slice::<Manifest>(&bytes) { keep.extend(m.assets); }
            }
        }
        if let Ok(rd) = fs::read_dir(self.root.join("backups").join("assets")) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if !keep.contains(&n) { let _ = fs::remove_file(e.path()); }
            }
        }
        Ok(())
    }

    /// Clean temp files left by an interrupted import (startup only).
    pub fn clean_tmp(&self) {
        if let Ok(rd) = fs::read_dir(self.tmp()) { for e in rd.flatten() { let p = e.path(); if p.is_dir() { let _ = fs::remove_dir_all(p); } else { let _ = fs::remove_file(p); } } }
    }
}

/// The label of a backup name (`backup-2026-11-06_101530-before-update` → `before-update`).
pub fn backup_label(name: &str) -> Option<&str> {
    let stamp_end = "backup-".len() + "2026-01-01_000000".len();
    name.get(stamp_end..).and_then(|rest| rest.strip_prefix('-')).filter(|l| !l.is_empty())
}

/// The backups pruning keeps, from a list newest first: the newest of each label, then the newest
/// `KEEP_BACKUPS` of the others.
fn kept_backups(all: &[BackupInfo]) -> std::collections::BTreeSet<String> {
    let mut labels = std::collections::BTreeSet::new();
    let mut kept = std::collections::BTreeSet::new();
    let mut others = 0;
    for b in all {
        match backup_label(&b.name) {
            Some(l) if labels.insert(l.to_string()) => { kept.insert(b.name.clone()); }
            _ if others < KEEP_BACKUPS => { others += 1; kept.insert(b.name.clone()); }
            _ => {}
        }
    }
    kept
}

pub fn sanitize_ext(ext: &str) -> String {
    let e: String = ext.trim_start_matches('.').chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>().to_ascii_lowercase();
    if e.is_empty() { "bin".into() } else { e }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// UTC timestamp like `2026-10-29_201530` without extra crates.
pub fn now_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}_{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
pub(crate) fn test_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("rp-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&d).unwrap();
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    #[test]
    fn backup_restores_into_empty_folder_without_original_assets() {
        let a = test_dir();
        let store = Store::open(a.clone()).unwrap();
        let src = a.join("clip.mp4");
        fs::write(&src, b"video-bytes").unwrap();
        let asset = store.import_file(&src, "mp4").unwrap();
        let mut data = Data::default();
        data.media.push(MediaFile { id: uuid::Uuid::new_v4(), name: "clip".into(), kind: MediaKind::Video, asset: asset.clone(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() });
        data.decks.push(Deck { id: uuid::Uuid::new_v4(), title: "d".into(), slides: vec![asset.clone()], notes: vec![], transitions: vec![], advance_ms: vec![], show: None });
        let name = store.backup(&data).unwrap();
        // Lose the working asset store and data.
        fs::remove_dir_all(store.assets()).unwrap();
        fs::create_dir_all(store.assets()).unwrap();
        fs::remove_file(store.data_path()).ok();
        let restored = store.restore(&name).unwrap();
        assert_eq!(restored.media.len(), 1);
        assert_eq!(fs::read(store.asset_path(&asset)).unwrap(), b"video-bytes");
    }

    #[test]
    fn partial_backups_are_not_offered() {
        let a = test_dir();
        let store = Store::open(a.clone()).unwrap();
        fs::create_dir_all(a.join("backups/backup-2026-01-01_000000.partial")).unwrap();
        fs::create_dir_all(a.join("backups/backup-2026-01-02_000000")).unwrap(); // no COMPLETE
        assert!(store.backups().is_empty());
    }

    /// A data.json as v0.2 wrote it: format 1, a picture background on the church and on one item.
    const V02: &str = r##"{"format":1,"settings":{"theme":{"font":"Segoe UI","size":84,"color":"#ffffff","shadow":true,"position":"center","uppercase":false,"scripture_size":64},
        "background":{"type":"image","asset":"sky.png"},"logo":null,"lines_per_slide":2},
        "songs":[],"media":[],"decks":[],"services":[{"id":"6f7b3c1e-1a2b-4c3d-8e9f-001122334455","name":"Sunday","items":[
        {"id":"6f7b3c1e-1a2b-4c3d-8e9f-001122334466","kind":"text","title":"Welcome","body":"Hi","background":{"type":"video","asset":"loop.mp4"},"notes":""}]}],
        "current_service":"6f7b3c1e-1a2b-4c3d-8e9f-001122334455"}"##;

    fn with_data(json: &str) -> Store {
        let store = Store::open(test_dir()).unwrap();
        fs::write(store.data_path(), json).unwrap();
        store
    }

    fn labeled(store: &Store) -> Vec<String> {
        store.backups().into_iter().map(|b| b.name).filter(|n| backup_label(n) == Some("before-update")).collect()
    }

    #[test]
    fn old_data_json_loads_with_defaults() {
        let store = with_data(V02);
        let l = store.load_full().unwrap();
        assert!(!l.read_only);
        assert_eq!(l.data.settings.theme, crate::model::Theme::default());
        assert_eq!(l.data.settings.background, Background::Image { asset: "sky.png".into(), dim: 0 });
        assert_eq!(l.data.services[0].items[0].background, Some(Background::Video { asset: "loop.mp4".into(), dim: 0 }));
        assert_eq!(l.data.format, SUPPORTED_FORMAT);
    }

    #[test]
    fn migration_saves_immediately_and_does_not_rerun() {
        let store = with_data(V02);
        let first = store.load().unwrap();
        assert_eq!(first.backgrounds.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Background 1", "Background 2"]);
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(store.data_path()).unwrap()).unwrap();
        assert_eq!(saved["format"], SUPPORTED_FORMAT, "saved at once");
        let again = store.load().unwrap();
        assert_eq!(again.backgrounds.len(), 2);
        assert_eq!(again.backgrounds[0].id, first.backgrounds[0].id, "not migrated a second time");
        let copies = fs::read_dir(&store.root).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("data.before-update")).count();
        assert_eq!(labeled(&store).len() + copies, 1, "one before-update backup across loads");
    }

    #[test]
    fn migration_makes_labeled_backup_first() {
        let store = with_data(V02);
        for a in ["sky.png", "loop.mp4"] { fs::write(store.asset_path(a), a).unwrap(); }
        store.load().unwrap();
        let names = labeled(&store);
        assert_eq!(names.len(), 1);
        let backed: serde_json::Value = serde_json::from_slice(&fs::read(store.root.join("backups").join(&names[0]).join("data.json")).unwrap()).unwrap();
        assert_eq!(backed["format"], 1, "the backup holds the data as it was");
        assert!(backed.get("backgrounds").is_none());
    }

    #[test]
    fn migration_with_missing_asset_still_loads() {
        let store = with_data(V02);
        let d = store.load().unwrap();
        assert_eq!(d.format, SUPPORTED_FORMAT);
        assert!(labeled(&store).is_empty(), "the assets are missing, so no backup");
        let copy = fs::read_dir(&store.root).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with("data.before-update-")).expect("a copy instead");
        assert_eq!(fs::read_to_string(copy.path()).unwrap(), V02);
    }

    #[test]
    fn migration_chain_backs_up_before_each_step() {
        let store = with_data(r#"{"format":0}"#);
        let d = store.load().unwrap();
        assert_eq!(d.format, SUPPORTED_FORMAT);
        assert_eq!(labeled(&store).len(), 1, "format 0 and 1 are one step");
    }

    #[test]
    fn labeled_backup_survives_prune() {
        let store = Store::open(test_dir()).unwrap();
        let data = Data::default();
        let first = store.backup_labeled(&data, "before-update").unwrap();
        // Stamps are per second: write the rest under later names by hand.
        for i in 0..KEEP_BACKUPS + 2 {
            let dir = store.root.join("backups").join(format!("backup-2099-01-01_0000{i:02}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("data.json"), serde_json::to_vec(&data).unwrap()).unwrap();
            fs::write(dir.join("manifest.json"), br#"{"created":"x","assets":[]}"#).unwrap();
            fs::write(dir.join("COMPLETE"), b"ok").unwrap();
        }
        store.prune_backups().unwrap();
        let names: Vec<String> = store.backups().into_iter().map(|b| b.name).collect();
        assert!(names.contains(&first), "{names:?}");
        assert_eq!(names.len(), KEEP_BACKUPS + 1);
        assert_eq!(backup_label("backup-2026-11-06_101530"), None);
        assert_eq!(backup_label(&first), Some("before-update"));
    }

    #[test]
    fn newer_format_opens_read_only() {
        let store = with_data(&V02.replacen(r#""format":1"#, r#""format":99"#, 1));
        let l = store.load_full().unwrap();
        assert!(l.read_only);
        assert_eq!(l.data.services[0].name, "Sunday", "what it holds is shown");
        assert!(l.data.backgrounds.is_empty(), "never migrated");
        assert!(fs::read_to_string(store.data_path()).unwrap().contains(r#""format":99"#), "never saved over");
    }

    #[test]
    fn newer_format_unparseable_still_starts_read_only() {
        let store = with_data(r#"{"format":99,"services":"a shape this build can't read"}"#);
        let l = store.load_full().unwrap();
        assert!(l.read_only);
        assert_eq!(l.data.services.len(), 1, "empty data");
    }

    #[test]
    fn syntax_error_is_still_could_not_be_read() {
        let store = with_data("{\"format\":2,");
        assert!(store.load_full().unwrap_err().contains("could not be read"));
        let store = with_data(r#"{"format":1,"services":7}"#);
        assert!(store.load_full().unwrap_err().contains("could not be read"), "an older file that does not parse");
    }

    #[test]
    fn after_load_is_the_only_hook_and_saves_when_it_returns_true() {
        // An overlay whose bold runs past its text: `after_load` trims it, and the load saves that at once.
        let mut v: serde_json::Value = serde_json::to_value(Data::default()).unwrap();
        v["overlays"] = serde_json::json!([{ "id": "6f7b3c1e-1a2b-4c3d-8e9f-001122334477", "name": "Hi",
            "source": { "type": "text", "text": "Hello", "styles": [{ "start": 0, "end": 99, "bold": true }] } }]);
        let store = with_data(&v.to_string());
        let d = store.load().unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(store.data_path()).unwrap()).unwrap();
        assert_eq!(saved["overlays"][0]["source"]["styles"][0]["end"], 5, "saved after the fix-up");
        assert!(serde_json::to_value(&d.overlays).unwrap()[0]["source"]["styles"][0]["end"] == 5);
        let read_only = with_data(&v.to_string().replacen(&format!("\"format\":{SUPPORTED_FORMAT}"), "\"format\":99", 1));
        read_only.load_full().unwrap();
        assert!(fs::read_to_string(read_only.data_path()).unwrap().contains("\"end\":99"), "never saved when read-only");
    }

    #[test]
    fn date_math() { assert_eq!(civil(0), (1970, 1, 1)); assert_eq!(civil(20390), (2025, 10, 29)); }
}
