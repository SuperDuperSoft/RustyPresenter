//! 1.1: a PowerPoint file (.pptx, and Google Slides' "Download as PowerPoint") as designed slides the slide editor
//! can change: text boxes, shapes, lines, pictures, videos, tables, backgrounds, notes, the transition and the
//! object animations. Placeholders take their place and text style from the slide's layout and master; theme colours
//! and fonts are looked up. Slides are put on the 16:9 slide as large as they fit (a 4:3 deck sits in the middle).
//!
//! 1.2: the box takes the look of most of its words and every run keeps its own (bold, italic, underline, strike,
//! size, colour, font), with paragraph alignment, line spacing, space before/after, levels and marks; paragraph
//! builds animate each paragraph; tables are tables; SmartArt comes in as the shapes PowerPoint drew for it; a chart
//! or embedded object as its stored picture; flipped shapes and pictures are mirrored.
//!
//! What still has no match (odd shapes, charts with no stored picture or only an EMF/WMF one, pictures we can't draw)
//! becomes a grey box with its name and any words it has, and is counted in `Converted::lost`, so the operator can
//! choose Pictures instead.
use super::notes::{self, attr, is, Package};
use crate::design::{
    Anim, AnimKind, Cell, Design, Dir, Effect, Fit, HAlign, Look, MotionPath, Obj, ObjKind, Outline, Para, PctRect, Run, Seg, Shape, Start,
    Table, TextStyle, VAlign, MAX_OBJECTS,
};
use super::{
    sniff::{self, Kind}, strings::{pdf_no_slides, slide_not_saved}, WorkDir, CANCELLED,
};
use crate::app::App;
use crate::design::MAX_SLIDES;
use crate::model::{AssetId, ItemKind, ServiceItem};
use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The slide the objects are placed on, in 1080p pixels.
const W: f64 = 1920.0;
const H: f64 = 1080.0;
const EMU_PER_PT: f64 = 12700.0;
/// All parts and pictures of one file together inflate at most this much.
const BUDGET: u64 = 1 << 30;
/// The largest video copied over.
const MAX_VIDEO: u64 = 300 << 20;
const PICTURE_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// What the import made.
#[derive(Debug, Default)]
pub struct Converted {
    pub slides: Vec<Design>,
    /// Objects that came over only as a grey box (or not at all).
    pub lost: usize,
}

/// Saves a picture or video's bytes (with its extension) in the store.
pub type Put<'a> = &'a mut dyn FnMut(&[u8], &str) -> Option<AssetId>;

/// Each shown slide as a designed slide. `None` when the file is not a presentation that can be read.
pub fn convert<R: Read + Seek>(r: R, put: Put) -> Option<Converted> {
    let mut p = Package::open(r)?.with_budget(BUDGET);
    let pres_xml = p.part("ppt/presentation.xml")?;
    let pres = Document::parse(&pres_xml).ok()?;
    let size = pres.descendants().find(|n| is(n, "sldSz"));
    let num = |n: Option<Node>, a: &str, d: f64| n.and_then(|n| attr(&n, a)).and_then(|v| v.parse::<f64>().ok()).filter(|v| *v > 0.0).unwrap_or(d);
    let (cx, cy) = (num(size, "cx", 12_192_000.0), num(size, "cy", 6_858_000.0));
    let s = (W / cx).min(H / cy);
    let geo = Geo { s, ox: (W - cx * s) / 2.0, oy: (H - cy * s) / 2.0, cx, cy };
    let mut out = Converted::default();
    for (part, xml) in notes::pptx_slides(&mut p)? {
        let Some(xml) = xml else { out.slides.push(Design::default()); continue };
        let mut io = Io { p: &mut p, put: &mut *put };
        let (mut d, lost) = slide(&mut io, &geo, &part, &xml, &pres_xml).unwrap_or_default();
        d.notes = notes::slide_notes(&mut p, &part);
        out.lost += lost;
        out.slides.push(d);
    }
    Some(out)
}

/// Reads `path` (see `convert`).
pub fn read(path: &std::path::Path, put: Put) -> Option<Converted> {
    convert(std::io::BufReader::new(std::fs::File::open(path).ok()?), put)
}

struct Io<'p, 'q, R> {
    p: &'p mut Package<R>,
    put: Put<'q>,
}

/// From the file's slide to ours.
struct Geo { s: f64, ox: f64, oy: f64, cx: f64, cy: f64 }

/// A part's relationships: id → (type, part name).
type Rels = HashMap<String, (String, String)>;

fn rels_of<R: Read + Seek>(p: &mut Package<R>, part: &str) -> Rels {
    let (dir, file) = notes::split(part);
    p.rels(&format!("{dir}/_rels/{file}.rels"), dir)
}

fn rel_target(rels: &Rels, kind: &str) -> Option<String> {
    rels.values().find(|(ty, _)| ty.ends_with(kind)).map(|(_, t)| t.clone())
}

/// One slide, and how many of its objects did not come over.
fn slide<R: Read + Seek>(io: &mut Io<R>, geo: &Geo, part: &str, xml: &str, pres_xml: &str) -> Option<(Design, usize)> {
    let slide_rels = rels_of(io.p, part);
    let layout_part = rel_target(&slide_rels, "/slideLayout");
    let layout_rels = layout_part.as_deref().map(|l| rels_of(io.p, l)).unwrap_or_default();
    let master_part = rel_target(&layout_rels, "/slideMaster");
    let master_rels = master_part.as_deref().map(|m| rels_of(io.p, m)).unwrap_or_default();
    let theme_part = rel_target(&master_rels, "/theme");
    let layout_xml = layout_part.as_deref().and_then(|l| io.p.part(l)).unwrap_or_default();
    let master_xml = master_part.as_deref().and_then(|m| io.p.part(m)).unwrap_or_default();
    let theme_xml = theme_part.as_deref().and_then(|t| io.p.part(t)).unwrap_or_default();

    let sld = Document::parse(xml).ok()?;
    let layout = Document::parse(&layout_xml).ok();
    let master = Document::parse(&master_xml).ok();
    let theme = Document::parse(&theme_xml).ok();
    let pres = Document::parse(pres_xml).ok();
    let layout = layout.as_ref().map(|d| d.root_element());
    let master = master.as_ref().map(|d| d.root_element());

    let mut colors = Colors::from_theme(theme.as_ref().map(|d| d.root_element()));
    if let Some(m) = master.and_then(|m| m.children().find(|n| is(n, "clrMap"))) { colors.set_map(m) }
    for over in [layout, Some(sld.root_element())].into_iter().flatten() {
        if let Some(m) = over.descendants().find(|n| is(n, "overrideClrMapping")) { colors.set_map(m) }
    }
    let font = |which: &str| theme.as_ref().and_then(|t| t.descendants().find(|n| is(n, which))).and_then(|f| f.children().find(|n| is(n, "latin"))).and_then(|l| attr(&l, "typeface")).unwrap_or("").to_string();
    let styles = master.and_then(|m| m.children().find(|n| is(n, "txStyles")));
    let style = |name: &str| levels(styles.and_then(|s| s.children().find(|n| is(n, name))), &colors);
    let table_styles = io.p.part("ppt/tableStyles.xml").map(|x| table_styles(&x, &colors, |emu| (emu * geo.s) as f32)).unwrap_or_default();
    let cv = Conv {
        geo,
        major: font("majorFont"),
        minor: font("minorFont"),
        title: style("titleStyle"),
        body: style("bodyStyle"),
        other: style("otherStyle"),
        deflt: levels(pres.as_ref().and_then(|d| d.descendants().find(|n| is(n, "defaultTextStyle"))), &colors),
        layout,
        master,
        colors,
        table_styles,
    };
    let mut made = Made::default();
    cv.background(io, &mut made, &[(Some(sld.root_element()), &slide_rels), (layout, &layout_rels), (master, &master_rels)]);
    let shows = |n: Option<Node>| n.is_none_or(|n| !matches!(attr(&n, "showMasterSp"), Some("0" | "false")));
    if shows(Some(sld.root_element())) {
        if shows(layout) && let Some(t) = tree(master) { cv.walk(io, &mut made, t, &mut vec![], &master_rels, false) }
        if let Some(t) = tree(layout) { cv.walk(io, &mut made, t, &mut vec![], &layout_rels, false) }
    }
    if let Some(t) = tree(Some(sld.root_element())) { cv.walk(io, &mut made, t, &mut vec![], &slide_rels, true) }

    // No transition in the file: the app's normal crossfade, as for picture slides.
    let transition = super::transitions::slide_transition(&sld).unwrap_or_default();
    let anims = cv.anims(sld.root_element(), &made);
    let lost = made.lost + made.objs.len().saturating_sub(MAX_OBJECTS);
    let mut d = Design { objects: made.objs, transition, notes: String::new(), anims };
    d.sanitise();
    Some((d, lost))
}

/// What a slide has made so far.
#[derive(Default)]
struct Made {
    objs: Vec<Obj>,
    /// The file's shape id → the objects made for it (animations find them by it).
    ids: HashMap<String, Vec<Uuid>>,
    /// A text object → where each of the file's paragraphs went (paragraph builds find them by it).
    paras: HashMap<Uuid, Vec<(usize, usize)>>,
    /// The next text object pushed has these paragraphs (`words`).
    next_paras: Option<Vec<(usize, usize)>>,
    lost: usize,
}

impl Made {
    fn push(&mut self, o: Obj) -> Uuid {
        let id = o.id;
        if let Some(p) = self.next_paras.take() { self.paras.insert(id, p); }
        self.objs.push(o);
        id
    }

    /// The next object pushed is a text box with these paragraphs.
    fn words(&mut self, starts: Vec<(usize, usize)>) { self.next_paras = Some(starts) }
}

