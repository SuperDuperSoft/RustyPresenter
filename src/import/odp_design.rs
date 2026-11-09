//! LibreOffice Impress files (.odp; Google Slides downloads them too) as designed slides (1.1), read straight from
//! the zip so they stay editable: text boxes, shapes, pictures, page backgrounds, the master page's own shapes,
//! notes, transitions and object animations. Tables, charts, other embedded objects and pictures that can't be read
//! become grey boxes with their name (`OdpDesigns::placeholders` counts them). A page of another shape than 16:9
//! sits in the middle of the slide. Words take the style of their first piece of text (a text box has one style).
use super::notes::{attr, is, odf_inline, odp_notes, odp_pages, Package};
use super::strings;
use crate::design::{Anim, AnimKind, Design, Dir, Effect, Fit, HAlign, Look, MotionPath, Obj, ObjKind, Outline, PctRect, Seg, Shape, Start, TextStyle, VAlign};
use crate::model::AssetId;
use crate::store::Store;
use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::io::{BufReader, Read, Seek};
use std::path::Path;
use uuid::Uuid;

/// What an .odp file became.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OdpDesigns {
    /// One per shown page, in order.
    pub designs: Vec<Design>,
    /// Objects that could not be copied and are grey boxes now.
    pub placeholders: usize,
    /// Each slide's automatic time in ms (LibreOffice's automatic page change); designed slides keep none yet.
    pub advance_ms: Vec<Option<u32>>,
}

/// An .odp file as editable slides, its pictures copied into the store. For the deck import's "Editable slides".
pub fn odp_to_designs(path: &Path, store: &Store) -> Result<OdpDesigns, String> {
    if matches!(super::sniff::sniff(path), Ok(super::sniff::Kind::Encrypted)) {
        return Err(strings::encrypted());
    }
    let file = std::fs::File::open(path).map_err(|_| strings::odp_unreadable())?;
    let tmp = store.tmp();
    let _ = std::fs::create_dir_all(&tmp);
    let mut put = |bytes: &[u8], ext: &str| -> Option<AssetId> {
        let f = tmp.join(format!("odp-{}.{ext}", Uuid::new_v4()));
        std::fs::write(&f, bytes).ok()?;
        let id = store.import_file(&f, ext).ok();
        let _ = std::fs::remove_file(&f);
        id
    };
    let out = convert(BufReader::new(file), &mut put).ok_or_else(strings::odp_unreadable)?;
    if out.designs.is_empty() {
        return Err(strings::pdf_no_slides());
    }
    Ok(out)
}

/// The slides of an .odp file; `put` keeps a picture's bytes (with its extension) and names it. `None` when the
/// file is not one.
pub fn convert<R: Read + Seek>(r: R, put: &mut dyn FnMut(&[u8], &str) -> Option<AssetId>) -> Option<OdpDesigns> {
    let mut pkg = Package::open(r)?;
    let content = pkg.part("content.xml")?;
    let styles = pkg.part("styles.xml").unwrap_or_else(|| "<none/>".into());
    let cdoc = Document::parse(&content).ok()?;
    let sdoc = Document::parse(&styles).ok()?;
    let sheet = Sheet::new(&cdoc, &sdoc);
    let transitions = super::transitions::odp_doc(&cdoc);
    let advance_ms = super::transitions::odp_advance_doc(&cdoc);
    let mut cv = Converter { pkg, put, pictures: HashMap::new(), placeholders: 0 };
    let mut designs = Vec::new();
    for (i, page) in odp_pages(&cdoc).into_iter().enumerate() {
        let mut d = cv.page(&sheet, page);
        d.transition = transitions.get(i).copied().flatten().unwrap_or_default();
        d.notes = page.children().find(|n| is(n, "notes")).map(odp_notes).unwrap_or_default();
        d.sanitise();
        designs.push(d);
    }
    Some(OdpDesigns { designs, placeholders: cv.placeholders, advance_ms })
}

// ---- Styles ----

type StyleMap<'a, 'i> = HashMap<(&'a str, &'a str), Node<'a, 'i>>;

/// Both files' styles: `content.xml`'s automatic ones (for the pages), `styles.xml`'s automatic ones (for the
/// master pages) and the common ones both use.
struct Sheet<'a, 'i> {
    page_auto: StyleMap<'a, 'i>,
    master_auto: StyleMap<'a, 'i>,
    common: StyleMap<'a, 'i>,
    defaults: HashMap<&'a str, Node<'a, 'i>>,
    lists: HashMap<&'a str, Node<'a, 'i>>,
    fonts: HashMap<&'a str, String>,
    gradients: HashMap<&'a str, Node<'a, 'i>>,
    fill_images: HashMap<&'a str, &'a str>,
    layouts: HashMap<&'a str, (f32, f32)>,
    masters: HashMap<&'a str, Node<'a, 'i>>,
}

fn child<'a, 'i>(n: Node<'a, 'i>, local: &str) -> Option<Node<'a, 'i>> { n.children().find(|c| is(c, local)) }

/// An attribute in one namespace (`draw`, `presentation`, `text`…), by the end of its URI.
fn attr_in<'a, 'i>(n: &Node<'a, 'i>, ns: &str, local: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == local && a.namespace().is_some_and(|u| u.contains(ns))).map(|a| a.value())
}

fn styles_in<'a, 'i>(parent: Option<Node<'a, 'i>>) -> StyleMap<'a, 'i> {
    parent
        .into_iter()
        .flat_map(|p| p.children())
        .filter(|n| is(n, "style"))
        .filter_map(|n| Some(((attr(&n, "family")?, attr(&n, "name")?), n)))
        .collect()
}

