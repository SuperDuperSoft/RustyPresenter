//! The YouVersion catalog: every Bible the YouVersion key in use may read (`GET /v1/bibles`), 99 to a page, asked at
//! most once a week and kept in `bibles/catalog/youversion.json` so the list still shows offline.
//!
//! A YouVersion Bible is read over the internet, never downloaded: adding one only puts it in the church's list
//! (`bible::online`). YouVersion lists only what the key is licensed for, so nothing here needs a licence check; the
//! blocklist still runs on every row (`CatalogRow::checked`) and on the verses as they arrive.
use super::{CatalogError, CatalogRow, CatalogSource, Ctx, Fields, LicenceState, Listing, Scope};
use crate::bible::{USFM, library::{Dir, Licence}, sources::{Provider, Secrets, client}, youversion::VERSIONS};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, sync::OnceLock};

pub const BASE: &str = "https://api.youversion.com/v1/bibles";
const WEEK: i64 = 7 * 24 * 60 * 60;
const STATE_FILE: &str = "youversion.json";
/// The largest page YouVersion gives.
const PAGE: &str = "99";
/// Far more than the list needs (about 15 today); a loop of page tokens stops here.
const MAX_PAGES: usize = 60;
/// Tag, ISO 639-3 code (what eBible's rows use, so both sources group as one language), English name, own name,
/// "rtl". Made from YouVersion's `/v1/languages` in November 2026.
const LANGUAGES: &str = include_str!("../../../assets/bibles/youversion-languages.tsv");

/// One Bible as YouVersion lists it. Only what the app shows is kept.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Row {
    pub id: u32,
    pub abbreviation: Option<String>,
    pub localized_abbreviation: Option<String>,
    pub title: Option<String>,
    pub localized_title: Option<String>,
    pub copyright: Option<String>,
    pub publisher_name: Option<String>,
    pub language_tag: Option<String>,
    pub books: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Page { data: Vec<Row>, next_page_token: Option<String> }

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct State { checked_at: i64, rows: Vec<Row> }

/// A language of the table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lang { pub code: &'static str, pub name_en: &'static str, pub name: &'static str, pub rtl: bool }

/// The language of a YouVersion tag ("es", "zh-Hant-TW", "kud"); a tag with a region or script the table lacks
/// reads as its language alone.
pub fn language(tag: &str) -> Option<Lang> {
    static TABLE: OnceLock<HashMap<&'static str, Lang>> = OnceLock::new();
    let table = TABLE.get_or_init(|| LANGUAGES.lines().filter_map(|l| {
        let mut c = l.split('\t');
        Some((c.next()?, Lang { code: c.next()?, name_en: c.next()?, name: c.next()?, rtl: c.next() == Some("rtl") }))
    }).collect());
    table.get(tag).or_else(|| table.get(tag.split('-').next().unwrap_or(tag))).copied()
}