/// A place on the file's slide, in EMU.
#[derive(Clone, Copy, Debug, Default)]
struct Box_ { x: f64, y: f64, w: f64, h: f64, rot: f64, flip_h: bool, flip_v: bool }

/// A group's way of placing its children.
#[derive(Clone, Copy)]
struct Grp { off: (f64, f64), scale: (f64, f64), ch: (f64, f64), rot: f64, centre: (f64, f64) }

fn xfrm(holder: Node) -> Option<(Box_, Option<((f64, f64), (f64, f64))>)> {
    let x = holder.children().find(|n| is(n, "xfrm"))?;
    let f = |n: Node, a: &str| attr(&n, a).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0);
    let pair = |name: &str, a: &str, b: &str| x.children().find(|n| is(n, name)).map(|n| (f(n, a), f(n, b)));
    let (off, ext) = (pair("off", "x", "y")?, pair("ext", "cx", "cy")?);
    let yes = |a: &str| matches!(attr(&x, a), Some("1" | "true"));
    let rot = attr(&x, "rot").and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0) / 60000.0;
    let child = pair("chOff", "x", "y").zip(pair("chExt", "cx", "cy"));
    Some((Box_ { x: off.0, y: off.1, w: ext.0, h: ext.1, rot, flip_h: yes("flipH"), flip_v: yes("flipV") }, child))
}

/// `b` in a group's children's space, placed on the slide through every group around it (innermost last).
fn place(mut b: Box_, groups: &[Grp]) -> Box_ {
    for g in groups.iter().rev() {
        let c = (b.x + b.w / 2.0, b.y + b.h / 2.0);
        let mut c = (g.off.0 + (c.0 - g.ch.0) * g.scale.0, g.off.1 + (c.1 - g.ch.1) * g.scale.1);
        b.w *= g.scale.0;
        b.h *= g.scale.1;
        if g.rot != 0.0 {
            let (sin, cos) = g.rot.to_radians().sin_cos();
            let (dx, dy) = (c.0 - g.centre.0, c.1 - g.centre.1);
            c = (g.centre.0 + dx * cos - dy * sin, g.centre.1 + dx * sin + dy * cos);
            b.rot += g.rot;
        }
        b.x = c.0 - b.w / 2.0;
        b.y = c.1 - b.h / 2.0;
    }
    b
}

/// Text properties at one level; unset ones come from below.
#[derive(Clone, Default, Debug, PartialEq)]
struct Props {
    size: Option<f64>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    color: Option<[u8; 4]>,
    font: Option<String>,
    align: Option<HAlign>,
    /// Some(None): a bullet; Some(Some(_)): numbered; None: not set. `no_bullet` turns it off.
    bullet: Option<Bullet>,
    line: Option<Spacing>,
    before: Option<Spacing>,
    after: Option<Spacing>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Bullet { None, Char, Number }

/// A spacing: a share of the font size (1.0 = single) or points.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Spacing { Pct(f64), Pts(f64) }

impl Props {
    fn over(&mut self, o: &Props) {
        if o.size.is_some() { self.size = o.size }
        if o.bold.is_some() { self.bold = o.bold }
        if o.italic.is_some() { self.italic = o.italic }
        if o.underline.is_some() { self.underline = o.underline }
        if o.strike.is_some() { self.strike = o.strike }
        if o.color.is_some() { self.color = o.color }
        if o.font.is_some() { self.font = o.font.clone() }
        if o.align.is_some() { self.align = o.align }
        if o.bullet.is_some() { self.bullet = o.bullet }
        if o.line.is_some() { self.line = o.line }
        if o.before.is_some() { self.before = o.before }
        if o.after.is_some() { self.after = o.after }
    }

    /// Only what a run's look is made of (paragraph settings left out), to count words by look.
    fn run_look(&self) -> Props { Props { size: self.size, bold: self.bold, italic: self.italic, underline: self.underline, strike: self.strike, color: self.color, font: self.font.clone(), ..Props::default() } }
}

/// The nine levels of a list style (`lvl1pPr`…).
type Levels = Vec<Props>;

fn levels(n: Option<Node>, c: &Colors) -> Levels {
    (1..=9).map(|i| n.and_then(|n| n.children().find(|k| is(k, &format!("lvl{i}pPr")))).map(|p| ppr(p, c)).unwrap_or_default()).collect()
}

/// `a:lnSpc`, `a:spcBef`, `a:spcAft`: a percentage (thousandths) or points (hundredths).
fn spacing(n: Option<Node>) -> Option<Spacing> {
    let n = n?;
    let val = |name: &str| n.children().find(|k| is(k, name)).and_then(|k| attr(&k, "val")).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite() && *v >= 0.0);
    val("spcPct").map(|v| Spacing::Pct(v / 100_000.0)).or_else(|| val("spcPts").map(|v| Spacing::Pts(v / 100.0)))
}

/// A paragraph's properties (`a:pPr` or `a:lvlNpPr`) with its default run.
fn ppr(n: Node, c: &Colors) -> Props {
    let mut p = n.children().find(|k| is(k, "defRPr")).map(|r| rpr(r, c)).unwrap_or_default();
    p.align = attr(&n, "algn").map(|a| match a { "ctr" => HAlign::Center, "r" => HAlign::Right, _ => HAlign::Left });
    for k in n.children() {
        match k.tag_name().name() {
            "buNone" => p.bullet = Some(Bullet::None),
            "buChar" | "buBlip" => p.bullet = Some(Bullet::Char),
            "buAutoNum" => p.bullet = Some(Bullet::Number),
            "lnSpc" => p.line = spacing(Some(k)),
            "spcBef" => p.before = spacing(Some(k)),
            "spcAft" => p.after = spacing(Some(k)),
            _ => {}
        }
    }
    p
}

/// A run's properties (`a:rPr`, `a:defRPr`, `a:endParaRPr`).
fn rpr(n: Node, c: &Colors) -> Props {
    let flag = |a: &str| attr(&n, a).map(|v| matches!(v, "1" | "true"));
    Props {
        size: attr(&n, "sz").and_then(|v| v.parse::<f64>().ok()).map(|v| v / 100.0),
        bold: flag("b"),
        italic: flag("i"),
        underline: attr(&n, "u").map(|v| v != "none"),
        strike: attr(&n, "strike").map(|v| v != "noStrike"),
        color: n.children().find(|k| is(k, "solidFill")).and_then(|f| c.color(f, None)),
        font: n.children().find(|k| is(k, "latin")).and_then(|l| attr(&l, "typeface")).map(str::to_string),
        ..Props::default()
    }
}

/// One part of a table style (whole table, banded rows, first row…): cell fill, borders (left, top, right,
/// bottom) and words.
#[derive(Clone, Default)]
struct TblPart { fill: Option<[u8; 4]>, borders: [Option<Outline>; 4], text: Props }

impl TblPart {
    fn over(&mut self, o: &TblPart) {
        if o.fill.is_some() { self.fill = o.fill }
        for (a, b) in self.borders.iter_mut().zip(&o.borders) { if b.is_some() { *a = b.clone() } }
        self.text.over(&o.text);
    }
}

#[derive(Clone, Default)]
struct TblStyle { whole: TblPart, band: TblPart, first_row: TblPart, last_row: TblPart, first_col: TblPart }

/// The file's table styles (`ppt/tableStyles.xml`), by id.
fn table_styles(xml: &str, c: &Colors, px: impl Fn(f64) -> f32) -> HashMap<String, TblStyle> {
    let Ok(doc) = Document::parse(xml) else { return HashMap::new() };
    let part = |st: Node, name: &str| -> TblPart {
        let Some(n) = st.children().find(|k| is(k, name)) else { return TblPart::default() };
        let tc = n.children().find(|k| is(k, "tcStyle"));
        let fill = tc.and_then(|t| t.children().find(|k| is(k, "fill"))).and_then(|f| f.children().find(|k| is(k, "solidFill"))).and_then(|f| c.color(f, None));
        let bdr = tc.and_then(|t| t.children().find(|k| is(k, "tcBdr")));
        let line = |names: &[&str]| -> Option<Outline> {
            let ln = names.iter().find_map(|nm| bdr.and_then(|b| b.children().find(|k| is(k, nm)))).and_then(|s| s.children().find(|k| is(k, "ln")))?;
            let color = ln.children().find(|k| is(k, "solidFill")).and_then(|f| c.color(f, None))?;
            Some(Outline { color: css(color), width: px(attr(&ln, "w").and_then(|v| v.parse::<f64>().ok()).unwrap_or(12_700.0)).max(1.0) })
        };
        let tx = n.children().find(|k| is(k, "tcTxStyle"));
        let text = Props {
            bold: tx.and_then(|t| attr(&t, "b")).map(|v| v == "on"),
            italic: tx.and_then(|t| attr(&t, "i")).map(|v| v == "on"),
            color: tx.and_then(|t| c.color(t, None)),
            ..Props::default()
        };
        TblPart { fill, borders: [line(&["left", "insideV"]), line(&["top", "insideH"]), line(&["right", "insideV"]), line(&["bottom", "insideH"])], text }
    };
    doc.descendants().filter(|n| is(n, "tblStyle")).filter_map(|st| {
        let id = attr(&st, "styleId")?.to_string();
        Some((id, TblStyle { whole: part(st, "wholeTbl"), band: part(st, "band1H"), first_row: part(st, "firstRow"), last_row: part(st, "lastRow"), first_col: part(st, "firstCol") }))
    }).collect()
}

/// The theme's colours, by scheme name, and the master's map from `tx1`/`bg1`… to them.
#[derive(Clone)]
struct Colors { scheme: HashMap<String, [u8; 3]>, map: HashMap<String, String> }