impl<'a, 'i> Sheet<'a, 'i> {
    fn new(cdoc: &'a Document<'i>, sdoc: &'a Document<'i>) -> Self {
        let (c, s) = (cdoc.root_element(), sdoc.root_element());
        let common_el = child(s, "styles");
        let mut fonts = HashMap::new();
        for decl in [child(s, "font-face-decls"), child(c, "font-face-decls")].into_iter().flatten() {
            for f in decl.children().filter(|n| is(n, "font-face")) {
                if let (Some(name), Some(family)) = (attr(&f, "name"), attr(&f, "font-family")) {
                    fonts.insert(name, family.trim().trim_matches(['\'', '"']).to_string());
                }
            }
        }
        let all = || [child(c, "automatic-styles"), common_el, child(s, "automatic-styles")].into_iter().flatten().flat_map(|p| p.children());
        let mut lists = HashMap::new();
        for l in all().filter(|n| is(n, "list-style")) {
            if let Some(name) = attr(&l, "name") { lists.entry(name).or_insert(l); }
        }
        let gradients = all().filter(|n| is(n, "gradient")).filter_map(|n| Some((attr(&n, "name")?, n))).collect();
        let fill_images = all().filter(|n| is(n, "fill-image")).filter_map(|n| Some((attr(&n, "name")?, attr(&n, "href")?))).collect();
        let layouts = all()
            .filter(|n| is(n, "page-layout"))
            .filter_map(|n| {
                let p = child(n, "page-layout-properties")?;
                Some((attr(&n, "name")?, (length(attr(&p, "page-width")?)?, length(attr(&p, "page-height")?)?)))
            })
            .collect();
        let masters = child(s, "master-styles")
            .into_iter()
            .flat_map(|m| m.children())
            .filter(|n| is(n, "master-page"))
            .filter_map(|n| Some((attr(&n, "name")?, n)))
            .collect();
        let defaults = common_el.into_iter().flat_map(|p| p.children()).filter(|n| is(n, "default-style")).filter_map(|n| Some((attr(&n, "family")?, n))).collect();
        Sheet {
            page_auto: styles_in(child(c, "automatic-styles")),
            master_auto: styles_in(child(s, "automatic-styles")),
            common: styles_in(common_el),
            defaults,
            lists,
            fonts,
            gradients,
            fill_images,
            layouts,
            masters,
        }
    }
}

/// One style to look in: (family, name).
type Src<'a> = (&'static str, Option<&'a str>);

/// The styles seen from a page (`master: false`) or a master page.
#[derive(Clone, Copy)]
struct Ctx<'s, 'a, 'i> { sheet: &'s Sheet<'a, 'i>, master: bool }

