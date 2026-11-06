//! NLT text from Tyndale's NLT API (api.nlt.to). Free for non-commercial use with the church's own key;
//! Tyndale's fair use allows up to 500 verses and never a whole book (enforced by `sources::Cache`).
//! Docs: https://api.nlt.to/Documentation (`/api/passages` returns an HTML excerpt).
use super::{Part, sources::{FetchError, Verse, human_query, tidy}};

const URL: &str = "https://api.nlt.to/api/passages";

pub async fn fetch(client: &reqwest::Client, key: &str, book: u8, part: &Part) -> Result<Vec<Verse>, FetchError> {
    // The key travels in the query string, so errors drop the URL before they reach a message.
    let res = client.get(URL)
        .query(&[("ref", query(book, part).as_str()), ("version", "NLT"), ("key", key)])
        .send().await.map_err(|e| FetchError::from(e.without_url()))?;
    if !res.status().is_success() { return Err(FetchError::from_status(res.status().as_u16())); }
    let html = res.text().await.map_err(|e| FetchError::from(e.without_url()))?;
    // An unknown reference comes back 200 with no verses; an out-of-range one is clamped to the
    // book's last chapter or verse, so anything outside the request is dropped.
    let out = within(parse(&html), part);
    if out.is_empty() { return Err(FetchError::NotFound); }
    Ok(out)
}

/// "John 3:16-18", "John 1:51-2:2". The API knows the book as "Song of Songs", not "Song of Solomon".
pub fn query(book: u8, part: &Part) -> String {
    let q = human_query(book, part);
    match q.strip_prefix("Song of Solomon") { Some(rest) => format!("Song of Songs{rest}"), None => q }
}

/// Keeps the verses that fall inside the requested part.
pub fn within(verses: Vec<Verse>, p: &Part) -> Vec<Verse> {
    let (from, to) = ((p.start.0, p.start.1.unwrap_or(1)), (p.end.0, p.end.1.unwrap_or(u16::MAX)));
    verses.into_iter().filter(|v| (v.0, v.1) >= from && (v.0, v.1) <= to).collect()
}

/// Elements that break a line; their tags become a space.
const BLOCK: &[&str] = &["p", "br", "div", "table", "tr", "td", "li", "section", "verse_export"];
const VOID: &[&str] = &["br", "hr", "img", "link", "meta"];

/// `<verse_export ch="3" vn="16">` wraps each verse and names its chapter; `<span class="vn">16</span>`
/// starts its text. Lists can put many verses' text, each behind its own `vn` span, inside one later
/// export (Ezra 2), so the spans decide the verse. Headings, psalm titles, footnote markers (`a.a-tn`)
/// and notes (`span.tn`) are dropped; small caps (`span.sc`, the divine name) become LORD.
pub fn parse(html: &str) -> Vec<Verse> {
    let mut out: Vec<Verse> = Vec::new();
    let Some(start) = html.find("<verse_export") else { return out };
    let mut rest = &html[start..];
    let (mut chapter, mut current, mut body) = (0u16, None::<(u16, u16)>, String::new());
    let mut skip: Option<(&str, usize)> = None; // element being dropped: tag name, nesting depth
    let mut open: Vec<(&str, bool)> = Vec::new(); // open elements; true = small caps
    loop {
        let lt = rest.find('<').unwrap_or(rest.len());
        if skip.is_none() {
            let text = decode(&rest[..lt]);
            body.push_str(&if open.iter().any(|o| o.1) { text.to_uppercase() } else { text });
        }
        let Some(gt) = rest[lt..].find('>') else { break };
        let tag = &rest[lt + 1..lt + gt];
        rest = &rest[lt + gt + 1..];
        let closing = tag.starts_with('/');
        let self_closing = tag.ends_with('/');
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        if let Some((n, d)) = skip {
            let d = if n != name || self_closing { d } else if closing { d - 1 } else if name == "p" { d } else { d + 1 };
            skip = (d > 0).then_some((n, d));
            continue;
        }
        if closing {
            if let Some(i) = open.iter().rposition(|o| o.0 == name) { open.truncate(i); }
            if BLOCK.contains(&name) { body.push(' '); }
            continue;
        }
        let class = attr(tag, "class").unwrap_or("");
        let has = |c: &str| class.split_whitespace().any(|x| x == c);
        if name == "verse_export" {
            chapter = attr(tag, "ch").and_then(num).unwrap_or(chapter);
            if let Some(v) = attr(tag, "vn").and_then(num) { start_verse(&mut current, (chapter, v), &mut body, &mut out); }
        } else if name == "span" && has("vn") {
            let end = rest.find("</span>").unwrap_or(rest.len());
            if let Some(v) = num(&rest[..end]) { start_verse(&mut current, (chapter, v), &mut body, &mut out); }
            rest = &rest[(end + "</span>".len()).min(rest.len())..];
        } else if matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") || has("tn") || has("a-tn") || has("psa-title") {
            if !self_closing { skip = Some((name, 1)); }
        } else {
            if BLOCK.contains(&name) { body.push(' '); }
            if !self_closing && !VOID.contains(&name) { open.push((name, has("sc"))); }
        }
    }
    flush(current, &mut body, &mut out);
    out.sort_by_key(|v| (v.0, v.1));
    out
}

