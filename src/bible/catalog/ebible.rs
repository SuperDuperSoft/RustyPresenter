//! The eBible.org catalog (Bible library spec §3.1): `translations.csv`, fetched with a conditional GET at most once
//! a day, kept in `bibles/catalog/` so the list still shows offline, and filtered to what the app may offer.
//!
//! The CSV has no licence column. A row's licence comes from the licence cache (an earlier download of the same
//! edition), else from the seed shipped in `assets/bibles/ebible-licences.json`, else it is checked on download.
use std::{collections::BTreeMap, fs, io, path::Path};

use serde::{Deserialize, Serialize};

use super::{CatalogError, CatalogRow, CatalogSource, Ctx, Fields, LicenceState, Listing, Scope};
use crate::bible::library::{Dir, Licence};

pub const BASE: &str = "https://ebible.org/Scriptures";
/// The real file is about 730 KB.
const MAX_CSV: usize = 8 << 20;
const DAY: i64 = 24 * 60 * 60;
const CSV_FILE: &str = "ebible-translations.csv";
const STATE_FILE: &str = "ebible.json";
const LICENCE_FILE: &str = "licences.json";
/// Scripts written right to left. Hidden until step 3 brings right-to-left layout and a Hebrew font.
const RTL_SCRIPTS: &[&str] = &["Hebr", "Arab", "Syrc", "Thaa"];
const SEED: &str = include_str!("../../../assets/bibles/ebible-licences.json");

/// What the last fetch was told, for the next conditional GET.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
#[serde(default)]
struct State { etag: String, last_modified: String, checked_at: i64 }

pub async fn refresh(ctx: &Ctx) -> Result<Listing, CatalogError> {
    let dir = ctx.dir();
    let saved = fs::read(dir.join(CSV_FILE)).ok();
    let mut state: State = fs::read(dir.join(STATE_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let fresh = ctx.now.saturating_sub(state.checked_at) < DAY && ctx.now >= state.checked_at;
    let (csv, offline) = match saved {
        Some(csv) if fresh && !ctx.force => (csv, false),
        saved => match get(ctx, &state, saved.is_some()).await {
            Ok(Got::NotModified) => {
                state.checked_at = ctx.now;
                save(&dir, None, &state)?;
                (saved.expect("asked conditionally only with a saved list"), false)
            }
            // Read before saving, so a broken answer (a maintenance page, a cut-off file) never replaces a good list.
            Ok(Got::Body { csv, etag, last_modified }) => match (parse(&csv), saved) {
                (Ok(rows), _) if !rows.is_empty() => {
                    state = State { etag, last_modified, checked_at: ctx.now };
                    save(&dir, Some(&csv), &state)?;
                    (csv, false)
                }
                (_, Some(saved)) => { eprintln!("eBible catalog: unreadable answer; showing the saved list"); (saved, true) }
                (Err(e), None) => return Err(CatalogError::Bad(e)),
                (Ok(_), None) => return Err(CatalogError::Bad(crate::tr!("no rows"))),
            },
            Err(e) => match saved {
                Some(csv) => { eprintln!("eBible catalog: {e:?}; showing the saved list"); (csv, true) }
                None => return Err(e),
            },
        },
    };
    let licences = Licences::load(&ctx.root);
    let rows = parse(&csv).map_err(CatalogError::Bad)?.iter().filter_map(|r| offer(r, &licences)).collect();
    Ok(Listing { rows, offline })
}

enum Got { NotModified, Body { csv: Vec<u8>, etag: String, last_modified: String } }

async fn get(ctx: &Ctx, state: &State, conditional: bool) -> Result<Got, CatalogError> {
    let client = crate::bible::sources::client().map_err(|_| CatalogError::Offline)?;
    let mut req = client.get(format!("{}/translations.csv", ctx.base.trim_end_matches('/')));
    if conditional {
        if !state.etag.is_empty() { req = req.header(reqwest::header::IF_NONE_MATCH, &state.etag); }
        if !state.last_modified.is_empty() { req = req.header(reqwest::header::IF_MODIFIED_SINCE, &state.last_modified); }
    }
    let mut resp = req.send().await.map_err(|e| {
        if e.is_connect() || e.is_timeout() || e.is_request() { CatalogError::Offline } else { CatalogError::Bad(e.to_string()) }
    })?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_MODIFIED && conditional { return Ok(Got::NotModified); }
    if !status.is_success() { return Err(CatalogError::Bad(crate::tr!("eBible answered {status}", status = status.as_u16()))); }
    let header = |h| resp.headers().get(h).and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok()).unwrap_or("").to_string();
    let (etag, last_modified) = (header(reqwest::header::ETAG), header(reqwest::header::LAST_MODIFIED));
    let mut csv = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|_| CatalogError::Offline)? {
        csv.extend_from_slice(&chunk);
        if csv.len() > MAX_CSV { return Err(CatalogError::Bad(crate::tr!("the list is far larger than expected"))); }
    }
    Ok(Got::Body { csv, etag, last_modified })
}

