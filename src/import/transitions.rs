//! Slide transitions from a presentation file, read straight from the zip, one entry per shown slide (the same
//! slides `notes` reads). PowerPoint's many kinds are mapped to the few the room draws.
use super::notes::{attr, is, Format, Package};
use crate::design::{Dir, Transition, TransitionKind, TRANSITION_MAX_MS};
use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

/// Each shown slide's transition, in order (`None`: the slide has none, so the app's own is used). `None` when
/// the file could not be read.
pub fn read(path: &Path, format: Format) -> Option<Vec<Option<Transition>>> {
    let f = BufReader::new(std::fs::File::open(path).ok()?);
    match format {
        Format::Pptx => pptx(f),
        Format::Odp => odp(f),
    }
}

/// Each shown slide's auto-advance time in milliseconds (PowerPoint's "After", LibreOffice's automatic page
/// change), in order. `None` when the file could not be read.
pub fn read_advance(path: &Path, format: Format) -> Option<Vec<Option<u32>>> {
    let f = BufReader::new(std::fs::File::open(path).ok()?);
    match format {
        Format::Pptx => pptx_advance(f),
        Format::Odp => odp_advance(f),
    }
}

/// The transitions (or times) to keep for a deck of `slides` pictures: none unless there is one entry per picture
/// and at least one slide has one.
pub fn line_up<T>(found: Option<Vec<Option<T>>>, slides: usize) -> Vec<Option<T>> {
    match found {
        Some(t) if t.len() == slides && t.iter().any(Option::is_some) => t,
        Some(t) if t.len() != slides => {
            eprintln!("Slide transitions left out: {} for {slides} slides", t.len());
            Vec::new()
        }
        _ => Vec::new(),
    }
}

pub fn pptx<R: Read + Seek>(r: R) -> Option<Vec<Option<Transition>>> {
    let mut p = Package::open(r)?;
    let slides = super::notes::pptx_slides(&mut p)?;
    Some(slides.into_iter().map(|(_, xml)| xml.and_then(|x| Document::parse(&x).ok().and_then(|d| slide_transition(&d)))).collect())
}

pub fn pptx_advance<R: Read + Seek>(r: R) -> Option<Vec<Option<u32>>> {
    let mut p = Package::open(r)?;
    let slides = super::notes::pptx_slides(&mut p)?;
    Some(slides.into_iter().map(|(_, xml)| xml.and_then(|x| Document::parse(&x).ok().and_then(|d| slide_advance(&d)))).collect())
}

/// The `<p:transition>` to use: the one in `mc:Choice` (newer PowerPoint), else `mc:Fallback`, else a plain one.
fn chosen<'a, 'i>(doc: &'a Document<'i>) -> Option<Node<'a, 'i>> {
    let all: Vec<Node> = doc.descendants().filter(|n| is(n, "transition") && n.parent().is_some_and(|p| !is(&p, "transition"))).collect();
    let in_ = |n: &Node, local: &str| n.ancestors().skip(1).take_while(|a| !is(a, "sld")).any(|a| is(&a, local));
    all.iter().find(|n| in_(n, "Choice")).or_else(|| all.iter().find(|n| in_(n, "Fallback"))).or(all.first()).copied()
}

/// A slide's transition (`None`: PowerPoint's "None", so the app's normal crossfade).
pub fn slide_transition(doc: &Document) -> Option<Transition> { chosen(doc).and_then(pptx_transition) }

/// A slide's "After" time (`advTm`, milliseconds).
pub fn slide_advance(doc: &Document) -> Option<u32> { attr(&chosen(doc)?, "advTm")?.trim().parse().ok() }

/// One `<p:transition>` element as the room's transition; none when it has no effect.
fn pptx_transition(t: Node) -> Option<Transition> {
    // `p14:dur` is milliseconds; `spd` is PowerPoint's three old speeds.
    let ms = attr(&t, "dur").and_then(|d| d.trim().parse::<u32>().ok()).unwrap_or(match attr(&t, "spd") {
        Some("slow") => 1000,
        Some("med") => 750,
        _ => 500,
    });
    let ms = ms.min(TRANSITION_MAX_MS);
    // No effect (only a sound or an "After" time): PowerPoint's "None".
    let e = t.children().find(|n| n.is_element() && !is(n, "sndAc") && !is(n, "extLst"))?;
    let black = attr(&e, "thruBlk").is_some_and(|v| matches!(v, "1" | "true"));
    let dir = attr(&e, "dir").and_then(side);
    let kind = match e.tag_name().name() {
        "cut" if black => TransitionKind::Black,
        "cut" => TransitionKind::Cut,
        "fade" if black => TransitionKind::Black,
        "fade" => TransitionKind::Fade,
        "dissolve" => TransitionKind::Dissolve,
        "zoom" => TransitionKind::Zoom,
        "push" => return Some(Transition { kind: TransitionKind::Push, ms, dir: dir.unwrap_or_default() }),
        "cover" => return Some(Transition { kind: TransitionKind::Cover, ms, dir: dir.unwrap_or_default() }),
        "pull" => return Some(Transition { kind: TransitionKind::Uncover, ms, dir: dir.unwrap_or_default() }),
        // A vertical split line opens sideways.
        "split" => return Some(Transition { kind: TransitionKind::Split, ms, dir: if attr(&e, "orient") == Some("vert") { Dir::Left } else { Dir::Up } }),
        _ => match dir {
            Some(dir) => return Some(Transition { kind: TransitionKind::Wipe, ms, dir }),
            None => TransitionKind::Fade,
        },
    };
    Some(Transition { kind, ms, dir: Dir::default() })
}