impl Colors {
    fn from_theme(theme: Option<Node>) -> Colors {
        let mut scheme = HashMap::new();
        if let Some(cs) = theme.and_then(|t| t.descendants().find(|n| is(n, "clrScheme"))) {
            for c in cs.children().filter(|n| n.is_element()) {
                let v = c.children().find(|n| n.is_element()).and_then(|k| match k.tag_name().name() {
                    "srgbClr" => attr(&k, "val").and_then(hex3),
                    "sysClr" => attr(&k, "lastClr").and_then(hex3),
                    _ => None,
                });
                if let Some(v) = v { scheme.insert(c.tag_name().name().to_string(), v); }
            }
        }
        let map = [("bg1", "lt1"), ("tx1", "dk1"), ("bg2", "lt2"), ("tx2", "dk2")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        Colors { scheme, map }
    }

    fn set_map(&mut self, n: Node) {
        for a in n.attributes() { self.map.insert(a.name().to_string(), a.value().to_string()); }
    }

    /// The colour inside `holder` (a fill, or a style reference), with its changes; `ph` stands for `phClr`.
    fn color(&self, holder: Node, ph: Option<[u8; 4]>) -> Option<[u8; 4]> {
        let c = holder.children().find(|n| matches!(n.tag_name().name(), "srgbClr" | "schemeClr" | "sysClr" | "prstClr" | "scrgbClr" | "hslClr") && n.is_element())?;
        let val = attr(&c, "val").unwrap_or("");
        let base: [u8; 3] = match c.tag_name().name() {
            "srgbClr" => hex3(val)?,
            "schemeClr" if val == "phClr" => { let p = ph?; [p[0], p[1], p[2]] }
            "schemeClr" => { let name = self.map.get(val).map(String::as_str).unwrap_or(val); *self.scheme.get(name).or_else(|| self.scheme.get(val))? }
            "sysClr" => attr(&c, "lastClr").and_then(hex3).unwrap_or(if val == "window" { [255; 3] } else { [0; 3] }),
            "prstClr" => { let p = crate::render::color::parse_color(val); [p[0], p[1], p[2]] }
            "scrgbClr" => {
                let ch = |a: &str| (attr(&c, a).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) / 100_000.0).clamp(0.0, 1.0);
                [to_srgb(ch("r")), to_srgb(ch("g")), to_srgb(ch("b"))]
            }
            _ => return None,
        };
        let (mut rgb, mut alpha) = ([base[0] as f64 / 255.0, base[1] as f64 / 255.0, base[2] as f64 / 255.0], 1.0);
        for m in c.children().filter(|n| n.is_element()) {
            let v = attr(&m, "val").and_then(|v| v.parse::<f64>().ok()).unwrap_or(100_000.0) / 100_000.0;
            match m.tag_name().name() {
                "alpha" => alpha = v,
                "lumMod" => { let (h, s, l) = hsl(rgb); rgb = from_hsl(h, s, l * v) }
                "lumOff" => { let (h, s, l) = hsl(rgb); rgb = from_hsl(h, s, l + v) }
                "tint" => for x in &mut rgb { *x = *x * v + (1.0 - v) }
                "shade" => for x in &mut rgb { *x *= v }
                _ => {}
            }
        }
        let b = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        Some([b(rgb[0]), b(rgb[1]), b(rgb[2]), b(alpha)])
    }
}

fn to_srgb(x: f64) -> u8 { let v = if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }; (v.clamp(0.0, 1.0) * 255.0).round() as u8 }

fn hex3(v: &str) -> Option<[u8; 3]> {
    let v = v.trim();
    if v.len() != 6 { return None }
    let n = u32::from_str_radix(v, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

fn hsl([r, g, b]: [f64; 3]) -> (f64, f64, f64) {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min { return (0.0, 0.0, l) }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r { (g - b) / d + if g < b { 6.0 } else { 0.0 } } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
    (h / 6.0, s, l)
}

fn from_hsl(h: f64, s: f64, l: f64) -> [f64; 3] {
    let l = l.clamp(0.0, 1.0);
    if s == 0.0 { return [l; 3] }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let t = |mut t: f64| {
        if t < 0.0 { t += 1.0 }
        if t > 1.0 { t -= 1.0 }
        if t < 1.0 / 6.0 { p + (q - p) * 6.0 * t } else if t < 0.5 { q } else if t < 2.0 / 3.0 { p + (q - p) * (2.0 / 3.0 - t) * 6.0 } else { p }
    };
    [t(h + 1.0 / 3.0), t(h), t(h - 1.0 / 3.0)]
}

/// `#rrggbb`, or `#rrggbbaa` when not opaque.
fn css(c: [u8; 4]) -> String {
    if c[3] == 255 { format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]) } else { format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]) }
}

/// A shape's fill.
#[derive(Clone, Debug, PartialEq)]
enum Fill { Unset, None, Color([u8; 4]), Picture(String) }

/// Everything one slide's objects are converted with.
struct Conv<'g, 'a, 'i> {
    geo: &'g Geo,
    major: String,
    minor: String,
    title: Levels,
    body: Levels,
    other: Levels,
    deflt: Levels,
    layout: Option<Node<'a, 'i>>,
    master: Option<Node<'a, 'i>>,
    colors: Colors,
    table_styles: HashMap<String, TblStyle>,
}

/// A placeholder's kind as the master knows it.
fn ph_class(ty: &str) -> &str {
    match ty {
        "title" | "ctrTitle" => "title",
        "dt" | "ftr" | "sldNum" | "hdr" => ty,
        _ => "body",
    }
}

impl<'g, 'a, 'i> Conv<'g, 'a, 'i> {
    fn pct(&self, b: &Box_) -> PctRect {
        let g = self.geo;
        PctRect::new(((g.ox + b.x * g.s) / W * 100.0) as f32, ((g.oy + b.y * g.s) / H * 100.0) as f32, (b.w * g.s / W * 100.0) as f32, (b.h * g.s / H * 100.0) as f32)
    }

    fn px(&self, emu: f64) -> f32 { (emu * self.geo.s) as f32 }

    /// The layout's and the master's shape for a slide's placeholder.
    fn ph_parents(&self, ph: Node) -> (Option<Node<'a, 'i>>, Option<Node<'a, 'i>>) {
        let ty = attr(&ph, "type").unwrap_or("obj");
        let idx = attr(&ph, "idx");
        let phs = |root: Option<Node<'a, 'i>>| -> Vec<(Node<'a, 'i>, Node<'a, 'i>)> {
            root.map(|r| r.descendants().filter(|n| is(n, "sp")).filter_map(|sp| Some((sp, sp.descendants().find(|n| is(n, "ph"))?))).collect()).unwrap_or_default()
        };
        let in_layout = phs(self.layout);
        let found = idx.and_then(|i| in_layout.iter().find(|(_, p)| attr(p, "idx") == Some(i)))
            .or_else(|| in_layout.iter().find(|(_, p)| attr(p, "type").unwrap_or("obj") == ty))
            .or_else(|| in_layout.iter().find(|(_, p)| ph_class(attr(p, "type").unwrap_or("obj")) == ph_class(ty) && ph_class(ty) != "body"))
            .map(|(sp, p)| (*sp, *p));
        let class = ph_class(found.and_then(|(_, p)| attr(&p, "type")).unwrap_or(ty));
        let master = phs(self.master).into_iter().find(|(_, p)| ph_class(attr(p, "type").unwrap_or("obj")) == class).map(|(sp, _)| sp);
        (found.map(|(sp, _)| sp), master)
    }

    fn walk<R: Read + Seek>(&self, io: &mut Io<R>, made: &mut Made, tree: Node<'a, 'i>, groups: &mut Vec<Grp>, rels: &Rels, on_slide: bool) {
        for c in tree.children().filter(|n| n.is_element()) {
            match c.tag_name().name() {
                "sp" | "cxnSp" => self.shape(io, made, c, groups, rels, on_slide),
                "pic" => self.picture(io, made, c, groups, rels, on_slide),
                "grpSp" => {
                    let Some((b, ch)) = c.children().find(|n| is(n, "grpSpPr")).and_then(xfrm) else { continue };
                    let ((chx, chy), (chw, chh)) = ch.unwrap_or(((b.x, b.y), (b.w, b.h)));
                    let scale = (if chw > 0.0 { b.w / chw } else { 1.0 }, if chh > 0.0 { b.h / chh } else { 1.0 });
                    // Children are placed in the group's own frame, then that frame is placed like the group.
                    groups.push(Grp { off: (b.x, b.y), scale, ch: (chx, chy), rot: b.rot, centre: (b.x + b.w / 2.0, b.y + b.h / 2.0) });
                    let before = made.objs.len();
                    self.walk(io, made, c, groups, rels, on_slide);
                    groups.pop();
                    if on_slide && let Some(id) = shape_id(c) {
                        let ids = made.objs[before..].iter().map(|o| o.id).collect();
                        made.ids.insert(id, ids);
                    }
                }
                "graphicFrame" => self.frame(io, made, c, groups, rels, on_slide),
                "AlternateContent" => {
                    let pick = c.children().find(|n| is(n, "Fallback")).or_else(|| c.children().find(|n| is(n, "Choice")));
                    if let Some(p) = pick { self.walk(io, made, p, groups, rels, on_slide) }
                }
                "contentPart" => made.lost += 1,
                _ => {}
            }
        }
    }

    /// A grey box with `text` where something could not come over.
    fn lost(&self, made: &mut Made, node: Node, b: Box_, text: String, on_slide: bool) {
        made.lost += 1;
        // The same grey box as the .odp import's (`odp_design`).
        let style = TextStyle { size: 36.0, color: "#ffffff".into(), align: HAlign::Center, valign: VAlign::Middle, shrink: true, ..TextStyle::default() };
        let mut o = Obj::new(ObjKind::text(text, style), self.pct(&b));
        o.rotation = b.rot as f32;
        o.look = Look { fill: Some("#4b5563".into()), outline: Some(Outline { color: "#9ca3af".into(), width: 3.0 }), ..Look::default() };
        let id = made.push(o);
        if on_slide && let Some(s) = shape_id(node) { made.ids.entry(s).or_default().push(id); }
    }

