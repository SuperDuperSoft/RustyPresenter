//! Bible text from the YouVersion Platform API (api.youversion.com). Each church registers a free app at
//! platform.youversion.com and accepts each publisher's license there (Biblica for NIV, and so on);
//! a Bible whose license the church hasn't accepted answers 403 "Access denied for {id}".
//! Docs: https://developers.youversion.com/api-usage, https://developers.youversion.com/api (bibles),
//! https://developers.youversion.com/error-codes, and YouVersion's own agent notes at
//! https://github.com/youversion/platform-skills (skills/youversion-platform-api/SKILL.md).
use super::{Part, sources::{FetchError, Verse, tidy}};
use serde::Deserialize;

const BASE: &str = "https://api.youversion.com/v1/bibles";

/// A range may not cross a chapter, so each chapter is its own request. The default app allowance is
/// about 200 requests an hour, so very long chapter spans are refused rather than burning it.
const MAX_REQUESTS: usize = 20;

/// Translation label -> YouVersion Bible id (the number in bible.com/versions/{id}).
/// 111 and 3034 are named in YouVersion's docs; 72, 1713 and 116 are their bible.com version pages.
/// Whether a church's app may read one depends on the licenses it accepted, not on this table.
pub const VERSIONS: &[(&str, u32)] = &[("NIV", 111), ("HCSB", 72), ("CSB", 1713), ("NLT", 116), ("BSB", 3034)];

/// Known label, or a bare number for any other Bible the church's app is licensed for.
pub fn version_id(translation: &str) -> Option<u32> {
    VERSIONS.iter().find(|(t, _)| t.eq_ignore_ascii_case(translation.trim())).map(|&(_, id)| id)
        .or_else(|| translation.trim().parse().ok())
}

/// USFM book ids in `BOOKS` order (Genesis..Revelation). Same list YouVersion returns for BSB's `books`.
const USFM: [&str; 66] = [
    "GEN", "EXO", "LEV", "NUM", "DEU", "JOS", "JDG", "RUT", "1SA", "2SA", "1KI", "2KI", "1CH", "2CH", "EZR", "NEH",
    "EST", "JOB", "PSA", "PRO", "ECC", "SNG", "ISA", "JER", "LAM", "EZK", "DAN", "HOS", "JOL", "AMO", "OBA", "JON",
    "MIC", "NAM", "HAB", "ZEP", "HAG", "ZEC", "MAL",
    "MAT", "MRK", "LUK", "JHN", "ACT", "ROM", "1CO", "2CO", "GAL", "EPH", "PHP", "COL", "1TH", "2TH", "1TI", "2TI",
    "TIT", "PHM", "HEB", "JAS", "1PE", "2PE", "1JN", "2JN", "3JN", "JUD", "REV",
];

/// `GET /v1/bibles/{id}/passages/{usfm}`: `{"id": "JHN.3.16", "content": "...", "reference": "John 3:16"}`.
#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct Response {
    id: String,
    content: String,
}

/// One passage request: a USFM id inside a single chapter, and which of its verses to keep.
#[derive(Debug, PartialEq)]
pub struct Request {
    pub id: String,
    pub chapter: u16,
    pub from: u16,
    pub to: Option<u16>,
}

/// `JHN.3.16`, `JHN.3.16-18`, `PSA.23`. A cross-chapter range becomes the rest of the first chapter
/// (fetched whole, then trimmed), any whole chapters between, and `JHN.2.1-2` for the last.
pub fn requests(book: u8, part: &Part) -> Vec<Request> {
    let b = USFM[book as usize];
    let ((c, v), (e, w)) = (part.start, part.end);
    (c..=e).map(|ch| {
        let from = if ch == c { v.unwrap_or(1) } else { 1 };
        let to = if ch == e { w } else { None };
        let id = match to {
            Some(t) if t == from => format!("{b}.{ch}.{from}"),
            Some(t) => format!("{b}.{ch}.{from}-{t}"),
            None => format!("{b}.{ch}"),
        };
        Request { id, chapter: ch, from, to }
    }).collect()
}

pub async fn fetch(client: &reqwest::Client, key: &str, translation: &str, book: u8, part: &Part) -> Result<Vec<Verse>, FetchError> {
    let bible = version_id(translation).ok_or_else(|| FetchError::Other(format!("YouVersion has no Bible id for {translation}")))?;
    let reqs = requests(book, part);
    if reqs.len() > MAX_REQUESTS { return Err(FetchError::Other("that passage spans too many chapters; choose fewer".into())); }
    let mut out = Vec::new();
    for r in &reqs {
        // HTML, because the text format gives no verse boundaries. Headings and notes are off by
        // default for verses but on for whole chapters, so both are turned off explicitly.
        let res = client.get(format!("{BASE}/{bible}/passages/{}", r.id))
            .header("X-YVP-App-Key", key)
            .header("Accept", "application/json")
            .query(&[("format", "html"), ("include_headings", "false"), ("include_notes", "false")])
            .send().await?;
        let status = res.status().as_u16();
        if status == 204 { continue; }
        if !res.status().is_success() { return Err(status_error(status, bible)); }
        out.extend(parse(r, &res.json::<Response>().await?)?);
    }
    if out.is_empty() { return Err(FetchError::NotFound); }
    Ok(out)
}