fn start_verse(current: &mut Option<(u16, u16)>, next: (u16, u16), body: &mut String, out: &mut Vec<Verse>) {
    if *current == Some(next) { return; }
    flush(*current, body, out);
    *current = Some(next);
}

/// Adds the text gathered for a verse; a verse met twice (export, then its span) is joined.
fn flush(current: Option<(u16, u16)>, body: &mut String, out: &mut Vec<Verse>) {
    let t = tidy(body);
    body.clear();
    let Some((c, v)) = current else { return };
    if t.is_empty() { return; }
    match out.iter_mut().find(|x| (x.0, x.1) == (c, v)) {
        Some(x) => { x.2.push(' '); x.2.push_str(&t); }
        None => out.push((c, v, t)),
    }
}

/// `name="value"` inside a tag.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    tag[at..].find('"').map(|end| &tag[at..at + end])
}

/// Leading number: "16" → 16, "3-4" → 3.
fn num(s: &str) -> Option<u16> { s.trim().split(|c: char| !c.is_ascii_digit()).next()?.parse().ok() }

/// Decodes the few character references the API may use; anything unknown is left as written.
fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|&e| e <= 10);
        let ch = end.and_then(|e| match &rest[1..e] {
            "amp" => Some('&'), "lt" => Some('<'), "gt" => Some('>'), "quot" => Some('"'), "apos" => Some('\''), "nbsp" => Some(' '),
            n => n.strip_prefix("#x").or(n.strip_prefix("#X")).map(|h| u32::from_str_radix(h, 16))
                .or(n.strip_prefix('#').map(str::parse::<u32>)).and_then(Result::ok).and_then(char::from_u32),
        });
        match (ch, end) {
            (Some(c), Some(e)) => { out.push(c); rest = &rest[e + 1..]; }
            _ => { out.push('&'); rest = &rest[1..]; }
        }
    }
    out.push_str(rest);
    out
}

// Samples copy the markup the live API returned for /api/passages (see https://api.nlt.to/Documentation),
// with placeholder words in place of the NLT text so no copyrighted text is committed.
#[cfg(test)]
mod tests {
    use super::*;

    fn cv(v: &[Verse]) -> Vec<(u16, u16)> { v.iter().map(|x| (x.0, x.1)).collect() }

    #[test]
    fn drops_notes_and_verse_numbers() {
        // Shape of John 3:16-17.
        let html = r#" <!DOCTYPE html><html lang="en-US"><head><title>NLT API</title>
            <link rel="stylesheet" href="https://api.nlt.to/content/nlt-api-css?vers=1.04"/></head><body>
            <div id="bibletext" class=" NLT NLT BibleText section"><section><h2 class="bk_ch_vs_header">John 3:16-17, NLT</h2><verse_export orig="john_3_16" bk="john" ch="3" vn="16">
<p class="body"><span class="vn">16</span><span class="red">“Alpha beta gamma:<a class="a-tn">*</a><span class="tn"><span class="tn-ref">3:16</span> Or <em>delta.</em></span> epsilon zeta.</span> </verse_export><verse_export orig="john_3_17" bk="john" ch="3" vn="17"><span class="vn">17</span><span class="red">Eta theta.</span><p>
</verse_export></section></div></body></html>"#;
        assert_eq!(parse(html), vec![(3, 16, "“Alpha beta gamma: epsilon zeta.".into()), (3, 17, "Eta theta.".into())]);
    }

    #[test]
    fn crosses_a_chapter_and_drops_headings() {
        // Shape of John 1:51-2:2.
        let html = r#"<section><verse_export orig="john_1_51" bk="john" ch="1" vn="51"><span class="vn">51</span>One two.<a class="a-tn">*</a><span class="tn"><span class="tn-ref">1:51</span> Greek <em>x.</em></span>”</span><p>

</verse_export><verse_export orig="john_2_1" bk="john" ch="2" vn="1">
<h2 class="chapter-number"><span class="cw">John</span> <span class="cw_ch">2</span></h2>
<h3 class="subhead">A Heading</h3>
<p class="body-ch-hd"><span class="vn">1</span>Three four, </verse_export><verse_export orig="john_2_2" bk="john" ch="2" vn="2"><span class="vn">2</span>five six. </verse_export></section></div></body></html>"#;
        assert_eq!(parse(html), vec![(1, 51, "One two.”".into()), (2, 1, "Three four,".into()), (2, 2, "five six.".into())]);
    }