    /// A table (as a table), SmartArt (its drawn shapes), a chart or embedded object (its stored picture), else a
    /// grey box with its name.
    fn frame<R: Read + Seek>(&self, io: &mut Io<R>, made: &mut Made, f: Node, groups: &[Grp], rels: &Rels, on_slide: bool) {
        let Some((b, _)) = xfrm(f) else { return };
        let placed = place(b, groups);
        let before = made.objs.len();
        let data = f.descendants().find(|n| is(n, "graphicData"));
        let uri = data.and_then(|d| attr(&d, "uri")).unwrap_or("");
        let done = if let Some(tbl) = f.descendants().find(|n| is(n, "tbl")) {
            self.table(made, tbl, placed)
        } else if uri.ends_with("/diagram") {
            self.smartart(io, made, f, b, groups, rels)
        } else {
            self.stored_picture(io, made, f, placed, rels)
        };
        if !done {
            let nv = f.descendants().find(|n| is(n, "cNvPr"));
            let name = nv.and_then(|n| attr(&n, "descr").filter(|d| !d.trim().is_empty()).or(attr(&n, "name"))).unwrap_or("").to_string();
            return self.lost(made, f, placed, name, on_slide);
        }
        if on_slide && let Some(id) = shape_id(f) {
            let ids: Vec<Uuid> = made.objs[before..].iter().map(|o| o.id).collect();
            made.ids.entry(id).or_default().extend(ids);
        }
    }

    /// A chart's or embedded object's picture as the file keeps it (its preview, or a picture the chart part
    /// holds). False when there is none we can draw (an EMF/WMF preview stays a grey box).
    fn stored_picture<R: Read + Seek>(&self, io: &mut Io<R>, made: &mut Made, f: Node, b: Box_, rels: &Rels) -> bool {
        let mut found = f.descendants().filter(|n| is(n, "blip")).filter_map(|bl| attr(&bl, "embed")).find_map(|rid| self.media(io, rels, rid, None));
        if found.is_none() {
            // The chart part's own pictures (a saved preview).
            let chart = f.descendants().find(|n| is(n, "chart")).and_then(|c| c.attributes().find(|a| a.name() == "id").map(|a| a.value().to_string()));
            if let Some((_, part)) = chart.and_then(|c| rels.get(&c).cloned()) {
                let crels = rels_of(io.p, &part);
                let mut pics: Vec<&String> = crels.iter().filter(|(_, (ty, _))| ty.ends_with("/image")).map(|(id, _)| id).collect();
                pics.sort();
                found = pics.into_iter().find_map(|rid| self.media(io, &crels, rid, None));
            }
        }
        let Some(asset) = found else { return false };
        let mut o = Obj::new(ObjKind::Picture { asset, fit: Fit::Stretch }, self.pct(&b));
        o.rotation = b.rot as f32;
        made.push(o);
        true
    }

    /// SmartArt: the shapes PowerPoint drew for it (`ppt/diagrams/drawingN.xml`), placed in the frame. False when
    /// the file has no drawing for it.
    fn smartart<R: Read + Seek>(&self, io: &mut Io<R>, made: &mut Made, f: Node, b: Box_, groups: &[Grp], rels: &Rels) -> bool {
        let dm = f.descendants().find(|n| is(n, "relIds")).and_then(|r| r.attributes().find(|a| a.name() == "dm").map(|a| a.value().to_string()));
        // The data part names its drawing by the slide's relationship id; else the slide's only drawing.
        let by_data = dm.and_then(|dm| rels.get(&dm).cloned()).and_then(|(_, part)| io.p.part(&part)).and_then(|xml| {
            let doc = Document::parse(&xml).ok()?;
            let id = doc.descendants().find(|n| is(n, "dataModelExt")).and_then(|n| attr(&n, "relId"))?.to_string();
            rels.get(&id).map(|(_, p)| p.clone())
        });
        let drawings: Vec<&String> = rels.values().filter(|(ty, _)| ty.ends_with("/diagramDrawing")).map(|(_, p)| p).collect();
        let Some(part) = by_data.or_else(|| (drawings.len() == 1).then(|| drawings[0].clone())) else { return false };
        let Some(xml) = io.p.part(&part) else { return false };
        let Ok(doc) = Document::parse(&xml) else { return false };
        let Some(tree) = doc.descendants().find(|n| is(n, "spTree")) else { return false };
        let drels = rels_of(io.p, &part);
        let mut gs = groups.to_vec();
        // The drawing's shapes are placed from the frame's top left corner.
        gs.push(Grp { off: (b.x, b.y), scale: (1.0, 1.0), ch: (0.0, 0.0), rot: b.rot, centre: (b.x + b.w / 2.0, b.y + b.h / 2.0) });
        let before = made.objs.len();
        self.alone().walk(io, made, tree, &mut gs, &drels, false);
        made.objs.len() > before
    }

