//! Online Bible catalogs (Bible library design §3.3-3.4, step 2): every Bible the church's YouVersion or API.Bible
//! key may read, listed in one search in Settings → Bible, and the ones the operator added (`bibles/online.json`).
//! An added Bible keeps its own name, language and credit line, so it still shows its credit when the catalog is
//! refreshed, lost, or drops it. Its translation id is `yv:{id}` or `ab:{id}` and its text stays online, under the
//! same cache rules as today's NIV/CSB routes (`sources::CachePolicy`).
use super::{blocklist, sources::{FetchError, Provider, tidy}, strings};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io, path::{Path, PathBuf}, sync::{Arc, Mutex}};

const YV: &str = "https://api.youversion.com/v1";
const AB: &str = "https://rest.api.bible/v1";
/// A catalog older than this is fetched again the next time Settings → Bible is searched.
const MAX_AGE_SECS: u64 = 7 * 86_400;
/// Safety stop for paging: 60 pages of 99 is more than YouVersion lists today.
const MAX_PAGES: usize = 60;

/// One online Bible as the app keeps it: in a catalog file, and in `online.json` once added.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct OnlineBible {
    /// Translation id: `yv:111`, `ab:de4e12af7f28f599-02`.
    pub id: String,
    /// Shown on chips and slides ("NIV", "RVR1960").
    pub abbr: String,
    /// Title in the Bible's own language.
    pub name: String,
    /// English title; may equal `name`.
    pub name_en: String,
    /// BCP 47 or ISO 639-3 ("en", "es", "spa").
    pub lang: String,
    /// Language name as the provider gives it (usually English).
    pub lang_name: String,
    /// The language's name in itself ("Español"), when the provider gives it.
    pub lang_local: String,
    /// "ltr" or "rtl".
    pub dir: String,
    /// The credit line exactly as the provider gives it, with HTML tags removed.
    pub copyright: String,
    /// Publisher or information link, if any.
    pub url: String,
}

/// `yv:` → YouVersion, `ab:` → API.Bible.
pub fn provider_of(id: &str) -> Option<Provider> {
    if id.starts_with("yv:") { Some(Provider::YouVersion) } else if id.starts_with("ab:") { Some(Provider::ApiBible) } else { None }
}

/// The YouVersion number of `yv:111`.
pub fn yv_number(id: &str) -> Option<u32> { id.strip_prefix("yv:")?.parse().ok() }

