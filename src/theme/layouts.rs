//! N25: a theme is a set of slide layouts. Each layout is a few changes to the theme's text style (size, box,
//! alignment…) for one kind of slide: songs use Lyrics, Bible passages Scripture, and an announcement picks one.
//! A theme stores only the layouts it changed; the others are the shipped ones below. Lyrics, Scripture and Text
//! change nothing, so a theme's songs, passages and announcements look exactly as before layouts came.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::theme::{TextAlign, Theme};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[serde(rename_all = "snake_case")]
pub enum LayoutKind {
    Title,
    TitleSubtitle,
    Quote,
    MainPoint,
    Scripture,
    Picture,
    Points,
    #[default]
    Text,
    Lyrics,
    LowerThirdLyrics,
}

impl LayoutKind {
    /// Every layout, in the order the pickers list them.
    pub const ALL: [LayoutKind; 10] = [
        LayoutKind::Title, LayoutKind::TitleSubtitle, LayoutKind::Quote, LayoutKind::MainPoint, LayoutKind::Scripture,
        LayoutKind::Picture, LayoutKind::Points, LayoutKind::Text, LayoutKind::Lyrics, LayoutKind::LowerThirdLyrics,
    ];

    pub fn label(self) -> String {
        match self {
            LayoutKind::Title => crate::tr!("Title"),
            LayoutKind::TitleSubtitle => crate::tr!("Title + subtitle"),
            LayoutKind::Quote => crate::tr!("Quote"),
            LayoutKind::MainPoint => crate::tr!("Main point"),
            LayoutKind::Scripture => crate::tr!("Scripture"),
            LayoutKind::Picture => crate::tr!("Picture"),
            LayoutKind::Points => crate::tr!("Points"),
            LayoutKind::Text => crate::trc!("layout", "Text"),
            LayoutKind::Lyrics => crate::tr!("Lyrics"),
            LayoutKind::LowerThirdLyrics => crate::tr!("Lower-third lyrics"),
        }
    }

    /// How to type the slide for this layout (shown under the item editor's picker).
    pub fn hint(self) -> String {
        match self {
            LayoutKind::Title => crate::tr!("One big line in the middle."),
            LayoutKind::TitleSubtitle => crate::tr!("The first line is the title; the lines under it are smaller."),
            LayoutKind::Quote => crate::tr!("The quote on the first line, who said it on the next."),
            LayoutKind::MainPoint => crate::tr!("One short sentence, big and bold."),
            LayoutKind::Scripture => crate::tr!("Laid out like a Bible passage."),
            LayoutKind::Picture => crate::tr!("A small caption at the bottom, so the background picture shows."),
            LayoutKind::Points => crate::tr!("Each line becomes a bullet point."),
            LayoutKind::Text => crate::tr!("Plain text, like an announcement."),
            LayoutKind::Lyrics => crate::tr!("Laid out like song lyrics."),
            LayoutKind::LowerThirdLyrics => crate::tr!("Small text at the bottom of the screen."),
        }
    }

    pub fn index(self) -> usize { Self::ALL.iter().position(|k| *k == self).unwrap_or(0) }

    pub fn from_index(i: i32) -> LayoutKind { usize::try_from(i).ok().and_then(|i| Self::ALL.get(i).copied()).unwrap_or_default() }

    pub fn is_text(&self) -> bool { *self == LayoutKind::Text }
}

/// The layout a slide of this kind of item uses (`slides::build_item`'s kind): songs Lyrics, passages Scripture,
/// an announcement its own choice.
pub fn for_item(kind: &crate::model::ItemKind) -> LayoutKind {
    use crate::model::ItemKind;
    match kind {
        ItemKind::Song { .. } => LayoutKind::Lyrics,
        ItemKind::Scripture { .. } => LayoutKind::Scripture,
        ItemKind::Text { layout, .. } => *layout,
        _ => LayoutKind::Picture,
    }
}

/// Where the text box sits, in thousandths of the screen's width (`x`, `w`) and height (`y`, `h`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Frame {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// The smallest box side, in thousandths.
pub const MIN_SIDE: u16 = 80;

impl Frame {
    /// The box kept on the screen and at least `MIN_SIDE` each way.
    pub fn clamped(self) -> Frame {
        let w = self.w.clamp(MIN_SIDE, 1000);
        let h = self.h.clamp(MIN_SIDE, 1000);
        Frame { x: self.x.min(1000 - w), y: self.y.min(1000 - h), w, h }
    }
}

/// The box the renderer uses when a layout has none: the screen inside its usual margins (110 × 66 px at 1080p).
pub const SAFE: Frame = Frame { x: 57, y: 61, w: 886, h: 878 };

