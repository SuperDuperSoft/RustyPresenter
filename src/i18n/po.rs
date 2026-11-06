//! A gettext `.po` reader: header, `msgctxt`, `msgid`, `msgid_plural`, `msgstr`, `msgstr[i]`, strings over several
//! lines, C escapes and comments. Obsolete (`#~`) entries are kept apart; fuzzy ones are marked, not dropped (the
//! catalog skips them, the extractor keeps them).

use std::fmt;

/// One message. `strs` holds `msgstr`, or `msgstr[0..]` when `plural` is set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub ctx: Option<String>,
    pub id: String,
    pub plural: Option<String>,
    pub strs: Vec<String>,
    /// Translator comments (`# …`), without the `# `.
    pub comments: Vec<String>,
    /// Comments for translators from the code (`#. …`).
    pub extracted: Vec<String>,
    /// Where the text is used (`#: file:line`), one per line as written.
    pub refs: Vec<String>,
    /// `#, fuzzy, …`.
    pub flags: Vec<String>,
    /// The line of its `msgid`.
    pub line: usize,
}

impl Entry {
    pub fn fuzzy(&self) -> bool { self.flags.iter().any(|f| f == "fuzzy") }

    /// Every form has text (and the entry is not fuzzy).
    pub fn translated(&self) -> bool { !self.fuzzy() && !self.strs.is_empty() && self.strs.iter().all(|s| !s.is_empty()) }

    /// The `(context, msgid)` key; no context is `""`.
    pub fn key(&self) -> (String, String) { (self.ctx.clone().unwrap_or_default(), self.id.clone()) }
}

/// A parsed file. The header (the entry with an empty `msgid`) is split into `header`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PoFile {
    /// `Key: value` lines of the header, in order.
    pub header: Vec<(String, String)>,
    /// Comments above the header (`# …` lines), without the `# `.
    pub header_comments: Vec<String>,
    pub entries: Vec<Entry>,
    /// `#~` entries.
    pub obsolete: Vec<Entry>,
}

impl PoFile {
    /// A header field, e.g. `Plural-Forms`. Keys compare without case.
    pub fn header(&self, key: &str) -> Option<&str> {
        self.header.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParseError { pub line: usize, pub message: String }

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "line {}: {}", self.line, self.message) }
}

impl std::error::Error for ParseError {}

#[derive(Clone, Copy, PartialEq)]
enum Field { None, Ctx, Id, Plural, Str(usize) }

#[derive(Default)]
struct Building { entry: Entry, strs: Vec<(usize, String)>, has_id: bool, has_str: bool, started: bool }

/// Reads a `.po` (or `.pot`) file.
pub fn parse(text: &str) -> Result<PoFile, ParseError> {
    let mut file = PoFile::default();
    let mut entries = parse_entries(text.lines().enumerate().map(|(i, l)| (i + 1, l)), false)?;
    let obsolete_lines: Vec<(usize, String)> = text.lines().enumerate()
        .filter_map(|(i, l)| l.strip_prefix("#~").filter(|r| !r.starts_with('|')).map(|r| (i + 1, r.trim_start().to_string())))
        .collect();
    file.obsolete = parse_entries(obsolete_lines.iter().map(|(n, l)| (*n, l.as_str())), true)?;
    if let Some(pos) = entries.iter().position(|e| e.id.is_empty() && e.ctx.is_none()) {
        let h = entries.remove(pos);
        file.header_comments = h.comments.clone();
        let body = h.strs.first().cloned().unwrap_or_default();
        file.header = body.lines().filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
    }
    file.entries = entries;
    Ok(file)
}

