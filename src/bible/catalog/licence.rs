//! Reads a Bible's licence from its own notice page (`copr.htm`, `{id}_about.htm`), Bible library spec §3.1.
//!
//! Every `href` to `creativecommons.org` is read. Links of one kind (a deed and its legal code, two versions) count
//! once, and the highest version is credited. Links of different kinds credit the most restrictive: the union of
//! their NC, ND and SA terms, at the lowest version. ND with SA is ND, as no Creative Commons licence has both and
//! "no changes" already rules out sharing changes. A public-domain link (CC0, the PD Mark) or the words "public
//! domain" count only when there is no licence link. Anything else is no licence, and the Bible is not kept.
use crate::bible::library::Licence;

/// The terms of one Creative Commons licence link. `by` is always there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Terms { nc: bool, nd: bool, sa: bool }

impl Terms {
    fn kind(self) -> String {
        let mut k = String::from("by");
        if self.nc { k.push_str("-nc"); }
        if self.nd { k.push_str("-nd"); } else if self.sa { k.push_str("-sa"); }
        k
    }
}

/// A licence path's version as numbers, so "2.5" < "3.0" < "4.0".
fn version_key(v: &str) -> Vec<u32> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() }

/// The licence a notice page gives, or `None` when it gives none the app may use. `engnet` is the NET Bible, whose
/// terms are its own (spec §3.1).
pub fn read(translation_id: &str, html: &str) -> Option<Licence> {
    if translation_id == "engnet" { return Some(Licence::Net); }
    let mut cc: Vec<(Terms, String)> = Vec::new();
    let mut pd_link = false;
    for href in hrefs(html) {
        let lower = href.to_ascii_lowercase();
        let Some(at) = lower.find("creativecommons.org/") else { continue };
        let path: Vec<&str> = lower[at + "creativecommons.org/".len()..].split(['/', '?', '#']).filter(|s| !s.is_empty()).collect();
        match path.as_slice() {
            ["publicdomain", "zero" | "mark", ..] => pd_link = true,
            ["licenses", kind, version, ..] if version.chars().next().is_some_and(|c| c.is_ascii_digit()) => {
                if let Some(t) = terms(kind) { cc.push((t, version.to_string())); }
            }
            _ => {}
        }
    }
    if let Some(first) = cc.first().map(|(t, _)| *t) {
        let same = cc.iter().all(|(t, _)| *t == first);
        let terms = if same { first } else {
            let any = |f: fn(&Terms) -> bool| cc.iter().any(|(t, _)| f(t));
            let nd = any(|t| t.nd);
            Terms { nc: any(|t| t.nc), nd, sa: !nd && any(|t| t.sa) }
        };
        let pick = cc.iter().map(|(_, v)| v);
        let version = if same { pick.max_by_key(|v| version_key(v)) } else { pick.min_by_key(|v| version_key(v)) }.cloned()?;
        let kind = terms.kind();
        let url = format!("https://creativecommons.org/licenses/{kind}/{version}/");
        return Some(Licence::Cc { kind, version, url });
    }
    let words = html.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    (pd_link || words.contains("public domain")).then_some(Licence::Pd)
}

/// `by`, `by-sa`, `by-nc-nd`… as terms; `None` for anything that isn't a Creative Commons licence name.
fn terms(kind: &str) -> Option<Terms> {
    let mut parts = kind.split('-');
    if parts.next() != Some("by") { return None; }
    let mut t = Terms { nc: false, nd: false, sa: false };
    for p in parts {
        match p { "nc" => t.nc = true, "nd" => t.nd = true, "sa" => t.sa = true, _ => return None }
    }
    (!(t.nd && t.sa)).then_some(t)
}