/// One layout's changes to the theme. Every field left at its default keeps what the theme says.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[serde(default)]
pub struct Layout {
    /// Percent of the theme's text sizes; 0 is 100.
    #[serde(skip_serializing_if = "is_zero16")]
    pub size: u16,
    /// Where the text sits in its box: "top", "center" or "lower"; empty is the theme's.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub position: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<TextAlign>,
    /// 400, 600 or 700; 0 is the theme's.
    #[serde(skip_serializing_if = "is_zero16")]
    pub weight: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub band: Option<bool>,
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    /// Each line becomes a bullet point.
    #[serde(skip_serializing_if = "is_false")]
    pub bullets: bool,
    /// The lines after the first at this percent of the text size; 0 is the same size.
    #[serde(skip_serializing_if = "is_zero8")]
    pub subtitle: u8,
    /// The text box; None is the whole screen inside its margins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<Frame>,
}

fn is_zero8(n: &u8) -> bool { *n == 0 }
fn is_zero16(n: &u16) -> bool { *n == 0 }
fn is_false(b: &bool) -> bool { !*b }

/// The shipped look of each layout, used until a theme changes it.
pub fn default_layout(kind: LayoutKind) -> Layout {
    let f = |x, y, w, h| Some(Frame { x, y, w, h });
    match kind {
        LayoutKind::Title => Layout { size: 130, weight: 700, align: Some(TextAlign::Center), position: "center".into(), ..Layout::default() },
        LayoutKind::TitleSubtitle => Layout { size: 115, weight: 700, align: Some(TextAlign::Center), position: "center".into(), subtitle: 55, ..Layout::default() },
        LayoutKind::Quote => Layout { size: 90, italic: true, subtitle: 60, align: Some(TextAlign::Center), position: "center".into(), frame: f(140, 120, 720, 760), ..Layout::default() },
        LayoutKind::MainPoint => Layout { size: 120, weight: 700, align: Some(TextAlign::Center), position: "center".into(), frame: f(100, 220, 800, 560), ..Layout::default() },
        LayoutKind::Picture => Layout { size: 60, position: "lower".into(), band: Some(true), frame: f(57, 700, 886, 239), ..Layout::default() },
        LayoutKind::Points => Layout { size: 80, bullets: true, align: Some(TextAlign::Left), position: "top".into(), frame: f(120, 110, 760, 780), ..Layout::default() },
        LayoutKind::LowerThirdLyrics => Layout { size: 70, position: "lower".into(), band: Some(true), frame: f(57, 640, 886, 299), ..Layout::default() },
        LayoutKind::Scripture | LayoutKind::Text | LayoutKind::Lyrics => Layout::default(),
    }
}

/// The layouts a theme changed; the rest are `default_layout`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[serde(transparent)]
pub struct Layouts(pub BTreeMap<LayoutKind, Layout>);

impl Layouts {
    pub fn is_empty(&self) -> bool { self.0.is_empty() }

    pub fn get(&self, kind: LayoutKind) -> Layout { self.0.get(&kind).cloned().unwrap_or_else(|| default_layout(kind)) }

    /// Stores `l` for `kind`; the shipped look is not stored.
    pub fn set(&mut self, kind: LayoutKind, l: Layout) {
        if l == default_layout(kind) { self.0.remove(&kind); } else { self.0.insert(kind, l); }
    }

    /// Whether `kind` differs from the shipped look.
    pub fn changed(&self, kind: LayoutKind) -> bool { self.0.contains_key(&kind) }
}

/// The style a slide of layout `kind` draws with: the theme's text style with the layout's changes on top.
pub fn apply(theme: &Theme, kind: LayoutKind) -> Theme {
    let l = theme.layouts.get(kind);
    let mut t = Theme { layouts: Layouts::default(), ..theme.clone() };
    let pct = |n: u32| (n * u32::from(l.size) / 100).clamp(24, 200);
    if l.size > 0 {
        t.size = pct(t.size);
        t.scripture_size = pct(t.scripture_size);
    }
    if !l.position.is_empty() { t.position = l.position.clone() }
    if let Some(a) = l.align { t.align = a }
    if l.weight > 0 { t.weight = l.weight }
    if let Some(b) = l.band { t.band = b }
    t.italic |= l.italic;
    t.bullets |= l.bullets;
    if l.subtitle > 0 { t.subtitle = l.subtitle }
    if l.frame.is_some() { t.frame = l.frame }
    t
}

