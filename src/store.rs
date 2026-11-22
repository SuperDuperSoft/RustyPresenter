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
struct Manifest {
    created: String,
    assets: Vec<AssetId>,
    /// Files the data names that were not on the disk when the backup was made. The songs and services are the
    /// part that cannot be remade, so a lost picture or video never stops a backup.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    missing: Vec<AssetId>,
}

/// Backups are thinned by age, not by count: every one from the last two days (at most `KEEP_RECENT`), then the
/// newest of each day for two weeks, then the newest of each week for eight weeks. Ten starts in one week no longer
/// push out the backup from before a mistake.
const KEEP_RECENT: usize = 20;
/// The newest few are kept however old they are.
const KEEP_NEWEST: usize = 3;
const DAY: i64 = 86_400;
const KEEP_ALL_FOR: i64 = 2 * DAY;
const KEEP_DAILY_FOR: i64 = 14 * DAY;
const KEEP_WEEKLY_FOR: i64 = 56 * DAY;
static BACKUP_IO: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    /// a "before-update" backup; when that can't be made (the disk is full), `data.json` is copied aside instead.
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
    pub fn save(&self, data: &Data) -> io::Result<()> { self.write_data(&self.encode(data)?) }

    /// The bytes `save` writes to `data.json`. Quick, so it may run while the data is locked; the slow disk write
    /// (`write_data`) can then run after the lock is let go.
    pub fn encode(&self, data: &Data) -> io::Result<Vec<u8>> {
        let data = crate::scripture::online::without_online_text(data, &self.root);
        serde_json::to_vec_pretty(&*data).map_err(io::Error::other)
    }

    /// Writes bytes from `encode` to `data.json`, safely (a temp file, flushed to the disk, then renamed).
    pub fn write_data(&self, bytes: &[u8]) -> io::Result<()> { write_atomic(&self.data_path(), bytes) }

    /// Move a finished temp file into the asset store under its hash.
    pub fn publish(&self, tmp: &Path, sha: &str, ext: &str) -> io::Result<AssetId> {
        let id = format!("{sha}.{}", sanitize_ext(ext));
        let dest = self.asset_path(&id);
        if dest.is_file() {
            let _ = fs::remove_file(tmp);
        } else if let Err(e) = fs::rename(tmp, &dest) {
            let _ = fs::remove_file(tmp);
            if !dest.is_file() {
                return Err(e);
            }
        }
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

    /// A backup named `backup-<stamp>-<label>`. The newest one of each label is kept apart from the others
    /// (`kept_backups`), so app updates never push out the church's own backups.
    pub fn backup_labeled(&self, data: &Data, label: &str) -> io::Result<String> { self.backup_named(data, Some(label)) }

    fn backup_named(&self, data: &Data, label: Option<&str>) -> io::Result<String> {
        // Backup snapshots share the same asset pool. Keep pruning out of another backup or restore.
        let _guard = BACKUP_IO.lock().unwrap_or_else(|e| e.into_inner());
        // Names carry the time to the second. Two backups in one second (a restore right after a backup) would
        // collide and the second would fail, so it waits for the clock instead.
        let named = |created: &str| match label { Some(l) => format!("backup-{created}-{l}"), None => format!("backup-{created}") };
        let mut created = now_stamp();
        for _ in 0..8 {
            if !self.root.join("backups").join(named(&created)).exists() { break }
            std::thread::sleep(std::time::Duration::from_millis(250));
            created = now_stamp();
        }
        let name = named(&created);
        let dir = self.root.join("backups").join(&name);
        let partial = self.root.join("backups").join(format!("{name}.partial"));
        fs::create_dir_all(&partial)?;
        let store = self.root.join("backups").join("assets");
        let (mut assets, mut missing) = (vec![], vec![]);
        for a in data.referenced_assets() {
            let src = self.asset_path(&a);
            let dst = store.join(&a);
            // A file that has gone (deleted by hand, quarantined, lost in a partial copy) is left out and named in
            // the manifest; everything else is still saved.
            if !dst.is_file() && !src.is_file() { missing.push(a); continue; }
            if !dst.is_file() && fs::hard_link(&src, &dst).is_err() { copy_atomic(&src, &dst)?; }
            assets.push(a);
        }
        let data = crate::scripture::online::without_online_text(data, &self.root);
        // Each file reaches the disk before the marker is written, so a power cut can never leave a backup that is
        // marked complete with an empty `data.json`.
        write_synced(&partial.join("data.json"), &serde_json::to_vec_pretty(&*data).map_err(io::Error::other)?)?;
        write_synced(&partial.join("manifest.json"), &serde_json::to_vec_pretty(&Manifest { created: created.clone(), assets, missing }).map_err(io::Error::other)?)?;
        write_synced(&partial.join("COMPLETE"), b"ok")?;
        fs::rename(&partial, &dir)?;
        self.prune_backups()?;
        Ok(name)
    }

    /// The finished backups, newest first. The operator screen asks for this after every change, so each backup's
    /// `data.json` (the whole song library) is read only the first time it is listed: its details are remembered
    /// by its path and its `data.json`'s size and time, so a folder replaced under the same name (copied in from
    /// another computer) is read again.
    pub fn backups(&self) -> Vec<BackupInfo> {
        type Stamp = Option<(u64, Option<std::time::SystemTime>)>;
        static KNOWN: std::sync::Mutex<std::collections::BTreeMap<PathBuf, (Stamp, BackupInfo)>> = std::sync::Mutex::new(std::collections::BTreeMap::new());
        let mut list = vec![];
        let Ok(rd) = fs::read_dir(self.root.join("backups")) else { return list };
        let mut known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
        let mut seen = std::collections::BTreeSet::new();
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with("backup-") || name.ends_with(".partial") || !p.join("COMPLETE").exists() { continue; }
            seen.insert(p.clone());
            let stamp: Stamp = fs::metadata(p.join("data.json")).ok().map(|m| (m.len(), m.modified().ok()));
            if let Some((s, info)) = known.get(&p) && *s == stamp { list.push(info.clone()); continue; }
            #[cfg(test)]
            BACKUPS_READ.with(|n| n.set(n.get() + 1));
            let manifest: Option<Manifest> = fs::read(p.join("manifest.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
            let data: Option<Data> = fs::read(p.join("data.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
            match (manifest, data) {
                (Some(m), Some(d)) => {
                    let info = BackupInfo { name, created: m.created, assets: m.assets.len(), songs: d.songs.len(), services: d.services.len() };
                    known.insert(p, (stamp, info.clone()));
                    list.push(info);
                }
                _ => { known.remove(&p); }
            }
        }
        // Forget backups of this folder that were pruned; other data folders (tests) keep theirs.
        let backups = self.root.join("backups");
        known.retain(|p, _| !p.starts_with(&backups) || seen.contains(p));
        list.sort_by(|a, b| b.name.cmp(&a.name));
        list
    }

    /// The newest backup already holds exactly this data, so another copy would say nothing new.
    pub fn backed_up(&self, data: &Data) -> bool {
        let Some(newest) = self.backups().into_iter().next() else { return false };
        let Ok(now) = self.encode(data) else { return false };
        fs::read(self.root.join("backups").join(&newest.name).join("data.json")).is_ok_and(|saved| saved == now)
    }

    /// Restore a backup using only the backup set (works into an empty data folder).
    pub fn restore(&self, name: &str) -> Result<Data, String> {
        let _guard = BACKUP_IO.lock().unwrap_or_else(|e| e.into_inner());
        if name.contains(['/', '\\', '.']) || !name.starts_with("backup-") { return Err(crate::tr!("Unknown backup.")); }
        let dir = self.root.join("backups").join(name);
        if !dir.join("COMPLETE").exists() { return Err(crate::tr!("That backup is incomplete and cannot be restored.")); }
        let manifest: Manifest = serde_json::from_slice(&fs::read(dir.join("manifest.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let mut data: Data = serde_json::from_slice::<Data>(&fs::read(dir.join("data.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?.upgrade();
        if data.format > SUPPORTED_FORMAT { return Err(crate::theme::strings::newer_data().into()); }
        // An older backup comes back in this build's format; the data it replaces is copied aside below.
        self.upgrade_format(&mut data, false);
        data.after_load();
        let mut assets = data.referenced_assets();
        assets.extend(manifest.assets.iter().cloned());
        // Older data used plain filenames as IDs, so accept those too, provided they stay in the asset folder.
        if let Some(a) = assets.iter().find(|a| !asset_filename(a)) {
            return Err(format!("Invalid backup asset name: {a:?}"));
        }
        let store = self.root.join("backups").join("assets");
        for a in &assets {
            let dst = self.asset_path(a);
            if dst.is_file() { continue; }
            let src = store.join(a);
            // A file that was already gone when the backup was made stays gone; the rest comes back.
            if manifest.missing.contains(a) && !src.is_file() { continue; }
            copy_atomic(&src, &dst).map_err(|e| format!("Could not restore backup asset {a}: {e}"))?;
        }
        // Keep the data we are replacing, just in case.
        if self.data_path().exists() {
            let previous = self.root.join(format!("data.before-restore-{}-{}.json", now_stamp(), uuid::Uuid::new_v4()));
            copy_atomic(&self.data_path(), &previous).map_err(|e| e.to_string())?;
        }
        self.save(&data).map_err(|e| e.to_string())?;
        Ok(data)
    }

    fn prune_backups(&self) -> io::Result<()> {
        let all = self.backups();
        let kept = kept_backups(&all, now_secs());
        for old in all.iter().filter(|b| !kept.contains(&b.name)) {
            let _ = fs::remove_dir_all(self.root.join("backups").join(&old.name));
        }
        // Drop backup asset copies that no backup left on the disk names. Every backup folder counts, also one this
        // build cannot list (made by a newer RustyPresenter, or still being written): if any of them cannot say which
        // files it needs, nothing is dropped this time.
        let Some(keep) = self.backup_assets_in_use() else { return Ok(()) };
        if let Ok(rd) = fs::read_dir(self.root.join("backups").join("assets")) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if !keep.contains(&n) { let _ = fs::remove_file(e.path()); }
            }
        }
        Ok(())
    }

    /// The asset copies named by the backup folders on the disk. None when a folder's list cannot be read.
    fn backup_assets_in_use(&self) -> Option<std::collections::BTreeSet<AssetId>> {
        let mut keep = std::collections::BTreeSet::new();
        for e in fs::read_dir(self.root.join("backups")).ok()?.flatten() {
            // A `.partial` folder is a backup that never finished; it can't be restored and holds no claim.
            if e.file_name() == "assets" || !e.path().is_dir() || e.file_name().to_string_lossy().ends_with(".partial") { continue }
            let manifest: Manifest = serde_json::from_slice(&fs::read(e.path().join("manifest.json")).ok()?).ok()?;
            keep.extend(manifest.assets);
        }
        Some(keep)
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

/// When a backup was made, in seconds since 1970, from its name (`backup-2026-11-06_101530…`).
fn backup_secs(name: &str) -> Option<i64> {
    let stamp = name.strip_prefix("backup-")?.get(.."2026-01-01_000000".len())?;
    let (date, time) = stamp.split_once('_')?;
    let mut ymd = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (ymd.next()??, ymd.next()??, ymd.next()??);
    if time.len() != 6 || !(1..=12).contains(&m) || !(1..=31).contains(&d) { return None }
    let part = |at: usize| time.get(at..at + 2)?.parse::<i64>().ok();
    Some(days_from_civil(y, m as u32, d as u32) * DAY + part(0)? * 3600 + part(2)? * 60 + part(4)?)
}

/// The backups pruning keeps at `now` (seconds since 1970), from a list newest first: the newest of each label,
/// the newest `KEEP_NEWEST`, every one from the last two days (at most `KEEP_RECENT`), the newest of each day for
/// two weeks and the newest of each week for eight weeks. A name whose time cannot be read is kept.
fn kept_backups(all: &[BackupInfo], now: i64) -> std::collections::BTreeSet<String> {
    let mut labels = std::collections::BTreeSet::new();
    let mut kept = std::collections::BTreeSet::new();
    let (mut days, mut weeks) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
    let (mut newest, mut recent) = (0, 0);
    for b in all {
        if let Some(l) = backup_label(&b.name) && labels.insert(l.to_string()) { kept.insert(b.name.clone()); continue }
        let age = backup_secs(&b.name).map(|made| (made, now - made));
        let keep = match age {
            None => true,
            Some((_, age)) if age < KEEP_ALL_FOR => { recent += 1; recent <= KEEP_RECENT }
            Some((made, age)) if age < KEEP_DAILY_FOR => days.insert(made.div_euclid(DAY)),
            Some((made, age)) if age < KEEP_WEEKLY_FOR => weeks.insert(made.div_euclid(7 * DAY)),
            Some(_) => false,
        };
        newest += 1;
        if keep || newest <= KEEP_NEWEST { kept.insert(b.name.clone()); }
    }
    kept
}

pub fn sanitize_ext(ext: &str) -> String {
    let e: String = ext.trim_start_matches('.').chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>().to_ascii_lowercase();
    if e.is_empty() { "bin".into() } else { e }
}

fn asset_filename(name: &str) -> bool {
    !name.contains(['/', '\\', ':', '\0']) && Path::new(name).file_name() == Some(std::ffi::OsStr::new(name))
}

/// Writes a new file and waits until it is on the disk.
fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_with(path, |file| file.write_all(bytes))
}

fn copy_atomic(source: &Path, destination: &Path) -> io::Result<()> {
    let mut source = fs::File::open(source)?;
    write_atomic_with(destination, |out| io::copy(&mut source, out).map(|_| ()))
}

fn write_atomic_with(path: &Path, write: impl FnOnce(&mut fs::File) -> io::Result<()>) -> io::Result<()> {
    let tmp = path.with_file_name(format!(".rp-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::File::create_new(&tmp)?;
    let written = write(&mut file).and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| fs::rename(&tmp, path));
    if result.is_err() { let _ = fs::remove_file(&tmp); }
    result
}

fn now_secs() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) }

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

/// The day number (days since 1970-01-01) of a date; `civil` the other way round.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let (era, yoe) = (y.div_euclid(400), y.rem_euclid(400));
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}

#[cfg(test)]
thread_local! {
    /// How many backup folders `Store::backups` read from the disk on this thread (the rest came from memory).
    static BACKUPS_READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
    fn atomic_write_preserves_the_destination_on_error_and_cleans_up() {
        let root = test_dir();
        let path = root.join("state.json");
        let other = root.join("state.json.tmp");
        fs::write(&other, b"another writer's temporary file").unwrap();
        write_atomic(&path, b"old").unwrap();
        let result = write_atomic_with(&path, |out| {
            out.write_all(b"partial")?;
            Err(io::Error::other("copy failed"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(path).unwrap(), b"old");
        assert_eq!(fs::read(other).unwrap(), b"another writer's temporary file");
        assert_eq!(fs::read_dir(root).unwrap().count(), 2);
    }

    #[test]
    fn restore_rejects_asset_paths_outside_the_backup_before_copying() {
        let store = Store::open(test_dir()).unwrap();
        let mut data = Data::default();
        store.save(&data).unwrap();
        let name = store.backup(&data).unwrap();
        let backup = store.root.join("backups").join(&name);
        data.settings.logo = Some("../data.json".into());
        fs::write(backup.join("data.json"), serde_json::to_vec(&data).unwrap()).unwrap();

        assert!(store.restore(&name).unwrap_err().contains("Invalid backup asset name"));
        assert_eq!(store.load().unwrap().settings.logo, None);
    }

    #[test]
    fn pruning_keeps_the_files_of_a_backup_this_build_cannot_read() {
        let store = Store::open(test_dir()).unwrap();
        // A backup from a newer build: complete, with a list of its files, but data this build cannot parse.
        let newer = store.root.join("backups").join("backup-2020-01-01_000000");
        fs::create_dir_all(&newer).unwrap();
        fs::write(newer.join("data.json"), br#"{"format":99,"songs":"not a list"}"#).unwrap();
        fs::write(newer.join("manifest.json"), br#"{"created":"2020-01-01_000000","assets":["aa.png"]}"#).unwrap();
        fs::write(newer.join("COMPLETE"), b"ok").unwrap();
        let copies = store.root.join("backups").join("assets");
        fs::write(copies.join("aa.png"), b"the newer backup's picture").unwrap();
        fs::write(copies.join("bb.png"), b"nobody's").unwrap();
        assert!(store.backups().is_empty(), "this build does not list it");

        store.backup(&Data::default()).unwrap();
        assert!(copies.join("aa.png").exists(), "still there for the build that can restore it");
        assert!(!copies.join("bb.png").exists(), "a copy no backup names still goes");

        // A folder that cannot say what it needs: nothing is dropped.
        fs::write(copies.join("cc.png"), b"unknown").unwrap();
        fs::write(newer.join("manifest.json"), b"torn").unwrap();
        store.backup(&Data::default()).unwrap();
        assert!(copies.join("cc.png").exists() && copies.join("aa.png").exists());
        assert_eq!(store.backups().len(), 2, "two backups in the same second both exist");
    }

    #[test]
    fn simultaneous_backups_get_distinct_complete_snapshots() {
        let store = Store::open(test_dir()).unwrap();
        let start = std::sync::Barrier::new(2);
        let backup = || {
            let data = Data::default();
            start.wait();
            store.backup(&data).unwrap()
        };
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(backup);
            let second = scope.spawn(backup);
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_ne!(first, second);
        assert_eq!(store.backups().len(), 2);
    }

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

    #[test]
    fn the_backup_list_reads_each_backups_data_once_and_drops_removed_ones() {
        let a = test_dir();
        let store = Store::open(a.clone()).unwrap();
        let mut data = Data::default();
        data.songs.push(Song { title: "Amazing Grace".into(), ..Song::default() });
        let name = store.backup(&data).unwrap();
        assert_eq!(store.backups()[0].songs, 1);
        // Listed again from memory: the folder's data is not read a second time.
        let read = BACKUPS_READ.with(|n| n.get());
        assert_eq!((store.backups().len(), store.backups()[0].songs), (1, 1));
        assert_eq!(BACKUPS_READ.with(|n| n.get()), read, "nothing was read from the disk");
        // P5: the folder replaced under the same name (copied in from another computer) is read again.
        data.songs.push(Song { title: "Be Thou My Vision".into(), ..Song::default() });
        fs::write(a.join("backups").join(&name).join("data.json"), serde_json::to_vec_pretty(&data).unwrap()).unwrap();
        assert_eq!(store.backups()[0].songs, 2, "the new folder's counts");
        fs::remove_dir_all(a.join("backups").join(&name)).unwrap();
        assert!(store.backups().is_empty(), "a removed backup is no longer listed");
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
        let names = labeled(&store);
        assert_eq!(names.len(), 1, "missing pictures do not stop the before-update backup");
        let backed: serde_json::Value = serde_json::from_slice(&fs::read(store.root.join("backups").join(&names[0]).join("data.json")).unwrap()).unwrap();
        assert_eq!(backed["format"], 1);
    }

    /// B2: one lost media file must not stop every backup.
    #[test]
    fn a_missing_asset_does_not_stop_a_backup_or_its_restore() {
        let store = Store::open(test_dir()).unwrap();
        let src = store.root.join("here.png");
        fs::write(&src, b"picture").unwrap();
        let here = store.import_file(&src, "png").unwrap();
        let mut data = Data::default();
        data.songs.push(Song { title: "Amazing Grace".into(), ..Song::default() });
        data.settings.logo = Some("gone.png".into());
        data.settings.background = Background::Image { asset: here.clone(), dim: 0 };
        store.save(&data).unwrap();
        let name = store.backup(&data).expect("the backup is still made");
        assert_eq!(store.backups().iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), [name.as_str()]);
        let manifest: Manifest = serde_json::from_slice(&fs::read(store.root.join("backups").join(&name).join("manifest.json")).unwrap()).unwrap();
        assert_eq!((manifest.missing, manifest.assets), (vec!["gone.png".to_string()], vec![here.clone()]));
        fs::remove_file(store.asset_path(&here)).unwrap();
        let restored = store.restore(&name).expect("and it restores");
        assert_eq!(restored.songs.len(), 1);
        assert_eq!(fs::read(store.asset_path(&here)).unwrap(), b"picture", "the files it does hold come back");
        assert!(!store.asset_path("gone.png").exists());
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
        for i in 0..KEEP_RECENT + 2 {
            let dir = store.root.join("backups").join(format!("backup-2099-01-01_0000{i:02}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("data.json"), serde_json::to_vec(&data).unwrap()).unwrap();
            fs::write(dir.join("manifest.json"), br#"{"created":"x","assets":[]}"#).unwrap();
            fs::write(dir.join("COMPLETE"), b"ok").unwrap();
        }
        store.prune_backups().unwrap();
        let names: Vec<String> = store.backups().into_iter().map(|b| b.name).collect();
        assert!(names.contains(&first), "{names:?}");
        assert_eq!(names.len(), KEEP_RECENT + 1);
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
    fn date_math() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20390), (2025, 10, 29));
        for days in [-1, 0, 59, 60, 11016, 20390, 47481] { let (y, m, d) = civil(days); assert_eq!(days_from_civil(y, m, d), days); }
        assert_eq!(backup_secs("backup-1970-01-02_000102"), Some(DAY + 62));
        assert_eq!(backup_secs("backup-2025-10-29_201530-before-update"), Some(20390 * DAY + 20 * 3600 + 15 * 60 + 30));
        assert_eq!(backup_secs("backup-yesterday"), None);
    }

    fn named(names: &[String]) -> Vec<BackupInfo> {
        let mut all: Vec<BackupInfo> = names.iter().map(|n| BackupInfo { name: n.clone(), created: String::new(), assets: 0, songs: 0, services: 0 }).collect();
        all.sort_by(|a, b| b.name.cmp(&a.name));
        all
    }

    /// B8: ten starts in a week used to push out every older backup.
    #[test]
    fn backups_are_thinned_by_age_not_by_count() {
        let now = backup_secs("backup-2026-11-22_120000").unwrap();
        // Twelve within one day, and one from three weeks before.
        let mut names: Vec<String> = (0..12).map(|i| format!("backup-2026-11-22_08{i:02}00")).collect();
        names.push("backup-2026-11-01_090000".into());
        let kept = kept_backups(&named(&names), now);
        assert!(kept.contains("backup-2026-11-01_090000"), "the three-week-old one is kept: {kept:?}");
        assert_eq!(kept.len(), 13, "and so is every one from today");

        // A long history: all of the last two days, one a day for two weeks, one a week for eight weeks.
        let mut names = vec![];
        for day in 0..120i64 {
            for hour in [9, 18] {
                let (y, m, d) = civil((now - day * DAY).div_euclid(DAY));
                names.push(format!("backup-{y:04}-{m:02}-{d:02}_{hour:02}0000"));
            }
        }
        let all = named(&names);
        let kept = kept_backups(&all, now);
        let age = |n: &String| now - backup_secs(n).unwrap();
        assert!(all.iter().filter(|b| age(&b.name) < KEEP_ALL_FOR).all(|b| kept.contains(&b.name)));
        let daily: Vec<&String> = kept.iter().filter(|n| (KEEP_ALL_FOR..KEEP_DAILY_FOR).contains(&age(n))).collect();
        // (The day at the two-day edge has its evening backup among the recent ones, so its morning one stays too.)
        assert!((11..=13).contains(&daily.len()) && daily.iter().filter(|n| !n.ends_with("_180000")).count() <= 1, "the newest of each day: {daily:?}");
        let weekly = kept.iter().filter(|n| (KEEP_DAILY_FOR..KEEP_WEEKLY_FOR).contains(&age(n))).count();
        assert!((6..=7).contains(&weekly), "{weekly}");
        assert!(kept.iter().all(|n| age(n) < KEEP_WEEKLY_FOR), "nothing older than eight weeks");

        // Only old backups left (the app was not opened for a year): the newest few stay.
        let old: Vec<String> = (1..=6).map(|d| format!("backup-2025-01-{d:02}_090000")).collect();
        let kept = kept_backups(&named(&old), now);
        assert_eq!(kept.into_iter().collect::<Vec<_>>(), ["backup-2025-01-04_090000", "backup-2025-01-05_090000", "backup-2025-01-06_090000"]);
        // Far more than a person makes in two days: capped.
        let many: Vec<String> = (0..50).map(|i| format!("backup-2026-11-22_10{i:02}00")).collect();
        assert_eq!(kept_backups(&named(&many), now).len(), KEEP_RECENT);
    }

    /// B8: a start with nothing changed does not make another copy.
    #[test]
    fn the_newest_backup_knows_whether_it_holds_this_data() {
        let store = Store::open(test_dir()).unwrap();
        let mut data = Data::default();
        data.songs.push(Song { title: "Amazing Grace".into(), ..Song::default() });
        assert!(!store.backed_up(&data), "no backup yet");
        store.backup(&data).unwrap();
        assert!(store.backed_up(&data));
        data.songs[0].title = "Amazing Grace (My Chains Are Gone)".into();
        assert!(!store.backed_up(&data), "the data changed since");
    }
}