    /// The same look-ups without the slide's layout and master (for parts of their own, as SmartArt drawings).
    fn alone<'b, 'j>(&self) -> Conv<'g, 'b, 'j> {
        Conv {
            geo: self.geo, major: self.major.clone(), minor: self.minor.clone(), title: self.title.clone(), body: self.body.clone(),
            other: self.other.clone(), deflt: self.deflt.clone(), layout: None, master: None, colors: self.colors.clone(), table_styles: self.table_styles.clone(),
        }
    }

    /// A table: its grid, cells' words, fills and borders (the cell's own, else the table style's).
    /// False when it has no cells.
    fn table(&self, made: &mut Made, tbl: Node, b: Box_) -> bool {
        let mut cols: Vec<f32> = tbl.children().find(|n| is(n, "tblGrid")).map(|g| g.children().filter(|n| is(n, "gridCol")).map(|c| attr(&c, "w").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0)).collect()).unwrap_or_default();
        let trs: Vec<Node> = tbl.children().filter(|n| is(n, "tr")).take(crate::design::rich::TABLE_MAX).collect();
        // No grid: as many even columns as the widest row has cells.
        if cols.is_empty() { cols = vec![1.0; trs.iter().map(|r| r.children().filter(|n| is(n, "tc")).count()).max().unwrap_or(0)] }
        cols.truncate(crate::design::rich::TABLE_MAX);
        if cols.is_empty() || trs.is_empty() { return false }
        let rows: Vec<f32> = trs.iter().map(|r| attr(r, "h").and_then(|v| v.parse::<f32>().ok()).filter(|h| *h > 0.0).unwrap_or(370_840.0)).collect();
        let pr = tbl.children().find(|n| is(n, "tblPr"));
        let flag = |a: &str| pr.and_then(|p| attr(&p, a)).is_some_and(|v| matches!(v, "1" | "true"));
        let style_id = pr.and_then(|p| p.children().find(|n| is(n, "tableStyleId"))).and_then(|n| n.text()).map(|t| t.trim().to_string());
        let ts = self.table_style(style_id.as_deref());
        let (nr, nc) = (trs.len(), cols.len());
        let mut cells: Vec<Cell> = Vec::with_capacity(nr * nc);
        let mut looks: Vec<TextStyle> = Vec::new();
        for (r, tr) in trs.iter().enumerate() {
            let tcs: Vec<Node> = tr.children().filter(|n| is(n, "tc")).collect();
            for c in 0..nc {
                let Some(tc) = tcs.get(c) else { cells.push(Cell::default()); looks.push(self.text_default(&ts.whole.text)); continue };
                // The style's part for this cell: whole table, banded rows, then first/last row and column.
                let mut part = ts.whole.clone();
                // The first body row is a band.
                let first = usize::from(flag("firstRow"));
                if flag("bandRow") && r >= first && (r - first) % 2 == 0 { part.over(&ts.band) }
                if flag("firstRow") && r == 0 { part.over(&ts.first_row) }
                if flag("lastRow") && r == nr - 1 && nr > 1 { part.over(&ts.last_row) }
                if flag("firstCol") && c == 0 && !(flag("firstRow") && r == 0) { part.over(&ts.first_col) }
                let tcpr = tc.children().find(|n| is(n, "tcPr"));
                let words = tc.children().find(|n| is(n, "txBody")).map(|tx| self.text(tx, None, None, None, None, Some(&part.text)));
                let fill = match self.fill(tcpr, None) { Fill::Color(c) => Some(css(c)), Fill::None => None, _ => part.fill.map(css) };
                let side = |name: &str, i: usize| -> Option<Outline> {
                    match tcpr.and_then(|p| p.children().find(|n| is(n, name))) {
                        Some(ln) => {
                            let fill = ln.children().find(|n| n.is_element() && matches!(n.tag_name().name(), "noFill" | "solidFill"))?;
                            let color = if is(&fill, "solidFill") { self.colors.color(fill, None)? } else { return None };
                            let w = attr(&ln, "w").and_then(|v| v.parse::<f64>().ok()).unwrap_or(12_700.0);
                            Some(Outline { color: css(color), width: self.px(w).max(1.0) })
                        }
                        None => part.borders[i].clone(),
                    }
                };
                let num = |a: &str| attr(tc, a).and_then(|v| v.parse::<u16>().ok()).unwrap_or(1);
                let merged = attr(tc, "hMerge").or(attr(tc, "vMerge")).is_some_and(|v| matches!(v, "1" | "true"));
                let valign = match tcpr.and_then(|p| attr(&p, "anchor")) { Some("ctr") => Some(VAlign::Middle), Some("b") => Some(VAlign::Bottom), _ => None };
                let (text, paras, look) = match words.map(|w| (w.kind, w.style)) {
                    Some((ObjKind::Text { text, paras, .. }, st)) => (text, paras, st),
                    _ => (String::new(), vec![], self.text_default(&part.text)),
                };
                looks.push(look.clone());
                cells.push(Cell {
                    text, paras, fill,
                    borders: [side("lnL", 0), side("lnT", 1), side("lnR", 2), side("lnB", 3)],
                    span_cols: num("gridSpan").max(1), span_rows: num("rowSpan").max(1), merged, valign,
                });
            }
        }
        // The table's look is the one most cells have; each cell's own differences go in its runs.
        let style = looks.iter().max_by_key(|l| looks.iter().filter(|m| m == l).count()).cloned().unwrap_or_default();
        let style = TextStyle { shrink: false, valign: VAlign::Top, ..style };
        for (cell, look) in cells.iter_mut().zip(&looks) {
            cell.paras = crate::design::rich::rebase(&cell.paras, &cell.text, look, &style);
        }
        let table = Table { cols, rows, cells, style };
        let mut o = Obj::new(ObjKind::Table { table }, self.pct(&b));
        o.rotation = b.rot as f32;
        made.push(o);
        true
    }

    /// The look of an empty cell's words.
    fn text_default(&self, over: &Props) -> TextStyle {
        let mut p = Props { size: Some(18.0), color: Some([0, 0, 0, 255]), ..Props::default() };
        p.over(&self.deflt[0]);
        p.over(over);
        TextStyle {
            font: self.minor.clone(), size: self.px(p.size.unwrap_or(18.0) * EMU_PER_PT).clamp(crate::design::SIZE_MIN, crate::design::SIZE_MAX),
            color: css(p.color.unwrap_or([0, 0, 0, 255])), bold: p.bold.unwrap_or(false), align: HAlign::Left, valign: VAlign::Top, shrink: false, ..TextStyle::default()
        }
    }

    /// A table style by id: the file's (`ppt/tableStyles.xml`), else PowerPoint's default look
    /// ("Medium Style 2 – Accent 1").
    fn table_style(&self, id: Option<&str>) -> TblStyle {
        if let Some(st) = id.and_then(|id| self.table_styles.get(id)) { return st.clone() }
        let accent = self.colors.scheme.get("accent1").copied().unwrap_or([0x44, 0x72, 0xc4]);
        let tint = |k: f64| { let t = |c: u8| (f64::from(c) * k + 255.0 * (1.0 - k)).round() as u8; [t(accent[0]), t(accent[1]), t(accent[2]), 255] };
        let white = Some(Outline { color: "#ffffff".into(), width: self.px(12_700.0).max(1.0) });
        let lines = [white.clone(), white.clone(), white.clone(), white];
        let head = Props { bold: Some(true), color: Some([255, 255, 255, 255]), ..Props::default() };
        TblStyle {
            whole: TblPart { fill: Some(tint(0.2)), borders: lines.clone(), text: Props::default() },
            band: TblPart { fill: Some(tint(0.4)), borders: lines.clone(), text: Props::default() },
            first_row: TblPart { fill: Some([accent[0], accent[1], accent[2], 255]), borders: lines.clone(), text: head.clone() },
            last_row: TblPart { fill: Some([accent[0], accent[1], accent[2], 255]), borders: lines.clone(), text: head.clone() },
            first_col: TblPart { fill: Some([accent[0], accent[1], accent[2], 255]), borders: lines, text: head },
        }
    }

    fn fill(&self, sppr: Option<Node>, style_ref: Option<Node>) -> Fill {
        let found = sppr.and_then(|s| s.children().find(|n| matches!(n.tag_name().name(), "noFill" | "solidFill" | "gradFill" | "blipFill" | "pattFill" | "grpFill") && n.is_element()));
        match found {
            Some(n) => match n.tag_name().name() {
                "noFill" => Fill::None,
                "solidFill" => self.colors.color(n, None).map_or(Fill::None, Fill::Color),
                "gradFill" => n.descendants().find(|g| is(g, "gs")).and_then(|g| self.colors.color(g, None)).map_or(Fill::None, Fill::Color),
                "pattFill" => n.children().find(|g| is(g, "fgClr")).and_then(|g| self.colors.color(g, None)).map_or(Fill::None, Fill::Color),
                "blipFill" => n.descendants().find(|b| is(b, "blip")).and_then(|b| attr(&b, "embed")).map_or(Fill::None, |r| Fill::Picture(r.to_string())),
                _ => Fill::None,
            },
            None => match style_ref.filter(|r| attr(r, "idx").is_some_and(|i| i != "0")) {
                Some(r) => self.colors.color(r, None).map_or(Fill::Unset, Fill::Color),
                None => Fill::Unset,
            },
        }
    }

    /// An outline: its colour and width in 1080p pixels.
    fn outline(&self, sppr: Option<Node>, style_ref: Option<Node>) -> Option<Outline> {
        let ln = sppr.and_then(|s| s.children().find(|n| is(n, "ln")));
        let width = ln.and_then(|l| attr(&l, "w")).and_then(|w| w.parse::<f64>().ok()).unwrap_or(9525.0);
        let width = self.px(width).max(1.0);
        match ln.and_then(|l| l.children().find(|n| matches!(n.tag_name().name(), "noFill" | "solidFill" | "gradFill") && n.is_element())) {
            Some(n) if is(&n, "noFill") => None,
            Some(n) => {
                let c = if is(&n, "solidFill") { self.colors.color(n, None) } else { n.descendants().find(|g| is(g, "gs")).and_then(|g| self.colors.color(g, None)) };
                c.map(|c| Outline { color: css(c), width })
            }
            None => style_ref.filter(|r| attr(r, "idx").is_some_and(|i| i != "0")).and_then(|r| self.colors.color(r, None)).map(|c| Outline { color: css(c), width }),
        }
    }

    fn shape<R: Read + Seek>(&self, io: &mut Io<R>, made: &mut Made, sp: Node<'a, 'i>, groups: &[Grp], rels: &Rels, on_slide: bool) {
        let ph = sp.children().find(|n| is(n, "nvSpPr") || is(n, "nvCxnSpPr")).and_then(|nv| nv.descendants().find(|n| is(n, "ph")));
        // A layout's or master's placeholders are only prompts.
        if ph.is_some() && !on_slide { return }
        let (lay, mas) = ph.map(|p| self.ph_parents(p)).unwrap_or((None, None));
        let chain: Vec<Node> = [Some(sp), lay, mas].into_iter().flatten().collect();
        let sppr_of = |n: &Node<'a, 'i>| n.children().find(|c| is(c, "spPr"));
        let sppr = sp.children().find(|n| is(n, "spPr"));
        let Some((b, _)) = chain.iter().find_map(|n| sppr_of(n).and_then(xfrm)) else { return };
        let b = place(b, groups);
        let geom = chain.iter().filter_map(sppr_of).find_map(|s| s.children().find(|n| is(n, "prstGeom") || is(n, "custGeom")))
            .map(|g| attr(&g, "prst").unwrap_or("custom").to_string()).unwrap_or_else(|| "rect".into());
        let style = sp.children().find(|n| is(n, "style"));
        let sref = |name: &str| style.and_then(|s| s.children().find(|n| is(n, name)));
        let mut fill = Fill::Unset;
        for s in chain.iter().filter_map(sppr_of) {
            fill = self.fill(Some(s), None);
            if fill != Fill::Unset { break }
        }
        if fill == Fill::Unset { fill = self.fill(None, sref("fillRef")) }
        let outline = chain.iter().filter_map(sppr_of).find(|s| s.children().any(|n| is(&n, "ln") && n.has_children())).map_or_else(|| self.outline(sppr, sref("lnRef")), |s| self.outline(Some(s), sref("lnRef")));
        let text = sp.children().find(|n| is(n, "txBody")).map(|tx| self.text(tx, ph, lay, mas, sref("fontRef"), None));
        let text = text.filter(|w| !w.is_empty());
        let rect = self.pct(&b);
        let mut ids = Vec::new();
        let finish = |mut o: Obj, made: &mut Made, ids: &mut Vec<Uuid>| {
            // A block arrow turned a quarter: the file's sides are its other sides.
            let quarter = o.rotation.abs() == 90.0;
            o.rotation += b.rot as f32;
            // Words are not mirrored: flipped top to bottom they are upside down, as in PowerPoint.
            if matches!(o.kind, ObjKind::Text { .. }) {
                if b.flip_v { o.rotation += 180.0 }
            } else if quarter {
                (o.flip_h, o.flip_v) = (b.flip_v, b.flip_h);
            } else {
                (o.flip_h, o.flip_v) = (b.flip_h, b.flip_v);
            }
            ids.push(made.push(o));
        };
        let fill_css = match &fill { Fill::Color(c) => Some(css(*c)), _ => None };
        let line_like = matches!(geom.as_str(), "line" | "straightConnector1") || geom.contains("Connector");
        let known_box = matches!(geom.as_str(), "rect" | "roundRect" | "snip1Rect" | "round1Rect" | "round2SameRect" | "flowChartProcess" | "flowChartAlternateProcess" | "frame" | "plaque");
        if line_like {
            self.line(made, &mut ids, b, sppr, sref("lnRef"));
        } else if let Fill::Picture(rid) = &fill {
            match self.media(io, rels, rid, sppr.and_then(|s| s.descendants().find(|n| is(n, "srcRect")))) {
                Some(asset) => finish(Obj::new(ObjKind::Picture { asset, fit: Fit::Stretch }, rect), made, &mut ids),
                None => return self.lost(made, sp, b, name_of(sp), on_slide),
            }
            if let Some(w) = text { made.words(w.starts); finish(Obj::new(w.kind, rect), made, &mut ids) }
        } else if known_box || geom == "custom" || !matches!(geom.as_str(), "ellipse" | "flowChartConnector" | "triangle" | "rightArrow" | "leftArrow" | "upArrow" | "downArrow") {
            if !known_box { made.lost += 1 }
            let radius = if matches!(geom.as_str(), "roundRect" | "flowChartAlternateProcess") { self.px(b.w.min(b.h) * 0.166_67) } else { 0.0 };
            let look = Look { fill: fill_css, outline, radius, ..Look::default() };
            match text {
                Some(w) => { made.words(w.starts); finish(Obj { look, ..Obj::new(w.kind, rect) }, made, &mut ids) }
                None if look.fill.is_some() || look.outline.is_some() => finish(Obj { look, ..Obj::new(ObjKind::Shape { shape: Shape::Rect }, rect) }, made, &mut ids),
                None => {}
            }
        } else {
            let (shape, turn) = match geom.as_str() {
                "ellipse" | "flowChartConnector" => (Shape::Ellipse, 0.0),
                "triangle" => (Shape::Triangle, 0.0),
                "rightArrow" => (Shape::Arrow, 0.0),
                "leftArrow" => (Shape::Arrow, 180.0),
                "upArrow" => (Shape::Arrow, -90.0),
                _ => (Shape::Arrow, 90.0),
            };
            let mut o = Obj::new(ObjKind::Shape { shape }, rect);
            if shape == Shape::Arrow {
                // The room's arrow is a line: a block arrow becomes a thick one in its colour.
                let color = fill_css.clone().or(outline.as_ref().map(|o| o.color.clone())).unwrap_or_else(|| "#000000".into());
                let thick = self.px(if turn == 0.0 || turn == 180.0 { b.h } else { b.w } * 0.4).clamp(2.0, 100.0);
                if turn != 0.0 && turn != 180.0 {
                    // Turned a quarter, the box swaps its sides about its middle.
                    let c = (b.x + b.w / 2.0, b.y + b.h / 2.0);
                    o.rect = self.pct(&Box_ { x: c.0 - b.h / 2.0, y: c.1 - b.w / 2.0, w: b.h, h: b.w, ..b });
                }
                o.look = Look { outline: Some(Outline { color, width: thick }), ..Look::default() };
            } else {
                o.look = Look { fill: fill_css, outline, ..Look::default() };
            }
            o.rotation = turn;
            finish(o, made, &mut ids);
            if let Some(w) = text { made.words(w.starts); finish(Obj::new(w.kind, rect), made, &mut ids) }
        }
        if on_slide && let Some(s) = shape_id(sp) && !ids.is_empty() { made.ids.entry(s).or_default().extend(ids) }
    }

    /// A straight line or connector: a drawn line from corner to corner (a flat one with an arrow head: the arrow).
    fn line(&self, made: &mut Made, ids: &mut Vec<Uuid>, b: Box_, sppr: Option<Node>, lnref: Option<Node>) {
        let ol = self.outline(sppr, lnref).unwrap_or(Outline { color: "#000000".into(), width: 2.0 });
        let ln = sppr.and_then(|s| s.children().find(|n| is(n, "ln")));
        let head = |end: &str| ln.and_then(|l| l.children().find(|n| is(n, end))).and_then(|e| attr(&e, "type")).is_some_and(|t| t != "none");
        let mut rect = self.pct(&b);
        let flat = rect.h < 1.0;
        if flat && (head("tailEnd") || head("headEnd")) {
            rect = PctRect::new(rect.x, rect.y + rect.h / 2.0 - 1.5, rect.w, 3.0);
            let mut o = Obj::new(ObjKind::Shape { shape: Shape::Arrow }, rect);
            let backwards = head("headEnd") != b.flip_h;
            o.rotation = b.rot as f32 + if backwards { 180.0 } else { 0.0 };
            o.look = Look { outline: Some(ol), ..Look::default() };
            ids.push(made.push(o));
            return;
        }
        let (mut a, mut z) = ((0.0f32, 0.0f32), (1.0f32, 1.0f32));
        if rect.h < 1.0 { rect = PctRect::new(rect.x, rect.y + rect.h / 2.0 - 0.5, rect.w, 1.0); a.1 = 0.5; z.1 = 0.5 }
        if rect.w < 1.0 { rect = PctRect::new(rect.x + rect.w / 2.0 - 0.5, rect.y, 1.0, rect.h); a.0 = 0.5; z.0 = 0.5 }
        if b.flip_h { std::mem::swap(&mut a.0, &mut z.0) }
        if b.flip_v { std::mem::swap(&mut a.1, &mut z.1) }
        let mut o = Obj::new(ObjKind::Path { points: vec![a, z], width: ol.width, color: ol.color }, rect);
        o.rotation = b.rot as f32;
        ids.push(made.push(o));
    }

    fn picture<R: Read + Seek>(&self, io: &mut Io<R>, made: &mut Made, pic: Node<'a, 'i>, groups: &[Grp], rels: &Rels, on_slide: bool) {
        let ph = pic.descendants().find(|n| is(n, "ph"));
        let (lay, mas) = ph.map(|p| self.ph_parents(p)).unwrap_or((None, None));
        let sppr = pic.children().find(|n| is(n, "spPr"));
        let Some((b, _)) = [sppr, lay.and_then(|l| l.children().find(|n| is(n, "spPr"))), mas.and_then(|m| m.children().find(|n| is(n, "spPr")))].into_iter().flatten().find_map(xfrm) else { return };
        let b = place(b, groups);
        let blip_fill = pic.children().find(|n| is(n, "blipFill"));
        let blip = blip_fill.and_then(|f| f.children().find(|n| is(n, "blip")));
        let crop = blip_fill.and_then(|f| f.children().find(|n| is(n, "srcRect")));
        let picture = blip.and_then(|bl| attr(&bl, "embed")).and_then(|rid| self.media(io, rels, rid, crop));
        // A video: PowerPoint 2010's own media part, else the old video link.
        let video = pic.descendants().find(|n| is(n, "media")).and_then(|m| attr(&m, "embed"))
            .or_else(|| pic.descendants().find(|n| is(n, "videoFile")).and_then(|v| attr(&v, "link")));
        let kind = match video {
            Some(rid) => match self.video(io, rels, rid) {
                Some(asset) => ObjKind::Video { asset, thumb: picture, looped: false },
                None => return self.lost(made, pic, b, name_of(pic), on_slide),
            },
            None => match picture {
                Some(asset) => ObjKind::Picture { asset, fit: Fit::Stretch },
                None => return self.lost(made, pic, b, name_of(pic), on_slide),
            },
        };
        let mut o = Obj::new(kind, self.pct(&b));
        o.rotation = b.rot as f32;
        (o.flip_h, o.flip_v) = (b.flip_h, b.flip_v);
        if let Some(a) = blip.and_then(|bl| bl.children().find(|n| is(n, "alphaModFix"))).and_then(|a| attr(&a, "amt")).and_then(|v| v.parse::<f64>().ok()) {
            o.opacity = (a / 1000.0).round().clamp(0.0, 100.0) as u8;
        }
        o.look.outline = self.outline(sppr, None);
        let id = made.push(o);
        if on_slide && let Some(s) = shape_id(pic) { made.ids.entry(s).or_default().push(id); }
    }

    /// A picture part into the store, cut by its crop (`a:srcRect`, thousandths of a percent from each side).
    fn media<R: Read + Seek>(&self, io: &mut Io<R>, rels: &Rels, rid: &str, crop: Option<Node>) -> Option<AssetId> {
        let (_, part) = rels.get(rid)?;
        let ext = part.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if !PICTURE_EXT.contains(&ext.as_str()) { return None }
        let bytes = io.p.bytes(part)?;
        let side = |a: &str| crop.and_then(|c| attr(&c, a)).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) / 100_000.0;
        let (l, t, r, b) = (side("l"), side("t"), side("r"), side("b"));
        if (l, t, r, b) != (0.0, 0.0, 0.0, 0.0) && let Ok(img) = image::load_from_memory(&bytes) {
            let (w, h) = (img.width() as f64, img.height() as f64);
            let (x0, y0) = ((w * l.max(0.0)).round(), (h * t.max(0.0)).round());
            let (x1, y1) = ((w * (1.0 - r.max(0.0))).round(), (h * (1.0 - b.max(0.0))).round());
            if x1 - x0 >= 1.0 && y1 - y0 >= 1.0 {
                let cut = img.crop_imm(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
                let mut png = Vec::new();
                if cut.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_ok() { return (io.put)(&png, "png") }
            }
        }
        (io.put)(&bytes, &ext)
    }

    fn video<R: Read + Seek>(&self, io: &mut Io<R>, rels: &Rels, rid: &str) -> Option<AssetId> {
        let (_, part) = rels.get(rid)?;
        let ext = part.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        if !super::media::VIDEO.contains(&ext.as_str()) { return None }
        let bytes = io.p.bytes_max(part, MAX_VIDEO)?;
        (io.put)(&bytes, &ext)
    }

    /// The slide's background (its own, else its layout's, else its master's) as a picture or a box filling the
    /// slide, below everything. White when none says (as PowerPoint).
    fn background<R: Read + Seek>(&self, io: &mut Io<R>, made: &mut Made, parts: &[(Option<Node>, &Rels)]) {
        let full = PctRect::new(0.0, 0.0, 100.0, 100.0);
        for (root, rels) in parts {
            let Some(bg) = root.and_then(|r| r.children().find(|n| is(n, "cSld"))).and_then(|c| c.children().find(|n| is(n, "bg"))) else { continue };
            let fill = match bg.children().find(|n| n.is_element()) {
                Some(p) if is(&p, "bgPr") => self.fill(Some(p), None),
                Some(r) if is(&r, "bgRef") => self.colors.color(r, None).map_or(Fill::Unset, Fill::Color),
                _ => Fill::Unset,
            };
            match fill {
                Fill::Color(c) => { let mut o = Obj::new(ObjKind::Shape { shape: Shape::Rect }, full); o.look.fill = Some(css(c)); made.push(o); return }
                Fill::Picture(rid) => if let Some(asset) = self.media(io, rels, &rid, None) { made.push(Obj::new(ObjKind::Picture { asset, fit: Fit::Fill }, full)); return },
                Fill::None => return,
                Fill::Unset => {}
            }
        }
        let mut o = Obj::new(ObjKind::Shape { shape: Shape::Rect }, full);
        o.look.fill = Some("#ffffff".into());
        made.push(o);
    }

    /// A text body's paragraphs (a line break starts another), each run in its own look, and the box's look: the
    /// look of most of its words. `Words::starts` maps the file's paragraphs to ours (for paragraph builds).
    fn text(&self, tx: Node, ph: Option<Node>, lay: Option<Node>, mas: Option<Node>, font_ref: Option<Node>, extra: Option<&Props>) -> Words {
        let class = ph.map(|p| ph_class(attr(&p, "type").unwrap_or("obj")));
        let base_levels = match class { Some("title") => &self.title, Some("body") => &self.body, Some(_) => &self.other, None => &self.deflt };
        let lst = |n: Option<Node>| levels(n.and_then(|n| n.children().find(|c| is(c, "txBody"))).and_then(|t| t.children().find(|c| is(c, "lstStyle"))), &self.colors);
        let chain = [lst(mas), lst(lay)];
        let own = levels(tx.children().find(|c| is(c, "lstStyle")), &self.colors);
        let base = Props { size: Some(18.0), color: Some([0, 0, 0, 255]), align: Some(HAlign::Left), ..Props::default() };
        // A shape's own font colour (PowerPoint's default shapes) beats the presentation's.
        let font_color = font_ref.and_then(|f| self.colors.color(f, None));
        let prs: Vec<Node> = [tx.children().find(|c| is(c, "bodyPr")), body_pr(lay), body_pr(mas)].into_iter().flatten().collect();
        let anchor = prs.iter().find_map(|b| attr(b, "anchor")).unwrap_or("t");
        let fit = prs.iter().find_map(|b| b.children().find(|c| matches!(c.tag_name().name(), "normAutofit" | "noAutofit" | "spAutoFit") && c.is_element()));
        let scale = fit.filter(|f| is(f, "normAutofit")).and_then(|f| attr(&f, "fontScale")).and_then(|v| v.parse::<f64>().ok()).map_or(1.0, |v| v / 100_000.0);
        let size_px = |pt: f64| self.px(pt * scale * EMU_PER_PT).clamp(crate::design::SIZE_MIN, crate::design::SIZE_MAX);

        // (paragraph look, its runs as (words, look)), per line.
        let mut lines: Vec<(Props, u8, String, Vec<(String, Props)>)> = Vec::new();
        let mut starts = Vec::new();
        let (mut counts, mut align, mut number) = (Vec::<(usize, Props)>::new(), None, 0);
        for p in tx.children().filter(|n| is(n, "p")) {
            let ppr_n = p.children().find(|n| is(n, "pPr"));
            let lvl = ppr_n.and_then(|n| attr(&n, "lvl")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0).min(8);
            let mut pp = base.clone();
            pp.over(&base_levels[lvl]);
            for l in &chain { pp.over(&l[lvl]) }
            if font_color.is_some() { pp.color = font_color }
            if let Some(e) = extra { pp.over(e) }
            pp.over(&own[lvl]);
            if let Some(n) = ppr_n { pp.over(&ppr(n, &self.colors)) }
            let first = lines.len();
            let mut runs: Vec<(String, Props)> = Vec::new();
            let mut cut: Vec<Vec<(String, Props)>> = Vec::new();
            for r in p.children().filter(|n| n.is_element()) {
                match r.tag_name().name() {
                    "r" | "fld" => {
                        let t: String = r.children().filter(|n| is(n, "t")).filter_map(|n| n.text()).collect();
                        let mut rp = pp.clone();
                        if let Some(n) = r.children().find(|n| is(n, "rPr")) { rp.over(&rpr(n, &self.colors)) }
                        counts.push((t.replace('\u{b}', "").trim().chars().count(), rp.run_look()));
                        for (k, piece) in t.split('\u{b}').enumerate() {
                            if k > 0 { cut.push(std::mem::take(&mut runs)) }
                            if !piece.is_empty() { runs.push((piece.to_string(), rp.clone())) }
                        }
                    }
                    "br" => cut.push(std::mem::take(&mut runs)),
                    _ => {}
                }
            }
            cut.push(runs);
            let empty = cut.iter().all(|r| r.iter().all(|(t, _)| t.trim().is_empty()));
            let mark = if empty { number = 0; String::new() } else {
                if align.is_none() { align = pp.align }
                match pp.bullet {
                    Some(Bullet::Char) => { number = 0; "•".to_string() }
                    Some(Bullet::Number) => { number += 1; format!("{number}.") }
                    _ => { number = 0; String::new() }
                }
            };
            for (k, mut runs) in cut.into_iter().enumerate() {
                // Trailing spaces go, as they did when words were one line.
                if let Some((t, _)) = runs.last_mut() { let n = t.trim_end().len(); t.truncate(n) }
                runs.retain(|(t, _)| !t.is_empty());
                lines.push((pp.clone(), lvl as u8, if k == 0 { mark.clone() } else { String::new() }, runs));
            }
            starts.push((first, lines.len() - 1));
        }
        // The look of the most words (the first such on a tie).
        let mut tally: Vec<(usize, Props)> = Vec::new();
        for (n, p) in counts {
            match tally.iter_mut().find(|(_, q)| *q == p) {
                Some((m, _)) => *m += n,
                None => tally.push((n, p)),
            }
        }
        let most = tally.iter().map(|(n, _)| *n).max().unwrap_or(0);
        let st = tally.into_iter().find(|(n, _)| *n == most).map(|(_, p)| p).unwrap_or(base);
        let font_of = |f: Option<&str>| match f {
            Some(f) if f.starts_with("+mj") => self.major.clone(),
            Some(f) if f.starts_with("+mn") => self.minor.clone(),
            Some(f) => f.to_string(),
            None => self.minor.clone(),
        };
        let style = TextStyle {
            font: font_of(st.font.as_deref()),
            size: size_px(st.size.unwrap_or(18.0)),
            color: css(st.color.unwrap_or([0, 0, 0, 255])),
            bold: st.bold.unwrap_or(false),
            italic: st.italic.unwrap_or(false),
            underline: st.underline.unwrap_or(false),
            strike: st.strike.unwrap_or(false),
            align: align.unwrap_or(HAlign::Left),
            valign: match anchor { "ctr" => VAlign::Middle, "b" => VAlign::Bottom, _ => VAlign::Top },
            shrink: fit.is_some_and(|f| is(&f, "normAutofit")),
            line: 1.0,
        };
        let mut paras: Vec<Para> = lines.into_iter().enumerate().map(|(i, (pp, level, bullet, runs))| {
            let first_pt = runs.first().and_then(|(_, r)| r.size).or(pp.size).unwrap_or(18.0);
            let space = |s: Option<Spacing>| match s { Some(Spacing::Pts(v)) => size_px(v), Some(Spacing::Pct(v)) => size_px(first_pt * v), None => 0.0 };
            let cont = starts.iter().any(|(a, b)| i > *a && i <= *b);
            let last_of = starts.iter().any(|(_, b)| i == *b);
            let line = match pp.line { Some(Spacing::Pct(v)) => v as f32, Some(Spacing::Pts(v)) => (v / (first_pt * 1.2)) as f32, None => 1.0 };
            let runs = runs.into_iter().map(|(text, rp)| {
                let mut r = Run {
                    text,
                    font: Some(font_of(rp.font.as_deref())),
                    size: Some(size_px(rp.size.unwrap_or(18.0))),
                    color: Some(css(rp.color.unwrap_or([0, 0, 0, 255]))),
                    bold: Some(rp.bold.unwrap_or(false)),
                    italic: Some(rp.italic.unwrap_or(false)),
                    underline: Some(rp.underline.unwrap_or(false)),
                    strike: Some(rp.strike.unwrap_or(false)),
                };
                r.simplify(&style);
                r
            }).collect();
            Para {
                runs,
                align: pp.align.filter(|a| *a != style.align),
                line: Some(line).filter(|l| (l - 1.0).abs() > 0.01),
                before: if cont { 0.0 } else { space(pp.before) },
                after: if last_of { space(pp.after) } else { 0.0 },
                level,
                bullet,
                hidden: false,
            }
        }).collect();
        // Empty lines at either end go (the words' box stays where it is).
        let lead = paras.iter().take_while(|p| p.words().is_empty() && p.bullet.is_empty()).count();
        paras.drain(..lead);
        while paras.last().is_some_and(|p| p.words().is_empty() && p.bullet.is_empty()) { paras.pop(); }
        let n = paras.len();
        let starts = starts.into_iter().map(|(a, b)| (a.saturating_sub(lead).min(n.saturating_sub(1)), b.saturating_sub(lead).min(n.saturating_sub(1)))).collect();
        Words { kind: ObjKind::rich(paras, style.clone()), style, starts }
    }

    /// The slide's main animation sequence as steps. Steps that start on a shape's own click (triggers) are left out.
    fn anims(&self, sld: Node, made: &Made) -> Vec<Anim> {
        let Some(main) = sld.descendants().find(|n| is(n, "cTn") && attr(n, "nodeType") == Some("mainSeq")) else { return vec![] };
        let mut out = Vec::new();
        for ctn in main.descendants().filter(|n| is(n, "cTn") && attr(n, "presetClass").is_some()) {
            let Some(tgt) = ctn.descendants().find(|n| is(n, "spTgt")) else { continue };
            let Some(objs) = attr(&tgt, "spid").and_then(|s| made.ids.get(s)) else { continue };
            // "By paragraph": only the text box's paragraphs `st`..`end` (the file's numbering).
            let prg = tgt.descendants().find(|n| is(n, "pRg")).and_then(|r| Some((attr(&r, "st")?.parse::<usize>().ok()?, attr(&r, "end")?.parse::<usize>().ok()?)));
            let para_target = prg.and_then(|(st, end)| objs.iter().find_map(|o| {
                let starts = made.paras.get(o)?;
                let (a, b) = (starts.get(st)?.0, starts.get(end.max(st)).or(starts.last())?.1);
                Some((*o, (a.min(usize::from(u16::MAX)) as u16, b.min(usize::from(u16::MAX)) as u16)))
            }));
            let one;
            let objs: &[Uuid] = match para_target { Some((o, _)) => { one = [o]; &one } None => objs };
            let start = match attr(&ctn, "nodeType") { Some("clickEffect") => Start::OnClick, Some("afterEffect") => Start::AfterPrevious, _ => Start::WithPrevious };
            let ms = |v: Option<&str>| v.and_then(|v| v.trim().parse::<u32>().ok());
            let delay = ctn.children().find(|n| is(n, "stCondLst")).and_then(|c| c.children().find(|n| is(n, "cond"))).and_then(|c| ms(attr(&c, "delay"))).unwrap_or(0);
            let dur = ctn.descendants().skip(1).filter(|n| is(n, "cTn")).filter_map(|n| ms(attr(&n, "dur"))).max().unwrap_or(500);
            let class = attr(&ctn, "presetClass").unwrap_or("");
            let id = ms(attr(&ctn, "presetID")).unwrap_or(0);
            let sub = ms(attr(&ctn, "presetSubtype")).unwrap_or(0);
            let Some(mut a) = self.preset(ctn, class, id, sub) else { continue };
            a.delay_ms = delay;
            a.dur_ms = if a.effect == Effect::Appear { 0 } else { dur };
            for (k, obj) in objs.iter().enumerate() {
                out.push(Anim { obj: *obj, start: if k == 0 { start } else { Start::WithPrevious }, para: para_target.map(|(_, r)| r), ..a.clone() });
            }
        }
        out
    }

    /// PowerPoint's preset effect as ours (object and start set later). See the table in the tests.
    fn preset(&self, ctn: Node, class: &str, id: u32, sub: u32) -> Option<Anim> {
        let dir = if sub & 4 != 0 { Some(Dir::Down) } else if sub & 1 != 0 { Some(Dir::Up) } else if sub & 8 != 0 { Some(Dir::Left) } else if sub & 2 != 0 { Some(Dir::Right) } else { None };
        let mut a = Anim::new(Uuid::nil(), AnimKind::Entrance, Effect::Fade, Start::OnClick);
        match class {
            "entr" | "exit" => {
                a.kind = if class == "entr" { AnimKind::Entrance } else { AnimKind::Exit };
                a.effect = match id {
                    1 => Effect::Appear,
                    2 | 7 => Effect::Fly,
                    12 | 22 => Effect::Wipe,
                    16 => Effect::Split,
                    23 | 31 | 53 | 55 => Effect::Zoom,
                    30 | 42 => Effect::Float,
                    47 => { a.dir = Some(Dir::Up); Effect::Float }
                    _ => Effect::Fade,
                };
                if a.effect.has_dir() && a.dir.is_none() { a.dir = dir }
            }
            "emph" => {
                a.kind = AnimKind::Emphasis;
                let by = |name: &str| ctn.descendants().find(|n| is(n, name));
                a.effect = match id {
                    8 => {
                        a.amount = by("animRot").and_then(|r| attr(&r, "by")).and_then(|v| v.parse::<f32>().ok()).map(|v| v / 60000.0);
                        Effect::Spin
                    }
                    6 => {
                        a.amount = by("animScale").and_then(|s| s.children().find(|n| is(n, "by"))).and_then(|b| attr(&b, "x")).and_then(|v| v.parse::<f32>().ok()).map(|v| v / 1000.0);
                        Effect::Grow
                    }
                    _ => Effect::Pulse,
                };
            }
            "path" => {
                a.kind = AnimKind::Motion;
                a.effect = Effect::Path;
                let path = ctn.descendants().find(|n| is(n, "animMotion")).and_then(|m| attr(&m, "path")).and_then(MotionPath::parse_ppt)?;
                a.path = Some(self.slide_path(path));
            }
            _ => return None,
        }
        Some(a)
    }

    /// A motion path in fractions of the file's slide, onto ours (the same unless the slide is not 16:9).
    fn slide_path(&self, mut p: MotionPath) -> MotionPath {
        let g = self.geo;
        let (kx, ky) = ((g.cx * g.s / W) as f32, (g.cy * g.s / H) as f32);
        let k = |v: (f32, f32)| (v.0 * kx, v.1 * ky);
        for s in &mut p.segs {
            *s = match *s {
                Seg::Move { to } => Seg::Move { to: k(to) },
                Seg::Line { to } => Seg::Line { to: k(to) },
                Seg::Cubic { c1, c2, to } => Seg::Cubic { c1: k(c1), c2: k(c2), to: k(to) },
            };
        }
        p
    }
}

