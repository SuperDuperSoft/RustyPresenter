//! Finds the text to translate and keeps `translations/` in step (spec D10). `cargo run --example i18n_extract`
//! writes the files; `tests/i18n_catalogs.rs` checks them.
//!
//! - `.slint`: every `@tr("text")`, `@tr("context" => "text")`, `@tr("one" | "many" % n)`, with or without args.
//! - Rust (`src/**/*.rs`): every `tr!`, `trn!` and `trc!`, outside comments and `#[cfg(test)]` items, and not in
//!   `tests.rs` / `*_tests.rs` files.

use super::po::{self, Entry, PoFile};
use std::{collections::HashMap, path::Path};

/// One text to translate.
#[derive(Clone, Debug, PartialEq)]
pub struct Msg {
    pub ctx: Option<String>,
    pub id: String,
    pub plural: Option<String>,
    /// The files that use it (no line numbers, so moving code does not change the template).
    pub files: Vec<String>,
}

impl Msg {
    fn new(ctx: Option<String>, id: String, plural: Option<String>, file: &str) -> Msg { Msg { ctx, id, plural, files: vec![file.to_string()] } }

    fn key(&self) -> (String, String) { (self.ctx.clone().unwrap_or_default(), self.id.clone()) }
}

/// Reads code character by character, knowing what is a comment and what is a string.
struct Scan<'a> { s: &'a str, at: usize }

impl<'a> Scan<'a> {
    fn rest(&self) -> &'a str { &self.s[self.at..] }

    fn line(&self) -> usize { self.s[..self.at].matches('\n').count() + 1 }

    fn bump(&mut self) { self.at += self.rest().chars().next().map_or(0, char::len_utf8) }

    /// Skips spaces and comments.
    fn space(&mut self) {
        loop {
            let r = self.rest();
            let t = r.trim_start();
            self.at += r.len() - t.len();
            if t.starts_with("//") { self.at += t.find('\n').unwrap_or(t.len()) }
            else if t.starts_with("/*") { self.block_comment() }
            else { return }
        }
    }

    /// At `/*`: skips to its end (nested, as Rust allows).
    fn block_comment(&mut self) {
        let mut depth = 0;
        while !self.rest().is_empty() {
            if self.rest().starts_with("/*") { depth += 1; self.at += 2 }
            else if self.rest().starts_with("*/") { depth -= 1; self.at += 2; if depth == 0 { return } }
            else { self.bump() }
        }
    }

    fn eat(&mut self, tok: &str) -> bool {
        self.space();
        if self.rest().starts_with(tok) { self.at += tok.len(); true } else { false }
    }