/// 403 here means a recognised key without that publisher's license, not a bad key.
fn status_error(status: u16, bible: u32) -> FetchError {
    match status {
        403 => FetchError::Other(format!("this YouVersion app isn't licensed for Bible {bible}; accept the publisher's license at platform.youversion.com")),
        412 => FetchError::Other("the YouVersion app is archived or removed; check it at platform.youversion.com".into()),
        s => FetchError::from_status(s),
    }
}

/// Verses of one chapter's response, trimmed to the request's range.
pub fn parse(r: &Request, res: &Response) -> Result<Vec<Verse>, FetchError> {
    if !res.id.is_empty() && res.id.split('.').next() != r.id.split('.').next() {
        return Err(FetchError::Other("YouVersion returned a different book".into()));
    }
    Ok(split_verses(&res.content).into_iter()
        .filter(|(v, _)| *v >= r.from && r.to.is_none_or(|t| *v <= t))
        .map(|(v, t)| (r.chapter, v, t))
        .collect())
}

/// Each verse starts at `<span class="yv-v" v="16"></span><span class="yv-vlbl">16</span>` and runs to the next
/// marker, across paragraph `<div>`s. Verse labels, headings (`yv-h`) and notes (`yv-n`) are dropped.
pub fn split_verses(html: &str) -> Vec<(u16, String)> {
    let mut out: Vec<(u16, String)> = Vec::new();
    let (mut body, mut current) = (String::new(), None::<u16>);
    // One entry per open element: true inside a label, heading or note.
    let mut skip: Vec<bool> = Vec::new();
    let flush = |cur: Option<u16>, body: &mut String, out: &mut Vec<(u16, String)>| {
        if let Some(v) = cur { let t = tidy(&decode(body)); if !t.is_empty() { out.push((v, t)); } }
        body.clear();
    };
    let mut rest = html;
    loop {
        let lt = rest.find('<').unwrap_or(rest.len());
        if !skip.contains(&true) { body.push_str(&rest[..lt]); }
        rest = &rest[lt..];
        let Some(gt) = rest.find('>') else { break };
        let tag = &rest[1..gt];
        rest = &rest[gt + 1..];
        if tag.starts_with('!') { continue; }
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("").to_ascii_lowercase();
        let block = matches!(name.as_str(), "div" | "p" | "br" | "li" | "tr" | "td" | "table");
        if tag.starts_with('/') {
            skip.pop();
            if block { body.push(' '); }
            continue;
        }
        if block { body.push(' '); }
        let classes = attr(tag, "class").unwrap_or("");
        if classes.split_whitespace().any(|c| c == "yv-v") {
            let n = attr(tag, "v").and_then(|v| v.split(|c: char| !c.is_ascii_digit()).next()?.parse::<u16>().ok());
            if let Some(n) = n.filter(|&n| Some(n) != current) {
                flush(current, &mut body, &mut out);
                current = Some(n);
            }
        }
        if tag.ends_with('/') || matches!(name.as_str(), "br" | "hr" | "img" | "wbr") { continue; }
        skip.push(classes.split_whitespace().any(|c| matches!(c, "yv-vlbl" | "yv-h" | "yv-n")));
    }
    flush(current, &mut body, &mut out);
    out
}

/// `class="s1 yv-h"` -> `s1 yv-h`.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    ['"', '\''].into_iter().find_map(|q| {
        let pat = format!(" {name}={q}");
        let start = tag.find(&pat)? + pat.len();
        tag[start..].find(q).map(|end| &tag[start..start + end])
    })
}

