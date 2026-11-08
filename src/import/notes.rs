//! Speaker notes from a presentation file, read straight from the zip. One entry per slide the export makes:
//! PowerPoint and LibreOffice leave hidden slides out of the pictures, so their notes are left out here too.
//! Works for PowerPoint's own files and for Google Slides' "Download as PowerPoint", which writes the notes a
//! little differently (no `type="body"`, other part names).
use super::sniff::{read_part, Kind, MAX_PART};
use roxmltree::{Document, Node};
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read, Seek};
use std::path::Path;

/// All parts together inflate at most this much (zip-bomb guard).
const BUDGET: u64 = 4 * MAX_PART;
/// The notes slide's placeholders that are not the notes.
const NOT_NOTES: &[&str] = &["sldImg", "sldNum", "hdr", "ftr", "dt"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pptx,
    Odp,
}

/// Which reader a file needs; `None` for files whose notes can't be read (PDF, `.ppt`, Keynote).
pub fn format(kind: &Kind) -> Option<Format> {
    match kind {
        Kind::Ooxml { .. } => Some(Format::Pptx),
        Kind::Odp => Some(Format::Odp),
        _ => None,
    }
}

/// The notes of each shown slide, in order. `None` when the file could not be read.
pub fn read(path: &Path, format: Format) -> Option<Vec<String>> {
    let f = BufReader::new(std::fs::File::open(path).ok()?);
    match format {
        Format::Pptx => pptx(f),
        Format::Odp => odp(f),
    }
}