    /// A Slint string literal at the cursor (after spaces).
    fn slint_string(&mut self) -> Result<String, String> {
        self.space();
        if !self.rest().starts_with('"') { return Err(format!("line {}: @tr needs a plain string", self.line())) }
        self.at += 1;
        let mut out = String::new();
        loop {
            let Some(c) = self.rest().chars().next() else { return Err("a string never ends".into()) };
            self.bump();
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(e) = self.rest().chars().next() else { return Err("a string never ends".into()) };
                    self.bump();
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        'u' => out.push(self.unicode()?),
                        '{' => return Err(format!("line {}: @tr text cannot hold \\{{…}}; use {{}} and an argument", self.line())),
                        o => return Err(format!("line {}: unknown escape \\{o}", self.line())),
                    }
                }
                c => out.push(c),
            }
        }
    }

    /// After `\u`: `{XXXX}`.
    fn unicode(&mut self) -> Result<char, String> {
        let r = self.rest();
        let end = r.find('}').filter(|_| r.starts_with('{')).ok_or("bad \\u escape")?;
        let c = u32::from_str_radix(&r[1..end], 16).ok().and_then(char::from_u32).ok_or("bad \\u escape")?;
        self.at += end + 1;
        Ok(c)
    }

    /// A Rust string literal at the cursor (after spaces): `"…"` or `r"…"`, `r#"…"#`.
    fn rust_string(&mut self) -> Result<String, String> {
        self.space();
        let r = self.rest();
        if let Some(raw) = r.strip_prefix('r') {
            let hashes = raw.len() - raw.trim_start_matches('#').len();
            let body = &raw[hashes..];
            if !body.starts_with('"') { return Err(format!("line {}: expected a string literal", self.line())) }
            let close = format!("\"{}", "#".repeat(hashes));
            let end = body[1..].find(&close).ok_or("a raw string never ends")?;
            let text = body[1..1 + end].to_string();
            self.at += 1 + hashes + 1 + end + close.len();
            return Ok(text);
        }
        if !r.starts_with('"') { return Err(format!("line {}: expected a string literal", self.line())) }
        self.at += 1;
        let mut out = String::new();
        loop {
            let Some(c) = self.rest().chars().next() else { return Err("a string never ends".into()) };
            self.bump();
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(e) = self.rest().chars().next() else { return Err("a string never ends".into()) };
                    self.bump();
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        '0' => out.push('\0'),
                        '"' => out.push('"'),
                        '\'' => out.push('\''),
                        '\\' => out.push('\\'),
                        'u' => out.push(self.unicode()?),
                        'x' => {
                            let hex = self.rest().get(..2).ok_or("bad \\x escape")?;
                            out.push(u8::from_str_radix(hex, 16).map_err(|_| "bad \\x escape")? as char);
                            self.at += 2;
                        }
                        // A backslash at the end of a line joins it to the next, without the spaces.
                        '\n' | '\r' => { let r = self.rest(); self.at += r.len() - r.trim_start().len(); }
                        o => return Err(format!("line {}: unknown escape \\{o}", self.line())),
                    }
                }
                c => out.push(c),
            }
        }
    }

    /// At a quote: skips a plain string (`"…"`, with escapes).
    fn skip_string(&mut self) {
        self.at += 1;
        while let Some(c) = self.rest().chars().next() {
            self.bump();
            if c == '\\' { self.bump() } else if c == '"' { return }
        }
    }

    /// At a Rust `'`: skips a char literal, or just the quote of a lifetime.
    fn skip_char_or_lifetime(&mut self) {
        let r = self.rest();
        let lit = if r[1..].starts_with('\\') { r.get(3..).and_then(|t| t.find('\'')).map(|i| i + 4) } else {
            r[1..].chars().next().filter(|_| r[1..].chars().nth(1) == Some('\'')).map(|c| 1 + c.len_utf8() + 1)
        };
        self.at += lit.unwrap_or(1);
    }

    /// At a Rust raw string's `r`: skips it, if it is one.
    fn skip_raw(&mut self) -> bool {
        let r = self.rest();
        let Some(raw) = r.strip_prefix('r') else { return false };
        let hashes = raw.len() - raw.trim_start_matches('#').len();
        if !raw[hashes..].starts_with('"') { return false }
        let close = format!("\"{}", "#".repeat(hashes));
        let end = raw[hashes + 1..].find(&close).map_or(r.len(), |e| 1 + hashes + 1 + e + close.len());
        self.at += end;
        true
    }
}

/// The `@tr(…)` texts of one `.slint` file.
pub fn from_slint(src: &str, file: &str) -> Result<Vec<Msg>, String> {
    let mut sc = Scan { s: src, at: 0 };
    let mut out = Vec::new();
    while !sc.rest().is_empty() {
        let r = sc.rest();
        if r.starts_with("//") || r.starts_with("/*") { sc.space(); continue }
        if r.starts_with('"') { sc.skip_string(); continue }
        if r.starts_with("@tr") && sc.rest()[3..].trim_start().starts_with('(') {
            sc.at += 3;
            sc.eat("(");
            let line = sc.line();
            let first = sc.slint_string().map_err(|e| format!("{file}: {e}"))?;
            let (ctx, id) = if sc.eat("=>") { (Some(first), sc.slint_string().map_err(|e| format!("{file}: {e}"))?) } else { (None, first) };
            let plural = if sc.eat("|") { Some(sc.slint_string().map_err(|e| format!("{file}: {e}"))?) } else { None };
            if id.is_empty() { return Err(format!("{file}:{line}: @tr with empty text")) }
            out.push(Msg::new(ctx, id, plural, file));
            continue;
        }
        sc.bump();
    }
    Ok(out)
}