impl<'a, 'i> Ctx<'_, 'a, 'i> {
    fn style(&self, family: &'a str, name: &'a str) -> Option<Node<'a, 'i>> {
        let auto = if self.master { &self.sheet.master_auto } else { &self.sheet.page_auto };
        auto.get(&(family, name)).or_else(|| self.sheet.common.get(&(family, name))).copied()
    }

    /// A page's or master page's `drawing-page-properties`.
    fn page_props(&self, n: Option<Node<'a, 'i>>) -> Option<Node<'a, 'i>> {
        let name = attr_in(&n?, "drawing", "style-name")?;
        child(self.style("drawing-page", name)?, "drawing-page-properties")
    }

    /// Every value of `key` in `props` along `chain` (each style, then its parents), then the default styles.
    fn values(&self, chain: &[Src<'a>], props: &str, key: &str) -> Vec<&'a str> {
        let mut out = Vec::new();
        let found = |st: Node<'a, 'i>, out: &mut Vec<&'a str>| if let Some(v) = child(st, props).and_then(|p| attr(&p, key)) { out.push(v) };
        for (family, name) in chain {
            let mut cur = *name;
            for _ in 0..16 {
                let Some(st) = cur.and_then(|n| self.style(family, n)) else { break };
                found(st, &mut out);
                cur = attr(&st, "parent-style-name");
            }
        }
        let mut families: Vec<&str> = chain.iter().map(|(f, _)| *f).collect();
        families.push("graphic");
        families.dedup();
        for f in families {
            if let Some(d) = self.sheet.defaults.get(f) { found(*d, &mut out) }
        }
        out
    }

    fn get(&self, chain: &[Src<'a>], props: &str, key: &str) -> Option<&'a str> { self.values(chain, props, key).into_iter().next() }

    /// A frame's or shape's own styles.
    fn chain_of(&self, n: &Node<'a, 'i>) -> Vec<Src<'a>> {
        vec![("graphic", attr_in(n, "drawing", "style-name")), ("presentation", attr_in(n, "presentation", "style-name"))]
    }

    /// The fill and outline of a shape (`shape`: LibreOffice's blue look when nothing says otherwise) or a frame.
    fn look(&self, chain: &[Src<'a>], shape: bool, page: &Page) -> Look {
        let g = |k| self.get(chain, "graphic-properties", k);
        let fill = match g("fill") {
            Some("none") | Some("bitmap") => None,
            Some("gradient") => g("fill-gradient-name").and_then(|n| self.gradient(n)),
            Some(_) => Some(g("fill-color").and_then(hex).unwrap_or_else(|| "#729fcf".into())),
            None if shape => Some(g("fill-color").and_then(hex).unwrap_or_else(|| "#729fcf".into())),
            None => None,
        };
        let alpha = g("opacity").and_then(|v| v.trim().strip_suffix('%')?.trim().parse::<f32>().ok()).filter(|a| *a < 100.0);
        let fill = match (fill, alpha) {
            (Some(f), Some(a)) => Some(format!("{f}{:02x}", (a.clamp(0.0, 100.0) / 100.0 * 255.0).round() as u8)),
            (f, _) => f,
        };
        let outline = match g("stroke") {
            Some("none") => None,
            None if !shape => None,
            _ => Some(Outline {
                color: g("stroke-color").and_then(hex).unwrap_or_else(|| "#3465a4".into()),
                width: page.px(g("stroke-width").and_then(length).unwrap_or(0.0)).max(1.0),
            }),
        };
        Look { fill, outline, ..Look::default() }
    }

    /// A gradient as one colour: halfway between its ends.
    fn gradient(&self, name: &str) -> Option<String> {
        let g = self.sheet.gradients.get(name)?;
        let (a, b) = (rgb(attr(g, "start-color")?)?, rgb(attr(g, "end-color")?)?);
        let mid = |i: usize| ((u16::from(a[i]) + u16::from(b[i])) as f32 / 2.0).round() as u8;
        Some(format!("#{:02x}{:02x}{:02x}", mid(0), mid(1), mid(2)))
    }

    /// The words' style from the first piece of text (`span`) in its paragraph (`para`) in the frame (`frame`).
    fn text_style(&self, frame: &[Src<'a>], para: Option<&'a str>, span: Option<&'a str>, valign: VAlign, page: &Page, dark: bool) -> TextStyle {
        let mut chain: Vec<Src<'a>> = vec![("text", span), ("paragraph", para)];
        chain.extend_from_slice(frame);
        let t = |k| self.get(&chain, "text-properties", k);
        let mut factor = 1.0;
        let mut pt = None;
        for v in self.values(&chain, "text-properties", "font-size") {
            match v.trim().strip_suffix('%').and_then(|p| p.trim().parse::<f32>().ok()) {
                Some(p) => factor *= p / 100.0,
                None => if let Some(l) = length(v) { pt = Some(l); break },
            }
        }
        let size = page.px(pt.unwrap_or(18.0) * factor);
        let font = t("font-name").map(|n| self.sheet.fonts.get(n).cloned().unwrap_or_else(|| n.to_string()))
            .or_else(|| t("font-family").map(|f| f.trim().trim_matches(['\'', '"']).to_string()))
            .unwrap_or_default();
        let color = t("color").and_then(hex).unwrap_or_else(|| if dark { "#ffffff" } else { "#000000" }.into());
        let bold = t("font-weight").is_some_and(|w| matches!(w.trim(), "bold" | "600" | "700" | "800" | "900"));
        let italic = t("font-style").is_some_and(|s| matches!(s.trim(), "italic" | "oblique"));
        let mut pchain: Vec<Src<'a>> = vec![("paragraph", para)];
        pchain.extend_from_slice(frame);
        let align = match self.get(&pchain, "paragraph-properties", "text-align") {
            Some("center") => HAlign::Center,
            Some("end" | "right") => HAlign::Right,
            _ => HAlign::Left,
        };
        TextStyle { font, size, color, bold, italic, align, valign, shrink: true }
    }

    /// How list level `level` of list style `style` marks its items.
    fn marker(&self, style: Option<&str>, level: usize, num: usize) -> String {
        let lvl = style.and_then(|s| self.sheet.lists.get(s)).and_then(|l| {
            l.children().filter(|c| c.is_element()).find(|c| attr(c, "level").and_then(|v| v.parse::<usize>().ok()) == Some(level))
        });
        let indent = "    ".repeat(level.saturating_sub(1));
        match lvl {
            Some(l) if is(&l, "list-level-style-number") => {
                let n = match attr(&l, "num-format").unwrap_or("1") {
                    "" => return indent,
                    "a" => char::from(b'a' + ((num - 1) % 26) as u8).to_string(),
                    "A" => char::from(b'A' + ((num - 1) % 26) as u8).to_string(),
                    _ => num.to_string(),
                };
                format!("{indent}{}{n}{} ", attr(&l, "num-prefix").unwrap_or(""), attr(&l, "num-suffix").unwrap_or(""))
            }
            Some(l) if is(&l, "list-level-style-bullet") && attr(&l, "bullet-char").is_some_and(|c| c.trim().is_empty()) => indent,
            _ => format!("{indent}• "),
        }
    }
}

// ---- Places ----

/// Lengths as LibreOffice writes them (cm, mm, in, pt, pc, px), in points.
fn length(v: &str) -> Option<f32> {
    let v = v.trim();
    let at = v.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(v.len());
    let n: f32 = v[..at].trim().parse().ok().filter(|n: &f32| n.is_finite())?;
    let scale = match &v[at..] {
        "cm" => 72.0 / 2.54,
        "mm" => 72.0 / 25.4,
        "in" | "inch" => 72.0,
        "pc" => 12.0,
        "px" => 0.75,
        "pt" | "" => 1.0,
        _ => return None,
    };
    Some(n * scale)
}

/// A page (points) fitted into the 1920 × 1080 slide, in the middle.
#[derive(Clone, Copy, Debug)]
struct Page { s: f32, ox: f32, oy: f32, w: f32, h: f32 }

impl Page {
    fn new(w: f32, h: f32) -> Page {
        let (w, h) = if w > 0.0 && h > 0.0 { (w, h) } else { (28.0 * 72.0 / 2.54, 15.75 * 72.0 / 2.54) };
        let s = (1920.0 / w).min(1080.0 / h);
        Page { s, ox: (1920.0 - w * s) / 2.0, oy: (1080.0 - h * s) / 2.0, w, h }
    }
    /// Points to 1080p pixels.
    fn px(&self, pt: f32) -> f32 { pt * self.s }
    /// A box in pixels on the slide, in %.
    fn pct_px(x: f32, y: f32, w: f32, h: f32) -> PctRect { PctRect::new(x / 19.2, y / 10.8, w / 19.2, h / 10.8) }
    fn pct(&self, x: f32, y: f32, w: f32, h: f32) -> PctRect { Self::pct_px(self.ox + x * self.s, self.oy + y * self.s, w * self.s, h * self.s) }
    fn whole(&self) -> PctRect { self.pct(0.0, 0.0, self.w, self.h) }
}

/// An affine map `[a, b, c, d, e, f]`: (x, y) → (a x + c y + e, b x + d y + f).
type Affine = [f32; 6];

fn then(m: Affine, op: Affine) -> Affine {
    [
        op[0] * m[0] + op[2] * m[1],
        op[1] * m[0] + op[3] * m[1],
        op[0] * m[2] + op[2] * m[3],
        op[1] * m[2] + op[3] * m[3],
        op[0] * m[4] + op[2] * m[5] + op[4],
        op[1] * m[4] + op[3] * m[5] + op[5],
    ]
}

/// `draw:transform` ("rotate (0.52) translate (5cm 3cm)", applied left to right; a positive turn is to the left),
/// with the whole turn in radians.
fn transform(s: &str) -> (Affine, f32) {
    let (mut m, mut turn) = ([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0.0);
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim();
        let Some(close) = rest[open..].find(')') else { break };
        let args: Vec<&str> = rest[open + 1..open + close].split(|c: char| c.is_whitespace() || c == ',').filter(|a| !a.is_empty()).collect();
        rest = &rest[open + close + 1..];
        let num = |i: usize| args.get(i).and_then(|a| a.parse::<f32>().ok());
        let len = |i: usize| args.get(i).and_then(|a| length(a));
        let op = match name {
            "rotate" => {
                let Some(a) = num(0) else { continue };
                turn += a;
                [a.cos(), -a.sin(), a.sin(), a.cos(), 0.0, 0.0]
            }
            "translate" => [1.0, 0.0, 0.0, 1.0, len(0).unwrap_or(0.0), len(1).unwrap_or(0.0)],
            "scale" => { let sx = num(0).unwrap_or(1.0); [sx, 0.0, 0.0, num(1).unwrap_or(sx), 0.0, 0.0] }
            _ => continue,
        };
        m = then(m, op);
    }
    (m, turn)
}

/// Where a shape sits: its box (before turning) and its turn, degrees clockwise.
fn place(n: &Node, page: &Page) -> Option<(PctRect, f32)> {
    let (w, h) = (length(attr(n, "width")?)?, length(attr(n, "height")?)?);
    if let Some(t) = attr_in(n, "drawing", "transform") {
        let (m, turn) = transform(t);
        let (cx, cy) = (m[0] * w / 2.0 + m[2] * h / 2.0 + m[4], m[1] * w / 2.0 + m[3] * h / 2.0 + m[5]);
        return Some((page.pct(cx - w / 2.0, cy - h / 2.0, w, h), -turn.to_degrees()));
    }
    let at = |k| attr(n, k).and_then(length).unwrap_or(0.0);
    Some((page.pct(at("x"), at("y"), w, h), 0.0))
}

/// The same middle, turned a quarter: width and height swapped (in pixels).
fn quarter(r: PctRect) -> PctRect {
    let (w, h) = (r.w * 19.2, r.h * 10.8);
    let (cx, cy) = ((r.x + r.w / 2.0) * 19.2, (r.y + r.h / 2.0) * 10.8);
    Page::pct_px(cx - h / 2.0, cy - w / 2.0, h, w)
}

// ---- Colours ----

fn rgb(v: &str) -> Option<[u8; 3]> {
    let h = v.trim().strip_prefix('#')?;
    if h.len() != 6 { return None }
    let p = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}

fn hex(v: &str) -> Option<String> { rgb(v).map(|c| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])) }

fn dark(color: &str) -> bool {
    rgb(color).is_some_and(|c| 0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2]) < 128.0)
}

