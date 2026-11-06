//! ESV text from Crossway's API (api.esv.org). Free for church and ministry use with the church's own key;
//! at most 500 verses or half a book may be stored locally (enforced by `sources::Cache`).
use super::{Part, sources::{FetchError, Verse, human_query, tidy}};
use serde::Deserialize;

const URL: &str = "https://api.esv.org/v3/passage/text/";

#[derive(Deserialize, Debug)]
pub struct Response {
    #[serde(default)]
    parsed: Vec<Vec<u32>>,
    #[serde(default)]
    passages: Vec<String>,
}

pub async fn fetch(client: &reqwest::Client, key: &str, book: u8, part: &Part) -> Result<Vec<Verse>, FetchError> {
    let res = client.get(URL)
        .header("Authorization", format!("Token {key}"))
        .query(&[
            ("q", human_query(book, part).as_str()),
            ("include-passage-references", "false"),
            ("include-verse-numbers", "true"),
            ("include-first-verse-numbers", "true"),
            ("include-footnotes", "false"),
            ("include-footnote-body", "false"),
            ("include-headings", "false"),
            ("include-short-copyright", "false"),
            ("include-copyright", "false"),
            ("include-selahs", "true"),
            ("indent-paragraphs", "0"),
            ("indent-poetry", "false"),
            ("indent-declares", "0"),
            ("indent-psalm-doxology", "0"),
            ("line-length", "0"),
        ])
        .send().await?;
    if !res.status().is_success() { return Err(FetchError::from_status(res.status().as_u16())); }
    parse(book, &res.json::<Response>().await?)
}

/// Splits each returned passage into verses. Passage ids are `BBCCCVVV` (book 1-66).
pub fn parse(book: u8, r: &Response) -> Result<Vec<Verse>, FetchError> {
    let mut out = Vec::new();
    for (range, text) in r.parsed.iter().zip(&r.passages) {
        let Some(&start) = range.first() else { continue };
        if start / 1_000_000 != book as u32 + 1 { return Err(FetchError::Other("the ESV service returned a different book".into())); }
        out.extend(split_verses(text, (start / 1000 % 1000) as u16, book == PSALMS));
    }
    if out.is_empty() { return Err(FetchError::NotFound); }
    Ok(out)
}

const PSALMS: u8 = 18;

/// `[16] For God… [17] For God did not…`. A `[C:V]` marker names its chapter; a plain number lower
/// than the one before it means the passage crossed into the next chapter. In `psalms`, the paragraph
/// before a new chapter's first marker is that psalm's superscription, not the last verse's text.
pub fn split_verses(text: &str, first_chapter: u16, psalms: bool) -> Vec<Verse> {
    let mut out: Vec<Verse> = Vec::new();
    let (mut chapter, mut last) = (first_chapter, 0u16);
    let mut rest = text;
    let mut current: Option<(u16, u16)> = None;
    let mut body = String::new();
    let flush = |cur: Option<(u16, u16)>, body: &mut String, out: &mut Vec<Verse>| {
        if let Some((c, v)) = cur { let t = tidy(body); if !t.is_empty() { out.push((c, v, t)); } }
        body.clear();
    };
    while let Some(open) = rest.find('[') {
        let marker = rest[open + 1..].find(']').map(|end| &rest[open + 1..open + 1 + end]);
        let parsed = marker.and_then(|m| match m.split_once(':') {
            Some((c, v)) => Some((c.trim().parse::<u16>().ok()?, v.trim().parse::<u16>().ok()?)),
            None => { let v = m.trim().parse::<u16>().ok()?; Some((if v <= last { chapter + 1 } else { chapter }, v)) }
        });
        match (parsed, marker) {
            (Some((c, v)), Some(m)) => {
                body.push_str(&rest[..open]);
                let cut = if psalms && c != chapter && current.is_some() { body.trim_end().rfind("\n\n") } else { None };
                if let Some(i) = cut { body.truncate(i); }
                flush(current, &mut body, &mut out);
                (chapter, last) = (c, v);
                current = Some((c, v));
                rest = &rest[open + m.len() + 2..];
            }
            _ => { body.push_str(&rest[..=open]); rest = &rest[open + 1..]; }
        }
    }
    body.push_str(rest);
    flush(current, &mut body, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_verse_markers() {
        let v = split_verses("  [16] For God so loved the world,\n    that he gave   his only Son.  [17] For God did not send his Son.\n\n", 3, false);
        assert_eq!(v, vec![(3, 16, "For God so loved the world, that he gave his only Son.".into()), (3, 17, "For God did not send his Son.".into())]);
    }

    #[test]
    fn crossing_a_chapter_with_plain_numbers() {
        let v = split_verses("[51] a [1] b [2] c", 1, false);
        assert_eq!(v.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(1, 51), (2, 1), (2, 2)]);
    }

    #[test]
    fn crossing_a_chapter_with_chapter_markers() {
        let v = split_verses("[51] a [2:1] b [2] c", 1, false);
        assert_eq!(v.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(1, 51), (2, 1), (2, 2)]);
    }

    #[test]
    fn psalm_superscription_is_not_the_last_verse_of_the_previous_psalm() {
        let text = "[5] You prepare a table.\n\n[6] Surely goodness and mercy\nshall follow me\nforever.\n\nA Psalm of David.\n\n[1] The earth is the LORD's\n\n";
        let v = split_verses(text, 23, true);
        assert_eq!(v, vec![(23, 5, "You prepare a table.".into()), (23, 6, "Surely goodness and mercy shall follow me forever.".into()),
            (24, 1, "The earth is the LORD's".into())]);
        // Same with a chapter marker, and a psalm with no superscription keeps its last verse whole.
        let v = split_verses("[6] forever.\n\nA Psalm of David.\n\n[24:1] The earth", 23, true);
        assert_eq!(v[0].2, "forever.");
        let v = split_verses("[6] they will perish.\n\n[1] Why do the nations", 1, true);
        assert_eq!(v[0].2, "they will perish.");
        // Other books keep text across a chapter break.
        let v = split_verses("[51] a\n\nb [1] c", 1, false);
        assert_eq!(v[0].2, "a b");
    }

    #[test]
    fn keeps_brackets_that_are_not_markers() {
        let v = split_verses("[1] Blessed is the man [who walks] not. [2] But his delight", 1, false);
        assert_eq!(v[0].2, "Blessed is the man [who walks] not.");
        assert_eq!(v[1].2, "But his delight");
    }

    #[test]
    fn parses_a_response() {
        let r: Response = serde_json::from_str(r#"{"query":"John 3:16-17","canonical":"John 3:16–17","parsed":[[43003016,43003017]],
            "passage_meta":[{}],"passages":["[16] For God so loved the world. [17] For God did not send his Son.\n\n"]}"#).unwrap();
        let v = parse(42, &r).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[1], (3, 17, "For God did not send his Son.".into()));
    }

    #[test]
    fn empty_or_wrong_book_is_an_error() {
        let r: Response = serde_json::from_str(r#"{"query":"x","parsed":[],"passages":[]}"#).unwrap();
        assert_eq!(parse(42, &r), Err(FetchError::NotFound));
        let r: Response = serde_json::from_str(r#"{"parsed":[[1001001,1001001]],"passages":["[1] In the beginning"]}"#).unwrap();
        assert!(parse(42, &r).is_err());
    }
}