/// The `tr!`, `trn!` and `trc!` texts of one Rust file.
pub fn from_rust(src: &str, file: &str) -> Result<Vec<Msg>, String> {
    let mut sc = Scan { s: src, at: 0 };
    let mut out = Vec::new();
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    while !sc.rest().is_empty() {
        let r = sc.rest();
        let prev = sc.s[..sc.at].chars().next_back();
        if r.starts_with("//") || r.starts_with("/*") { sc.space(); continue }
        if r.starts_with('"') { sc.skip_string(); continue }
        if r.starts_with('\'') { sc.skip_char_or_lifetime(); continue }
        if r.starts_with("br") && !prev.is_some_and(ident) { sc.at += 1; if sc.skip_raw() { continue } }
        if r.starts_with('r') && !prev.is_some_and(ident) && sc.skip_raw() { continue }
        if r.starts_with("#[cfg(test)]") { sc.at += "#[cfg(test)]".len(); skip_item(&mut sc); continue }
        if !prev.is_some_and(ident) {
            let kind = ["trn!", "trc!", "tr!"].into_iter().find(|k| r.starts_with(k) && r[k.len()..].trim_start().starts_with(['(', '[', '{']));
            if let Some(kind) = kind {
                sc.at += kind.len();
                sc.space();
                sc.at += 1;
                let line = sc.line();
                let err = |e: String| format!("{file}:{line}: {kind} needs string literals ({e})");
                let a = sc.rust_string().map_err(err)?;
                let msg = match kind {
                    "tr!" => Msg::new(None, a, None, file),
                    _ => {
                        if !sc.eat(",") { return Err(err("expected a comma".into())) }
                        let b = sc.rust_string().map_err(err)?;
                        if kind == "trn!" { Msg::new(None, a, Some(b), file) } else { Msg::new(Some(a), b, None, file) }
                    }
                };
                if msg.id.is_empty() { return Err(format!("{file}:{line}: {kind} with empty text")) }
                out.push(msg);
                continue;
            }
        }
        sc.bump();
    }
    Ok(out)
}

/// After `#[cfg(test)]`: skips the item it marks (to its `;`, or its `{ … }`).
fn skip_item(sc: &mut Scan) {
    let mut depth = 0usize;
    while !sc.rest().is_empty() {
        let r = sc.rest();
        if r.starts_with("//") || r.starts_with("/*") { sc.space(); continue }
        if r.starts_with('"') { sc.skip_string(); continue }
        if r.starts_with('\'') { sc.skip_char_or_lifetime(); continue }
        if r.starts_with('r') && sc.skip_raw() { continue }
        let c = r.chars().next().unwrap();
        sc.bump();
        match c {
            '{' => depth += 1,
            '}' => { depth = depth.saturating_sub(1); if depth == 0 { return } }
            ';' if depth == 0 => return,
            _ => {}
        }
    }
}

/// Every text in `root`'s `ui/**/*.slint` and `src/**/*.rs`, files in name order, each text once (its files listed).
pub fn collect(root: &Path) -> Result<Vec<Msg>, String> {
    let mut found = Vec::new();
    for (dir, ext) in [("ui", "slint"), ("src", "rs")] {
        let mut files = Vec::new();
        walk(&root.join(dir), ext, &mut files).map_err(|e| format!("{dir}: {e}"))?;
        files.sort();
        for f in files {
            let rel = f.strip_prefix(root).unwrap_or(&f).to_string_lossy().replace('\\', "/");
            let name = f.file_name().unwrap_or_default().to_string_lossy();
            if ext == "rs" && (name == "tests.rs" || name.ends_with("_tests.rs")) { continue }
            let src = std::fs::read_to_string(&f).map_err(|e| format!("{rel}: {e}"))?;
            found.extend(if ext == "slint" { from_slint(&src, &rel)? } else { from_rust(&src, &rel)? });
        }
    }
    Ok(unique(found))
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() { walk(&p, ext, out)? } else if p.extension().is_some_and(|x| x == ext) { out.push(p) }
    }
    Ok(())
}