// ---- Converting ----

struct Converter<'p, R> {
    pkg: Package<R>,
    put: &'p mut dyn FnMut(&[u8], &str) -> Option<AssetId>,
    /// Pictures already in the store, by part name.
    pictures: HashMap<String, Option<AssetId>>,
    placeholders: usize,
}

/// One page's objects as they are made: the objects, which shapes (`draw:id`, `xml:id`) became which objects, and
/// whether the background is dark.
struct Out<'a> { objects: Vec<Obj>, ids: HashMap<&'a str, Vec<Uuid>>, dark: bool }

const RASTER: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp"];

/// A picture's part name: `./Pictures/a.png` is `Pictures/a.png`.
fn part_name(href: &str) -> String { href.trim().trim_start_matches("./").to_string() }

impl<R: Read + Seek> Converter<'_, R> {
    /// A picture part in the store; `None` when it is missing or not a picture we draw.
    fn picture(&mut self, href: &str) -> Option<AssetId> {
        let name = part_name(href);
        if let Some(id) = self.pictures.get(&name) { return id.clone() }
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).filter(|e| RASTER.contains(&e.as_str()));
        let id = ext.and_then(|ext| { let bytes = self.pkg.bytes(&name)?; (self.put)(&bytes, if ext == "jpeg" { "jpg" } else { &ext }) });
        self.pictures.insert(name, id.clone());
        id
    }

    fn page<'a, 'i>(&mut self, sheet: &Sheet<'a, 'i>, page: Node<'a, 'i>) -> Design {
        let ctx = Ctx { sheet, master: false };
        let master = attr(&page, "master-page-name").and_then(|m| sheet.masters.get(m)).copied();
        let size = master.and_then(|m| attr(&m, "page-layout-name")).and_then(|l| sheet.layouts.get(l)).copied().unwrap_or((0.0, 0.0));
        let geo = Page::new(size.0, size.1);
        let mctx = Ctx { sheet, master: true };
        let (pp, mp) = (ctx.page_props(Some(page)), mctx.page_props(master));
        let shown = |k: &str| pp.and_then(|p| attr(&p, k)) != Some("false");
        let mut out = Out { objects: Vec::new(), ids: HashMap::new(), dark: false };
        // The page's own background, else the master's, else white.
        let bg = [pp.map(|p| (ctx, p)), mp.filter(|_| shown("background-visible")).map(|p| (mctx, p))]
            .into_iter()
            .flatten()
            .find_map(|(c, p)| self.background(&c, p, &geo))
            .unwrap_or_else(|| shape_obj(Shape::Rect, geo.whole(), Some("#ffffff".into())));
        out.dark = bg.look.fill.as_deref().is_some_and(dark);
        out.objects.push(bg);
        if let Some(m) = master.filter(|_| shown("background-objects-visible")) {
            for n in m.children().filter(|n| n.is_element() && attr_in(n, "presentation", "class").is_none()) {
                self.shape(&mctx, n, &geo, None, &mut out);
            }
        }
        for n in page.children().filter(|n| n.is_element()) {
            self.shape(&ctx, n, &geo, master, &mut out);
        }
        let anims = anims(page, &out.ids, &geo);
        Design { objects: out.objects, anims, ..Design::default() }
    }

    fn background<'a, 'i>(&mut self, ctx: &Ctx<'_, 'a, 'i>, p: Node<'a, 'i>, geo: &Page) -> Option<Obj> {
        let fill = match attr(&p, "fill")? {
            "solid" => attr(&p, "fill-color").and_then(hex),
            "gradient" => ctx.gradient(attr(&p, "fill-gradient-name")?),
            "bitmap" => {
                let href = ctx.sheet.fill_images.get(attr(&p, "fill-image-name")?)?;
                let asset = self.picture(href)?;
                return Some(Obj::new(ObjKind::Picture { asset, fit: Fit::Stretch }, geo.whole()));
            }
            _ => None,
        }?;
        Some(shape_obj(Shape::Rect, geo.whole(), Some(fill)))
    }

    /// One element of a page: whatever objects it makes are added to `out`, under its ids.
    fn shape<'a, 'i>(&mut self, ctx: &Ctx<'_, 'a, 'i>, n: Node<'a, 'i>, geo: &Page, master: Option<Node<'a, 'i>>, out: &mut Out<'a>) {
        let first = out.objects.len();
        match n.tag_name().name() {
            "g" => for c in n.children().filter(|c| c.is_element()) { self.shape(ctx, c, geo, master, out) },
            "frame" => self.frame(ctx, n, geo, master, out),
            "custom-shape" | "rect" | "ellipse" | "circle" => self.basic(ctx, n, geo, out),
            "line" | "connector" => if let Some(o) = line(ctx, n, geo) { out.objects.push(o) },
            "polyline" | "polygon" | "path" => if let Some(o) = drawn_path(ctx, n, geo) { out.objects.push(o) },
            _ => {}
        }
        let made: Vec<Uuid> = out.objects[first..].iter().map(|o| o.id).collect();
        if made.is_empty() { return }
        // LibreOffice writes the same name as `draw:id` and `xml:id`.
        for id in n.attributes().filter(|a| a.name() == "id").map(|a| a.value()) {
            let list = out.ids.entry(id).or_default();
            for m in &made {
                if !list.contains(m) { list.push(*m) }
            }
        }
    }

    fn placeholder(&mut self, label: String, rect: PctRect, rotation: f32) -> Obj {
        self.placeholders += 1;
        let style = TextStyle { size: 36.0, color: "#ffffff".into(), ..TextStyle::default() };
        let mut o = Obj::new(ObjKind::Text { text: label, style }, rect);
        o.rotation = rotation;
        o.look = Look { fill: Some("#4b5563".into()), outline: Some(Outline { color: "#9ca3af".into(), width: 3.0 }), ..Look::default() };
        o
    }

    fn frame<'a, 'i>(&mut self, ctx: &Ctx<'_, 'a, 'i>, n: Node<'a, 'i>, geo: &Page, master: Option<Node<'a, 'i>>, out: &mut Out<'a>) {
        if attr_in(&n, "presentation", "placeholder") == Some("true") { return }
        let class = attr_in(&n, "presentation", "class");
        // A placeholder that keeps the master's place.
        let from_master = || master?.children().find(|m| class.is_some() && attr_in(m, "presentation", "class") == class);
        let Some((rect, rotation)) = place(&n, geo).or_else(|| place(&from_master()?, geo)) else { return };
        let name = attr_in(&n, "drawing", "name").filter(|s| !s.trim().is_empty()).map(str::to_string);
        let chain = ctx.chain_of(&n);
        if let Some(tb) = child(n, "text-box") {
            let mut look = ctx.look(&chain, false, geo);
            look.radius = attr(&tb, "corner-radius").and_then(length).map_or(0.0, |r| geo.px(r));
            if let Some(o) = text_obj(ctx, tb, &chain, &n, rect, rotation, VAlign::Top, look, geo, out.dark) { out.objects.push(o) }
        } else if n.children().any(|c| is(&c, "image")) && !n.children().any(|c| is(&c, "object") || is(&c, "object-ole")) {
            let asset = n.children().filter(|c| is(c, "image")).filter_map(|i| attr(&i, "href")).find_map(|h| self.picture(h));
            let o = match asset {
                Some(asset) => { let mut o = Obj::new(ObjKind::Picture { asset, fit: Fit::Stretch }, rect); o.rotation = rotation; o }
                None => self.placeholder(name.unwrap_or_else(strings::odp_picture), rect, rotation),
            };
            out.objects.push(o);
        } else if n.children().any(|c| is(&c, "table")) {
            let o = self.placeholder(name.unwrap_or_else(strings::odp_table), rect, rotation);
            out.objects.push(o);
        } else if let Some(obj) = n.children().find(|c| is(c, "object") || is(c, "object-ole")) {
            let chart = class == Some("chart")
                || attr(&obj, "href").and_then(|h| self.pkg.part(&format!("{}/content.xml", part_name(h)))).is_some_and(|x| x.contains("<chart:chart"));
            let label = name.unwrap_or_else(|| if chart { strings::odp_chart() } else { strings::odp_object() });
            let o = self.placeholder(label, rect, rotation);
            out.objects.push(o);
        } else if n.children().any(|c| is(&c, "plugin")) {
            let o = self.placeholder(name.unwrap_or_else(strings::odp_media), rect, rotation);
            out.objects.push(o);
        }
    }

    /// Custom shapes, rectangles and ellipses, with any words in them.
    fn basic<'a, 'i>(&mut self, ctx: &Ctx<'_, 'a, 'i>, n: Node<'a, 'i>, geo: &Page, out: &mut Out<'a>) {
        let Some((mut rect, mut rotation)) = place(&n, geo) else { return };
        let chain = ctx.chain_of(&n);
        let mut look = ctx.look(&chain, true, geo);
        let (w, h) = (rect.w * 19.2, rect.h * 10.8);
        let geom = child(n, "enhanced-geometry");
        let ty = match n.tag_name().name() {
            "custom-shape" => geom.and_then(|g| attr(&g, "type")).unwrap_or("rectangle"),
            "rect" => "rectangle",
            _ => "ellipse",
        };
        let (shape, turn, block): (Shape, f32, bool) = match ty {
            "rectangle" | "rect" | "flowchart-process" | "mso-spt1" => (Shape::Rect, 0.0, false),
            "round-rectangle" | "round-rect" | "flowchart-alternate-process" => {
                let m = geom.and_then(|g| attr(&g, "modifiers")).and_then(|m| m.split_whitespace().next()?.parse::<f32>().ok()).unwrap_or(3600.0);
                look.radius = m.clamp(0.0, 10800.0) / 21600.0 * w.min(h);
                (Shape::Rect, 0.0, false)
            }
            "ellipse" | "circle" | "flowchart-connector" | "ring" => (Shape::Ellipse, 0.0, false),
            "isosceles-triangle" | "triangle" | "right-triangle" | "flowchart-extract" => (Shape::Triangle, 0.0, false),
            "right-arrow" | "mso-spt13" => (Shape::Arrow, 0.0, true),
            "left-arrow" => (Shape::Arrow, 180.0, true),
            "up-arrow" => (Shape::Arrow, -90.0, true),
            "down-arrow" => (Shape::Arrow, 90.0, true),
            _ => (Shape::Rect, 0.0, false),
        };
        if n.tag_name().name() == "rect" && let Some(r) = attr(&n, "corner-radius").and_then(length) { look.radius = geo.px(r) }
        let has_words = n.children().any(|c| is(&c, "p") || is(&c, "list"));
        if shape == Shape::Rect && has_words {
            if let Some(o) = text_obj(ctx, n, &chain, &n, rect, rotation, VAlign::Middle, look, geo, out.dark) { out.objects.push(o) }
            return;
        }
        let words_rect = rect;
        if block {
            // A block arrow is drawn as a thick arrow in its fill colour.
            let along = if turn == 0.0 || turn == 180.0 { h } else { w };
            look.outline = look.fill.take().map(|color| Outline { color, width: (along * 0.3).max(1.0) });
            if turn.abs() == 90.0 { rect = quarter(rect) }
            rotation += turn;
        }
        let mut o = shape_obj(shape, rect, None);
        o.rotation = rotation;
        o.look = look;
        out.objects.push(o);
        if has_words {
            let words = text_obj(ctx, n, &chain, &n, words_rect, rotation - turn, VAlign::Middle, Look::default(), geo, out.dark);
            if let Some(w) = words { out.objects.push(w) }
        }
    }
}

