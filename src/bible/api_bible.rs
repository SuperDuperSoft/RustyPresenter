//! CSB, NLT, NIV, NASB, NKJV… from API.Bible (American Bible Society, docs.api.bible) with the church's own key.
//! The free Starter plan is non-commercial, 5,000 calls a month and up to 3 copyrighted Bibles, which the church
//! picks under Plan in its API.Bible dashboard. Terms: stored text must be re-checked within 30 days and kept
//! protected (`CachePolicy { max_age_days: Some(30), .. }`); every passage shown links to its full copyright
//! notice (`copyright()`); each passage's `fumsToken` is returned (and cached) so `sources::lookup` reports it to
//! FUMS (`report_fums`) every time the passage is shown, fetched or cached.
use super::{Part, sources::{FetchError, Fetched, Verse, tidy}};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path, sync::{Mutex, OnceLock}};

/// Current base (docs.api.bible/quick-start/authentication). The old `api.scripture.api.bible/v1` host is legacy.
const URL: &str = "https://rest.api.bible/v1";
/// Manual FUMS report endpoint for non-browser apps (docs.api.bible/guides/fair-use).
const FUMS: &str = "https://fums.api.bible/f3";

use super::USFM;

// ---------- responses ----------

#[derive(Deserialize, Debug)]
pub struct BibleList { #[serde(default)] data: Vec<BibleSummary> }

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BibleSummary {
    id: String,
    #[serde(default)]
    abbreviation: String,
    #[serde(default)]
    abbreviation_local: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    language: Language,
}

#[derive(Deserialize, Debug, Default)]
pub struct Language { #[serde(default)] id: String }

/// A passage or chapter fetched with `content-type=json`.
#[derive(Deserialize, Debug)]
pub struct Response {
    data: Passage,
    #[serde(default)]
    meta: Meta,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Passage {
    #[serde(default)]
    content: Vec<Node>,
    #[serde(default)]
    copyright: String,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Meta { #[serde(default)] fums_token: Option<String> }

/// One node of the JSON content tree: a `tag` (para, verse, char, note…) with children, or a `text` leaf.
#[derive(Deserialize, Debug)]
pub struct Node {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    attrs: Attrs,
    #[serde(default)]
    items: Vec<Node>,
}

#[derive(Deserialize, Debug, Default)]
pub struct Attrs {
    #[serde(default, rename = "verseId")]
    verse_id: Option<String>,
    #[serde(default)]
    style: Option<String>,
}

// ---------- memo ----------

/// Bible ids are opaque (`a556c5305ee15c3f-01`), so each translation is looked up once per run.
static BIBLE_IDS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
/// Full copyright notice API.Bible returned for each translation. Terms §7: show it on a credits page
/// linked from every citation, with the translation name, abbreviation and publisher.
static COPYRIGHTS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// Copyright text from the last passage fetched in this translation (e.g. the CSB's Holman notice).
pub fn copyright(translation: &str) -> Option<String> { COPYRIGHTS.lock().unwrap().get(&translation.to_uppercase()).cloned() }

// ---------- fetch ----------

/// The passage and its FUMS tokens (one per request), which the caller reports.
pub async fn fetch(client: &reqwest::Client, key: &str, translation: &str, book: u8, part: &Part) -> Result<Fetched, FetchError> {
    let id = bible_id(client, key, translation).await?;
    let code = USFM[book as usize];
    let mut fums = Vec::new();
    let verses = match (part.start, part.end) {
        // A verse range, possibly across chapters: one passage request (at most 200 verses come back).
        ((c, Some(v)), (e, Some(w))) => {
            let r = get(client, key, translation, &format!("{URL}/bibles/{id}/passages/{}", passage_id(code, (c, v), (e, w)))).await?;
            fums.extend(r.meta.fums_token.clone());
            let out = parse(code, &r)?;
            if out.last().is_some_and(|l| (l.0, l.1) < (e, w)) && out.len() >= 200 {
                return Err(FetchError::Other("API.Bible sends at most 200 verses at a time; choose a shorter passage".into()));
            }
            out
        }
        // Whole chapters (or a range open at one end): one chapter request each, trimmed to the range.
        ((c, v), (e, w)) => {
            let mut out = Vec::new();
            for ch in c..=e {
                let r = get(client, key, translation, &format!("{URL}/bibles/{id}/chapters/{code}.{ch}")).await?;
                fums.extend(r.meta.fums_token.clone());
                out.extend(parse(code, &r)?);
            }
            out.retain(|x| (x.0, x.1) >= (c, v.unwrap_or(0)) && (x.0, x.1) <= (e, w.unwrap_or(u16::MAX)));
            out
        }
    };
    if verses.is_empty() { return Err(FetchError::NotFound); }
    Ok(Fetched { verses, fums })
}

/// One content request; records the copyright.
async fn get(client: &reqwest::Client, key: &str, translation: &str, url: &str) -> Result<Response, FetchError> {
    let res = client.get(url)
        .header("api-key", key)
        .query(&[
            ("content-type", "json"),
            ("include-notes", "false"),
            ("include-titles", "false"),
            ("include-chapter-numbers", "false"),
            ("include-verse-numbers", "true"),
            ("include-verse-spans", "false"),
        ])
        .send().await?;
    if !res.status().is_success() {
        // 403/404 can mean the Bible left the key's plan: look its id up again next time.
        BIBLE_IDS.lock().unwrap().remove(&translation.to_uppercase());
        return Err(status_error(res.status().as_u16(), translation));
    }
    let r = res.json::<Response>().await?;
    if !r.data.copyright.trim().is_empty() {
        COPYRIGHTS.lock().unwrap().insert(translation.to_uppercase(), tidy(&r.data.copyright));
    }
    Ok(r)
}

/// 403 is "this Bible isn't on your plan", not a bad key (docs.api.bible/quick-start/errors).
fn status_error(status: u16, translation: &str) -> FetchError {
    match status {
        403 => FetchError::Other(format!("{translation} isn't on this key's API.Bible plan; add it under Plan in the API.Bible dashboard")),
        s => FetchError::from_status(s),
    }
}

/// Finds (and memoizes) the opaque Bible id: `?abbreviation=` first, then the whole English list.
async fn bible_id(client: &reqwest::Client, key: &str, translation: &str) -> Result<String, FetchError> {
    // A Bible added from the catalog carries its real id (design §3.4: by id, never by abbreviation).
    if super::catalog::provider_of(translation).is_some() {
        return super::catalog::ab_id(translation).map(str::to_string).ok_or_else(|| FetchError::Other("that isn't an API.Bible Bible".into()));
    }
    let name = translation.to_uppercase();
    if let Some(id) = BIBLE_IDS.lock().unwrap().get(&name) { return Ok(id.clone()); }
    for q in [("abbreviation", name.as_str()), ("language", "eng")] {
        let res = client.get(format!("{URL}/bibles")).header("api-key", key).query(&[q]).send().await?;
        if !res.status().is_success() { return Err(status_error(res.status().as_u16(), translation)); }
        let list = res.json::<BibleList>().await?;
        if let Some(id) = pick_bible(&list, &name) {
            BIBLE_IDS.lock().unwrap().insert(name.clone(), id.to_string());
            return Ok(id.to_string());
        }
    }
    Err(FetchError::Other(format!("{translation} isn't available with this API.Bible key; add it under Plan in the API.Bible dashboard")))
}

fn norm(s: &str) -> String { s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect() }

/// The Bible whose abbreviation (or local abbreviation) is exactly `translation`, English first. Blocked Bibles
/// (design §4) are never picked.
pub fn pick_bible<'a>(list: &'a BibleList, translation: &str) -> Option<&'a str> {
    let want = norm(translation);
    let ok = |b: &BibleSummary| !super::blocklist::blocked_row(&b.language.id, &b.abbreviation, &[&b.name, &b.abbreviation_local], &[], &[&format!("ab:{}", b.id)]);
    let hits = || list.data.iter().filter(|b| norm(&b.abbreviation) == want || norm(&b.abbreviation_local) == want).filter(|b| ok(b));
    hits().find(|b| b.language.id == "eng").or_else(|| hits().next()).map(|b| b.id.as_str())
}

/// `JHN.3.16-JHN.3.18`; a single verse is just `JHN.3.16`.
pub fn passage_id(code: &str, (c, v): (u16, u16), (e, w): (u16, u16)) -> String {
    if (c, v) == (e, w) { format!("{code}.{c}.{v}") } else { format!("{code}.{c}.{v}-{code}.{e}.{w}") }
}

/// `JHN.3.16` → (3, 16). A combined verse (`JHN.3.16-17`) counts as its first number.
pub fn parse_verse_id(code: &str, id: &str) -> Option<(u16, u16)> {
    let mut it = id.split('.');
    if it.next()? != code { return None; }
    let c = it.next()?.parse().ok()?;
    let v = it.next()?;
    let digits = v.find(|ch: char| !ch.is_ascii_digit()).map_or(v, |i| &v[..i]);
    Some((c, digits.parse().ok()?))
}

/// Collects every text leaf under its `verseId`. Titles, verse numbers and notes carry none (or are skipped),
/// so only Scripture text survives; chapter boundaries come straight from the ids.
pub fn parse(code: &str, r: &Response) -> Result<Vec<Verse>, FetchError> {
    let mut w = Walk { code, out: Vec::new(), gap: false, wrong_book: false };
    w.walk(&r.data.content, None, false);
    if w.wrong_book { return Err(FetchError::Other("API.Bible returned a different book".into())); }
    let mut out = w.out;
    for v in &mut out { v.2 = tidy(&v.2); }
    out.retain(|v| !v.2.is_empty());
    Ok(out)
}

/// Leaves join as they come (their own spaces are the source's); a new `para` (p, q1, q2…) adds one space
/// so poetry lines don't run together.
struct Walk<'a> { code: &'a str, out: Vec<Verse>, gap: bool, wrong_book: bool }

impl Walk<'_> {
    /// `upper` is set under the divine name (`char` style `nd`), shown as LORD like the printed small caps.
    fn walk<'n>(&mut self, nodes: &'n [Node], inherited: Option<&'n str>, upper: bool) {
        for n in nodes {
            if matches!(n.name.as_str(), "note" | "verse" | "chapter") || n.attrs.style.as_deref() == Some("v") { continue; }
            let id = n.attrs.verse_id.as_deref().or(inherited);
            if n.kind == "text" {
                let Some(id) = id else { continue };
                let Some((c, v)) = parse_verse_id(self.code, id) else { self.wrong_book |= !id.starts_with(&format!("{}.", self.code)); continue };
                let text = if upper { n.text.to_uppercase() } else { n.text.clone() };
                match self.out.iter_mut().rev().find(|x| (x.0, x.1) == (c, v)) {
                    Some(x) => { if self.gap { x.2.push(' '); } x.2.push_str(&text); }
                    None => self.out.push((c, v, text)),
                }
                self.gap = false;
            } else {
                if n.name == "para" { self.gap = true; }
                self.walk(&n.items, id, upper || n.attrs.style.as_deref() == Some("nd"));
            }
        }
    }
}

