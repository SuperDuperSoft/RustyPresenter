//! N26 designed slides: free objects (text boxes, shapes, pictures, video, drawn lines) on a 16:9 slide, with
//! builds (objects that appear one Next at a time) and a transition per slide. Pure model; the editor's logic is
//! `edit`, the drawing `render::design`. Places are in % of the slide, as the stage layout editor's (`PctRect`), so
//! its snapping and resizing helpers (`stage::geom`) work here too.
pub mod edit;
#[path = "../strings/design.rs"]
pub mod strings;

use crate::model::AssetId;
pub use crate::stage::PctRect;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Most objects one slide holds, and slides one item holds.
pub const MAX_OBJECTS: usize = 60;
pub const MAX_SLIDES: usize = 200;
/// Text sizes a text box may have, in pixels on a 1080p screen.
pub const SIZE_MIN: f32 = 8.0;
pub const SIZE_MAX: f32 = 400.0;

/// One designed slide.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Design {
    /// Drawn in order: later objects on top.
    #[serde(default)]
    pub objects: Vec<Obj>,
    #[serde(default)]
    pub transition: Transition,
    /// The slide's notes for the stage screen and the operator.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

/// One object on a slide.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Obj {
    pub id: Uuid,
    pub kind: ObjKind,
    /// The object before it is turned, in % of the slide.
    pub rect: PctRect,
    /// Degrees clockwise, about the middle.
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub rotation: f32,
    /// 0–100.
    #[serde(default = "hundred")]
    pub opacity: u8,
    #[serde(default)]
    pub look: Look,
    /// 0: shown with the slide. 1, 2…: appears on that Next (a build); objects with the same number appear together.
    #[serde(default, skip_serializing_if = "is_zero_u")]
    pub build: u16,
}

fn hundred() -> u8 { 100 }
fn is_zero_f(v: &f32) -> bool { *v == 0.0 }
fn is_zero_u(v: &u16) -> bool { *v == 0 }