fn save(dir: &Path, csv: Option<&[u8]>, state: &State) -> Result<(), CatalogError> {
    let disk = |e: io::Error| CatalogError::Disk(e.to_string());
    fs::create_dir_all(dir).map_err(disk)?;
    if let Some(csv) = csv { crate::store::write_atomic(&dir.join(CSV_FILE), csv).map_err(disk)?; }
    crate::store::write_atomic(&dir.join(STATE_FILE), &serde_json::to_vec(state).map_err(|e| CatalogError::Disk(e.to_string()))?).map_err(disk)
}

// ---------- the CSV ----------

/// One line of `translations.csv`, by column name, so a reordered or wider file still reads.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Row {
    pub language_code: String,
    pub translation_id: String,
    pub language_name: String,
    pub language_name_en: String,
    pub title: String,
    pub short_title: String,
    pub description: String,
    pub redistributable: bool,
    pub downloadable: bool,
    pub copyright: String,
    pub update_date: String,
    pub ot_books: u32,
    pub nt_books: u32,
    pub dc_books: u32,
    pub text_direction: String,
    pub script: String,
}

/// Every row of the file. Fails only when the header lacks a column the app needs.
pub fn parse(bytes: &[u8]) -> Result<Vec<Row>, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut records = records(text.strip_prefix('\u{feff}').unwrap_or(&text)).into_iter();
    let header = records.next().ok_or_else(|| crate::tr!("empty file"))?;
    let col = |name: &str| header.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
    let need = |name: &str| col(name).ok_or_else(|| crate::tr!("no {column} column", column = name));
    let (code, id, title, redist, down) =
        (need("languageCode")?, need("translationId")?, need("title")?, need("Redistributable")?, need("downloadable")?);
    let opt = ["languageName", "languageNameInEnglish", "shortTitle", "description", "Copyright", "UpdateDate", "OTbooks",
        "NTbooks", "DCbooks", "textDirection", "script"].map(col);
    Ok(records.filter(|r| r.iter().any(|f| !f.trim().is_empty())).map(|r| {
        let get = |i: Option<usize>| i.and_then(|i| r.get(i)).map(|s| s.trim().to_string()).unwrap_or_default();
        let yes = |i: usize| get(Some(i)).eq_ignore_ascii_case("true");
        let n = |i: Option<usize>| get(i).parse().unwrap_or(0);
        Row {
            language_code: get(Some(code)), translation_id: get(Some(id)), language_name: get(opt[0]),
            language_name_en: get(opt[1]), title: get(Some(title)), short_title: get(opt[2]), description: get(opt[3]),
            redistributable: yes(redist), downloadable: yes(down), copyright: get(opt[4]), update_date: get(opt[5]),
            ot_books: n(opt[6]), nt_books: n(opt[7]), dc_books: n(opt[8]), text_direction: get(opt[9]), script: get(opt[10]),
        }
    }).collect())
}

/// RFC 4180: commas, quoted fields with `""` for a quote, line breaks inside quotes, CRLF or LF.
fn records(text: &str) -> Vec<Vec<String>> {
    let (mut out, mut rec, mut field) = (Vec::new(), Vec::new(), String::new());
    let (mut quoted, mut chars) = (false, text.chars().peekable());
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => { chars.next(); field.push('"'); }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') => quoted = true,
            (false, ',') => rec.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => { rec.push(std::mem::take(&mut field)); out.push(std::mem::take(&mut rec)); }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !rec.is_empty() { rec.push(field); out.push(rec); }
    out
}

/// The abbreviation shown for an eBible Bible: its id without the language code, trimmed of `-` and `_`,
/// uppercased, at most 10 characters (`spaRV1909` → `RV1909`, `eng-Brenton` → `BRENTON`). A curated one wins.
pub fn abbreviation(translation_id: &str, language_code: &str) -> String {
    let rest = match translation_id.get(..language_code.len()) {
        Some(head) if !language_code.is_empty() && head.eq_ignore_ascii_case(language_code) => &translation_id[language_code.len()..],
        _ => translation_id,
    };
    let rest = rest.trim_start_matches(['-', '_']);
    let pick = if rest.is_empty() { translation_id } else { rest };
    pick.to_uppercase().chars().take(10).collect()
}