// ---------- FUMS ----------

/// Device id for FUMS: anonymous and constant per install (`init_device_id`); a random one per run if never set.
static DEVICE_ID: OnceLock<String> = OnceLock::new();

pub fn set_device_id(id: &str) { let _ = DEVICE_ID.set(id.to_string()); }

/// Loads the install's device id once per run (see `device_id`).
pub fn init_device_id(root: &Path) { DEVICE_ID.get_or_init(|| device_id(root)); }

/// The id kept in `root/bible-device-id`, made on first use; a fresh one (this run only) if it can't be saved.
pub fn device_id(root: &Path) -> String {
    let path = root.join("bible-device-id");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim();
        if s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit()) { return s.to_string(); }
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let _ = crate::store::write_atomic(&path, id.as_bytes());
    id
}

/// FUMS is required of web apps and native apps report by GET to the f3 endpoint (docs.api.bible/guides/fair-use).
/// Fire and forget: a failed report never blocks Scripture.
pub fn report_fums(client: &reqwest::Client, tokens: &[String]) {
    static SESSION_ID: OnceLock<String> = OnceLock::new();
    let device = DEVICE_ID.get_or_init(random_id);
    let session = SESSION_ID.get_or_init(random_id);
    let Some(url) = fums_url(tokens, device, session) else { return };
    let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
    let client = client.clone();
    rt.spawn(async move { let _ = client.get(url).send().await; });
}