/// What an object is.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ObjKind {
    Text { text: String, #[serde(default)] style: TextStyle },
    Shape { shape: Shape },
    Picture { asset: AssetId, #[serde(default)] fit: Fit },
    /// A video on the slide. The room shows its first frame (`thumb`) where it sits.
    Video { asset: AssetId, #[serde(default, skip_serializing_if = "Option::is_none")] thumb: Option<AssetId>, #[serde(default)] looped: bool },
    /// A drawn line: points in 0–1 of the object's box.
    Path { points: Vec<(f32, f32)>, #[serde(default = "path_width")] width: f32, #[serde(default = "white")] color: String },
    /// A web page (shown as its address until pages can be drawn).
    Web { url: String },
    /// Made by a newer RustyPresenter: kept, not drawn.
    #[serde(other)]
    Unknown,
}

fn path_width() -> f32 { 8.0 }
fn white() -> String { "#ffffff".into() }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Shape { #[default] Rect, Ellipse, Triangle, Line, Arrow }

/// How a picture fills its box.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Fit { #[default] Fit, Fill, Stretch }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HAlign { Left, #[default] Center, Right }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VAlign { Top, #[default] Middle, Bottom }

/// How a text box's words look.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct TextStyle {
    /// Empty: the theme's font.
    pub font: String,
    /// Pixels on a 1080p screen.
    pub size: f32,
    pub color: String,
    pub bold: bool,
    pub italic: bool,
    pub align: HAlign,
    pub valign: VAlign,
    /// Words too long for the box get smaller instead of running out of it.
    pub shrink: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle { font: String::new(), size: 72.0, color: "#ffffff".into(), bold: false, italic: false, align: HAlign::Center, valign: VAlign::Middle, shrink: true }
    }
}

/// An object's fill, outline, shadow and soft edges.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Look {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<Outline>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow: Option<ObjShadow>,
    /// How far in from the edges the object fades out, in 1080p pixels; 0 = sharp.
    #[serde(skip_serializing_if = "is_zero_f")]
    pub soft: f32,
    /// Rounded corners of a box or rectangle, in 1080p pixels.
    #[serde(skip_serializing_if = "is_zero_f")]
    pub radius: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Outline { pub color: String, pub width: f32 }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ObjShadow { pub color: String, pub blur: f32, pub dx: f32, pub dy: f32, pub opacity: u8 }

impl Default for ObjShadow {
    fn default() -> Self { ObjShadow { color: "#000000".into(), blur: 16.0, dx: 6.0, dy: 6.0, opacity: 70 } }
}

/// How the room changes to this slide.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct Transition {
    pub kind: TransitionKind,
    pub ms: u32,
    /// Which way a push or wipe moves (the new slide comes in from the other side).
    #[serde(skip_serializing_if = "Dir::is_left")]
    pub dir: Dir,
}

impl Default for Transition {
    fn default() -> Self { Transition { kind: TransitionKind::Fade, ms: 400, dir: Dir::Left } }
}

/// The longest transition offered.
pub const TRANSITION_MAX_MS: u32 = 5000;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind { #[default] Fade, Cut, Black, Push, Wipe }

impl TransitionKind {
    /// Every kind, in the order the pickers offer them.
    pub const ALL: [TransitionKind; 5] = [TransitionKind::Fade, TransitionKind::Cut, TransitionKind::Black, TransitionKind::Push, TransitionKind::Wipe];

    /// Its place in `ALL` (the pickers' index).
    pub fn index(self) -> i32 { Self::ALL.iter().position(|k| *k == self).unwrap_or(0) as i32 }
    pub fn from_index(i: i32) -> TransitionKind { Self::ALL[i.clamp(0, Self::ALL.len() as i32 - 1) as usize] }
    /// Push and Wipe move one way.
    pub fn moves(self) -> bool { matches!(self, TransitionKind::Push | TransitionKind::Wipe) }
}

/// The way a push or wipe moves.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Dir { #[default] Left, Right, Up, Down }

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::Left, Dir::Right, Dir::Up, Dir::Down];
    pub fn index(self) -> i32 { Self::ALL.iter().position(|d| *d == self).unwrap_or(0) as i32 }
    pub fn from_index(i: i32) -> Dir { Self::ALL[i.clamp(0, 3) as usize] }
    pub fn reverse(self) -> Dir { match self { Dir::Left => Dir::Right, Dir::Right => Dir::Left, Dir::Up => Dir::Down, Dir::Down => Dir::Up } }
    /// One step the way it moves, as (x, y) on screen (y down).
    pub fn step(self) -> (f32, f32) { match self { Dir::Left => (-1.0, 0.0), Dir::Right => (1.0, 0.0), Dir::Up => (0.0, -1.0), Dir::Down => (0.0, 1.0) } }
    fn is_left(&self) -> bool { *self == Dir::Left }
}

impl Obj {
    /// A new object of `kind` at `rect`, fully shown, no look.
    pub fn new(kind: ObjKind, rect: PctRect) -> Obj { Obj { id: Uuid::new_v4(), kind, rect, rotation: 0.0, opacity: 100, look: Look::default(), build: 0 } }

    pub fn text_style(&self) -> Option<&TextStyle> { match &self.kind { ObjKind::Text { style, .. } => Some(style), _ => None } }
}

impl Design {
    /// How many Nexts the slide's builds take (0: none).
    pub fn steps(&self) -> u16 {
        let mut b: Vec<u16> = self.objects.iter().map(|o| o.build).filter(|b| *b > 0).collect();
        b.sort_unstable();
        b.dedup();
        b.len() as u16
    }

    /// The slide as shown after `step` Nexts: objects of later builds left out. Steps count the distinct build
    /// numbers in order, so numbers with gaps (1, 4) still take one Next each.
    pub fn at_step(&self, step: u16) -> Design {
        let mut b: Vec<u16> = self.objects.iter().map(|o| o.build).filter(|b| *b > 0).collect();
        b.sort_unstable();
        b.dedup();
        let upto = if step == 0 { 0 } else { b.get(usize::from(step) - 1).copied().unwrap_or(u16::MAX) };
        Design { objects: self.objects.iter().filter(|o| o.build <= upto).cloned().collect(), ..self.clone() }
    }

    /// The slide's words, top to bottom, for the stage screen and the phone.
    pub fn words(&self) -> String {
        let mut texts: Vec<(&PctRect, &str)> = self.objects.iter().filter_map(|o| match &o.kind {
            ObjKind::Text { text, .. } if !text.trim().is_empty() => Some((&o.rect, text.trim())),
            _ => None,
        }).collect();
        texts.sort_by(|a, b| a.0.y.total_cmp(&b.0.y).then(a.0.x.total_cmp(&b.0.x)));
        texts.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("\n")
    }