fn parse_entries<'a>(lines: impl Iterator<Item = (usize, &'a str)>, obsolete: bool) -> Result<Vec<Entry>, ParseError> {
    let mut out = Vec::new();
    let mut b = Building::default();
    let mut field = Field::None;
    let err = |line: usize, message: &str| ParseError { line, message: message.into() };
    for (n, raw) in lines {
        let line = raw.trim();
        if line.is_empty() { continue }
        if line.starts_with('#') {
            if !obsolete && line.starts_with("#~") { continue }
            // A comment after a msgstr starts the next entry.
            if b.has_str { finish(&mut b, &mut out)?; field = Field::None; }
            b.started = true;
            if let Some(r) = line.strip_prefix("#,") {
                b.entry.flags.extend(r.split(',').map(|f| f.trim().to_string()).filter(|f| !f.is_empty()));
            } else if let Some(r) = line.strip_prefix("#.") {
                b.entry.extracted.push(r.trim().to_string());
            } else if let Some(r) = line.strip_prefix("#:") {
                b.entry.refs.push(r.trim().to_string());
            } else if line.starts_with("#|") {
                // Previous msgid: not needed.
            } else {
                let r = &line[1..];
                b.entry.comments.push(r.strip_prefix(' ').unwrap_or(r).to_string());
            }
            continue;
        }
        if line.starts_with('"') {
            let s = unquote(line).map_err(|m| err(n, &m))?;
            match field {
                Field::None => return Err(err(n, "text with no keyword before it")),
                Field::Ctx => b.entry.ctx.get_or_insert_with(String::new).push_str(&s),
                Field::Id => b.entry.id.push_str(&s),
                Field::Plural => b.entry.plural.get_or_insert_with(String::new).push_str(&s),
                Field::Str(i) => b.strs.iter_mut().find(|(k, _)| *k == i).expect("msgstr started").1.push_str(&s),
            }
            continue;
        }
        let (kw, rest) = line.split_once(|c: char| c.is_whitespace()).ok_or_else(|| err(n, "expected a keyword and a quoted string"))?;
        let s = unquote(rest.trim()).map_err(|m| err(n, &m))?;
        match kw {
            "msgctxt" => {
                if b.has_str || b.has_id { finish(&mut b, &mut out)?; }
                b.started = true;
                if b.entry.ctx.is_some() { return Err(err(n, "two msgctxt lines")) }
                b.entry.ctx = Some(s);
                field = Field::Ctx;
            }
            "msgid" => {
                if b.has_str { finish(&mut b, &mut out)?; }
                if b.has_id { return Err(err(n, "msgid without a msgstr before it")) }
                b.started = true;
                b.has_id = true;
                b.entry.id = s;
                b.entry.line = n;
                field = Field::Id;
            }
            "msgid_plural" => {
                if !b.has_id || b.has_str { return Err(err(n, "msgid_plural must follow msgid")) }
                b.entry.plural = Some(s);
                field = Field::Plural;
            }
            _ if kw.starts_with("msgstr") => {
                if !b.has_id { return Err(err(n, "msgstr without a msgid")) }
                let idx = if kw == "msgstr" {
                    if b.entry.plural.is_some() { return Err(err(n, "a plural entry needs msgstr[0], msgstr[1]…")) }
                    0
                } else {
                    let i = kw.strip_prefix("msgstr[").and_then(|r| r.strip_suffix(']')).and_then(|d| d.parse::<usize>().ok())
                        .ok_or_else(|| err(n, &format!("unknown keyword {kw}")))?;
                    if b.entry.plural.is_none() { return Err(err(n, "msgstr[n] without msgid_plural")) }
                    if i != b.strs.len() { return Err(err(n, "msgstr[n] out of order")) }
                    i
                };
                if b.strs.iter().any(|(k, _)| *k == idx) { return Err(err(n, "msgstr given twice")) }
                b.strs.push((idx, s));
                b.has_str = true;
                field = Field::Str(idx);
            }
            _ => return Err(err(n, &format!("unknown keyword {kw}"))),
        }
    }
    finish(&mut b, &mut out)?;
    Ok(out)
}

fn finish(b: &mut Building, out: &mut Vec<Entry>) -> Result<(), ParseError> {
    let done = std::mem::take(b);
    if !done.started { return Ok(()) }
    if !done.has_id {
        // Only comments (e.g. at the end of a file): nothing to keep.
        if done.entry.ctx.is_none() { return Ok(()) }
        return Err(ParseError { line: done.entry.line, message: "msgctxt without a msgid".into() });
    }
    if !done.has_str { return Err(ParseError { line: done.entry.line, message: "msgid without a msgstr".into() }) }
    let mut e = done.entry;
    e.strs = done.strs.into_iter().map(|(_, s)| s).collect();
    out.push(e);
    Ok(())
}

/// `"text"` → text, with `\" \\ \n \t \r` and friends undone.
fn unquote(s: &str) -> Result<String, String> {
    let inner = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')).filter(|_| s.len() >= 2).ok_or("expected a quoted string")?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('a') => out.push('\x07'),
                Some('b') => out.push('\x08'),
                Some('f') => out.push('\x0c'),
                Some('v') => out.push('\x0b'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(o) => return Err(format!("unknown escape \\{o}")),
                None => return Err("a string ends in a backslash".into()),
            },
            '"' => return Err("a quote inside a string must be written \\\"".into()),
            c => out.push(c),
        }
    }
    Ok(out)
}