    #[test]
    fn poetry_small_caps_and_psalm_titles() {
        // Shape of Psalm 3:1-2.
        let html = r#"<verse_export orig="psal_3_1" bk="psal" ch="3" vn="1">
<h3 class="chapter-number"><span class="cw">Psalm</span> <span class="cw_ch">3</span></h3>
<h4 class="subhead">The <span class="subhead-sc">Lord</span> Title</h4>
<p class="psa-title">A psalm of someone.</p>
<p class="poet1-vn-sp"><span class="vn">1</span>O <span class="sc">Lord</span>, line one;</p>
<p class="poet2">line two.</p>
</verse_export><verse_export orig="psal_3_2" bk="psal" ch="3" vn="2">
<p class="poet1-vn"><span class="vn">2</span>Line three,</p>
<p class="selah">Interlude<a class="a-tn">*</a><span class="tn"><span class="tn-ref">3:2</span> Hebrew <em>Selah.</em></span></p>
</verse_export></section></div></body></html>"#;
        assert_eq!(parse(html), vec![(3, 1, "O LORD, line one; line two.".into()), (3, 2, "Line three, Interlude".into())]);
    }

    #[test]
    fn lists_inside_a_later_export() {
        // Shape of Ezra 2: verses 3.. are empty exports; their text sits in a table under a later one.
        let html = r#"<verse_export orig="ezra_2_2" bk="ezra" ch="2" vn="2"><span class="vn">2</span>Leaders listed.<p>
<p class="body-fl-sp">Count of the men:</p>
</verse_export><verse_export orig="ezra_2_3" bk="ezra" ch="2" vn="3">
</verse_export><verse_export orig="ezra_2_4" bk="ezra" ch="2" vn="4"></verse_export><verse_export orig="ezra_2_5" bk="ezra" ch="2" vn="5">
<table class="col2a">
<tr><td class="table-col"><span class="vn">3</span>Family A</td><td class="table-col">2,172</td></tr>
<tr><td class="table-col"><span class="vn">4</span>Family B</td><td class="table-col">372</td></tr>
</table>
<p class="body"><span class="vn">5</span>Family C 775.</p></verse_export>"#;
        assert_eq!(parse(html), vec![
            (2, 2, "Leaders listed. Count of the men:".into()), (2, 3, "Family A 2,172".into()),
            (2, 4, "Family B 372".into()), (2, 5, "Family C 775.".into()),
        ]);
    }

    #[test]
    fn unknown_reference_has_no_verses() {
        // "Foo 1:1" and "Song of Solomon 1:2" both come back 200 with an empty page.
        assert!(parse(r#"<!DOCTYPE html><html><body><div id="bibletext"></div></body></html>"#).is_empty());
        assert!(parse("").is_empty());
    }

    #[test]
    fn clamped_answers_are_filtered_out() {
        // "John 99:1" is answered with John 21:1; "John 3:99" with John 3:36.
        let got = parse(r#"<verse_export orig="john_21_1" bk="john" ch="21" vn="1"><p class="body-ch-hd"><span class="vn">1</span>Later.</p></verse_export>"#);
        assert!(within(got.clone(), &Part { start: (99, Some(1)), end: (99, Some(1)) }).is_empty());
        assert_eq!(within(got, &Part { start: (21, None), end: (21, None) }).len(), 1);
        let v = vec![(1, 50, "a".into()), (1, 51, "b".into()), (2, 1, "c".into()), (2, 2, "d".into()), (2, 3, "e".into())];
        assert_eq!(cv(&within(v, &Part { start: (1, Some(51)), end: (2, Some(2)) })), vec![(1, 51), (2, 1), (2, 2)]);
    }

    #[test]
    fn queries() {
        assert_eq!(query(42, &Part { start: (3, Some(16)), end: (3, Some(18)) }), "John 3:16-18");
        assert_eq!(query(42, &Part { start: (1, Some(51)), end: (2, Some(2)) }), "John 1:51-2:2");
        assert_eq!(query(18, &Part { start: (23, None), end: (24, None) }), "Psalms 23-24");
        assert_eq!(query(21, &Part { start: (1, Some(2)), end: (1, Some(2)) }), "Song of Songs 1:2");
    }

    #[test]
    fn decodes_character_references() {
        assert_eq!(decode("Tom &amp; Jerry &#8212; &#x2019;s &nbsp;x &bogus; a&b"), "Tom & Jerry — ’s  x &bogus; a&b");
    }
}
