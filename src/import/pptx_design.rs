//! 1.1: a PowerPoint file (.pptx, and Google Slides' "Download as PowerPoint") as designed slides the slide editor
//! can change: text boxes, shapes, lines, pictures, videos, backgrounds, notes, the transition and the object
//! animations. Placeholders take their place and text style from the slide's layout and master; theme colours and
//! fonts are looked up. Slides are put on the 16:9 slide as large as they fit (a 4:3 deck sits in the middle).
//!
//! What has no match here (tables, charts, SmartArt, odd shapes, pictures we can't draw) becomes a grey box with
//! its name and any words it has, and is counted in `Converted::lost`, so the operator can choose Pictures instead.
//! Text boxes have one style: the style of most of their words. Paragraph builds animate the whole text box.
use super::notes::{self, attr, is, Package};
use super::sniff::MAX_PART;
use crate::design::{
    Anim, AnimKind, Design, Dir, Effect, Fit, HAlign, Look, MotionPath, Obj, ObjKind, Outline, PctRect, Seg, Shape, Start,
    TextStyle, Transition, TransitionKind, VAlign, MAX_OBJECTS,
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
    };
    let mut made = Made::default();
    cv.background(io, &mut made, &[(Some(sld.root_element()), &slide_rels), (layout, &layout_rels), (master, &master_rels)]);
    let shows = |n: Option<Node>| n.is_none_or(|n| !matches!(attr(&n, "showMasterSp"), Some("0" | "false")));
    if shows(Some(sld.root_element())) {
        if shows(layout) && let Some(t) = tree(master) { cv.walk(io, &mut made, t, &mut vec![], &master_rels, false) }
        if let Some(t) = tree(layout) { cv.walk(io, &mut made, t, &mut vec![], &layout_rels, false) }
    }
    if let Some(t) = tree(Some(sld.root_element())) { cv.walk(io, &mut made, t, &mut vec![], &slide_rels, true) }

    let transition = super::transitions::slide_transition(&sld).unwrap_or(Transition { kind: TransitionKind::Cut, ..Transition::default() });
    let anims = cv.anims(sld.root_element(), &made.ids);
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
    lost: usize,
}

impl Made {
    fn push(&mut self, o: Obj) -> Uuid { let id = o.id; self.objs.push(o); id }
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
    color: Option<[u8; 4]>,
    font: Option<String>,
    align: Option<HAlign>,
    /// Some(None): a bullet; Some(Some(_)): numbered; None: not set. `no_bullet` turns it off.
    bullet: Option<Bullet>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Bullet { None, Char, Number }

impl Props {
    fn over(&mut self, o: &Props) {
        if o.size.is_some() { self.size = o.size }
        if o.bold.is_some() { self.bold = o.bold }
        if o.italic.is_some() { self.italic = o.italic }
        if o.color.is_some() { self.color = o.color }
        if o.font.is_some() { self.font = o.font.clone() }
        if o.align.is_some() { self.align = o.align }
        if o.bullet.is_some() { self.bullet = o.bullet }
    }
}

/// The nine levels of a list style (`lvl1pPr`…).
type Levels = Vec<Props>;

fn levels(n: Option<Node>, c: &Colors) -> Levels {
    (1..=9).map(|i| n.and_then(|n| n.children().find(|k| is(k, &format!("lvl{i}pPr")))).map(|p| ppr(p, c)).unwrap_or_default()).collect()
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
        color: n.children().find(|k| is(k, "solidFill")).and_then(|f| c.color(f, None)),
        font: n.children().find(|k| is(k, "latin")).and_then(|l| attr(&l, "typeface")).map(str::to_string),
        ..Props::default()
    }
}

/// The theme's colours, by scheme name, and the master's map from `tx1`/`bg1`… to them.
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
}

/// A placeholder's kind as the master knows it.
fn ph_class(ty: &str) -> &str {
    match ty {
        "title" | "ctrTitle" => "title",
        "dt" | "ftr" | "sldNum" | "hdr" => ty,
        _ => "body",
    }
}