/// An OOXML direction (`l`, `u`, `r`, `d`, or a corner like `lu`, which goes by its first side).
fn side(d: &str) -> Option<Dir> {
    match d.trim().chars().next()? {
        'l' => Some(Dir::Left),
        'r' => Some(Dir::Right),
        'u' => Some(Dir::Up),
        'd' => Some(Dir::Down),
        _ => None,
    }
}

pub fn odp<R: Read + Seek>(r: R) -> Option<Vec<Option<Transition>>> {
    let mut p = Package::open(r)?;
    let xml = p.part("content.xml")?;
    Some(odp_doc(&Document::parse(&xml).ok()?))
}

pub fn odp_advance<R: Read + Seek>(r: R) -> Option<Vec<Option<u32>>> {
    let mut p = Package::open(r)?;
    let xml = p.part("content.xml")?;
    Some(odp_advance_doc(&Document::parse(&xml).ok()?))
}

/// Each page style's `drawing-page-properties`, by style name.
fn page_props<'a, 'i>(doc: &'a Document<'i>) -> HashMap<&'a str, Node<'a, 'i>> {
    doc.descendants()
        .filter(|n| is(n, "style"))
        .filter_map(|n| Some((attr(&n, "name")?, n.children().find(|c| is(c, "drawing-page-properties"))?)))
        .collect()
}

/// Each shown page's transition, from a parsed `content.xml`.
pub(super) fn odp_doc(doc: &Document) -> Vec<Option<Transition>> {
    let styles = page_props(doc);
    super::notes::odp_pages(doc)
        .into_iter()
        .map(|page| attr(&page, "style-name").and_then(|s| styles.get(s)).and_then(|props| odp_transition(*props)))
        .collect()
}

/// Each shown page's automatic time, from a parsed `content.xml`.
pub(super) fn odp_advance_doc(doc: &Document) -> Vec<Option<u32>> {
    let styles = page_props(doc);
    super::notes::odp_pages(doc)
        .into_iter()
        .map(|page| {
            let props = attr(&page, "style-name").and_then(|s| styles.get(s))?;
            if attr(props, "transition-type") != Some("automatic") { return None }
            attr(props, "duration").and_then(iso_duration)
        })
        .collect()
}

/// An ISO 8601 time (`PT00H00M05S`, `PT1M30.5S`) in milliseconds, as LibreOffice writes a page's duration.
fn iso_duration(v: &str) -> Option<u32> {
    let mut rest = v.trim().strip_prefix("PT")?;
    let mut ms = 0.0;
    while !rest.is_empty() {
        let end = rest.find(|c: char| c.is_ascii_alphabetic())?;
        let n: f64 = rest[..end].parse().ok()?;
        ms += n * match &rest[end..end + 1] { "H" => 3_600_000.0, "M" => 60_000.0, "S" => 1000.0, _ => return None };
        rest = &rest[end + 1..];
    }
    (ms.is_finite() && ms >= 0.0).then(|| ms.round().min(f64::from(u32::MAX)) as u32)
}

/// A LibreOffice page style's transition (`smil:type`, `smil:subtype`, `smil:direction`, `smil:dur`).
fn odp_transition(props: Node) -> Option<Transition> {
    let ty = attr(&props, "type")?;
    let sub = attr(&props, "subtype").unwrap_or("");
    let ms = attr(&props, "dur").and_then(seconds).unwrap_or(match attr(&props, "transition-speed") {
        Some("slow") => 1000,
        Some("fast") => 500,
        _ => 750,
    });
    let ms = ms.min(TRANSITION_MAX_MS);
    let mut dir = match sub {
        "fromLeft" | "leftToRight" => Some(Dir::Right),
        "fromRight" | "rightToLeft" => Some(Dir::Left),
        "fromTop" | "topToBottom" => Some(Dir::Down),
        "fromBottom" | "bottomToTop" => Some(Dir::Up),
        _ => None,
    };
    if attr(&props, "direction") == Some("reverse") { dir = dir.map(Dir::reverse) }
    let kind = match ty {
        "fade" if matches!(sub, "fadeOverColor" | "fadeToColor" | "fadeFromColor") => TransitionKind::Black,
        "pushWipe" | "slideWipe" if dir.is_some() => TransitionKind::Push,
        "barWipe" if dir.is_some() => TransitionKind::Wipe,
        _ if dir.is_some() && ty.ends_with("Wipe") => TransitionKind::Wipe,
        _ => TransitionKind::Fade,
    };
    let dir = if matches!(kind, TransitionKind::Push | TransitionKind::Wipe) { dir.unwrap_or_default() } else { Dir::default() };
    Some(Transition { kind, ms, dir })
}

/// A SMIL clock value in milliseconds: "2s", "0.5s", "750ms" or plain seconds.
pub(super) fn seconds(v: &str) -> Option<u32> {
    let v = v.trim();
    let (n, scale) = match v.strip_suffix("ms") {
        Some(n) => (n, 1.0),
        None => (v.strip_suffix('s').unwrap_or(v), 1000.0),
    };
    let f: f64 = n.trim().parse().ok()?;
    (f.is_finite() && f >= 0.0).then(|| (f * scale).round().min(f64::from(u32::MAX)) as u32)
}

#[cfg(test)]
#[path = "transitions_tests.rs"]
pub(crate) mod tests;