/// The HTML entities scripture text carries: named punctuation and numeric references.
fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let found = rest.find(';').filter(|&e| e <= 10).and_then(|e| Some((e, entity(&rest[1..e])?)));
        match found {
            Some((e, c)) => { out.push(c); rest = &rest[e + 1..]; }
            None => { out.push('&'); rest = &rest[1..]; }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&', "lt" => '<', "gt" => '>', "quot" => '"', "apos" => '\'', "nbsp" => ' ',
        "lsquo" => '‘', "rsquo" => '’', "ldquo" => '“', "rdquo" => '”', "mdash" => '—', "ndash" => '–', "hellip" => '…',
        n if n.starts_with("#x") || n.starts_with("#X") => char::from_u32(u32::from_str_radix(&n[2..], 16).ok()?)?,
        n if n.starts_with('#') => char::from_u32(n[1..].parse().ok()?)?,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const JOHN: u8 = 42;
    fn ids(book: u8, start: (u16, Option<u16>), end: (u16, Option<u16>)) -> Vec<String> {
        requests(book, &Part { start, end }).into_iter().map(|r| r.id).collect()
    }

    #[test]
    fn usfm_ids_follow_books_order() {
        assert_eq!(USFM.len(), super::super::BOOKS.len());
        assert_eq!((USFM[0], USFM[18], USFM[21], USFM[42], USFM[49], USFM[65]), ("GEN", "PSA", "SNG", "JHN", "PHP", "REV"));
        let mut u = USFM.to_vec(); u.sort(); u.dedup();
        assert_eq!(u.len(), 66);
    }

    #[test]
    fn version_ids() {
        assert_eq!(version_id("NIV"), Some(111));
        assert_eq!(version_id("hcsb"), Some(72));
        assert_eq!(version_id(" 1713 "), Some(1713));
        assert_eq!(version_id("XYZ"), None);
    }

    #[test]
    fn passage_ids_stay_inside_one_chapter() {
        assert_eq!(ids(JOHN, (3, Some(16)), (3, Some(16))), ["JHN.3.16"]);
        assert_eq!(ids(JOHN, (3, Some(16)), (3, Some(18))), ["JHN.3.16-18"]);
        assert_eq!(ids(18, (23, None), (23, None)), ["PSA.23"]);
        assert_eq!(ids(18, (23, None), (24, None)), ["PSA.23", "PSA.24"]);
        assert_eq!(ids(JOHN, (1, Some(51)), (3, Some(1))), ["JHN.1", "JHN.2", "JHN.3.1"]);
        let r = requests(JOHN, &Part { start: (1, Some(51)), end: (2, Some(2)) });
        assert_eq!(r, vec![
            Request { id: "JHN.1".into(), chapter: 1, from: 51, to: None },
            Request { id: "JHN.2.1-2".into(), chapter: 2, from: 1, to: Some(2) },
        ]);
    }

    // Markup as in YouVersion's SDK fixtures (github.com/youversion/platform-sdk-react,
    // packages/ui/src/test/mock-data/passages.json and packages/core/src/__tests__/MockPassages.ts),
    // with placeholder words in place of the licensed text.
    const ROM1: &str = r#"<div><div class="p"><span class="yv-v" v="6"></span><span class="yv-vlbl">6</span>Six text.</div><div class="p"><span class="yv-v" v="7"></span><span class="yv-vlbl">7</span>Seven starts,</div><div class="p">and seven &#8220;continues&#8221;.</div><div class="s1 yv-h">A Heading</div><div class="p"><span class="yv-v" v="8"></span><span class="yv-vlbl">8</span>Eight<span class="yv-n f"><span class="fr">1:8 </span><span class="ft">A note</span></span> has <span class="wj">red</span> letters&nbsp;&amp; more.</div></div>"#;

    #[test]
    fn splits_verses_and_strips_labels_headings_and_notes() {
        assert_eq!(split_verses(ROM1), vec![
            (6, "Six text.".into()),
            (7, "Seven starts, and seven \u{201c}continues\u{201d}.".into()),
            (8, "Eight has red letters & more.".into()),
        ]);
    }

    #[test]
    fn text_before_the_first_verse_is_dropped() {
        let v = split_verses(r#"<div><div class="d">For the choir director.</div><div class="q1"><span class="yv-v" v="1"></span><span class="yv-vlbl">1</span>One</div></div>"#);
        assert_eq!(v, vec![(1, "One".into())]);
    }

    #[test]
    fn parses_a_response_and_trims_to_the_range() {
        let res: Response = serde_json::from_str(&serde_json::json!({
            "id": "ROM.1", "content": ROM1, "bible_id": 111, "reference": "Romans 1"
        }).to_string()).unwrap();
        let r = Request { id: "ROM.1".into(), chapter: 1, from: 7, to: None };
        let v = parse(&r, &res).unwrap();
        assert_eq!(v.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(1, 7), (1, 8)]);
        let r = Request { id: "ROM.1.6-7".into(), chapter: 1, from: 6, to: Some(7) };
        assert_eq!(parse(&r, &res).unwrap().len(), 2);
    }

    #[test]
    fn wrong_book_is_an_error_and_empty_content_is_empty() {
        let res: Response = serde_json::from_str(r#"{"id":"GEN.1.1","content":"<div></div>","reference":"Genesis 1:1"}"#).unwrap();
        let r = Request { id: "JHN.3.16".into(), chapter: 3, from: 16, to: Some(16) };
        assert!(parse(&r, &res).is_err());
        let res: Response = serde_json::from_str(r#"{"id":"JHN.3.16","content":"","reference":"John 3:16"}"#).unwrap();
        assert_eq!(parse(&r, &res), Ok(vec![]));
    }

    #[test]
    fn decodes_entities_and_keeps_stray_ampersands() {
        assert_eq!(decode("a &amp; b &#x2014; c &#39;d&#39; R&D &unknown;"), "a & b \u{2014} c 'd' R&D &unknown;");
    }

    #[test]
    fn license_refusal_is_not_a_bad_key() {
        assert!(matches!(status_error(403, 72), FetchError::Other(m) if m.contains("platform.youversion.com")));
        assert_eq!(status_error(401, 72), FetchError::BadKey);
        assert_eq!(status_error(429, 72), FetchError::RateLimited);
    }
}