impl<'a, 'i> Conv<'_, 'a, 'i> {
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
                "graphicFrame" => self.frame(made, c, groups, on_slide),
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
        let style = TextStyle { size: 28.0, color: "#374151".into(), align: HAlign::Center, valign: VAlign::Middle, shrink: true, ..TextStyle::default() };
        let mut o = Obj::new(ObjKind::Text { text, style }, self.pct(&b));
        o.rotation = b.rot as f32;
        o.look = Look { fill: Some("#e5e7eb".into()), outline: Some(Outline { color: "#9ca3af".into(), width: 2.0 }), ..Look::default() };
        let id = made.push(o);
        if on_slide && let Some(s) = shape_id(node) { made.ids.entry(s).or_default().push(id); }
    }

    /// A table, chart, SmartArt or embedded object: a grey box with its name (a table's words).
    fn frame(&self, made: &mut Made, f: Node, groups: &[Grp], on_slide: bool) {
        let Some((b, _)) = xfrm(f) else { return };
        let b = place(b, groups);
        let nv = f.descendants().find(|n| is(n, "cNvPr"));
        let name = nv.and_then(|n| attr(&n, "descr").filter(|d| !d.trim().is_empty()).or(attr(&n, "name"))).unwrap_or("").to_string();
        let rows: Vec<String> = f.descendants().filter(|n| is(n, "tr")).map(|tr| {
            tr.children().filter(|n| is(n, "tc")).map(|tc| tc.descendants().filter(|n| is(n, "t")).filter_map(|t| t.text()).collect::<String>()).collect::<Vec<_>>().join("  |  ")
        }).collect();
        let text = if rows.iter().any(|r| !r.trim_matches([' ', '|']).is_empty()) { rows.join("\n") } else { name };
        self.lost(made, f, b, text, on_slide);
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
        let text = sp.children().find(|n| is(n, "txBody")).map(|tx| self.text(tx, ph, lay, mas, sref("fontRef")));
        let text = text.filter(|(t, _)| !t.trim().is_empty());
        let rect = self.pct(&b);
        let mut ids = Vec::new();
        let finish = |mut o: Obj, made: &mut Made, ids: &mut Vec<Uuid>| {
            o.rotation += b.rot as f32;
            if b.flip_h && b.flip_v { o.rotation += 180.0 }
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
            if let Some((t, st)) = text { finish(Obj::new(ObjKind::Text { text: t, style: st }, rect), made, &mut ids) }
        } else if known_box || geom == "custom" || !matches!(geom.as_str(), "ellipse" | "flowChartConnector" | "triangle" | "rightArrow" | "leftArrow" | "upArrow" | "downArrow") {
            if !known_box { made.lost += 1 }
            let radius = if matches!(geom.as_str(), "roundRect" | "flowChartAlternateProcess") { self.px(b.w.min(b.h) * 0.166_67) } else { 0.0 };
            let look = Look { fill: fill_css, outline, radius, ..Look::default() };
            match text {
                Some((t, st)) => finish(Obj { look, ..Obj::new(ObjKind::Text { text: t, style: st }, rect) }, made, &mut ids),
                None if look.fill.is_some() || look.outline.is_some() => finish(Obj { look, ..Obj::new(ObjKind::Shape { shape: Shape::Rect }, rect) }, made, &mut ids),
                None => {}
            }
        } else {
            let (shape, turn) = match geom.as_str() {
                "ellipse" | "flowChartConnector" => (Shape::Ellipse, 0.0),
                "triangle" => (Shape::Triangle, if b.flip_v && !b.flip_h { 180.0 } else { 0.0 }),
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
            if let Some((t, st)) = text { finish(Obj::new(ObjKind::Text { text: t, style: st }, rect), made, &mut ids) }
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
        o.rotation = b.rot as f32 + if b.flip_h && b.flip_v { 180.0 } else { 0.0 };
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
        let bytes = io.p.bytes(part, MAX_PART)?;
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
        let bytes = io.p.bytes(part, MAX_VIDEO)?;
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

    /// A text body's words (paragraphs on lines, bullets as "• ", numbers as "1. ") and the style of most of them.
    fn text(&self, tx: Node, ph: Option<Node>, lay: Option<Node>, mas: Option<Node>, font_ref: Option<Node>) -> (String, TextStyle) {
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

        let (mut lines, mut counts, mut align, mut number) = (Vec::new(), Vec::<(usize, Props)>::new(), None, 0);
        for p in tx.children().filter(|n| is(n, "p")) {
            let ppr_n = p.children().find(|n| is(n, "pPr"));
            let lvl = ppr_n.and_then(|n| attr(&n, "lvl")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0).min(8);
            let mut pp = base.clone();
            pp.over(&base_levels[lvl]);
            for l in &chain { pp.over(&l[lvl]) }
            if font_color.is_some() { pp.color = font_color }
            pp.over(&own[lvl]);
            if let Some(n) = ppr_n { pp.over(&ppr(n, &self.colors)) }
            let mut line = String::new();
            for r in p.children().filter(|n| n.is_element()) {
                match r.tag_name().name() {
                    "r" | "fld" => {
                        let t: String = r.children().filter(|n| is(n, "t")).filter_map(|n| n.text()).collect::<String>().replace('\u{b}', "\n");
                        let mut rp = pp.clone();
                        if let Some(n) = r.children().find(|n| is(n, "rPr")) { rp.over(&rpr(n, &self.colors)) }
                        counts.push((t.trim().chars().count(), rp));
                        line.push_str(&t);
                    }
                    "br" => line.push('\n'),
                    _ => {}
                }
            }
            if line.trim().is_empty() { number = 0; lines.push(String::new()); continue }
            if align.is_none() { align = pp.align }
            match pp.bullet {
                Some(Bullet::Char) => { number = 0; line.insert_str(0, "• ") }
                Some(Bullet::Number) => { number += 1; line.insert_str(0, &format!("{number}. ")) }
                _ => number = 0,
            }
            lines.push(line.trim_end().to_string());
        }
        let text = lines.join("\n").trim_matches('\n').to_string();
        // The style of the most words (the first such on a tie).
        let mut tally: Vec<(usize, Props)> = Vec::new();
        for (n, p) in counts {
            match tally.iter_mut().find(|(_, q)| *q == p) {
                Some((m, _)) => *m += n,
                None => tally.push((n, p)),
            }
        }
        let most = tally.iter().map(|(n, _)| *n).max().unwrap_or(0);
        let st = tally.into_iter().find(|(n, _)| *n == most).map(|(_, p)| p).unwrap_or(base);
        let font = match st.font.as_deref() {
            Some(f) if f.starts_with("+mj") => self.major.clone(),
            Some(f) if f.starts_with("+mn") => self.minor.clone(),
            Some(f) => f.to_string(),
            None => self.minor.clone(),
        };
        let style = TextStyle {
            font,
            size: self.px(st.size.unwrap_or(18.0) * scale * EMU_PER_PT).clamp(crate::design::SIZE_MIN, crate::design::SIZE_MAX),
            color: css(st.color.unwrap_or([0, 0, 0, 255])),
            bold: st.bold.unwrap_or(false),
            italic: st.italic.unwrap_or(false),
            align: align.unwrap_or(HAlign::Left),
            valign: match anchor { "ctr" => VAlign::Middle, "b" => VAlign::Bottom, _ => VAlign::Top },
            shrink: fit.is_some_and(|f| is(&f, "normAutofit")),
        };
        (text, style)
    }

    /// The slide's main animation sequence as steps. Steps that start on a shape's own click (triggers) are left out.
    fn anims(&self, sld: Node, ids: &HashMap<String, Vec<Uuid>>) -> Vec<Anim> {
        let Some(main) = sld.descendants().find(|n| is(n, "cTn") && attr(n, "nodeType") == Some("mainSeq")) else { return vec![] };
        let mut out = Vec::new();
        for ctn in main.descendants().filter(|n| is(n, "cTn") && attr(n, "presetClass").is_some()) {
            let Some(objs) = ctn.descendants().find(|n| is(n, "spTgt")).and_then(|t| attr(&t, "spid")).and_then(|s| ids.get(s)) else { continue };
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
                out.push(Anim { obj: *obj, start: if k == 0 { start } else { Start::WithPrevious }, ..a.clone() });
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

fn only_pptx() -> String { crate::tr!("Only PowerPoint files saved as .pptx can come in as editable slides. Add it as pictures instead.") }

/// The job's note when some objects came over only as grey boxes.
pub fn lost_note(n: usize) -> String {
    crate::trn!("{n} object couldn't be converted; use Pictures for an exact look.", "{n} objects couldn't be converted; use Pictures for an exact look.", n)
}

/// Starts importing `file` (the staged copy, which this import owns) as one item of designed slides at the end of
/// the open service, and returns the job. The job's result is the new item's id.
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
    let made = tokio::task::spawn_blocking(move || -> Result<Converted, String> {
        match sniff::sniff(&f) {
            Ok(Kind::Ooxml { .. }) => {}
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
        Ok(made)
    }).await.map_err(|_| slide_not_saved())??;
    let mut slides = made.slides;
    if slides.is_empty() { return Err(pdf_no_slides()) }
    slides.truncate(MAX_SLIDES);
    let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Design { title, slides }, background: None, notes: String::new(), theme: None };
    let id = item.id;
    app.edit(|d| { d.service_mut().ok_or_else(|| crate::tr!("No service is open."))?.items.push(item); Ok(()) })?;
    app.job_result(job, id);
    if made.lost > 0 { app.job_note(job, lost_note(made.lost)) }
    Ok(())
}

#[cfg(test)]
#[path = "pptx_design_tests.rs"]
mod tests;