/// One `Msg` per `(context, text)`, in first-seen order, with all its files.
pub fn unique(msgs: Vec<Msg>) -> Vec<Msg> {
    let mut out: Vec<Msg> = Vec::new();
    let mut at: HashMap<(String, String), usize> = HashMap::new();
    for m in msgs {
        match at.get(&m.key()) {
            Some(&i) => {
                let o = &mut out[i];
                if o.plural.is_none() { o.plural = m.plural }
                for f in m.files { if !o.files.contains(&f) { o.files.push(f) } }
            }
            None => { at.insert(m.key(), out.len()); out.push(m) }
        }
    }
    out
}

/// The template's header.
const POT_HEADER: &[(&str, &str)] = &[
    ("Project-Id-Version", "RustyPresenter"),
    ("MIME-Version", "1.0"),
    ("Content-Type", "text/plain; charset=UTF-8"),
    ("Content-Transfer-Encoding", "8bit"),
    ("Language", ""),
    ("Plural-Forms", "nplurals=2; plural=(n != 1);"),
];

/// The template (`translations/rustypresenter.pot`) for `msgs`.
pub fn pot(msgs: &[Msg]) -> String {
    let file = PoFile {
        header: POT_HEADER.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        header_comments: vec!["The words of RustyPresenter, for translators. Made by `cargo run --example i18n_extract`; do not edit by hand.".into()],
        entries: msgs.iter().map(|m| Entry {
            ctx: m.ctx.clone(), id: m.id.clone(), plural: m.plural.clone(),
            strs: vec![String::new(); if m.plural.is_some() { 2 } else { 1 }],
            refs: vec![m.files.join(" ")],
            ..Entry::default()
        }).collect(),
        obsolete: Vec::new(),
    };
    write(&file)
}

/// Brings a language's `.po` in step with the template: translations kept, new texts added untranslated, texts no
/// longer used kept as obsolete (`#~`) so their translation is not lost. The header is kept.
pub fn merge(po_text: &str, pot_text: &str) -> Result<String, po::ParseError> {
    let old = po::parse(po_text)?;
    let template = po::parse(pot_text)?;
    let nplurals = super::plural::Plurals::from_header(old.header("Plural-Forms")).count;
    let mut have: HashMap<(String, String), Entry> = HashMap::new();
    for e in old.obsolete.iter().chain(&old.entries) { have.insert(e.key(), e.clone()); }
    let mut entries = Vec::new();
    for t in &template.entries {
        let forms = if t.plural.is_some() { nplurals } else { 1 };
        let mut e = match have.remove(&t.key()) {
            Some(mut e) => {
                if e.plural.is_some() != t.plural.is_some() { e.strs = vec![String::new(); forms]; e.flags.retain(|f| f != "fuzzy"); }
                e
            }
            None => Entry { strs: vec![String::new(); forms], ..Entry::default() },
        };
        e.ctx = t.ctx.clone();
        e.id = t.id.clone();
        e.plural = t.plural.clone();
        e.refs = t.refs.clone();
        e.extracted = t.extracted.clone();
        if e.strs.len() != forms { e.strs.resize(forms, String::new()) }
        entries.push(e);
    }
    let in_template: std::collections::HashSet<_> = template.entries.iter().map(Entry::key).collect();
    let mut obsolete: Vec<Entry> = old.entries.iter().chain(&old.obsolete)
        .filter(|e| !in_template.contains(&e.key()) && e.strs.iter().any(|s| !s.is_empty()))
        .cloned().collect();
    let mut seen = std::collections::HashSet::new();
    obsolete.retain(|e| seen.insert(e.key()));
    for e in &mut obsolete { e.refs.clear() }
    Ok(write(&PoFile { header: old.header, header_comments: old.header_comments, entries, obsolete }))
}

