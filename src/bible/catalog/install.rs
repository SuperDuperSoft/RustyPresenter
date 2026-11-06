//! Adding a Bible from a catalog (Bible library spec §6, step 1 item 5): download to `bibles/.partial/`, check
//! everything, then save it with `import::install`, which writes the file atomically and updates the index.
//!
//! Nothing is kept unless every check passes: the zip opens, `{id}_vpl.xml` reads and has verses, the notice gives a
//! licence the app may use, and nothing about it is blocked. The partial file goes when the job ends however it
//! ends, and `clean_partials` empties the folder at start-up, so a crash or a forced quit leaves nothing either. An
//! update that fails a check leaves the installed copy as it was.
use std::{
    fs,
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use tokio_util::sync::CancellationToken;

use super::{CatalogRow, CatalogSource, Ctx, ebible::{Checked, Licences}, licence, strings};
use crate::bible::{
    id,
    import::{self, ImportedBible},
    library::{BibleMeta, Source},
};

/// The real zips are 1 to 6 MB.
pub const MAX_ZIP: u64 = 50 << 20;
/// The largest real `_vpl.xml` is about 25 MB.
pub const MAX_XML: u64 = 200 << 20;
const MAX_ABOUT: u64 = 1 << 20;
/// Free space is checked again after each of these.
const CHECK_EVERY: u64 = 5 << 20;
/// A download with no `Content-Length` needs this much free space to start.
const NEED_UNKNOWN: u64 = 250 << 20;
/// How long a whole download may take. The shared client's 15 seconds is for short API calls.
const TIMEOUT: Duration = Duration::from_secs(10 * 60);
const PARTIAL: &str = "bibles/.partial";
const USER_AGENT: &str = concat!("RustyPresenter/", env!("CARGO_PKG_VERSION"), " (+https://github.com/SuperDuperSoft/RustyPresenter)");

/// Why a Bible wasn't added. `message` says it in plain words (spec §13).
#[derive(Debug, PartialEq, Eq)]
pub enum InstallError {
    Cancelled,
    /// The internet or the server failed.
    Download(String),
    /// Its notice gives no licence the app may use.
    Licence,
    TooLarge,
    /// Megabytes it needs.
    NoSpace(u64),
    Blocked,
    /// Anything else about the file: a damaged zip, no verses, a bad id, a write that failed. The reason is for the
    /// log; the operator sees `UNUSABLE`.
    Unusable(String),
}

impl InstallError {
    /// What the job card says. `update` is true when a copy is installed already and stays.
    pub fn message(&self, name: &str, update: bool) -> String {
        let named = |s: &str| s.replace("{name}", name);
        match self {
            InstallError::Cancelled => crate::import::CANCELLED.into(),
            InstallError::Download(_) => named(strings::DOWNLOAD_FAILED),
            InstallError::NoSpace(mb) => strings::NO_SPACE.replace("{n}", &mb.to_string()),
            InstallError::Blocked => crate::scripture::strings::BLOCKED.into(),
            _ if update => named(strings::UPDATE_UNUSABLE),
            InstallError::Licence => strings::LICENCE_REFUSED.into(),
            InstallError::TooLarge => strings::TOO_LARGE.into(),
            InstallError::Unusable(_) => named(strings::UNUSABLE),
        }
    }
}

/// A source's own id may become part of a file name: letters, digits, `_` and `-`, at most 40, never a Windows
/// device name (spec §6).
pub fn valid_source_id(s: &str) -> bool {
    let upper = s.to_ascii_uppercase();
    let device = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4 && (upper.starts_with("COM") || upper.starts_with("LPT")) && upper.as_bytes()[3].is_ascii_digit());
    (1..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') && !device
}

/// Empties `bibles/.partial/`. Called once at start-up, before any download can start.
pub fn clean_partials(root: &Path) {
    let dir = root.join(PARTIAL);
    if dir.exists() && let Err(e) = fs::remove_dir_all(&dir) { eprintln!("Bible downloads: couldn't empty {}: {e}", dir.display()); }
}

/// Deletes the partial file when the job ends, however it ends.
struct Partial(PathBuf);

impl Drop for Partial {
    fn drop(&mut self) { let _ = fs::remove_file(&self.0); }
}

fn megabytes(bytes: u64) -> u64 { bytes.div_ceil(1 << 20) }

/// Fails when the disk holding `dir` has less than `need` bytes free. A system that won't say lets it go ahead.
fn room_for(dir: &Path, need: u64) -> Result<(), InstallError> {
    match crate::platform::disk::free_space(dir) {
        Some(free) if free < need => Err(InstallError::NoSpace(megabytes(need))),
        _ => Ok(()),
    }
}

/// Downloads and adds one eBible.org Bible. `progress` gets 0 to 100.
pub async fn ebible(ctx: &Ctx, row: &CatalogRow, cancel: &CancellationToken, progress: &(dyn Fn(u8) + Send + Sync)) -> Result<BibleMeta, InstallError> {
    if row.source() != CatalogSource::Ebible { return Err(InstallError::Unusable("not an eBible row".into())); }
    let sid = row.source_id().to_string();
    if !valid_source_id(&sid) { return Err(InstallError::Unusable(format!("the id {sid:?} can't be used"))); }
    let partial_dir = ctx.root.join(PARTIAL);
    fs::create_dir_all(&partial_dir).map_err(|e| InstallError::Unusable(e.to_string()))?;
    let path = partial_dir.join(format!("{}.zip", id::file_stem(row.id())));
    let _partial = Partial(path.clone());

    download(ctx, &format!("{}/{sid}_vpl.zip", ctx.base.trim_end_matches('/')), &path, cancel, &|p| progress((u16::from(p) * 4 / 5) as u8)).await?;
    if cancel.is_cancelled() { return Err(InstallError::Cancelled); }

    let unpack_path = path.clone();
    let unpack_sid = sid.clone();
    let (verses, about) = tokio::task::spawn_blocking(move || unpack(&unpack_path, &unpack_sid)).await
        .map_err(|e| InstallError::Unusable(e.to_string()))??;
    progress(90);

    let licence = about.as_deref().and_then(|html| licence::read(&sid, html));
    let checked = Checked { update_date: row.update_date().to_string(), licence: licence.clone() };
    if let Err(e) = Licences::record(&ctx.root, &sid, checked) { eprintln!("Bible downloads: licence cache not saved: {e}"); }
    let Some(licence) = licence else { return Err(InstallError::Licence) };
    if cancel.is_cancelled() { return Err(InstallError::Cancelled); }

    let meta = BibleMeta {
        id: row.id().to_string(), abbr: row.abbr().to_string(), name: row.title().to_string(), name_en: row.title_en().to_string(),
        lang: row.lang().to_string(), lang_name: row.lang_name().to_string(), lang_name_en: row.lang_name_en().to_string(),
        dir: row.dir(), script: row.script().to_string(), copyright: row.copyright().to_string(), licence,
        source: Source::Ebible { update_date: row.update_date().to_string() },
        // Read from the text once versification is built (spec §5.4); every Bible offered in step 1 is English-style
        // or its numbering matches closely enough to look verses up by their own numbers.
        ..Default::default()
    };
    let bible = ImportedBible { abbreviation: meta.id.clone(), name: meta.name.clone(), copyright: meta.copyright.clone(), verses, meta: Some(meta) };
    let root = ctx.root.clone();
    let saved = tokio::task::spawn_blocking(move || import::install(&root, bible)).await.map_err(|e| InstallError::Unusable(e.to_string()))?;
    let meta = saved.map_err(|e| if e == crate::scripture::strings::BLOCKED { InstallError::Blocked } else { InstallError::Unusable(e) })?;
    progress(100);
    Ok(meta)
}

/// Streams `url` into `path`, within the size cap and the free disk space, until done or cancelled.
async fn download(ctx: &Ctx, url: &str, path: &Path, cancel: &CancellationToken, progress: &(dyn Fn(u8) + Send + Sync)) -> Result<(), InstallError> {
    let dir = path.parent().unwrap_or(&ctx.root).to_path_buf();
    let client = crate::bible::sources::client().map_err(|e| InstallError::Download(format!("{e:?}")))?;
    let send = client.get(url).header(reqwest::header::USER_AGENT, USER_AGENT).timeout(TIMEOUT).send();
    let mut resp = tokio::select! {
        _ = cancel.cancelled() => return Err(InstallError::Cancelled),
        r = send => r.map_err(|e| InstallError::Download(e.to_string()))?,
    };
    if !resp.status().is_success() { return Err(InstallError::Download(format!("the server answered {}", resp.status().as_u16()))); }
    let length = resp.content_length();
    if length.is_some_and(|l| l > MAX_ZIP) { return Err(InstallError::TooLarge); }
    let need = length.map_or(NEED_UNKNOWN, |l| l.saturating_mul(5));
    room_for(&dir, need)?;
    let mut file = fs::File::create(path).map_err(|e| InstallError::Unusable(e.to_string()))?;
    let (mut got, mut next_check) = (0u64, CHECK_EVERY);
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err(InstallError::Cancelled),
            c = resp.chunk() => c.map_err(|e| InstallError::Download(e.to_string()))?,
        };
        let Some(chunk) = chunk else { break };
        got += chunk.len() as u64;
        if got > MAX_ZIP { return Err(InstallError::TooLarge); }
        file.write_all(&chunk).map_err(|e| InstallError::Unusable(e.to_string()))?;
        if got >= next_check {
            next_check += CHECK_EVERY;
            room_for(&dir, need.saturating_sub(got).max(CHECK_EVERY))?;
        }
        if let Some(l) = length.filter(|l| *l > 0) { progress((got.min(l) * 100 / l) as u8); }
    }
    if length.is_some_and(|l| got < l) { return Err(InstallError::Download("the download was cut short".into())); }
    file.sync_all().map_err(|e| InstallError::Unusable(e.to_string()))?;
    progress(100);
    Ok(())
}