/// The saved list while it is under a week old, else YouVersion's, with the key in use. Without a key, or when
/// YouVersion can't be reached, the saved list (`offline` then), which may be empty.
pub async fn refresh(ctx: &Ctx) -> Result<Listing, CatalogError> {
    let dir = ctx.dir();
    let state: State = fs::read(dir.join(STATE_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let fresh = !state.rows.is_empty() && ctx.now >= state.checked_at && ctx.now - state.checked_at < WEEK;
    let listing = |rows: &[Row], offline| Listing { rows: rows.iter().filter_map(offer).collect(), offline };
    if fresh && !ctx.force { return Ok(listing(&state.rows, false)); }
    let Some(key) = Secrets::load(&ctx.root).key(Provider::YouVersion).map(str::to_string) else { return Ok(listing(&state.rows, false)) };
    match fetch(&ctx.base, &key).await {
        Ok(rows) if !rows.is_empty() => {
            let disk = |e: std::io::Error| CatalogError::Disk(e.to_string());
            fs::create_dir_all(&dir).map_err(disk)?;
            let state = State { checked_at: ctx.now, rows };
            crate::store::write_atomic(&dir.join(STATE_FILE), &serde_json::to_vec(&state).map_err(|e| CatalogError::Disk(e.to_string()))?).map_err(disk)?;
            Ok(listing(&state.rows, false))
        }
        r => {
            eprintln!("YouVersion catalog: {:?}; showing the saved list", r.err());
            Ok(listing(&state.rows, true))
        }
    }
}

async fn fetch(base: &str, key: &str) -> Result<Vec<Row>, CatalogError> {
    let client = client().map_err(|_| CatalogError::Offline)?;
    let (mut rows, mut token) = (Vec::new(), None::<String>);
    for _ in 0..MAX_PAGES {
        let mut query = vec![("language_ranges[]", "*".to_string()), ("page_size", PAGE.to_string())];
        if let Some(t) = &token { query.push(("page_token", t.clone())) }
        let res = client.get(base).header("X-YVP-App-Key", key).header("Accept", "application/json").query(&query)
            .send().await.map_err(|_| CatalogError::Offline)?;
        if !res.status().is_success() { return Err(CatalogError::Bad(crate::tr!("YouVersion answered {status}", status = res.status().as_u16()))); }
        let page: Page = res.json().await.map_err(|e| CatalogError::Bad(e.to_string()))?;
        rows.extend(page.data);
        token = page.next_page_token.filter(|t| !t.is_empty());
        if token.is_none() { break }
    }
    Ok(rows)
}

/// One Bible's own entry (`GET /v1/bibles/{id}`): the list leaves the copyright notice out, this has it.
pub async fn details(base: &str, key: &str, id: &str) -> Option<Row> {
    let res = client().ok()?.get(format!("{base}/{id}")).header("X-YVP-App-Key", key).header("Accept", "application/json").send().await.ok()?;
    if !res.status().is_success() { return None }
    res.json().await.ok()
}

/// The row to list, if the app may offer it. Left out: a Bible the app already has under its own name (NIV, BSB),
/// one written right to left (the slides don't set that yet, as with eBible's), and anything blocked.
pub fn offer(r: &Row) -> Option<CatalogRow> {
    if r.id == 0 || VERSIONS.iter().any(|&(_, id)| id == r.id) { return None; }
    let text = |s: &Option<String>| s.as_deref().unwrap_or("").trim().to_string();
    let tag = text(&r.language_tag);
    let lang = language(&tag);
    if lang.is_some_and(|l| l.rtl) { return None; }
    let (ot, nt) = (r.books.iter().filter(|b| USFM[..39].contains(&b.as_str())).count(), r.books.iter().filter(|b| USFM[39..].contains(&b.as_str())).count());
    let scope = match (ot >= 39, nt >= 27) {
        (true, true) => Scope::Full,
        (false, true) => Scope::NewTestament,
        (true, false) => Scope::OldTestament,
        (false, false) => Scope::Portions,
    };
    let or = |a: String, b: String| if a.is_empty() { b } else { a };
    let abbr = or(text(&r.localized_abbreviation), text(&r.abbreviation));
    let title_en = or(text(&r.title), text(&r.localized_title));
    let title = or(text(&r.localized_title), title_en.clone());
    if title.is_empty() { return None; }
    let f = Fields {
        source_id: r.id.to_string(), abbr: or(abbr, r.id.to_string()), title, title_en, description: String::new(),
        copyright: or(text(&r.copyright), text(&r.publisher_name)),
        lang: lang.map_or(tag.clone(), |l| l.code.to_string()), lang_name: lang.map_or(tag.clone(), |l| l.name.to_string()),
        lang_name_en: lang.map_or(tag, |l| l.name_en.to_string()), dir: Dir::Ltr, script: String::new(), update_date: String::new(),
        has_dc: r.books.len() > ot + nt,
    };
    CatalogRow::checked(CatalogSource::YouVersion, f, scope, LicenceState::Known(Licence::Provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u32, abbr: &str, title: &str, tag: &str, books: &[&str]) -> Row {
        Row { id, abbreviation: Some(abbr.into()), title: Some(title.into()), language_tag: Some(tag.into()),
            books: books.iter().map(|b| b.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn a_row_gets_its_language_scope_and_id() {
        let r = offer(&row(1588, "AMP", "Amplified Bible", "en", &USFM)).unwrap();
        assert_eq!((r.id(), r.abbr(), r.lang(), r.lang_name_en(), r.scope()), ("yv:1588", "AMP", "eng", "English", Scope::Full));
        let r = offer(&row(89, "LBLA", "La Biblia de las Américas", "es-ES", &USFM[39..])).unwrap();
        assert_eq!((r.lang(), r.scope()), ("spa", Scope::NewTestament));
        let r = offer(&row(7, "x", "Some Bible", "qq-unknown", &["GEN", "TOB"])).unwrap();
        assert_eq!((r.lang(), r.scope(), r.has_dc()), ("qq-unknown", Scope::Portions, true));
    }

    #[test]
    fn blocked_known_and_right_to_left_bibles_are_left_out() {
        assert!(offer(&row(1849, "TPT", "The Passion Translation", "en", &USFM)).is_none(), "by all three");
        assert!(offer(&row(1849, "X", "A Bible", "en", &USFM)).is_none(), "by its YouVersion id alone");
        assert!(offer(&row(9, "TPT", "A Bible", "en", &USFM)).is_none(), "by its abbreviation alone");
        assert!(offer(&row(9, "X", "The Passion Translation", "en", &USFM)).is_none(), "by its title alone");
        assert!(offer(&row(111, "NIV11", "New International Version 2011", "en", &USFM)).is_none(), "already in the app as NIV");
        assert!(offer(&row(9, "X", "A Bible", "he", &USFM)).is_none(), "right to left");
        assert!(offer(&row(9, "X", "", "en", &USFM)).is_none() && offer(&row(0, "X", "A Bible", "en", &USFM)).is_none());
    }

    #[test]
    fn a_page_reads_with_nulls_and_fields_it_does_not_know() {
        let page: Page = serde_json::from_str(r#"{"data":[{"id":12,"abbreviation":"ASV","copyright":null,"info":null,"language_tag":"en",
            "localized_title":"American Standard Version","title":"American Standard Version","books":["GEN"],"organization_id":"x"}],
            "next_page_token":null,"total_size":1}"#).unwrap();
        assert_eq!(page.data[0].id, 12);
        assert_eq!(offer(&page.data[0]).unwrap().title(), "American Standard Version");
        assert!(page.next_page_token.is_none());
    }

    #[test]
    fn the_language_table_reads() {
        assert_eq!(language("en").map(|l| (l.code, l.name_en)), Some(("eng", "English")));
        assert_eq!(language("zh-Hant-TW").map(|l| l.code), Some("cmn"));
        assert_eq!(language("es-XX").map(|l| l.code), Some("spa"), "an unknown region reads as the language");
        assert!(language("he").unwrap().rtl && language("zz-nothing").is_none());
    }

    #[tokio::test]
    async fn without_a_key_or_a_saved_list_there_is_nothing_and_no_request() {
        let root = std::env::temp_dir().join(format!("rp-test-{}", uuid::Uuid::new_v4()));
        let mut ctx = Ctx::new(root.clone(), CatalogSource::YouVersion);
        ctx.base = "http://127.0.0.1:9".into();
        let l = refresh(&ctx).await.unwrap();
        assert!(l.rows.is_empty() && !l.offline);
        // A saved list under a week old is shown as it is.
        fs::create_dir_all(ctx.dir()).unwrap();
        let state = State { checked_at: ctx.now - 60, rows: vec![row(12, "ASV", "American Standard Version", "en", &USFM), row(1849, "TPT", "The Passion Translation", "en", &USFM)] };
        fs::write(ctx.dir().join(STATE_FILE), serde_json::to_vec(&state).unwrap()).unwrap();
        let l = refresh(&ctx).await.unwrap();
        assert_eq!(l.rows.iter().map(|r| r.id()).collect::<Vec<_>>(), ["yv:12"], "a blocked Bible in a saved list is still left out");
        let _ = fs::remove_dir_all(root);
    }
}