    /// Every file the slide uses.
    pub fn assets(&self) -> Vec<AssetId> {
        self.objects.iter().flat_map(|o| match &o.kind {
            ObjKind::Picture { asset, .. } => vec![asset.clone()],
            ObjKind::Video { asset, thumb, .. } => std::iter::once(asset.clone()).chain(thumb.clone()).collect(),
            _ => vec![],
        }).collect()
    }

    /// Makes a stored slide safe to draw: sizes and numbers in range, at most `MAX_OBJECTS`. True when changed.
    pub fn sanitise(&mut self) -> bool {
        let before = self.clone();
        self.objects.truncate(MAX_OBJECTS);
        for o in &mut self.objects {
            o.rect = sane_rect(o.rect);
            if !o.rotation.is_finite() { o.rotation = 0.0 }
            o.rotation = ((o.rotation + 180.0).rem_euclid(360.0)) - 180.0;
            o.opacity = o.opacity.min(100);
            let l = &mut o.look;
            l.soft = finite(l.soft, 0.0).clamp(0.0, 400.0);
            l.radius = finite(l.radius, 0.0).clamp(0.0, 1000.0);
            if let Some(ol) = &mut l.outline { ol.width = finite(ol.width, 4.0).clamp(0.0, 100.0) }
            if let Some(s) = &mut l.shadow { s.blur = finite(s.blur, 16.0).clamp(0.0, 200.0); s.dx = finite(s.dx, 0.0).clamp(-200.0, 200.0); s.dy = finite(s.dy, 0.0).clamp(-200.0, 200.0); s.opacity = s.opacity.min(100) }
            match &mut o.kind {
                ObjKind::Text { style, .. } => style.size = finite(style.size, 72.0).clamp(SIZE_MIN, SIZE_MAX),
                ObjKind::Path { points, width, .. } => {
                    *width = finite(*width, 8.0).clamp(1.0, 100.0);
                    points.retain(|p| p.0.is_finite() && p.1.is_finite());
                    for p in points.iter_mut() { *p = (p.0.clamp(0.0, 1.0), p.1.clamp(0.0, 1.0)) }
                    points.truncate(2000);
                }
                _ => {}
            }
        }
        self.transition.ms = self.transition.ms.min(TRANSITION_MAX_MS);
        *self != before
    }
}

fn finite(v: f32, d: f32) -> f32 { if v.is_finite() { v } else { d } }