/// Brings a stored layout back into range (`style::clamp_style`).
pub fn clamp_layout(l: &mut Layout) {
    if l.size > 0 { l.size = l.size.clamp(30, 200) }
    if l.weight > 0 { l.weight = if l.weight < 500 { 400 } else if l.weight < 650 { 600 } else { 700 } }
    if !matches!(l.position.as_str(), "" | "center" | "lower" | "top") { l.position.clear() }
    l.subtitle = l.subtitle.min(100);
    l.frame = l.frame.map(Frame::clamped);
}

// ---- Snap guides (the theme editor's text box) ----

/// The lines a box edge or middle snaps to, in thousandths: the screen edges, its margins, thirds and middle.
pub const GUIDES_X: [u16; 7] = [0, 57, 333, 500, 667, 943, 1000];
pub const GUIDES_Y: [u16; 7] = [0, 61, 333, 500, 667, 939, 1000];
/// How near (in thousandths) an edge must come to a guide to snap to it.
pub const SNAP: i32 = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drag {
    /// The whole box moves.
    Move,
    /// The bottom-right corner moves.
    Resize,
}

/// A dragged box and the guides it lines up with now (drawn while dragging).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapped {
    pub frame: Frame,
    pub guides_x: Vec<u16>,
    pub guides_y: Vec<u16>,
}

/// The shift that puts the nearest of `edges` on a guide, if one is within `SNAP`.
fn snap_shift(edges: &[i32], guides: &[u16]) -> i32 {
    let mut best: Option<i32> = None;
    for e in edges {
        for g in guides {
            let d = i32::from(*g) - e;
            if d.abs() <= SNAP && best.is_none_or(|b| d.abs() < b.abs()) { best = Some(d) }
        }
    }
    best.unwrap_or(0)
}

/// The guides `edges` sit on.
fn touching(edges: &[i32], guides: &[u16]) -> Vec<u16> {
    let mut out: Vec<u16> = guides.iter().copied().filter(|g| edges.contains(&i32::from(*g))).collect();
    out.dedup();
    out
}