fn shape_obj(shape: Shape, rect: PctRect, fill: Option<String>) -> Obj {
    let mut o = Obj::new(ObjKind::Shape { shape }, rect);
    o.look.fill = fill;
    o
}

/// A line from (x1, y1) to (x2, y2): a box as long as the line, turned to it; an arrow when it has a head.
fn line<'a, 'i>(ctx: &Ctx<'_, 'a, 'i>, n: Node<'a, 'i>, geo: &Page) -> Option<Obj> {
    let at = |k| attr(&n, k).and_then(length);
    let (x1, y1, x2, y2) = (geo.px(at("x1")?) + geo.ox, geo.px(at("y1")?) + geo.oy, geo.px(at("x2")?) + geo.ox, geo.px(at("y2")?) + geo.oy);
    let chain = ctx.chain_of(&n);
    let g = |k| ctx.get(&chain, "graphic-properties", k);
    if g("stroke") == Some("none") { return None }
    let color = g("stroke-color").and_then(hex).unwrap_or_else(|| "#3465a4".into());
    let width = geo.px(g("stroke-width").and_then(length).unwrap_or(0.0)).max(1.0);
    let marked = |k| g(k).is_some_and(|m| !m.trim().is_empty());
    let (end, start) = (marked("marker-end"), marked("marker-start"));
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len = dx.hypot(dy).max(1.0);
    let thick = (width * 3.0).max(12.0);
    let (cx, cy) = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
    let mut angle = dy.atan2(dx).to_degrees();
    if start && !end { angle += 180.0 }
    let shape = if start || end { Shape::Arrow } else { Shape::Line };
    let mut o = shape_obj(shape, Page::pct_px(cx - len / 2.0, cy - thick / 2.0, len, thick), None);
    o.rotation = angle;
    o.look.outline = Some(Outline { color, width });
    Some(o)
}