/// A text body as we have it.
struct Words {
    /// `ObjKind::Text`.
    kind: ObjKind,
    style: TextStyle,
    /// Each of the file's paragraphs: our first and last.
    starts: Vec<(usize, usize)>,
}

impl Words {
    fn is_empty(&self) -> bool { matches!(&self.kind, ObjKind::Text { text, .. } if text.trim().is_empty()) }
}

fn tree<'a, 'i>(n: Option<Node<'a, 'i>>) -> Option<Node<'a, 'i>> { n.and_then(|n| n.descendants().find(|c| is(c, "spTree"))) }

fn body_pr<'a, 'i>(n: Option<Node<'a, 'i>>) -> Option<Node<'a, 'i>> {
    n.and_then(|n| n.children().find(|c| is(c, "txBody"))).and_then(|t| t.children().find(|c| is(c, "bodyPr")))
}

/// The file's own id of a shape (`p:cNvPr id`), which animations point at.
fn shape_id(n: Node) -> Option<String> {
    n.children().find(|c| c.tag_name().name().starts_with("nv")).and_then(|nv| nv.children().find(|c| is(c, "cNvPr"))).and_then(|c| attr(&c, "id")).map(str::to_string)
}

/// What a lost object's grey box says: its description, else its name.
fn name_of(n: Node) -> String {
    let c = n.descendants().find(|c| is(c, "cNvPr"));
    c.and_then(|c| attr(&c, "descr").filter(|d| !d.trim().is_empty()).or(attr(&c, "name"))).unwrap_or("").to_string()
}