/// `https://fums.api.bible/f3?t=…&t=…&dId=…&sId=…` (one `t` per token; no user id, nothing personal).
pub fn fums_url(tokens: &[String], device: &str, session: &str) -> Option<reqwest::Url> {
    if tokens.is_empty() { return None; }
    let params = tokens.iter().map(|t| ("t", t.as_str())).chain([("dId", device), ("sId", session)]);
    reqwest::Url::parse_with_params(FUMS, params).ok()
}

/// 32 hex digits from the std hasher's random keys and the clock. Anonymous, not secret.
fn random_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let half = |salt: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(salt);
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
        h.finish()
    };
    format!("{:016x}{:016x}", half(1), half(2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{BOOKS, parse_reference};

    fn usfm(eb: &str) -> &'static str { USFM[BOOKS.iter().position(|b| b.0 == eb).unwrap()] }

    #[test]
    fn usfm_codes_line_up_with_books() {
        assert_eq!(USFM.len(), BOOKS.len());
        for (eb, u) in [("GEN", "GEN"), ("SOL", "SNG"), ("EZE", "EZK"), ("JOE", "JOL"), ("NAH", "NAM"), ("MAR", "MRK"), ("JOH", "JHN"),
            ("PHI", "PHP"), ("JAM", "JAS"), ("1JO", "1JN"), ("2JO", "2JN"), ("3JO", "3JN"), ("JUD", "JUD"), ("PHM", "PHM"), ("REV", "REV"),
            ("PSA", "PSA"), ("1SA", "1SA"), ("ZEP", "ZEP"), ("MAL", "MAL"), ("MAT", "MAT")] {
            assert_eq!(usfm(eb), u, "{eb}");
        }
        let mut sorted = USFM.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 66, "no duplicates");
    }

    #[test]
    fn passage_ids() {
        let r = parse_reference("John 3:16-18").unwrap();
        let p = &r.parts[0];
        assert_eq!(passage_id(USFM[r.book as usize], (p.start.0, p.start.1.unwrap()), (p.end.0, p.end.1.unwrap())), "JHN.3.16-JHN.3.18");
        assert_eq!(passage_id("JHN", (1, 51), (2, 2)), "JHN.1.51-JHN.2.2");
        assert_eq!(passage_id("SNG", (2, 1), (2, 1)), "SNG.2.1");
    }

    #[test]
    fn verse_ids() {
        assert_eq!(parse_verse_id("JHN", "JHN.3.16"), Some((3, 16)));
        assert_eq!(parse_verse_id("JHN", "JHN.3.16-17"), Some((3, 16)));
        assert_eq!(parse_verse_id("JHN", "GEN.1.1"), None);
        assert_eq!(parse_verse_id("JHN", "JHN.3"), None);
    }

    // Shape from docs.api.bible/guides/bibles (GET /v1/bibles).
    #[test]
    fn picks_the_bible_by_exact_abbreviation() {
        let list: BibleList = serde_json::from_str(r#"{"data":[
            {"id":"aaa-01","dblId":"x","abbreviation":"CSBx","abbreviationLocal":"CSBx","language":{"id":"eng","name":"English"},"name":"Other"},
            {"id":"es-01","abbreviation":"CSB","abbreviationLocal":"CSB","language":{"id":"spa"},"name":"Spanish"},
            {"id":"en-01","abbreviation":"engCSB","abbreviationLocal":"CSB","language":{"id":"eng"},"name":"Christian Standard Bible"}]}"#).unwrap();
        assert_eq!(pick_bible(&list, "csb"), Some("en-01"));
        assert_eq!(pick_bible(&list, "NLT"), None);
        let list: BibleList = serde_json::from_str(r#"{"data":[
            {"id":"p-01","abbreviation":"TPT","name":"The Passion Translation","language":{"id":"eng"}},
            {"id":"n-01","abbreviation":"SB","abbreviationLocal":"SB","name":"New World Translation","language":{"id":"eng"}}]}"#).unwrap();
        assert_eq!(pick_bible(&list, "TPT"), None);
        assert_eq!(pick_bible(&list, "SB"), None, "blocked by its name whatever the abbreviation");
    }

    // Shape from docs.api.bible/resources/content-output-formats (content-type=json) and the `meta.fumsToken`
    // example in docs.api.bible/guides/fair-use.
    const JOHN: &str = r#"{"data":{"id":"JHN.3.16-JHN.3.17","bibleId":"en-01","orgId":"JHN.3.16-JHN.3.17","reference":"John 3:16-17",
        "verseCount":2,"copyright":"Christian Standard Bible®\n Copyright © 2017 by Holman Bible Publishers.",
        "content":[
          {"name":"para","type":"tag","attrs":{"style":"s1"},"items":[{"text":"God's Love","type":"text"}]},
          {"name":"para","type":"tag","attrs":{"style":"p"},"items":[
            {"name":"verse","type":"tag","attrs":{"number":"16","style":"v","sid":"JHN 3:16"},"items":[{"text":"16","type":"text"}]},
            {"text":"For God loved the world in this way: ","type":"text","attrs":{"verseId":"JHN.3.16","verseOrgIds":["JHN.3.16"]}},
            {"name":"char","type":"tag","attrs":{"style":"wj"},"items":[{"text":"he gave his one and only Son.","type":"text","attrs":{"verseId":"JHN.3.16","verseOrgIds":["JHN.3.16"]}}]},
            {"name":"note","type":"tag","attrs":{"style":"f"},"items":[{"text":"Or loved","type":"text","attrs":{"verseId":"JHN.3.16"}}]},
            {"name":"verse","type":"tag","attrs":{"number":"17","style":"v","sid":"JHN 3:17"},"items":[{"text":"17","type":"text"}]},
            {"text":"For God did not send","type":"text","attrs":{"verseId":"JHN.3.17","verseOrgIds":["JHN.3.17"]}}]},
          {"name":"para","type":"tag","attrs":{"style":"q1"},"items":[
            {"text":"  his Son.","type":"text","attrs":{"verseId":"JHN.3.17","verseOrgIds":["JHN.3.17"]}}]}]},
        "meta":{"fumsToken":"G0wBgBwHbqxlyoOg"}}"#;

    #[test]
    fn parses_json_content() {
        let r: Response = serde_json::from_str(JOHN).unwrap();
        assert_eq!(parse("JHN", &r).unwrap(), vec![
            (3, 16, "For God loved the world in this way: he gave his one and only Son.".into()),
            (3, 17, "For God did not send his Son.".into()),
        ]);
        assert_eq!(r.meta.fums_token.as_deref(), Some("G0wBgBwHbqxlyoOg"));
        assert!(r.data.copyright.contains("Holman"));
    }

    #[test]
    fn joins_leaves_without_extra_spaces() {
        let r: Response = serde_json::from_str(r#"{"data":{"content":[{"name":"para","type":"tag","attrs":{"style":"p"},"items":[
            {"text":"I rejoiced with those who said to me, “Let us go to the ","type":"text","attrs":{"verseId":"PSA.122.1"}},
            {"name":"char","type":"tag","attrs":{"style":"nd"},"items":[{"text":"Lord","type":"text","attrs":{"verseId":"PSA.122.1"}}]},
            {"text":"’s house.”","type":"text","attrs":{"verseId":"PSA.122.1"}}]}]}}"#).unwrap();
        assert_eq!(parse("PSA", &r).unwrap(), vec![(122, 1, "I rejoiced with those who said to me, “Let us go to the LORD’s house.”".into())]);
        let r: Response = serde_json::from_str(r#"{"data":{"content":[{"name":"para","type":"tag","attrs":{"style":"p"},"items":[
            {"text":"he gave his one and only ","type":"text","attrs":{"verseId":"JHN.3.16"}},
            {"name":"char","type":"tag","attrs":{"style":"wj"},"items":[{"text":"Son","type":"text","attrs":{"verseId":"JHN.3.16"}}]},
            {"text":", so that","type":"text","attrs":{"verseId":"JHN.3.16"}}]}]}}"#).unwrap();
        assert_eq!(parse("JHN", &r).unwrap()[0].2, "he gave his one and only Son, so that");
    }

    #[test]
    fn poetry_lines_get_a_space() {
        let r: Response = serde_json::from_str(r#"{"data":{"content":[
            {"name":"para","type":"tag","attrs":{"style":"q1"},"items":[{"text":"The Lord is my shepherd;","type":"text","attrs":{"verseId":"PSA.23.1"}}]},
            {"name":"para","type":"tag","attrs":{"style":"q2"},"items":[{"text":"I have what I need.","type":"text","attrs":{"verseId":"PSA.23.1"}}]}]}}"#).unwrap();
        assert_eq!(parse("PSA", &r).unwrap()[0].2, "The Lord is my shepherd; I have what I need.");
    }

    #[test]
    fn device_id_is_kept_on_disk() {
        let root = crate::store::test_dir();
        let id = device_id(&root);
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(device_id(&root), id, "same id next launch");
        std::fs::write(root.join("bible-device-id"), "junk").unwrap();
        let fresh = device_id(&root);
        assert_ne!(fresh, id);
        assert_eq!(device_id(&root), fresh);
    }

    #[test]
    fn crosses_chapters_by_verse_id() {
        let r: Response = serde_json::from_str(r#"{"data":{"content":[{"name":"para","type":"tag","attrs":{"style":"p"},"items":[
            {"text":"a","type":"text","attrs":{"verseId":"JHN.1.51"}}]},{"name":"para","type":"tag","attrs":{"style":"p"},"items":[
            {"text":"b","type":"text","attrs":{"verseId":"JHN.2.1"}},{"text":"c","type":"text","attrs":{"verseId":"JHN.2.2"}}]}]}}"#).unwrap();
        let v = parse("JHN", &r).unwrap();
        assert_eq!(v.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(1, 51), (2, 1), (2, 2)]);
    }

    #[test]
    fn wrong_book_is_an_error() {
        let r: Response = serde_json::from_str(r#"{"data":{"content":[{"text":"In the beginning","type":"text","attrs":{"verseId":"GEN.1.1"}}]}}"#).unwrap();
        assert!(parse("JHN", &r).is_err());
    }

    #[test]
    fn forbidden_means_not_on_plan() {
        assert!(matches!(status_error(403, "CSB"), FetchError::Other(m) if m.contains("Plan")));
        assert_eq!(status_error(401, "CSB"), FetchError::BadKey);
        assert_eq!(status_error(429, "CSB"), FetchError::RateLimited);
    }

    #[test]
    fn fums_urls() {
        let u = fums_url(&["tok1".into(), "a b".into()], "dev", "ses").unwrap();
        assert_eq!(u.as_str(), "https://fums.api.bible/f3?t=tok1&t=a+b&dId=dev&sId=ses");
        assert!(fums_url(&[], "d", "s").is_none());
        let id = random_id();
        assert_eq!(id.len(), 32);
        assert_ne!(id, random_id());
    }
}