/// A polyline, polygon or path as a drawn line in its box.
fn drawn_path<'a, 'i>(ctx: &Ctx<'_, 'a, 'i>, n: Node<'a, 'i>, geo: &Page) -> Option<Obj> {
    let (rect, rotation) = place(&n, geo)?;
    let vb: Vec<f32> = attr(&n, "viewBox")?.split_whitespace().filter_map(|v| v.parse().ok()).collect();
    let &[mx, my, vw, vh] = vb.as_slice() else { return None };
    if vw <= 0.0 || vh <= 0.0 { return None }
    let mut pts: Vec<(f32, f32)> = match n.tag_name().name() {
        "path" => MotionPath::parse_ppt(attr(&n, "d")?)?.points().into_iter().map(|(x, y)| (x / 100.0, y / 100.0)).collect(),
        _ => attr(&n, "points")?
            .split_whitespace()
            .filter_map(|p| { let (x, y) = p.split_once(',')?; Some((x.parse().ok()?, y.parse().ok()?)) })
            .collect(),
    };
    if n.tag_name().name() == "polygon" && let Some(f) = pts.first().copied() { pts.push(f) }
    let points: Vec<(f32, f32)> = pts.into_iter().map(|(x, y)| ((x - mx) / vw, (y - my) / vh)).collect();
    if points.len() < 2 { return None }
    let chain = ctx.chain_of(&n);
    let look = ctx.look(&chain, true, geo);
    let (color, width) = match look.outline {
        Some(ol) => (ol.color, ol.width),
        None => (look.fill.unwrap_or_else(|| "#3465a4".into()), 4.0),
    };
    let mut o = Obj::new(ObjKind::Path { points, width, color }, rect);
    o.rotation = rotation;
    Some(o)
}