/// A `.po` file's text, the way gettext's tools lay it out.
pub fn write(file: &PoFile) -> String {
    let mut out = String::new();
    for c in &file.header_comments { out.push_str(&format!("# {c}\n").replace("# \n", "#\n")) }
    out.push_str("msgid \"\"\nmsgstr \"\"\n");
    for (k, v) in &file.header {
        out.push_str(&po::quote(&format!("{k}: {v}\n")).replace("\"\"\n", ""));
        out.push('\n');
    }
    for e in &file.entries { out.push('\n'); out.push_str(&entry_text(e)); }
    for e in &file.obsolete {
        out.push('\n');
        for l in entry_text(e).lines() {
            out.push_str("#~ ");
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

fn entry_text(e: &Entry) -> String {
    let mut o = String::new();
    for c in &e.comments { o.push_str(if c.is_empty() { "#\n".to_string() } else { format!("# {c}\n") }.as_str()) }
    for c in &e.extracted { o.push_str(&format!("#. {c}\n")) }
    for r in &e.refs { o.push_str(&format!("#: {r}\n")) }
    if !e.flags.is_empty() { o.push_str(&format!("#, {}\n", e.flags.join(", "))) }
    if let Some(c) = &e.ctx { o.push_str(&format!("msgctxt {}\n", po::quote(c))) }
    o.push_str(&format!("msgid {}\n", po::quote(&e.id)));
    match &e.plural {
        Some(p) => {
            o.push_str(&format!("msgid_plural {}\n", po::quote(p)));
            for (i, s) in e.strs.iter().enumerate() { o.push_str(&format!("msgstr[{i}] {}\n", po::quote(s))) }
        }
        None => o.push_str(&format!("msgstr {}\n", po::quote(e.strs.first().map_or("", String::as_str)))),
    }
    o
}

/// A user-visible property set to a plain string with letters in a `.slint` file (should be `@tr`).
#[derive(Clone, Debug, PartialEq)]
pub struct BareText { pub line: usize, pub property: String, pub text: String }

/// The properties whose text people read.
pub const USER_TEXT_PROPERTIES: &[&str] = &[
    "text", "title", "label", "tooltip", "tip", "help", "note", "hint", "message", "caption", "placeholder",
    "placeholder-text", "accessible-label", "accessible-description",
];

/// Every bare user text in one `.slint` file: a string with letters in a `USER_TEXT_PROPERTIES` value, not inside
/// `@tr(…)` and not compared with `==` / `!=`. `\{…}` parts do not count as letters.
pub fn bare_texts(src: &str) -> Vec<BareText> {
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let code = line.split("//").next().unwrap_or_default();
        for prop in USER_TEXT_PROPERTIES {
            let mut from = 0;
            while let Some(p) = code[from..].find(prop) {
                let start = from + p;
                from = start + prop.len();
                let before = code[..start].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') { continue }
                let after = code[from..].trim_start();
                let Some(value) = after.strip_prefix(':').filter(|v| !v.starts_with(':') && !v.starts_with('=')) else { continue };
                let value = value.split(';').next().unwrap_or_default();
                for text in bare_strings(value) {
                    out.push(BareText { line: i + 1, property: prop.to_string(), text });
                }
            }
        }
    }
    out
}

/// The strings with letters in an expression that are not in `@tr(…)` and not compared.
fn bare_strings(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = expr.as_bytes();
    let mut i = 0;
    let mut tr_depth: Option<i32> = None;
    let mut depth = 0;
    while i < b.len() {
        match b[i] {
            b'@' if expr[i..].starts_with("@tr(") => { tr_depth = Some(depth); depth += 1; i += 4; continue }
            b'(' => depth += 1,
            b')' => { depth -= 1; if tr_depth == Some(depth) { tr_depth = None } }
            b'"' => {
                let start = i;
                i += 1;
                let mut text = String::new();
                let mut interp = 0;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' && b.get(i + 1) == Some(&b'{') { interp += 1; i += 2; continue }
                    if interp > 0 { if b[i] == b'}' { interp -= 1 } i += 1; continue }
                    if b[i] == b'\\' { i += 2; continue }
                    let c = expr[i..].chars().next().unwrap();
                    text.push(c);
                    i += c.len_utf8();
                }
                let raw = expr[start + 1..i.min(expr.len())].to_string();
                i += 1;
                let around = (expr[..start].trim_end(), expr[i.min(expr.len())..].trim_start());
                let compared = around.0.ends_with("==") || around.0.ends_with("!=") || around.1.starts_with("==") || around.1.starts_with("!=");
                if tr_depth.is_none() && !compared && text.chars().any(char::is_alphabetic) { out.push(raw) }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// What `translations/allow-bare.txt` allows: texts (one per line, exactly as in the `.slint`) and whole files
/// (`file: ui/gallery.slint`). `#` starts a comment line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Allow { pub texts: Vec<String>, pub files: Vec<String> }

impl Allow {
    pub fn parse(text: &str) -> Allow {
        let mut a = Allow::default();
        for l in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            match l.strip_prefix("file:") { Some(f) => a.files.push(f.trim().to_string()), None => a.texts.push(l.to_string()) }
        }
        a
    }

    pub fn allows(&self, file: &str, text: &str) -> bool { self.files.iter().any(|f| f == file) || self.texts.iter().any(|t| t == text) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(m: &[Msg]) -> Vec<(Option<&str>, &str, Option<&str>)> { m.iter().map(|m| (m.ctx.as_deref(), m.id.as_str(), m.plural.as_deref())).collect() }

    #[test]
    fn slint_forms() {
        let src = r#"
            // @tr("in a comment")
            /* @tr("in a block") */
            Text { text: @tr("Plain"); }
            Text { text: @tr( "Verses: {}", chip.value); }
            Text { text: @tr("verb" => "Clear"); }
            Text { text: @tr("{n} item" | "{n} items" % count); }
            Text { text: @tr("ctx" => "{} file" | "{} files" % n, n); }
            Text { text: "@tr(\"in a string\")"; }
            Text { text: @tr("Two\nlines \"quoted\" \u{2014}"); }
        "#;
        let m = from_slint(src, "ui/x.slint").unwrap();
        assert_eq!(ids(&m), [
            (None, "Plain", None),
            (None, "Verses: {}", None),
            (Some("verb"), "Clear", None),
            (None, "{n} item", Some("{n} items")),
            (Some("ctx"), "{} file", Some("{} files")),
            (None, "Two\nlines \"quoted\" —", None),
        ]);
        assert_eq!(m[0].files, ["ui/x.slint"]);
        assert!(from_slint(r#"text: @tr(name);"#, "ui/x.slint").is_err());
    }

    #[test]
    fn rust_forms() {
        let src = r####"
            // tr!("comment")
            /* trn!("a", "b", 1) */
            let a = tr!("Plain");
            let b = tr!("Delete “{name}”?", name = t);
            let c = trn!("{n} song", "{n} songs", n);
            let d = trc!("verb", "Clear");
            let e = crate::tr!(r#"Raw "quoted""#);
            let f = tr!("Esc \"q\" \\ \n\
                         joined");
            let g = "tr!(\"in a string\")";
            let h = r#"tr!("in a raw")"#;
            let i = 'x'; let j: &'static str = "";
            let k = mytr!("not ours");
            #[cfg(test)]
            mod tests { fn t() { tr!("test only"); let s = "}"; } }
            let l = tr!("After the tests");
        "####;
        let m = from_rust(src, "src/x.rs").unwrap();
        assert_eq!(ids(&m), [
            (None, "Plain", None),
            (None, "Delete “{name}”?", None),
            (None, "{n} song", Some("{n} songs")),
            (Some("verb"), "Clear", None),
            (None, "Raw \"quoted\"", None),
            (None, "Esc \"q\" \\ \njoined", None),
            (None, "After the tests", None),
        ]);
        assert!(from_rust("tr!(NAME)", "src/x.rs").is_err());
    }

    #[test]
    fn duplicates_merge_with_their_files() {
        let mut m = from_slint(r#"a: @tr("Close"); b: @tr("Close");"#, "ui/a.slint").unwrap();
        m.extend(from_rust(r#"tr!("Close"); trc!("x", "Close");"#, "src/b.rs").unwrap());
        let u = unique(m);
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].files, ["ui/a.slint", "src/b.rs"]);
    }

    #[test]
    fn pot_parses_back() {
        let msgs = unique(from_slint(r#"@tr("Close") @tr("{n} item" | "{n} items" % n) @tr("c" => "Two\nlines")"#, "ui/a.slint").unwrap());
        let text = pot(&msgs);
        let f = po::parse(&text).unwrap();
        assert_eq!(f.header("Plural-Forms"), Some("nplurals=2; plural=(n != 1);"));
        assert_eq!(f.entries.len(), 3);
        assert_eq!((f.entries[1].plural.as_deref(), f.entries[1].strs.len()), (Some("{n} items"), 2));
        assert_eq!((f.entries[2].ctx.as_deref(), f.entries[2].id.as_str()), (Some("c"), "Two\nlines"));
        assert_eq!(f.entries[0].refs, ["ui/a.slint"]);
    }

    #[test]
    fn merge_keeps_adds_and_retires() {
        let pot1 = pot(&unique(from_slint(r#"@tr("Close") @tr("Old one") @tr("{n} item" | "{n} items" % n)"#, "ui/a.slint").unwrap()));
        let es = "# Spanish.\nmsgid \"\"\nmsgstr \"\"\n\"Language: es\\n\"\n\"Plural-Forms: nplurals=3; plural=(n==1 ? 0 : n==2 ? 1 : 2);\\n\"\n\n# keep me\nmsgid \"Close\"\nmsgstr \"Cerrar\"\n\nmsgid \"Old one\"\nmsgstr \"Viejo\"\n";
        let merged1 = merge(es, &pot1).unwrap();
        let pot2 = pot(&unique(from_slint(r#"@tr("Close") @tr("New one") @tr("{n} item" | "{n} items" % n)"#, "ui/a.slint").unwrap()));
        let merged = merge(&merged1, &pot2).unwrap();
        let f = po::parse(&merged).unwrap();
        assert_eq!(f.header("Language"), Some("es"));
        assert_eq!(f.header_comments, ["Spanish."]);
        let get = |id: &str| f.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!(get("Close").strs, ["Cerrar"]);
        assert_eq!(get("Close").comments, ["keep me"]);
        assert_eq!(get("New one").strs, [""]);
        assert_eq!(get("{n} item").strs.len(), 3, "as many forms as the language has");
        assert!(f.entries.iter().all(|e| e.id != "Old one"));
        assert_eq!((f.obsolete[0].id.as_str(), f.obsolete[0].strs[0].as_str()), ("Old one", "Viejo"));
        // It comes back with its translation if the text is used again.
        let back = po::parse(&merge(&merged, &pot1).unwrap()).unwrap();
        assert_eq!(back.entries.iter().find(|e| e.id == "Old one").unwrap().strs, ["Viejo"]);
        assert!(back.obsolete.is_empty());
        // Merging twice changes nothing.
        assert_eq!(merge(&merged, &pot2).unwrap(), merged);
    }

    #[test]
    fn bare_text_found_and_allowed() {
        let src = r#"
            Text { text: "Hello"; }
            Text { text: @tr("Fine"); }
            Btn { tooltip: cond ? @tr("A") : "Bare B"; }
            Text { text: root.tab == "all" ? @tr("All") : @tr("Some"); }
            Text { text: "\{root.percent}%"; }
            Text { text: "—"; }
            Icon { name: "play"; }
            Text { font-family: "Segoe UI"; }
            Text { text: @tr("Verses: {}", "x" + "y"); }
            // text: "in a comment";
            Pill { label: "Room on"; accessible-label: "Room"; }
        "#;
        let found = bare_texts(src);
        let found: Vec<(usize, &str, &str)> = found.iter().map(|b| (b.line, b.property.as_str(), b.text.as_str())).collect();
        assert_eq!(found, [(2, "text", "Hello"), (4, "tooltip", "Bare B"), (12, "label", "Room on"), (12, "accessible-label", "Room")]);
        let allow = Allow::parse("# symbols\nHello\n\nfile: ui/gallery.slint\n");
        assert!(allow.allows("ui/x.slint", "Hello"));
        assert!(!allow.allows("ui/x.slint", "Bare B"));
        assert!(allow.allows("ui/gallery.slint", "Bare B"));
    }
}