/// The verses of `{sid}_vpl.xml` and the text of `{sid}_about.htm`, if the zip has one.
fn unpack(path: &Path, sid: &str) -> Result<(import::Verses, Option<String>), InstallError> {
    let bad = |e: &dyn std::fmt::Display| InstallError::Unusable(e.to_string());
    let mut zip = zip::ZipArchive::new(fs::File::open(path).map_err(|e| bad(&e))?).map_err(|e| bad(&e))?;
    let xml_name = format!("{sid}_vpl.xml").to_ascii_lowercase();
    let about_name = format!("{sid}_about.htm").to_ascii_lowercase();
    let xml = entry(&mut zip, &xml_name, MAX_XML)?.ok_or_else(|| InstallError::Unusable(format!("no {xml_name} in the zip")))?;
    let about = entry(&mut zip, &about_name, MAX_ABOUT)?.map(|b| String::from_utf8_lossy(&b).into_owned());
    let verses = import::vpl(&xml).map_err(InstallError::Unusable)?;
    Ok((verses, about))
}

/// The entry whose file name (in any folder of the zip, any letter case) is `name`, read up to `cap` bytes. Larger
/// is `TooLarge`, whatever its header says, so a zip bomb stops at the cap.
fn entry<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str, cap: u64) -> Result<Option<Vec<u8>>, InstallError> {
    let at = (0..zip.len()).find(|&i| zip.by_index(i).ok().and_then(|f| f.name().ok().map(|n| {
        n.rsplit(['/', '\\']).next().unwrap_or("").eq_ignore_ascii_case(name)
    })).unwrap_or(false));
    let Some(at) = at else { return Ok(None) };
    let file = zip.by_index(at).map_err(|e| InstallError::Unusable(e.to_string()))?;
    if file.size() > cap { return Err(InstallError::TooLarge); }
    let mut out = Vec::new();
    file.take(cap + 1).read_to_end(&mut out).map_err(|e: io::Error| InstallError::Unusable(e.to_string()))?;
    if out.len() as u64 > cap { return Err(InstallError::TooLarge); }
    Ok(Some(out))
}

#[cfg(test)]
mod tests;