/// One paragraph: its words (with any list mark), its style and its first piece of text's style.
struct Para<'a> { line: String, para: Option<&'a str>, span: Option<&'a str> }

fn para_of<'a, 'i>(p: Node<'a, 'i>, prefix: String) -> Para<'a> {
    let mut line = String::new();
    odf_inline(p, &mut line);
    let span = p.descendants().find(|d| is(d, "span") && d.descendants().any(|t| t.is_text() && !t.text().unwrap_or("").trim().is_empty())).and_then(|s| attr_in(&s, "text", "style-name"));
    Para { line: format!("{prefix}{}", line.trim_end()), para: attr_in(&p, "text", "style-name"), span }
}

fn paras<'a, 'i>(ctx: &Ctx<'_, 'a, 'i>, n: Node<'a, 'i>, out: &mut Vec<Para<'a>>) {
    for c in n.children() {
        if is(&c, "p") || is(&c, "h") {
            out.push(para_of(c, String::new()));
        } else if is(&c, "list") {
            list(ctx, c, 1, None, out);
        }
    }
}

fn list<'a, 'i>(ctx: &Ctx<'_, 'a, 'i>, l: Node<'a, 'i>, level: usize, style: Option<&'a str>, out: &mut Vec<Para<'a>>) {
    let style = attr_in(&l, "text", "style-name").or(style);
    let mut num = attr(&l, "start-value").and_then(|v| v.parse().ok()).unwrap_or(1usize).max(1);
    for item in l.children().filter(|c| is(c, "list-item") || is(c, "list-header")) {
        let header = is(&item, "list-header");
        let mut first = true;
        for c in item.children() {
            if is(&c, "p") || is(&c, "h") {
                let prefix = if first && !header { ctx.marker(style, level, num) } else { "    ".repeat(level) };
                out.push(para_of(c, prefix));
                first = false;
            } else if is(&c, "list") {
                list(ctx, c, level + 1, style, out);
            }
        }
        if !header { num += 1 }
    }
}

/// The words in `body` as a text box at `rect`; `None` when there are none and the box would not show.
#[allow(clippy::too_many_arguments)]
fn text_obj<'a, 'i>(ctx: &Ctx<'_, 'a, 'i>, body: Node<'a, 'i>, chain: &[Src<'a>], owner: &Node<'a, 'i>, rect: PctRect, rotation: f32, valign: VAlign, look: Look, geo: &Page, dark: bool) -> Option<Obj> {
    let mut ps = Vec::new();
    paras(ctx, body, &mut ps);
    let text = ps.iter().map(|p| p.line.as_str()).collect::<Vec<_>>().join("\n").trim_end().to_string();
    if text.trim().is_empty() && look.fill.is_none() && look.outline.is_none() { return None }
    let first = ps.iter().find(|p| !p.line.trim().is_empty());
    let mut frame: Vec<Src<'a>> = vec![("paragraph", attr_in(owner, "drawing", "text-style-name"))];
    frame.extend_from_slice(chain);
    let valign = match ctx.get(chain, "graphic-properties", "textarea-vertical-align") {
        Some("top") => VAlign::Top,
        Some("middle") => VAlign::Middle,
        Some("bottom") => VAlign::Bottom,
        _ => valign,
    };
    let dark = look.fill.as_deref().map_or(dark, self::dark);
    let style = ctx.text_style(&frame, first.and_then(|p| p.para), first.and_then(|p| p.span), valign, geo, dark);
    let mut o = Obj::new(ObjKind::Text { text, style }, rect);
    o.rotation = rotation;
    o.look = look;
    Some(o)
}