/// The notes to keep for a deck of `slides` pictures: none unless there is one entry per picture (a different
/// count means the notes would land on the wrong slides) and at least one slide has notes.
pub fn line_up(notes: Option<Vec<String>>, slides: usize) -> Vec<String> {
    match notes {
        Some(n) if n.len() == slides && n.iter().any(|s| !s.is_empty()) => n,
        Some(n) if n.len() != slides => {
            eprintln!("Speaker notes left out: {} notes for {slides} slides", n.len());
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// A zip with its entries found by lower-case name (OPC part names ignore case).
pub(super) struct Package<R> {
    zip: zip::ZipArchive<R>,
    names: HashMap<String, String>,
    budget: u64,
}

impl<R: Read + Seek> Package<R> {
    pub(super) fn open(r: R) -> Option<Self> {
        let zip = zip::ZipArchive::new(r).ok()?;
        let names = zip.file_names().flatten().map(|n| (n.to_ascii_lowercase(), n.into_owned())).collect();
        Some(Package { zip, names, budget: BUDGET })
    }

    pub(super) fn part(&mut self, name: &str) -> Option<String> {
        let entry = self.names.get(&name.to_ascii_lowercase())?.clone();
        let bytes = read_part(&mut self.zip, &entry, MAX_PART.min(self.budget))?;
        self.budget -= bytes.len() as u64;
        let text = String::from_utf8(bytes).ok()?;
        Some(text.trim_start_matches('\u{feff}').to_string())
    }

    /// `Id → (type, part name)` of a `.rels` part. Targets are relative to `base` (the source part's folder).
    fn rels(&mut self, name: &str, base: &str) -> HashMap<String, (String, String)> {
        let Some(xml) = self.part(name) else { return HashMap::new() };
        let Ok(doc) = Document::parse(&xml) else { return HashMap::new() };
        doc.descendants()
            .filter(|n| is(n, "Relationship") && attr(n, "TargetMode") != Some("External"))
            .filter_map(|n| Some((attr(&n, "Id")?.to_string(), (attr(&n, "Type").unwrap_or("").to_string(), resolve(base, attr(&n, "Target")?)))))
            .collect()
    }
}

/// A relationship target as a part name: absolute (`/ppt/…`) or relative to `base`, with `..` worked out.
fn resolve(base: &str, target: &str) -> String {
    let mut parts: Vec<&str> = match target.strip_prefix('/') {
        Some(_) => Vec::new(),
        None => base.split('/').filter(|s| !s.is_empty()).collect(),
    };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => { parts.pop(); }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// The folder and the file name of a part name.
fn split(part: &str) -> (&str, &str) {
    part.rsplit_once('/').unwrap_or(("", part))
}

pub(super) fn is(n: &Node, local: &str) -> bool {
    n.is_element() && n.tag_name().name() == local
}

/// An attribute by local name, whatever its namespace.
pub(super) fn attr<'a>(n: &Node<'a, '_>, local: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == local).map(|a| a.value())
}

pub fn pptx<R: Read + Seek>(r: R) -> Option<Vec<String>> {
    let mut p = Package::open(r)?;
    let mut out = Vec::new();
    for (slide, _) in pptx_slides(&mut p)? {
        let (dir, file) = split(&slide);
        let notes = p
            .rels(&format!("{dir}/_rels/{file}.rels"), dir)
            .into_values()
            .find(|(ty, _)| ty.ends_with("/notesSlide"))
            .and_then(|(_, target)| p.part(&target))
            .and_then(|xml| Document::parse(&xml).ok().map(|d| notes_text(&d)))
            .unwrap_or_default();
        out.push(notes);
    }
    Some(out)
}

/// The part name and XML of each slide that makes a picture, in show order (hidden ones left out). A slide that is
/// listed but missing still makes a picture, with no XML.
pub(super) fn pptx_slides<R: Read + Seek>(p: &mut Package<R>) -> Option<Vec<(String, Option<String>)>> {
    let xml = p.part("ppt/presentation.xml")?;
    let pres = Document::parse(&xml).ok()?;
    // `r:id` of each `<p:sldId>` (which also has a plain `id`), in show order.
    let ids: Vec<String> = pres
        .descendants()
        .filter(|n| is(n, "sldId") && n.parent().is_some_and(|l| is(&l, "sldIdLst")))
        .filter_map(|n| n.attributes().find(|a| a.name() == "id" && a.namespace().is_some()).map(|a| a.value().to_string()))
        .collect();
    let rels = p.rels("ppt/_rels/presentation.xml.rels", "ppt");
    let mut out = Vec::new();
    for slide in ids.iter().filter_map(|id| rels.get(id)).map(|(_, target)| target.clone()) {
        let xml = p.part(&slide);
        if let Some(xml) = &xml {
            let doc = Document::parse(xml).ok()?;
            if attr(&doc.root_element(), "show").is_some_and(|v| matches!(v.trim(), "0" | "false")) {
                continue;
            }
        }
        out.push((slide, xml));
    }
    Some(out)
}

/// The notes on a notes slide: the body placeholder's text, else the longest text of the other shapes that are
/// not the slide picture, the slide number, a header, footer or date (Google Slides marks the body differently).
fn notes_text(doc: &Document) -> String {
    let (mut body, mut best) = (None::<String>, String::new());
    for sp in doc.descendants().filter(|n| is(n, "sp")) {
        let ty = sp.descendants().find(|n| is(n, "ph")).and_then(|ph| attr(&ph, "type"));
        if ty.is_some_and(|t| NOT_NOTES.contains(&t)) {
            continue;
        }
        let Some(tx) = sp.children().find(|n| is(n, "txBody")) else { continue };
        let text = drawing_text(tx);
        if ty == Some("body") && body.is_none() && !text.is_empty() {
            body = Some(text);
        } else if text.chars().count() > best.chars().count() {
            best = text;
        }
    }
    body.unwrap_or(best)
}

/// DrawingML text: paragraphs as lines, runs joined, `<a:br/>` as a line break.
fn drawing_text(tx: Node) -> String {
    let lines: Vec<String> = tx
        .children()
        .filter(|n| is(n, "p"))
        .map(|p| {
            let mut line = String::new();
            for n in p.descendants() {
                if is(&n, "t") {
                    line.push_str(n.text().unwrap_or(""));
                } else if is(&n, "br") {
                    line.push('\n');
                }
            }
            // PowerPoint writes a soft line break inside a run as a vertical tab.
            line.replace('\u{b}', "\n").trim_end().to_string()
        })
        .collect();
    lines.join("\n").trim().to_string()
}

pub fn odp<R: Read + Seek>(r: R) -> Option<Vec<String>> {
    let mut p = Package::open(r)?;
    let xml = p.part("content.xml")?;
    let doc = Document::parse(&xml).ok()?;
    let out = odp_pages(&doc)
        .into_iter()
        .map(|page| page.children().find(|n| is(n, "notes")).map(|n| odp_notes(n)).unwrap_or_default())
        .collect();
    Some(out)
}

/// The pages of a LibreOffice presentation that make a picture, in order. Hidden slides use a page style whose
/// properties say `presentation:visibility="hidden"`.
pub(super) fn odp_pages<'a, 'i>(doc: &'a Document<'i>) -> Vec<Node<'a, 'i>> {
    let hidden: HashSet<&str> = doc
        .descendants()
        .filter(|n| is(n, "style") && n.children().any(|c| is(&c, "drawing-page-properties") && attr(&c, "visibility") == Some("hidden")))
        .filter_map(|n| attr(&n, "name"))
        .collect();
    doc.descendants()
        .filter(|n| is(n, "page") && n.parent().is_some_and(|p| is(&p, "presentation")))
        .filter(|page| !attr(page, "style-name").is_some_and(|s| hidden.contains(s)))
        .collect()
}

/// The `presentation:class="notes"` frame's text, else the longest frame that is not the slide picture.
fn odp_notes(notes: Node) -> String {
    let (mut found, mut best) = (None::<String>, String::new());
    for frame in notes.descendants().filter(|n| is(n, "frame")) {
        let class = attr(&frame, "class");
        if class == Some("page") {
            continue;
        }
        let mut lines = Vec::new();
        odf_lines(frame, &mut lines);
        let text = lines.join("\n").trim().to_string();
        if class == Some("notes") && found.is_none() && !text.is_empty() {
            found = Some(text);
        } else if text.chars().count() > best.chars().count() {
            best = text;
        }
    }
    found.unwrap_or(best)
}

fn odf_lines(n: Node, out: &mut Vec<String>) {
    for c in n.children() {
        if is(&c, "p") || is(&c, "h") {
            let mut s = String::new();
            odf_inline(c, &mut s);
            out.push(s.trim_end().to_string());
        } else if c.is_element() {
            odf_lines(c, out);
        }
    }
}

fn odf_inline(n: Node, s: &mut String) {
    for c in n.children() {
        if c.is_text() {
            s.push_str(c.text().unwrap_or(""));
        } else if is(&c, "s") {
            let count = attr(&c, "c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).min(100);
            s.extend(std::iter::repeat_n(' ', count));
        } else if is(&c, "tab") {
            s.push('\t');
        } else if is(&c, "line-break") {
            s.push('\n');
        } else if c.is_element() && !is(&c, "note") && !is(&c, "annotation") {
            odf_inline(c, s);
        }
    }
}

#[cfg(test)]
#[path = "notes_tests.rs"]
pub(crate) mod tests;
