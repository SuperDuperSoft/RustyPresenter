//! Data folder: `data.json`, content-addressed `assets/`, and complete backups
//! that carry their own copies of every asset they reference (review R2-04).
use crate::model::{AssetId, Data};
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

    pub fn load(&self) -> Result<Data, String> {
        match fs::read_to_string(self.data_path()) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!(
                "Your saved data at {} could not be read ({e}). Nothing was overwritten. Restore a backup from the backups folder or move the file aside.",
                self.data_path().display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Data::default()),
            Err(e) => Err(format!("Cannot read {}: {e}", self.data_path().display())),
        }
    }

    pub fn save(&self, data: &Data) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(data).map_err(io::Error::other)?;
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
    pub fn backup(&self, data: &Data) -> io::Result<String> {
        let created = now_stamp();
        // Names are to the second: a second backup in the same second gets `-2`, `-3`, … rather than failing.
        let free = |n: &str| !self.root.join("backups").join(n).exists() && !self.root.join("backups").join(format!("{n}.partial")).exists();
        let name = (1..).map(|i| if i == 1 { format!("backup-{created}") } else { format!("backup-{created}-{i}") }).find(|n| free(n)).unwrap();
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
        fs::write(partial.join("data.json"), serde_json::to_vec_pretty(data).map_err(io::Error::other)?)?;
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
        if name.contains(['/', '\\', '.']) || !name.starts_with("backup-") { return Err("Unknown backup.".into()); }
        let dir = self.root.join("backups").join(name);
        if !dir.join("COMPLETE").exists() { return Err("That backup is incomplete and cannot be restored.".into()); }
        let manifest: Manifest = serde_json::from_slice(&fs::read(dir.join("manifest.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let data: Data = serde_json::from_slice(&fs::read(dir.join("data.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
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
        for old in all.iter().skip(KEEP_BACKUPS) {
            let _ = fs::remove_dir_all(self.root.join("backups").join(&old.name));
        }
        // Drop backup asset copies no retained backup references.
        let mut keep = std::collections::BTreeSet::new();
        for b in all.iter().take(KEEP_BACKUPS) {
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
        data.media.push(MediaFile { id: uuid::Uuid::new_v4(), name: "clip".into(), kind: MediaKind::Video, asset: asset.clone(), status: MediaStatus::Ready, original_asset: None, folder: String::new(), thumb: None });
        data.decks.push(Deck { id: uuid::Uuid::new_v4(), title: "d".into(), slides: vec![asset.clone()] });
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

    #[test]
    fn two_backups_in_one_second_are_both_kept() {
        let store = Store::open(test_dir()).unwrap();
        let data = Data::default();
        let (first, second) = (store.backup(&data).unwrap(), store.backup(&data).unwrap());
        assert_ne!(first, second);
        assert_eq!(store.backups().len(), 2);
    }

    #[test]
    fn date_math() { assert_eq!(civil(0), (1970, 1, 1)); assert_eq!(civil(20390), (2025, 10, 29)); }
}