// ---- Animations ----

/// The page's main animation sequence as steps (click-started sequences on a shape are left out).
fn anims(page: Node, ids: &HashMap<&str, Vec<Uuid>>, geo: &Page) -> Vec<Anim> {
    let seconds = super::transitions::seconds;
    let mut out = Vec::new();
    for n in page.descendants().filter(|n| is(n, "par") && attr(n, "preset-class").is_some()) {
        if n.ancestors().any(|a| is(&a, "seq") && attr(&a, "node-type") == Some("interactive-sequence")) { continue }
        let Some(target) = n.descendants().find_map(|d| attr(&d, "targetElement")) else { continue };
        let Some(objs) = ids.get(target).filter(|o| !o.is_empty()) else { continue };
        let Some((kind, effect, dir)) = preset(attr(&n, "preset-class").unwrap_or(""), attr(&n, "preset-id").unwrap_or(""), attr(&n, "preset-sub-type").unwrap_or("")) else { continue };
        let start = match attr(&n, "node-type") {
            Some("with-previous") => Start::WithPrevious,
            Some("after-previous") => Start::AfterPrevious,
            _ => Start::OnClick,
        };
        let mut a = Anim::new(objs[0], kind, effect, start);
        a.delay_ms = attr(&n, "begin").and_then(seconds).unwrap_or(0);
        a.dur_ms = n.descendants().filter(|d| d.is_element() && !is(d, "set")).filter_map(|d| attr(&d, "dur").and_then(seconds)).max().unwrap_or(0);
        a.dir = dir;
        let transform = |ty: &str| n.descendants().find(|d| is(d, "animateTransform") && attr(d, "type") == Some(ty));
        let first_num = |v: &str| v.split([',', ' ']).find(|s| !s.is_empty())?.trim().parse::<f32>().ok();
        match effect {
            Effect::Spin => a.amount = transform("rotate").and_then(|t| attr(&t, "by").or(attr(&t, "to"))).and_then(first_num),
            Effect::Grow => a.amount = transform("scale").and_then(|t| attr(&t, "to").or(attr(&t, "by"))).and_then(first_num).map(|v| v * 100.0),
            Effect::Path => {
                let path = n.descendants().find(|d| is(d, "animateMotion")).and_then(|m| attr(&m, "path")).and_then(MotionPath::parse_ppt);
                let Some(mut path) = path else { continue };
                // Fractions of the page; the page may be narrower than the slide.
                let (fx, fy) = (geo.w * geo.s / 1920.0, geo.h * geo.s / 1080.0);
                let sc = |p: (f32, f32)| (p.0 * fx, p.1 * fy);
                for s in &mut path.segs {
                    *s = match *s {
                        Seg::Move { to } => Seg::Move { to: sc(to) },
                        Seg::Line { to } => Seg::Line { to: sc(to) },
                        Seg::Cubic { c1, c2, to } => Seg::Cubic { c1: sc(c1), c2: sc(c2), to: sc(to) },
                    }
                }
                a.path = Some(path);
            }
            _ => {}
        }
        for (i, id) in objs.iter().enumerate() {
            out.push(if i == 0 { a.clone() } else { Anim { obj: *id, start: Start::WithPrevious, ..a.clone() } });
        }
    }
    out
}

/// A LibreOffice preset as our kind, effect and side. Effects we don't have play as a fade (Pulse for emphasis).
fn preset(class: &str, id: &str, sub: &str) -> Option<(AnimKind, Effect, Option<Dir>)> {
    let kind = match class {
        "entrance" => AnimKind::Entrance,
        "exit" => AnimKind::Exit,
        "emphasis" => AnimKind::Emphasis,
        "motion-path" => AnimKind::Motion,
        _ => return None,
    };
    let name = ["ooo-entrance-", "ooo-exit-", "ooo-emphasis-"].iter().find_map(|p| id.strip_prefix(p)).unwrap_or(id);
    let effect = match kind {
        AnimKind::Motion => Effect::Path,
        AnimKind::Emphasis => match name {
            "spin" => Effect::Spin,
            "grow-and-shrink" => Effect::Grow,
            _ => Effect::Pulse,
        },
        _ => match name {
            "appear" | "disappear" => Effect::Appear,
            "fly-in" | "fly-out" | "fly-in-slow" | "fly-out-slow" | "crawl-in" | "crawl-out" => Effect::Fly,
            "wipe" | "peek-in" | "peek-out" => Effect::Wipe,
            "ascend" | "descend" | "rise-up" | "sink-down" | "float" | "float-in" | "float-out" => Effect::Float,
            "zoom" | "fade-in-and-zoom" | "fade-out-and-zoom" | "expand" | "contract" | "stretchy" | "compress" => Effect::Zoom,
            "split" => Effect::Split,
            _ => Effect::Fade,
        },
    };
    let dir = match (name, kind) {
        ("ascend" | "rise-up", AnimKind::Entrance) | ("descend" | "sink-down", AnimKind::Exit) => Some(Dir::Down),
        ("ascend" | "rise-up", AnimKind::Exit) | ("descend" | "sink-down", AnimKind::Entrance) => Some(Dir::Up),
        _ => {
            let side = sub.trim_start_matches("from-").trim_start_matches("to-");
            match side.split('-').next().unwrap_or("") {
                "bottom" => Some(Dir::Down),
                "top" => Some(Dir::Up),
                "left" => Some(Dir::Left),
                "right" => Some(Dir::Right),
                _ => None,
            }
        }
    };
    Some((kind, effect, dir.filter(|_| effect.has_dir())))
}

#[cfg(test)]
#[path = "odp_design_tests.rs"]
mod tests;