/// `start` dragged by (`dx`, `dy`) thousandths. With `snap`, an edge (or the middle, when moving) that comes near a
/// guide jumps onto it.
pub fn drag(start: Frame, mode: Drag, dx: i32, dy: i32, snap: bool) -> Snapped {
    let (x, y, w, h) = (i32::from(start.x), i32::from(start.y), i32::from(start.w), i32::from(start.h));
    let min = i32::from(MIN_SIDE);
    let (x, y, w, h) = match mode {
        Drag::Move => {
            let mut nx = (x + dx).clamp(0, 1000 - w);
            let mut ny = (y + dy).clamp(0, 1000 - h);
            if snap {
                nx = (nx + snap_shift(&[nx, nx + w / 2, nx + w], &GUIDES_X)).clamp(0, 1000 - w);
                ny = (ny + snap_shift(&[ny, ny + h / 2, ny + h], &GUIDES_Y)).clamp(0, 1000 - h);
            }
            (nx, ny, w, h)
        }
        Drag::Resize => {
            let mut nw = (w + dx).clamp(min, 1000 - x);
            let mut nh = (h + dy).clamp(min, 1000 - y);
            if snap {
                nw = (nw + snap_shift(&[x + nw], &GUIDES_X)).clamp(min, 1000 - x);
                nh = (nh + snap_shift(&[y + nh], &GUIDES_Y)).clamp(min, 1000 - y);
            }
            (x, y, nw, nh)
        }
    };
    let frame = Frame { x: x as u16, y: y as u16, w: w as u16, h: h as u16 };
    if !snap { return Snapped { frame, guides_x: vec![], guides_y: vec![] } }
    Snapped { frame, guides_x: touching(&[x, x + w / 2, x + w], &GUIDES_X), guides_y: touching(&[y, y + h / 2, y + h], &GUIDES_Y) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_layouts_with_names() {
        assert_eq!(LayoutKind::ALL.len(), 10);
        let names: Vec<String> = LayoutKind::ALL.iter().map(|k| k.label()).collect();
        assert_eq!(names, ["Title", "Title + subtitle", "Quote", "Main point", "Scripture", "Picture", "Points", "Text", "Lyrics", "Lower-third lyrics"]);
        for k in LayoutKind::ALL {
            assert_eq!(LayoutKind::from_index(k.index() as i32), k);
            assert!(!k.hint().is_empty());
        }
        assert_eq!(LayoutKind::from_index(-1), LayoutKind::Text);
        assert_eq!(LayoutKind::from_index(99), LayoutKind::Text);
    }

    #[test]
    fn songs_passages_and_announcements_keep_their_old_look() {
        let t = Theme { size: 90, position: "top".into(), ..Theme::default() };
        for k in [LayoutKind::Lyrics, LayoutKind::Scripture, LayoutKind::Text] {
            assert_eq!(apply(&t, k), t, "{k:?}");
        }
    }

    #[test]
    fn layouts_change_the_style_on_top_of_the_theme() {
        let t = Theme { size: 100, scripture_size: 60, color: "#ffcc00".into(), ..Theme::default() };
        let title = apply(&t, LayoutKind::Title);
        assert_eq!((title.size, title.weight, title.color.as_str()), (130, 700, "#ffcc00"));
        let points = apply(&t, LayoutKind::Points);
        assert!(points.bullets);
        assert_eq!((points.align, points.position.as_str(), points.size), (TextAlign::Left, "top", 80));
        assert!(points.frame.is_some());
        let quote = apply(&t, LayoutKind::Quote);
        assert!(quote.italic);
        assert_eq!(quote.subtitle, 60);
        let lower = apply(&t, LayoutKind::LowerThirdLyrics);
        assert!(lower.band);
        assert_eq!((lower.position.as_str(), lower.size), ("lower", 70));
        assert!(apply(&t, LayoutKind::Title).layouts.is_empty(), "the drawn style carries no layouts");
    }

    #[test]
    fn a_theme_stores_only_the_layouts_it_changed() {
        let mut ls = Layouts::default();
        ls.set(LayoutKind::Title, default_layout(LayoutKind::Title));
        assert!(ls.is_empty());
        let mine = Layout { size: 150, ..default_layout(LayoutKind::Title) };
        ls.set(LayoutKind::Title, mine.clone());
        assert!(ls.changed(LayoutKind::Title) && !ls.changed(LayoutKind::Quote));
        assert_eq!(ls.get(LayoutKind::Title), mine);
        let t = Theme { layouts: ls.clone(), ..Theme::default() };
        assert_eq!(apply(&t, LayoutKind::Title).size, 126);
        let json = serde_json::to_value(&ls).unwrap();
        assert_eq!(json, serde_json::json!({ "title": { "size": 150, "position": "center", "align": "center", "weight": 700 } }));
        assert_eq!(serde_json::from_value::<Layouts>(json).unwrap(), ls);
    }

    #[test]
    fn clamp_layout_limits() {
        let mut l = Layout { size: 900, weight: 550, position: "middle".into(), subtitle: 250, frame: Some(Frame { x: 990, y: 0, w: 5, h: 2000 }), ..Layout::default() };
        clamp_layout(&mut l);
        assert_eq!((l.size, l.weight, l.position.as_str(), l.subtitle), (200, 600, "", 100));
        assert_eq!(l.frame, Some(Frame { x: 920, y: 0, w: 80, h: 1000 }));
    }

    #[test]
    fn moving_a_box_snaps_its_middle_to_the_screen_middle() {
        let start = Frame { x: 100, y: 100, w: 400, h: 200 };
        let s = drag(start, Drag::Move, 195, 0, true);
        assert_eq!(s.frame.x, 300, "the middle (x + 200) lands on 500");
        assert_eq!(s.guides_x, [500]);
        let free = drag(start, Drag::Move, 195, 0, false);
        assert_eq!(free.frame.x, 295);
        assert!(free.guides_x.is_empty());
    }

    #[test]
    fn moving_snaps_edges_to_margins_and_stays_on_screen() {
        let start = Frame { x: 100, y: 100, w: 400, h: 200 };
        let s = drag(start, Drag::Move, -50, -45, true);
        assert_eq!((s.frame.x, s.frame.y), (57, 61));
        assert_eq!((s.guides_x.as_slice(), s.guides_y.as_slice()), (&[57][..], &[61][..]));
        let off = drag(start, Drag::Move, 5000, 5000, false);
        assert_eq!((off.frame.x, off.frame.y), (600, 800));
    }

    #[test]
    fn resizing_snaps_the_corner_and_keeps_a_smallest_size() {
        let start = Frame { x: 57, y: 61, w: 400, h: 400 };
        let s = drag(start, Drag::Resize, 480, 470, true);
        assert_eq!((s.frame.w, s.frame.h), (886, 878), "right and bottom margins");
        let tiny = drag(start, Drag::Resize, -1000, -1000, true);
        assert_eq!((tiny.frame.w, tiny.frame.h), (MIN_SIDE, MIN_SIDE));
        assert_eq!((tiny.frame.x, tiny.frame.y), (57, 61), "resizing never moves the box");
    }
}