/// `text` as a `.po` string, split after each `\n` the way gettext's tools write it.
pub fn quote(text: &str) -> String {
    let esc = |s: &str| {
        let mut o = String::with_capacity(s.len() + 2);
        o.push('"');
        for c in s.chars() {
            match c {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                '\n' => o.push_str("\\n"),
                '\t' => o.push_str("\\t"),
                '\r' => o.push_str("\\r"),
                c => o.push(c),
            }
        }
        o.push('"');
        o
    };
    let parts: Vec<&str> = text.split_inclusive('\n').collect();
    if parts.len() <= 1 { return esc(text) }
    let mut o = String::from("\"\"");
    for p in parts { o.push('\n'); o.push_str(&esc(p)); }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# Spanish for RustyPresenter.
msgid ""
msgstr ""
"Language: es\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"
"X-Native-Name: Español\n"

# A note from a translator
#. A note from the code
#: ui/operator.slint:12
msgid "Close"
msgstr "Cerrar"

msgctxt "verb"
msgid "Clear"
msgstr "Borrar"

msgid ""
"Two lines,\n"
"with a \"quote\" and a \\ and a\ttab"
msgstr "Dos líneas,\nlisto"

msgid "{n} item"
msgid_plural "{n} items"
msgstr[0] "{n} elemento"
msgstr[1] "{n} elementos"

#, fuzzy
msgid "Maybe"
msgstr "Quizás"

#~ msgid "Old"
#~ msgstr "Viejo"
"#;

    #[test]
    fn reads_header_entries_context_plurals_and_escapes() {
        let f = parse(SAMPLE).unwrap();
        assert_eq!(f.header("language"), Some("es"));
        assert_eq!(f.header("Plural-Forms"), Some("nplurals=2; plural=(n != 1);"));
        assert_eq!(f.header("X-Native-Name"), Some("Español"));
        assert_eq!(f.header_comments, ["Spanish for RustyPresenter."]);
        assert_eq!(f.entries.len(), 5);
        let close = &f.entries[0];
        assert_eq!((close.id.as_str(), close.strs.as_slice(), close.line), ("Close", ["Cerrar".to_string()].as_slice(), 11));
        assert_eq!((close.comments.as_slice(), close.extracted.as_slice(), close.refs.as_slice()),
            (["A note from a translator".to_string()].as_slice(), ["A note from the code".to_string()].as_slice(), ["ui/operator.slint:12".to_string()].as_slice()));
        assert_eq!(f.entries[1].ctx.as_deref(), Some("verb"));
        assert_eq!(f.entries[1].key(), ("verb".to_string(), "Clear".to_string()));
        assert_eq!(f.entries[2].id, "Two lines,\nwith a \"quote\" and a \\ and a\ttab");
        assert_eq!(f.entries[2].strs, ["Dos líneas,\nlisto"]);
        assert_eq!(f.entries[3].plural.as_deref(), Some("{n} items"));
        assert_eq!(f.entries[3].strs, ["{n} elemento", "{n} elementos"]);
        assert!(f.entries[4].fuzzy() && !f.entries[4].translated());
        assert!(f.entries[0].translated());
    }

    #[test]
    fn obsolete_entries_are_kept_apart() {
        let f = parse(SAMPLE).unwrap();
        assert!(f.entries.iter().all(|e| e.id != "Old"));
        assert_eq!(f.obsolete.len(), 1);
        assert_eq!((f.obsolete[0].id.as_str(), f.obsolete[0].strs[0].as_str()), ("Old", "Viejo"));
    }

    #[test]
    fn empty_and_header_only_files() {
        assert_eq!(parse("").unwrap(), PoFile::default());
        let f = parse("msgid \"\"\nmsgstr \"Language: es\\n\"\n").unwrap();
        assert_eq!((f.header("Language"), f.entries.len()), (Some("es"), 0));
    }

    #[test]
    fn errors_carry_the_line() {
        let e = parse("msgid \"a\"\nmsgstr \"b\"\n\nmsgid \"c\nmsgstr \"d\"\n").unwrap_err();
        assert_eq!(e.line, 4);
        let e = parse("msgid \"a\"\nmsgstr \"b\"\nmsgid \"c\"\nmsgid \"d\"\nmsgstr \"\"").unwrap_err();
        assert_eq!(e.line, 4, "{e}");
        let e = parse("msgid \"a\"\nmsgstr \"\\q\"").unwrap_err();
        assert_eq!(e.line, 2);
        let e = parse("msgid \"a\"\n\nmsgid \"b\"\nmsgstr \"\"").unwrap_err();
        assert_eq!(e.line, 3);
        let e = parse("\"stray\"").unwrap_err();
        assert_eq!(e.line, 1);
        let e = parse("msgid \"a\"\nmsgid_plural \"as\"\nmsgstr \"x\"").unwrap_err();
        assert_eq!(e.line, 3);
        assert!(e.to_string().starts_with("line 3: "));
    }

    #[test]
    fn quote_round_trips() {
        for s in ["plain", "a \"q\" \\ b", "two\nlines\n", "tab\there"] {
            let po = format!("msgid {}\nmsgstr \"\"\n", quote(s));
            assert_eq!(parse(&po).unwrap().entries[0].id, s, "{po}");
        }
        assert_eq!(quote("a\nb"), "\"\"\n\"a\\n\"\n\"b\"");
    }
}