/// A place that can be drawn: a number everywhere, at least 1 % each way, not wholly off the slide.
fn sane_rect(r: PctRect) -> PctRect {
    let (w, h) = (finite(r.w, 30.0).clamp(1.0, 400.0), finite(r.h, 15.0).clamp(1.0, 400.0));
    PctRect { x: finite(r.x, 0.0).clamp(-w + 1.0, 99.0), y: finite(r.y, 0.0).clamp(-h + 1.0, 99.0), w, h }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str, build: u16) -> Obj {
        Obj { build, ..Obj::new(ObjKind::Text { text: t.into(), style: TextStyle::default() }, PctRect::new(10.0, 10.0, 50.0, 20.0)) }
    }

    #[test]
    fn steps_count_distinct_builds_and_at_step_shows_them_in_order() {
        let d = Design { objects: vec![text("title", 0), text("one", 1), text("two", 4), text("two too", 4)], ..Default::default() };
        assert_eq!(d.steps(), 2);
        let words = |s| d.at_step(s).objects.iter().map(|o| match &o.kind { ObjKind::Text { text, .. } => text.clone(), _ => String::new() }).collect::<Vec<_>>();
        assert_eq!(words(0), ["title"]);
        assert_eq!(words(1), ["title", "one"]);
        assert_eq!(words(2), ["title", "one", "two", "two too"]);
        assert_eq!(words(9), words(2), "past the last step: everything");
        assert_eq!(Design::default().steps(), 0);
    }

    #[test]
    fn a_slide_round_trips_and_writes_little() {
        let d = Design { objects: vec![text("Hi", 0)], ..Default::default() };
        let json = serde_json::to_value(&d).unwrap();
        assert!(json["objects"][0].get("build").is_none() && json["objects"][0].get("rotation").is_none());
        assert_eq!(serde_json::from_value::<Design>(json).unwrap(), d);
        let old: Design = serde_json::from_str(r#"{"objects":[{"id":"00000000-0000-0000-0000-000000000001","kind":{"type":"shape","shape":"ellipse"},"rect":{"x":0,"y":0,"w":10,"h":10}}]}"#).unwrap();
        assert_eq!((old.objects[0].opacity, old.transition), (100, Transition::default()));
    }

    #[test]
    fn a_transition_writes_its_way_only_when_it_is_not_left() {
        let t = Transition { kind: TransitionKind::Fade, ms: 700, dir: Dir::Left };
        assert_eq!(serde_json::to_value(t).unwrap(), serde_json::json!({ "kind": "fade", "ms": 700 }));
        let t = Transition { kind: TransitionKind::Push, ms: 700, dir: Dir::Up };
        let json = serde_json::to_value(t).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "push", "ms": 700, "dir": "up" }));
        assert_eq!(serde_json::from_value::<Transition>(json).unwrap(), t);
        assert_eq!((TransitionKind::from_index(TransitionKind::Wipe.index()), Dir::from_index(9)), (TransitionKind::Wipe, Dir::Down));
    }

    #[test]
    fn an_object_kind_from_a_newer_version_loads_as_unknown() {
        let d: Design = serde_json::from_str(r#"{"objects":[{"id":"00000000-0000-0000-0000-000000000001","kind":{"type":"hologram","x":1},"rect":{"x":0,"y":0,"w":10,"h":10}}]}"#).unwrap();
        assert_eq!(d.objects[0].kind, ObjKind::Unknown);
    }

    #[test]
    fn words_read_top_to_bottom() {
        let mut low = text("second", 0);
        low.rect.y = 70.0;
        let d = Design { objects: vec![low, text("first", 0), Obj::new(ObjKind::Shape { shape: Shape::Rect }, PctRect::new(0.0, 0.0, 5.0, 5.0))], ..Default::default() };
        assert_eq!(d.words(), "first\nsecond");
    }

    #[test]
    fn sanitise_fixes_numbers_and_leaves_good_slides_alone() {
        let mut good = Design { objects: vec![text("ok", 0)], ..Default::default() };
        assert!(!good.sanitise());
        let mut o = text("x", 0);
        o.rect = PctRect::new(f32::NAN, 500.0, -3.0, f32::INFINITY);
        o.rotation = 540.0;
        o.opacity = 250;
        if let ObjKind::Text { style, .. } = &mut o.kind { style.size = 9000.0 }
        let mut d = Design { objects: vec![o], transition: Transition { kind: TransitionKind::Cut, ms: 99_999, ..Default::default() }, ..Default::default() };
        assert!(d.sanitise());
        let o = &d.objects[0];
        assert_eq!((o.rect.x, o.rect.y, o.rect.w, o.rect.h), (0.0, 99.0, 1.0, 15.0));
        assert_eq!((o.rotation, o.opacity, o.text_style().unwrap().size, d.transition.ms), (-180.0, 100, SIZE_MAX, TRANSITION_MAX_MS));
    }
}

#[cfg(test)]
mod slide_tests {
    use super::*;
    use crate::model::SlideContent;

    #[test]
    fn an_item_shows_each_build_as_its_own_next() {
        let t = |s: &str, build| Obj { build, ..Obj::new(ObjKind::Text { text: s.into(), style: TextStyle::default() }, PctRect::new(0.0, 0.0, 50.0, 20.0)) };
        let d = Design { objects: vec![t("title", 0), t("point", 1)], notes: "Pray first".into(), ..Default::default() };
        let slides = crate::slides::design_slides(&[d, Design::default()]);
        assert_eq!(slides.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["Slide 1", "Slide 1 · 1 of 1", "Slide 2"]);
        assert!(matches!(&slides[0].content, SlideContent::Design { design, step: 0, steps: 1 } if design.objects.len() == 1));
        assert!(matches!(&slides[1].content, SlideContent::Design { design, step: 1, .. } if design.objects.len() == 2));
        assert_eq!((slides[1].notes.as_str(), slides[2].notes.as_str()), ("Pray first", ""));
    }
}