/// The opaque API.Bible id of `ab:…`, if it looks like one (letters, digits, `-`; it goes into a URL path).
pub fn ab_id(id: &str) -> Option<&str> {
    id.strip_prefix("ab:").filter(|s| !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

fn blocked(b: &OnlineBible) -> bool {
    blocklist::blocked_row(&b.lang, &b.abbr, &[&b.name, &b.name_en], &[&b.copyright], &[&b.id])
}

/// Removes tags and decodes the few entities credit lines carry (`&copy;`, `&amp;`, numeric).
pub fn plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => { in_tag = true; out.push(' '); }
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&copy;", "©").replace("&reg;", "®").replace("&trade;", "™").replace("&nbsp;", " ")
        .replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&");
    tidy(&out)
}

// ---------- added Bibles ----------

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct AddedFile { bibles: Vec<OnlineBible> }

/// Serialises `online.json` and catalog file access.
static FILES: Mutex<()> = Mutex::new(());

fn added_path(root: &Path) -> PathBuf { root.join("bibles").join("online.json") }

/// The online Bibles the operator added, in the order added. Blocked or malformed entries are dropped.
pub fn added(root: &Path) -> Vec<OnlineBible> {
    let _g = FILES.lock().unwrap();
    read_added(root)
}

fn read_added(root: &Path) -> Vec<OnlineBible> {
    let f: AddedFile = std::fs::read(added_path(root)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    f.bibles.into_iter().filter(|b| provider_of(&b.id).is_some() && !blocked(b)).collect()
}

fn write_added(root: &Path, bibles: Vec<OnlineBible>) -> io::Result<()> {
    let p = added_path(root);
    std::fs::create_dir_all(p.parent().unwrap())?;
    crate::store::write_atomic(&p, &serde_json::to_vec_pretty(&AddedFile { bibles }).map_err(io::Error::other)?)
}

pub fn find_added(root: &Path, id: &str) -> Option<OnlineBible> { added(root).into_iter().find(|b| b.id == id) }

/// Adds a Bible (already there: kept as it was). Refused when blocked or when it has no credit line to show.
pub fn add(root: &Path, b: OnlineBible) -> Result<(), String> {
    if blocked(&b) { return Err(strings::BLOCKED.into()); }
    if b.copyright.trim().is_empty() { return Err(strings::NO_CREDIT.into()); }
    if provider_of(&b.id).is_none() { return Err(strings::UNKNOWN_BIBLE.into()); }
    let _g = FILES.lock().unwrap();
    let mut all = read_added(root);
    if all.iter().any(|x| x.id == b.id) { return Ok(()); }
    all.push(b);
    write_added(root, all).map_err(|e| e.to_string())
}

/// Removes an added Bible; already gone is fine.
pub fn remove(root: &Path, id: &str) -> io::Result<()> {
    let _g = FILES.lock().unwrap();
    let mut all = read_added(root);
    let n = all.len();
    all.retain(|b| b.id != id);
    if all.len() == n { return Ok(()); }
    write_added(root, all)
}

// ---------- catalogs ----------

/// A provider's list as saved in `bibles/catalog/{youversion|apibible}.json`.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct CatalogFile { pub fetched: u64, pub bibles: Vec<OnlineBible> }

/// A catalog in memory, with each row's search text worked out once.
pub struct Indexed { pub fetched: u64, pub rows: Vec<(OnlineBible, String)> }

impl Indexed {
    pub fn new(f: CatalogFile) -> Self {
        let rows = f.bibles.into_iter().filter(|b| !blocked(b) && b.dir != "rtl").map(|b| {
            let words = [b.abbr.as_str(), &b.name, &b.name_en, &b.lang, &b.lang_name, &b.lang_local].join(" ");
            let key = blocklist::normalise(&words);
            (b, key)
        }).collect();
        Self { fetched: f.fetched, rows }
    }
}

fn catalog_path(root: &Path, p: Provider) -> PathBuf {
    root.join("bibles").join("catalog").join(match p { Provider::YouVersion => "youversion.json", _ => "apibible.json" })
}

type Mem = BTreeMap<(PathBuf, &'static str), Arc<Indexed>>;
/// Parsed catalogs per data folder and provider; a few thousand rows each, kept for the run.
static MEM: Mutex<Mem> = Mutex::new(BTreeMap::new());

/// The saved catalog (memory first, then disk), if any.
pub fn cached(root: &Path, p: Provider) -> Option<Arc<Indexed>> {
    let k = (root.to_path_buf(), p.key_name());
    if let Some(i) = MEM.lock().unwrap().get(&k) { return Some(i.clone()); }
    let f: CatalogFile = {
        let _g = FILES.lock().unwrap();
        serde_json::from_slice(&std::fs::read(catalog_path(root, p)).ok()?).ok()?
    };
    let i = Arc::new(Indexed::new(f));
    MEM.lock().unwrap().insert(k, i.clone());
    Some(i)
}

/// Saves a freshly fetched catalog and puts it in memory.
pub fn store(root: &Path, p: Provider, f: CatalogFile) -> io::Result<Arc<Indexed>> {
    {
        let _g = FILES.lock().unwrap();
        let path = catalog_path(root, p);
        std::fs::create_dir_all(path.parent().unwrap())?;
        crate::store::write_atomic(&path, &serde_json::to_vec(&f).map_err(io::Error::other)?)?;
    }
    let i = Arc::new(Indexed::new(f));
    MEM.lock().unwrap().insert((root.to_path_buf(), p.key_name()), i.clone());
    Ok(i)
}

/// Deletes a provider's catalog (its key changed or was removed: what it may read changed too).
pub fn forget(root: &Path, p: Provider) {
    MEM.lock().unwrap().remove(&(root.to_path_buf(), p.key_name()));
    FAILED.lock().unwrap().remove(&(root.to_path_buf(), p.key_name()));
    let _g = FILES.lock().unwrap();
    let _ = std::fs::remove_file(catalog_path(root, p));
}

pub fn stale(i: &Indexed, now: u64) -> bool { now.saturating_sub(i.fetched) >= MAX_AGE_SECS }

/// One fetch per provider at a time: a search typed while the list downloads waits for it, not a second download.
static FETCHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// When a fetch last failed, per data folder and provider: the page is redrawn after every action, and an offline
/// computer should not wait for the network each time. Refresh list always tries.
static FAILED: Mutex<BTreeMap<(PathBuf, &'static str), (u64, FetchError)>> = Mutex::new(BTreeMap::new());
const RETRY_SECS: u64 = 300;

/// The provider's catalog, fetched when missing, older than a week, or `refresh`. On a failed fetch the saved list
/// (if any) comes back with the error, so the page can say it shows the last saved list.
pub async fn ensure(root: &Path, p: Provider, key: &str, refresh: bool, now: u64) -> (Option<Arc<Indexed>>, Option<FetchError>) {
    let _g = FETCHING.lock().await;
    let have = cached(root, p);
    if !refresh && have.as_ref().is_some_and(|i| !stale(i, now)) { return (have, None); }
    let k = (root.to_path_buf(), p.key_name());
    if !refresh && let Some((at, e)) = FAILED.lock().unwrap().get(&k) && now.saturating_sub(*at) < RETRY_SECS {
        return (have, Some(e.clone()));
    }
    let (i, e) = fetch_and_store(root, p, key, have, now).await;
    let mut failed = FAILED.lock().unwrap();
    match &e { Some(e) => { failed.insert(k, (now, e.clone())); } None => { failed.remove(&k); } }
    (i, e)
}

async fn fetch_and_store(root: &Path, p: Provider, key: &str, have: Option<Arc<Indexed>>, now: u64) -> (Option<Arc<Indexed>>, Option<FetchError>) {
    let client = match super::sources::client() { Ok(c) => c, Err(e) => return (have, Some(e)) };
    let fetched = match p {
        Provider::YouVersion => fetch_youversion(client, key).await,
        Provider::ApiBible => fetch_api_bible(client, key).await,
        _ => return (have, None),
    };
    match fetched {
        Ok(bibles) => match store(root, p, CatalogFile { fetched: now, bibles }) {
            Ok(i) => (Some(i), None),
            Err(e) => (have, Some(FetchError::Other(e.to_string()))),
        },
        Err(e) => (have, Some(e)),
    }
}

/// A row that is already one of the app's own Bibles: a built-in one (English KJV, WEB, BSB) or one of today's
/// routes (NIV, HCSB, CSB, NLT on YouVersion's fixed ids; an English NIV, CSB or NLT on API.Bible). The page says
/// "Already in your list" instead of Add, so the chips never show one Bible twice.
pub fn already_listed(p: Provider, b: &OnlineBible) -> bool {
    let abbr = b.abbr.to_uppercase();
    let english = b.lang == "en" || b.lang.starts_with("en-") || b.lang == "eng";
    if english && super::BUILT_IN.iter().any(|(a, _)| *a == abbr) { return true; }
    match p {
        Provider::YouVersion => yv_number(&b.id).is_some_and(|n| super::youversion::VERSIONS.iter().any(|(t, id)| *id == n && super::TRANSLATIONS.contains(t))),
        Provider::ApiBible => english && super::sources::SOURCES.iter().any(|s| s.provider == p && s.translation == abbr),
        _ => false,
    }
}

// ---------- search ----------

/// Every row that contains all the typed words (accent- and case-insensitive, any script), sorted by language and
/// abbreviation, at most `limit`; and how many matched in all.
pub fn search<'a>(catalogs: &'a [(Provider, Arc<Indexed>)], query: &str, limit: usize) -> (Vec<(Provider, &'a OnlineBible)>, usize) {
    let words: Vec<String> = query.split_whitespace().map(blocklist::normalise).filter(|w| !w.is_empty()).collect();
    if words.is_empty() { return (Vec::new(), 0); }
    let mut hits: Vec<(Provider, &OnlineBible)> = catalogs.iter()
        .flat_map(|(p, i)| i.rows.iter().filter(|(_, k)| words.iter().all(|w| k.contains(w.as_str()))).map(move |(b, _)| (*p, b)))
        .collect();
    let total = hits.len();
    // An exact abbreviation ("NIV") comes first, then by language and abbreviation.
    let exact = |b: &OnlineBible| words.len() == 1 && blocklist::normalise(&b.abbr) == words[0];
    hits.sort_by(|a, b| (!exact(a.1), a.1.lang_name.to_lowercase(), a.1.abbr.to_lowercase(), a.0.key_name())
        .cmp(&(!exact(b.1), b.1.lang_name.to_lowercase(), b.1.abbr.to_lowercase(), b.0.key_name())));
    hits.truncate(limit);
    (hits, total)
}

// ---------- YouVersion ----------

/// `GET /v1/bibles` and `/v1/languages` pages: `{data: [...], next_page_token, total_size}`
/// (developers.youversion.com/api/bibles, /api/languages).
#[derive(Deserialize, Debug)]
pub struct YvPage<T> {
    #[serde(default = "Vec::new")]
    pub data: Vec<T>,
    #[serde(default)]
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct YvBible {
    pub id: u32,
    pub abbreviation: Option<String>,
    pub localized_abbreviation: Option<String>,
    pub title: Option<String>,
    pub localized_title: Option<String>,
    pub language_tag: Option<String>,
    pub copyright: Option<String>,
    pub promotional_content: Option<String>,
    pub publisher_url: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct YvLanguage {
    pub id: String,
    pub localized_name: Option<String>,
    pub text_direction: Option<String>,
}

fn s(o: &Option<String>) -> String { o.as_deref().map(str::trim).unwrap_or("").to_string() }

/// Catalog rows from YouVersion's lists. The credit is `copyright`, else `promotional_content`; a Bible with neither
/// is left out, because it can't be shown with a credit.
pub fn yv_entries(bibles: Vec<YvBible>, langs: &[YvLanguage]) -> Vec<OnlineBible> {
    let by_tag: BTreeMap<&str, &YvLanguage> = langs.iter().map(|l| (l.id.as_str(), l)).collect();
    bibles.into_iter().filter(|b| b.id > 0).filter_map(|b| {
        let tag = s(&b.language_tag);
        let lang = by_tag.get(tag.as_str()).or_else(|| by_tag.get(tag.split('-').next().unwrap_or(""))).copied();
        let copyright = Some(plain(&s(&b.copyright))).filter(|c| !c.is_empty()).unwrap_or_else(|| plain(&s(&b.promotional_content)));
        if copyright.is_empty() { return None; }
        let abbr = Some(s(&b.localized_abbreviation)).filter(|a| !a.is_empty()).unwrap_or_else(|| s(&b.abbreviation));
        let name_en = s(&b.title);
        let name = Some(s(&b.localized_title)).filter(|t| !t.is_empty()).unwrap_or_else(|| name_en.clone());
        Some(OnlineBible {
            id: format!("yv:{}", b.id),
            abbr: if abbr.is_empty() { b.id.to_string() } else { abbr.chars().take(16).collect() },
            name, name_en,
            lang_name: lang.map(|l| s(&l.localized_name)).filter(|n| !n.is_empty()).unwrap_or_else(|| tag.clone()),
            lang_local: String::new(),
            dir: lang.and_then(|l| l.text_direction.clone()).unwrap_or_else(|| "ltr".into()).to_lowercase(),
            lang: tag,
            copyright,
            url: s(&b.publisher_url),
        })
    }).collect()
}

fn yv_status(status: u16) -> FetchError {
    match status { 403 => FetchError::BadKey, s => FetchError::from_status(s) }
}

/// Pages through one YouVersion list.
async fn yv_pages<T: serde::de::DeserializeOwned>(client: &reqwest::Client, key: &str, url: &str, query: &[(&str, &str)]) -> Result<Vec<T>, FetchError> {
    let mut out = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut q: Vec<(&str, &str)> = query.to_vec();
        q.push(("page_size", "99"));
        if let Some(t) = &token { q.push(("page_token", t)); }
        let res = client.get(url).header("X-YVP-App-Key", key).header("Accept", "application/json").query(&q).send().await?;
        if !res.status().is_success() { return Err(yv_status(res.status().as_u16())); }
        let page: YvPage<T> = res.json().await?;
        out.extend(page.data);
        token = page.next_page_token.filter(|t| !t.is_empty());
        if token.is_none() { break; }
    }
    Ok(out)
}

/// Every Bible the church's YouVersion app may read, with language names and directions.
pub async fn fetch_youversion(client: &reqwest::Client, key: &str) -> Result<Vec<OnlineBible>, FetchError> {
    let bibles: Vec<YvBible> = yv_pages(client, key, &format!("{YV}/bibles"), &[("language_ranges[]", "*")]).await?;
    // Names only make the list nicer: a failed language list still leaves every Bible usable.
    let langs: Vec<YvLanguage> = yv_pages(client, key, &format!("{YV}/languages"),
        &[("bibles_available", "true"), ("fields[]", "id"), ("fields[]", "localized_name"), ("fields[]", "text_direction")]).await.unwrap_or_default();
    Ok(yv_entries(bibles, &langs))
}

// ---------- API.Bible ----------

/// `GET /v1/bibles?include-full-details=true` (docs.api.bible/guides/bibles).
#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct AbList { pub data: Vec<AbBible> }

#[derive(Deserialize, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AbBible {
    pub id: String,
    pub abbreviation: Option<String>,
    pub abbreviation_local: Option<String>,
    pub name: Option<String>,
    pub name_local: Option<String>,
    pub language: AbLanguage,
    pub copyright: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AbLanguage {
    pub id: String,
    pub name: Option<String>,
    pub name_local: Option<String>,
    pub script_direction: Option<String>,
}

/// Catalog rows from API.Bible's list. A Bible with no copyright line is left out.
pub fn ab_entries(list: AbList) -> Vec<OnlineBible> {
    list.data.into_iter().filter_map(|b| {
        let id = format!("ab:{}", b.id);
        ab_id(&id)?;
        let copyright = plain(&s(&b.copyright));
        if copyright.is_empty() { return None; }
        let abbr = Some(s(&b.abbreviation_local)).filter(|a| !a.is_empty()).unwrap_or_else(|| s(&b.abbreviation));
        let name_en = s(&b.name);
        Some(OnlineBible {
            abbr: if abbr.is_empty() { b.id.chars().take(8).collect() } else { abbr.chars().take(16).collect() },
            name: Some(s(&b.name_local)).filter(|n| !n.is_empty()).unwrap_or_else(|| name_en.clone()),
            name_en,
            lang: b.language.id.clone(),
            lang_name: Some(s(&b.language.name)).filter(|n| !n.is_empty()).unwrap_or_else(|| s(&b.language.name_local)),
            lang_local: s(&b.language.name_local),
            dir: b.language.script_direction.clone().unwrap_or_else(|| "ltr".into()).to_lowercase(),
            copyright,
            url: String::new(),
            id,
        })
    }).collect()
}

pub async fn fetch_api_bible(client: &reqwest::Client, key: &str) -> Result<Vec<OnlineBible>, FetchError> {
    let res = client.get(format!("{AB}/bibles")).header("api-key", key).query(&[("include-full-details", "true")]).send().await?;
    if !res.status().is_success() { return Err(FetchError::from_status(res.status().as_u16())); }
    Ok(ab_entries(res.json().await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shapes from developers.youversion.com/api/bibles and /api/languages, with made-up rows.
    const YV_BIBLES: &str = r#"{"data":[
        {"id":111,"abbreviation":"NIV","localized_abbreviation":"NIV","title":"New International Version","localized_title":"New International Version",
         "language_tag":"en","copyright":"Holy Bible, New International Version® NIV® Copyright © 1973, 1978, 1984, 2011 by Biblica, Inc.®","promotional_content":null,"books":["GEN"],"publisher_url":"https://www.biblica.com"},
        {"id":149,"abbreviation":"RVR1960","localized_abbreviation":"RVR1960","title":"Reina-Valera 1960","localized_title":"Biblia Reina Valera 1960",
         "language_tag":"es","copyright":"","promotional_content":"<p>Reina-Valera 1960 &copy; Sociedades Bíblicas en América Latina</p>"},
        {"id":1849,"abbreviation":"TPT","title":"The Passion Translation","language_tag":"en","copyright":"Passion & Fire Ministries"},
        {"id":5,"abbreviation":"X","title":"No credit","language_tag":"en","copyright":null,"promotional_content":" "},
        {"id":7,"abbreviation":"SVD","title":"Smith Van Dyke","localized_title":"الكتاب المقدس","language_tag":"ar","copyright":"Public Domain"}
    ],"next_page_token":"abc","total_size":5}"#;
    const YV_LANGS: &str = r#"{"data":[{"id":"en","localized_name":"English","text_direction":"ltr"},
        {"id":"es","localized_name":"Spanish","text_direction":"ltr"},{"id":"ar","localized_name":"Arabic","text_direction":"rtl"}],"next_page_token":null}"#;

    fn yv() -> Vec<OnlineBible> {
        let b: YvPage<YvBible> = serde_json::from_str(YV_BIBLES).unwrap();
        assert_eq!(b.next_page_token.as_deref(), Some("abc"));
        let l: YvPage<YvLanguage> = serde_json::from_str(YV_LANGS).unwrap();
        assert!(l.next_page_token.is_none());
        yv_entries(b.data, &l.data)
    }

    #[test]
    fn youversion_rows_keep_credit_language_and_direction() {
        let rows = yv();
        let niv = rows.iter().find(|b| b.id == "yv:111").unwrap();
        assert_eq!((niv.abbr.as_str(), niv.lang.as_str(), niv.lang_name.as_str(), niv.dir.as_str()), ("NIV", "en", "English", "ltr"));
        assert!(niv.copyright.contains("Biblica"));
        let rv = rows.iter().find(|b| b.id == "yv:149").unwrap();
        assert_eq!(rv.copyright, "Reina-Valera 1960 © Sociedades Bíblicas en América Latina", "promotional text when copyright is empty");
        assert_eq!(rv.name, "Biblia Reina Valera 1960");
        assert_eq!(rv.name_en, "Reina-Valera 1960");
        assert!(!rows.iter().any(|b| b.id == "yv:5"), "no credit: left out");
        assert_eq!(rows.iter().find(|b| b.id == "yv:7").unwrap().dir, "rtl");
    }

    #[test]
    fn catalogs_hide_blocked_and_right_to_left_rows() {
        let i = Indexed::new(CatalogFile { fetched: 0, bibles: yv() });
        let ids: Vec<&str> = i.rows.iter().map(|(b, _)| b.id.as_str()).collect();
        assert_eq!(ids, ["yv:111", "yv:149"], "TPT is blocked, Arabic waits for right-to-left slides (step 3)");
    }

    // Shape from docs.api.bible/guides/bibles (include-full-details), made-up rows.
    const AB_LIST: &str = r#"{"data":[
        {"id":"a556c5305ee15c3f-01","dblId":"x","abbreviation":"CSB","abbreviationLocal":"CSB","name":"Christian Standard Bible","nameLocal":"Christian Standard Bible",
         "language":{"id":"eng","name":"English","nameLocal":"English","script":"Latin","scriptDirection":"LTR"},
         "copyright":"<p>Christian Standard Bible&reg; Copyright &copy; 2017 by Holman Bible Publishers.</p>","info":"<p>x</p>"},
        {"id":"592420522e16049f-01","abbreviation":"RVR09","abbreviationLocal":"RVR09","name":"Reina Valera 1909","nameLocal":"Reina Valera 1909",
         "language":{"id":"spa","name":"Spanish","nameLocal":"Español","scriptDirection":"LTR"},"copyright":"Public Domain"},
        {"id":"bad/../id","abbreviation":"B","name":"B","language":{"id":"eng"},"copyright":"x"},
        {"id":"nwt-01","abbreviation":"NW","name":"Holy Scriptures","language":{"id":"eng"},"copyright":"© Watch Tower Bible and Tract Society"}
    ]}"#;

    #[test]
    fn api_bible_rows() {
        let rows = ab_entries(serde_json::from_str(AB_LIST).unwrap());
        assert_eq!(rows.len(), 3, "the unsafe id is dropped");
        assert_eq!(rows[0].id, "ab:a556c5305ee15c3f-01");
        assert_eq!(rows[0].copyright, "Christian Standard Bible® Copyright © 2017 by Holman Bible Publishers.");
        assert_eq!((rows[1].lang.as_str(), rows[1].lang_name.as_str(), rows[1].dir.as_str()), ("spa", "Spanish", "ltr"));
        let i = Indexed::new(CatalogFile { fetched: 0, bibles: rows });
        assert_eq!(i.rows.len(), 2, "the Watch Tower notice blocks the row whatever its title");
    }

    #[test]
    fn search_matches_every_word_any_accent_and_sorts_by_language() {
        let yv = Arc::new(Indexed::new(CatalogFile { fetched: 0, bibles: yv() }));
        let ab = Arc::new(Indexed::new(CatalogFile { fetched: 0, bibles: ab_entries(serde_json::from_str(AB_LIST).unwrap()) }));
        let cats = vec![(Provider::YouVersion, yv), (Provider::ApiBible, ab)];
        let ids = |q: &str| search(&cats, q, 80).0.iter().map(|(_, b)| b.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids("reina"), ["ab:592420522e16049f-01", "yv:149"]);
        assert_eq!(ids("Español"), ["ab:592420522e16049f-01"]);
        assert_eq!(ids("espanol"), ids("ESPAÑOL"));
        assert_eq!(ids("spanish 1960"), ["yv:149"]);
        assert_eq!(ids("english"), ["ab:a556c5305ee15c3f-01", "yv:111"], "English rows by abbreviation");
        assert_eq!(ids("niv"), ["yv:111"]);
        assert!(ids("  ").is_empty());
        assert!(ids("passion").is_empty(), "blocked rows never show");
        let (hits, total) = search(&cats, "e", 1);
        assert_eq!((hits.len(), total), (1, 4));
    }

    #[test]
    fn added_bibles_round_trip_and_refuse_blocked_or_uncredited() {
        let root = crate::store::test_dir();
        let niv = yv().into_iter().find(|b| b.id == "yv:111").unwrap();
        add(&root, niv.clone()).unwrap();
        add(&root, niv.clone()).unwrap();
        assert_eq!(added(&root), vec![niv.clone()], "added once");
        assert_eq!(find_added(&root, "yv:111"), Some(niv.clone()));
        let tpt = OnlineBible { id: "yv:1849".into(), abbr: "TPT".into(), copyright: "x".into(), ..Default::default() };
        assert_eq!(add(&root, tpt).unwrap_err(), strings::BLOCKED);
        let bare = OnlineBible { id: "yv:9".into(), abbr: "A".into(), ..Default::default() };
        assert_eq!(add(&root, bare).unwrap_err(), strings::NO_CREDIT);
        // A blocked entry written by hand is dropped when read.
        std::fs::write(root.join("bibles/online.json"), r#"{"bibles":[{"id":"yv:1","abbr":"NWT","copyright":"x"},{"id":"zz","abbr":"Q"}]}"#).unwrap();
        assert!(added(&root).is_empty());
        remove(&root, "yv:111").unwrap();
    }

    #[test]
    fn catalog_files_are_saved_and_forgotten() {
        let root = crate::store::test_dir();
        assert!(cached(&root, Provider::YouVersion).is_none());
        store(&root, Provider::YouVersion, CatalogFile { fetched: 100, bibles: yv() }).unwrap();
        MEM.lock().unwrap().clear();
        let i = cached(&root, Provider::YouVersion).expect("read back from disk");
        assert_eq!(i.fetched, 100);
        assert!(!stale(&i, 100 + MAX_AGE_SECS - 1) && stale(&i, 100 + MAX_AGE_SECS));
        assert!(cached(&root, Provider::ApiBible).is_none(), "one file per provider");
        forget(&root, Provider::YouVersion);
        assert!(cached(&root, Provider::YouVersion).is_none());
    }

    #[test]
    fn ids() {
        assert_eq!(provider_of("yv:111"), Some(Provider::YouVersion));
        assert_eq!(provider_of("ab:x"), Some(Provider::ApiBible));
        assert_eq!(provider_of("KJV"), None);
        assert_eq!(yv_number("yv:111"), Some(111));
        assert_eq!(yv_number("yv:x"), None);
        assert_eq!(ab_id("ab:de4e12af7f28f599-02"), Some("de4e12af7f28f599-02"));
        assert_eq!(ab_id("ab:../x"), None);
        assert_eq!(ab_id("ab:"), None);
    }

    /// `RP_NET_TESTS=1` with `RP_YOUVERSION_KEY` and/or `RP_API_BIBLE_KEY`: the real lists, then John 3:16 in a
    /// Bible from each (`sources::lookup`, the same path the Bible box uses).
    #[tokio::test]
    async fn net_catalogs_list_and_look_up() {
        if std::env::var("RP_NET_TESTS").as_deref() != Ok("1") { return; }
        let client = super::super::sources::client().unwrap();
        // Without a key YouVersion refuses: the error says to check the key, not "no Bibles".
        assert_eq!(fetch_youversion(client, "not-a-key").await.unwrap_err(), FetchError::BadKey);
        for (p, var, want) in [(Provider::YouVersion, "RP_YOUVERSION_KEY", "BSB"), (Provider::ApiBible, "RP_API_BIBLE_KEY", "KJV")] {
            let Ok(key) = std::env::var(var) else { eprintln!("{var} not set: {} list skipped", p.label()); continue };
            let root = crate::store::test_dir();
            let mut s = super::super::sources::Secrets::default();
            s.keys.insert(p.key_name().into(), key.clone());
            s.save(&root).unwrap();
            let (i, e) = ensure(&root, p, &key, true, crate::app::now_secs()).await;
            assert!(e.is_none(), "{e:?}");
            let i = i.unwrap();
            assert!(!i.rows.is_empty());
            let b = i.rows.iter().map(|(b, _)| b).find(|b| b.abbr == want && b.lang.starts_with("en")).expect("a public-domain English Bible").clone();
            add(&root, b.clone()).unwrap();
            let (label, v) = super::super::sources::lookup(&root, &b.id, "John 3:16").await.unwrap();
            assert_eq!(label, "John 3:16");
            assert!(v[0].text.contains("God"), "{}", v[0].text);
        }
    }
}