/// The row as the app offers it, or `None` when it may not be offered: not redistributable, not downloadable,
/// right to left (until step 3), an id that can't be a file name, a licence already refused, or blocked.
pub fn offer(r: &Row, licences: &Licences) -> Option<CatalogRow> {
    if !r.redistributable || !r.downloadable || !super::install::valid_source_id(&r.translation_id) { return None; }
    let rtl = r.text_direction.eq_ignore_ascii_case("rtl") || RTL_SCRIPTS.iter().any(|s| s.eq_ignore_ascii_case(&r.script));
    if rtl { return None; }
    let scope = match (r.ot_books >= 39, r.nt_books >= 27) {
        (true, true) => Scope::Full,
        (false, true) => Scope::NewTestament,
        (true, false) => Scope::OldTestament,
        (false, false) => Scope::Portions,
    };
    let (licence, curated) = licences.state(&r.translation_id, &r.update_date);
    let title = if r.title.is_empty() { r.short_title.clone() } else { r.title.clone() };
    let f = Fields {
        abbr: curated.unwrap_or_else(|| abbreviation(&r.translation_id, &r.language_code)),
        source_id: r.translation_id.clone(), title: title.clone(),
        // The catalog gives one title; when it is not English, `shortTitle` is usually the same title, so English
        // names come only from the language columns.
        title_en: title, description: r.description.clone(), copyright: r.copyright.clone(),
        lang: r.language_code.clone(), lang_name: r.language_name.clone(), lang_name_en: r.language_name_en.clone(),
        dir: Dir::Ltr, script: r.script.clone(), update_date: r.update_date.clone(), has_dc: r.dc_books > 0,
    };
    CatalogRow::checked(CatalogSource::Ebible, f, scope, licence)
}

// ---------- licences ----------

/// One edition's licence, as read from its own notice at download. `None` means it had none the app may use.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Checked { pub update_date: String, pub licence: Option<Licence> }

#[derive(Deserialize, Default)]
#[serde(default)]
struct SeedEntry { licence: Option<Licence>, abbr: Option<String> }

#[derive(Deserialize, Default)]
#[serde(default)]
struct Seed { v: u32, bibles: BTreeMap<String, SeedEntry> }

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct CacheFile { v: u32, bibles: BTreeMap<String, Checked> }

/// The licence cache and the shipped seed together.
#[derive(Default)]
pub struct Licences { seed: BTreeMap<String, SeedEntry>, cache: BTreeMap<String, Checked> }

impl Licences {
    pub fn load(root: &Path) -> Self {
        let seed = serde_json::from_str::<Seed>(SEED).ok().filter(|s| s.v == 1).map(|s| s.bibles).unwrap_or_default();
        let cache = fs::read(root.join("bibles/catalog").join(LICENCE_FILE)).ok()
            .and_then(|b| serde_json::from_slice::<CacheFile>(&b).ok()).filter(|c| c.v == 1).map(|c| c.bibles).unwrap_or_default();
        Licences { seed, cache }
    }

    /// What is known before download, and the seed's curated abbreviation. A cached answer counts only for the
    /// edition it was read from: a new `UpdateDate` is checked again.
    fn state(&self, id: &str, update_date: &str) -> (LicenceState, Option<String>) {
        let seed = self.seed.get(id);
        let curated = seed.and_then(|s| s.abbr.clone()).filter(|a| !a.trim().is_empty());
        let state = match self.cache.get(id) {
            Some(c) if c.update_date == update_date => match &c.licence {
                Some(l) => LicenceState::Known(l.clone()),
                None => LicenceState::Refused,
            },
            _ => match seed.and_then(|s| s.licence.clone()) {
                Some(Licence::Unknown) | None => LicenceState::OnDownload,
                Some(l) => LicenceState::Known(l),
            },
        };
        (state, curated)
    }

    /// Records what a download found. Called by the download job (step 1 item 5).
    pub fn record(root: &Path, id: &str, checked: Checked) -> io::Result<()> {
        let dir = root.join("bibles/catalog");
        fs::create_dir_all(&dir)?;
        let mut file = fs::read(dir.join(LICENCE_FILE)).ok()
            .and_then(|b| serde_json::from_slice::<CacheFile>(&b).ok()).filter(|c| c.v == 1).unwrap_or_default();
        file.v = 1;
        file.bibles.insert(id.to_string(), checked);
        crate::store::write_atomic(&dir.join(LICENCE_FILE), &serde_json::to_vec_pretty(&file).map_err(io::Error::other)?)
    }
}

#[cfg(test)]
mod tests;