/// Every `href` value in the page, quoted with `"` or `'`, or bare.
fn hrefs(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("href") {
        let mut j = from + i + 4;
        from = j;
        let rest = &html[j..];
        let skip = rest.len() - rest.trim_start().len();
        j += skip;
        if !html[j..].starts_with('=') { continue; }
        j += 1;
        let rest = &html[j..];
        j += rest.len() - rest.trim_start().len();
        let value = match html[j..].chars().next() {
            Some(q @ ('"' | '\'')) => html[j + 1..].split(q).next().unwrap_or(""),
            _ => html[j..].split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        };
        out.push(value.replace("&amp;", "&"));
        from = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cc(kind: &str, version: &str) -> Option<Licence> {
        Some(Licence::Cc { kind: kind.into(), version: version.into(), url: format!("https://creativecommons.org/licenses/{kind}/{version}/") })
    }

    /// The shape of eBible's `_about.htm`: a copyright line, then the licence as a link.
    fn about(body: &str) -> String {
        format!("<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>About</title></head><body><h1>Bible</h1>\
            <p>Copyright © 2020 Someone.</p>\n<p>{body}</p>\n<p><a href='https://eBible.org/find/details.php?id=x'>eBible.org</a></p></body></html>")
    }

    #[test]
    fn public_domain_by_words_cc0_or_the_mark() {
        assert_eq!(read("engwebp", &about("This Bible is in the Public\n Domain.")), Some(Licence::Pd));
        assert_eq!(read("x", &about(r#"<a href="http://creativecommons.org/publicdomain/zero/1.0/">CC0</a>"#)), Some(Licence::Pd));
        assert_eq!(read("x", &about(r#"<a href="https://creativecommons.org/publicdomain/mark/1.0/">PD Mark</a>"#)), Some(Licence::Pd));
    }

    #[test]
    fn each_creative_commons_kind() {
        for kind in ["by", "by-sa", "by-nd", "by-nc", "by-nc-sa", "by-nc-nd"] {
            let page = about(&format!(r#"This work is licensed under a <a href="http://creativecommons.org/licenses/{kind}/4.0/">licence</a>."#));
            assert_eq!(read("x", &page), cc(kind, "4.0"), "{kind}");
        }
        let unquoted = about("<a href=https://creativecommons.org/licenses/by-sa/3.0/deed.en>CC BY-SA</a>");
        assert_eq!(read("x", &unquoted), cc("by-sa", "3.0"));
        let caps = about(r#"<A HREF = 'HTTPS://CreativeCommons.org/Licenses/BY-NC-ND/2.5/legalcode'>x</A>"#);
        assert_eq!(read("x", &caps), cc("by-nc-nd", "2.5"));
    }

    #[test]
    fn several_links_of_one_kind_credit_the_highest_version() {
        let page = about(r#"<a href="https://creativecommons.org/licenses/by-sa/4.0/">deed</a>
            <a href="https://creativecommons.org/licenses/by-sa/4.0/legalcode">legal code</a>
            <a href="https://creativecommons.org/licenses/by-sa/3.0/">older</a>"#);
        assert_eq!(read("x", &page), cc("by-sa", "4.0"));
        let ten = about(r#"<a href="https://creativecommons.org/licenses/by/2.0/">a</a><a href="https://creativecommons.org/licenses/by/10.0/">b</a>"#);
        assert_eq!(read("x", &ten), cc("by", "10.0"), "versions compare as numbers");
    }

    #[test]
    fn different_kinds_credit_the_most_restrictive_at_the_lowest_version() {
        let page = |a: &str, b: &str| about(&format!(r#"<a href="https://creativecommons.org/licenses/{a}/">x</a> and <a href="https://creativecommons.org/licenses/{b}/">y</a>"#));
        assert_eq!(read("x", &page("by/4.0", "by-nc/3.0")), cc("by-nc", "3.0"));
        assert_eq!(read("x", &page("by-sa/4.0", "by-nc/4.0")), cc("by-nc-sa", "4.0"));
        assert_eq!(read("x", &page("by-sa/4.0", "by-nd/4.0")), cc("by-nd", "4.0"), "no changes rules out sharing changes");
        assert_eq!(read("x", &page("by-nc-sa/4.0", "by-nd/2.0")), cc("by-nc-nd", "2.0"));
    }

    #[test]
    fn a_licence_link_wins_over_public_domain() {
        let page = about(r#"Public domain in the USA. <a href="https://creativecommons.org/publicdomain/zero/1.0/">CC0</a>
            <a href="https://creativecommons.org/licenses/by-nc/4.0/">BY-NC</a>"#);
        assert_eq!(read("x", &page), cc("by-nc", "4.0"));
    }

    #[test]
    fn net_is_its_own_licence() {
        assert_eq!(read("engnet", &about("All rights reserved.")), Some(Licence::Net));
    }

    #[test]
    fn nothing_usable_is_no_licence() {
        assert_eq!(read("x", &about("All rights reserved. Used by permission.")), None);
        assert_eq!(read("x", &about(r#"<a href="https://creativecommons.org/about/">about CC</a>"#)), None);
        assert_eq!(read("x", &about(r#"<a href="https://creativecommons.org/licenses/by-xx/4.0/">odd</a>"#)), None);
        assert_eq!(read("x", &about(r#"<a href="https://creativecommons.org/licenses/by/">no version</a>"#)), None);
        assert_eq!(read("x", &about("Mentions creativecommons.org/licenses/by/4.0/ in text only")), None, "only links count");
        assert_eq!(read("x", ""), None);
    }
}