fn only_pptx() -> String { crate::tr!("Only .pptx and .odp files can come in as editable slides. Add it as pictures instead.") }

/// The job's note when some objects came over only as grey boxes.
pub fn lost_note(n: usize) -> String {
    crate::trn!("{n} object couldn't be converted; use Pictures for an exact look.", "{n} objects couldn't be converted; use Pictures for an exact look.", n)
}

/// Starts importing `file` (the staged copy, which this import owns: a .pptx, or an .odp through `odp_design`) as
/// one item of designed slides at the end of the open service, and returns the job. The job's result is the new
/// item's id.
pub fn start(app: Arc<App>, file: PathBuf, display: String) -> Uuid {
    let (job, cancel) = app.job_start_cancellable(&super::strings::importing(&display));
    tokio::spawn(async move {
        let result = run(&app, job, &file, display, &cancel).await;
        let _ = std::fs::remove_file(&file);
        match result {
            Ok(()) => app.job_end(job, None),
            Err(e) if e == CANCELLED => app.job_end_cancelled(job),
            Err(e) => app.job_end(job, Some(e)),
        }
    });
    job
}

async fn run(app: &Arc<App>, job: Uuid, file: &Path, title: String, cancel: &CancellationToken) -> Result<(), String> {
    let (a, f, c) = (app.clone(), file.to_path_buf(), cancel.clone());
    let made = tokio::task::spawn_blocking(move || -> Result<(Vec<Design>, Option<String>), String> {
        match sniff::sniff(&f) {
            Ok(Kind::Ooxml { .. }) => {}
            Ok(Kind::Odp) => {
                let odp = super::odp_design::odp_to_designs(&f, &a.store)?;
                let note = (odp.placeholders > 0).then(|| super::strings::odp_placeholders(odp.placeholders));
                return Ok((odp.designs, note));
            }
            Ok(Kind::Encrypted) => return Err(sniff::encrypted()),
            _ => return Err(only_pptx()),
        }
        a.job_progress(job, 5);
        let work = WorkDir::new(&a.store.tmp()).map_err(|_| slide_not_saved())?;
        let mut n = 0;
        let mut put = |bytes: &[u8], ext: &str| -> Option<AssetId> {
            if c.is_cancelled() { return None }
            n += 1;
            let p = work.path().join(format!("media-{n}.{ext}"));
            std::fs::write(&p, bytes).ok()?;
            a.store.import_file(&p, ext).ok()
        };
        let made = read(&f, &mut put).ok_or_else(sniff::not_presentation)?;
        if c.is_cancelled() { return Err(CANCELLED.into()) }
        Ok((made.slides, (made.lost > 0).then(|| lost_note(made.lost))))
    }).await.map_err(|_| slide_not_saved())??;
    let (mut slides, note) = made;
    if slides.is_empty() { return Err(pdf_no_slides()) }
    slides.truncate(MAX_SLIDES);
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Design { title, slides }, background: None, notes: String::new(), theme: None };
    let id = item.id;
    app.edit(|d| { d.service_mut().ok_or_else(|| crate::tr!("No service is open."))?.items.push(item); Ok(()) })?;
    app.job_result(job, id);
    if let Some(note) = note { app.job_note(job, note) }
    Ok(())
}

#[cfg(test)]
#[path = "pptx_design_tests.rs"]
mod tests;
