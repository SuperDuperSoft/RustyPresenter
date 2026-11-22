//! The church's own livestream scenes (designs `docs/superpowers/specs/2026-11-17-own-scenes-design.md` and
//! `2026-11-17-scene-extras-design.md`): a name and layers, back to front, each a camera, the audience screen, the words,
//! a picture file, a colour, a short text, a clock, a countdown, a video or a web page, in a place, cropped, with a
//! colour taken out and as see-through as the church wants. A scene can have its own places for the tall (phone)
//! picture. `Prepared` is a scene made ready for one picture size: what does not move is drawn once, so putting a frame
//! together (`compose`) is quick enough for 1080p at 30 a second on an old PC.
use super::compose::{Corner, CornerSize, Inputs, Layout, black, corner_rect, over};
use super::key_out::{Cutout, Key};
use super::scene::Scene;
use super::transition::Transition;
use crate::model::Theme;
use crate::render::cache::{RenderKey, draw_key};
use crate::timers::TimeOfDay;
use crate::tr;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// The cameras a layer can name (besides the camera on air).
pub const CAMERAS: u8 = 4;

/// The size the editor draws its picture at (the tall picture the other way round).
pub const PREVIEW: (u32, u32) = (640, 360);

/// One of the church's scenes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct OwnScene {
    /// Slide actions and macros keep the scene by this, so renaming never breaks them.
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    /// Back to front: the last is in front.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
    /// The built-in scene this one stands in for when scenes are chosen by themselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instead_of: Option<Scene>,
    /// How the stream changes to this scene; `None`: the default from Settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
    /// The tall (phone) picture has places of its own (`Layer::tall`); otherwise it is the wide one fitted across.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tall: bool,
}

/// One layer of a scene.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Layer {
    pub shows: Shows,
    pub place: Place,
    /// 0 (solid) to 100 (not seen at all), in percent.
    #[serde(skip_serializing_if = "is_zero")]
    pub see_through: u8,
    /// The edges cut off its picture.
    #[serde(skip_serializing_if = "Crop::is_none")]
    pub crop: Crop,
    /// A colour, or the dark or light parts, taken out (cameras, pictures and videos).
    #[serde(skip_serializing_if = "Key::is_nothing")]
    pub key: Key,
    /// Its place in the scene's tall picture; `None`: the same as in the wide one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tall: Option<Place>,
}

fn is_zero(v: &u8) -> bool { *v == 0 }

/// How much is cut off each edge of a layer's picture, in percent of it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Crop { pub left: u8, pub top: u8, pub right: u8, pub bottom: u8 }

impl Crop {
    pub fn is_none(&self) -> bool { *self == Crop::default() }

    /// The part of a `(w, h)` picture left, `(x, y, width, height)`: at least one pixel each way.
    pub fn rect(&self, (w, h): (u32, u32)) -> Rect {
        let pct = |v: u8, of: u32| (u64::from(v.min(100)) * u64::from(of) / 100) as u32;
        let (x0, y0) = (pct(self.left, w).min(w.saturating_sub(1)), pct(self.top, h).min(h.saturating_sub(1)));
        let (x1, y1) = (w.saturating_sub(pct(self.right, w)).max(x0 + 1), h.saturating_sub(pct(self.bottom, h)).max(y0 + 1));
        (x0, y0, x1.min(w) - x0, y1.min(h) - y0)
    }

    /// Kept so that at least 1% of the picture is left each way.
    pub fn checked(self) -> Crop {
        let (left, right) = (self.left.min(99), self.right.min(99 - self.left.min(99)));
        let (top, bottom) = (self.top.min(99), self.bottom.min(99 - self.top.min(99)));
        Crop { left, top, right, bottom }
    }
}

/// What a layer shows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shows {
    /// `number` 0: the camera on air (the one the operator switched to); 1: the first camera, and so on.
    Camera { #[serde(default)] number: u8 },
    /// What the audience screen shows.
    #[default]
    Room,
    /// The live slide's words, as over the camera.
    Words,
    /// A picture file.
    Picture { #[serde(default)] path: String },
    /// A colour, `#rrggbb`.
    Colour { #[serde(default)] colour: String },
    /// A few words of the church's own, such as "Welcome".
    Text { #[serde(default)] text: String, #[serde(flatten)] look: TextLook },
    /// A video file, played again and again, without its sound.
    Video { #[serde(default)] path: String },
    /// The time now.
    Clock {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")] seconds: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")] h24: bool,
        #[serde(flatten)] look: TextLook,
    },
    /// The time left until `to`, or (`None`) until the service's start time.
    Countdown {
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::timers::lenient_time")] to: Option<TimeOfDay>,
        #[serde(flatten)] look: TextLook,
    },
    /// A web page (an address), as a browser shows it.
    Web { #[serde(default)] address: String },
    /// Made by a newer RustyPresenter: shows nothing here.
    #[serde(other)]
    Unknown,
}

/// How a text, clock or countdown layer's words look: their largest size in pixels on a 1080p stream (0: the
/// church's default size) and colour (`#rrggbb`; empty: the church's default colour).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct TextLook {
    #[serde(skip_serializing_if = "is_zero_u16")]
    pub size: u16,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub colour: String,
}

fn is_zero_u16(v: &u16) -> bool { *v == 0 }

/// Where a layer goes in the picture.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(tag = "at", rename_all = "snake_case")]
pub enum Place {
    /// A corner, in the same sizes as the built-in corner camera.
    Corner { #[serde(default)] corner: Corner, #[serde(default)] size: CornerSize },
    LeftHalf,
    RightHalf,
    /// A box set in percent of the picture: left, top, width, height.
    Box { #[serde(default)] x: u8, #[serde(default)] y: u8, #[serde(default)] w: u8, #[serde(default)] h: u8 },
    /// The whole picture (also anything a newer RustyPresenter made: serde wants that one last).
    #[default]
    #[serde(other)]
    Full,
}

/// `(x, y, width, height)` in pixels.
pub type Rect = (u32, u32, u32, u32);

const CORNERS: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];
const SIZES: [CornerSize; 3] = [CornerSize::Small, CornerSize::Medium, CornerSize::Large];

impl Place {
    /// Where it is in a `size` picture, always inside it.
    pub fn rect(&self, (w, h): (u32, u32)) -> Rect {
        match *self {
            Place::Full => (0, 0, w, h),
            Place::Corner { corner, size } => corner_rect((w, h), &Layout { corner, corner_size: size, words_top: false }),
            Place::LeftHalf => (0, 0, w / 2, h),
            Place::RightHalf => (w / 2, 0, w - w / 2, h),
            Place::Box { x, y, w: bw, h: bh } => {
                let pct = |v: u8, of: u32| (u64::from(v.min(100)) * u64::from(of) / 100) as u32;
                let (x0, y0) = (pct(x, w), pct(y, h));
                (x0, y0, pct(bw, w).min(w - x0), pct(bh, h).min(h - y0))
            }
        }
    }

    /// The same place as a box in percent (to start a box from where the layer was) in a picture of `size`'s shape.
    pub fn as_box_in(&self, (w, h): (u32, u32)) -> Place {
        if let Place::Box { .. } = self { return *self }
        let (x, y, rw, rh) = self.rect((w, h));
        let pct = |v: u32, of: u32| ((u64::from(v) * 100 + u64::from(of) / 2) / u64::from(of.max(1))) as u8;
        Place::Box { x: pct(x, w), y: pct(y, h), w: pct(rw, w), h: pct(rh, h) }
    }

    /// `as_box_in` a wide picture.
    pub fn as_box(&self) -> Place { self.as_box_in((1920, 1080)) }

    /// The places the editor's list offers, in order: the whole picture, each corner in each size, the halves, a box.
    pub fn choices() -> Vec<Place> {
        let mut out = vec![Place::Full];
        for corner in CORNERS { for size in SIZES { out.push(Place::Corner { corner, size }) } }
        out.extend([Place::LeftHalf, Place::RightHalf, Place::Box { x: 5, y: 5, w: 40, h: 20 }]);
        out
    }

    /// Its place in `choices` (any box is the last).
    pub fn index(&self) -> usize {
        match self {
            Place::Box { .. } => Place::choices().len() - 1,
            p => Place::choices().iter().position(|c| c == p).unwrap_or(0),
        }
    }

    pub fn label(&self) -> String {
        let size = |s: CornerSize| match s { CornerSize::Small => tr!("small"), CornerSize::Medium => tr!("medium"), CornerSize::Large => tr!("large") };
        match *self {
            Place::Full => tr!("Fills the picture"),
            Place::Corner { corner: Corner::TopLeft, size: s } => tr!("Top left, {size}", size = size(s)),
            Place::Corner { corner: Corner::TopRight, size: s } => tr!("Top right, {size}", size = size(s)),
            Place::Corner { corner: Corner::BottomLeft, size: s } => tr!("Bottom left, {size}", size = size(s)),
            Place::Corner { corner: Corner::BottomRight, size: s } => tr!("Bottom right, {size}", size = size(s)),
            Place::LeftHalf => tr!("Left half"),
            Place::RightHalf => tr!("Right half"),
            Place::Box { .. } => tr!("A box you set"),
        }
    }
}

impl Shows {
    /// What the editor's "Shows" list offers, in order (a picture, colour, text, video, countdown or web page with
    /// nothing in it yet).
    pub fn choices() -> Vec<Shows> {
        let mut out: Vec<Shows> = (0..=CAMERAS).map(|number| Shows::Camera { number }).collect();
        out.extend([Shows::Room, Shows::Words, Shows::Picture { path: String::new() }, Shows::Colour { colour: String::new() },
            Shows::Text { text: String::new(), look: TextLook::default() }, Shows::Video { path: String::new() },
            Shows::Clock { seconds: false, h24: false, look: TextLook::default() }, Shows::Countdown { to: None, look: TextLook::default() },
            Shows::Web { address: String::new() }]);
        out
    }

    /// Its place in `choices`; `None` for one made by a newer RustyPresenter.
    pub fn index(&self) -> Option<usize> {
        let n = usize::from(CAMERAS) + 1;
        Some(match self {
            Shows::Camera { number } => usize::from((*number).min(CAMERAS)),
            Shows::Room => n,
            Shows::Words => n + 1,
            Shows::Picture { .. } => n + 2,
            Shows::Colour { .. } => n + 3,
            Shows::Text { .. } => n + 4,
            Shows::Video { .. } => n + 5,
            Shows::Clock { .. } => n + 6,
            Shows::Countdown { .. } => n + 7,
            Shows::Web { .. } => n + 8,
            Shows::Unknown => return None,
        })
    }

    /// The list's names.
    pub fn kind_label(&self) -> String {
        match self {
            Shows::Camera { number: 0 } => tr!("The camera on air"),
            Shows::Camera { number } => tr!("Camera {n}", n = number),
            Shows::Room => tr!("The audience screen"),
            Shows::Words => tr!("The words"),
            Shows::Picture { .. } => tr!("A picture"),
            Shows::Colour { .. } => tr!("A colour"),
            Shows::Text { .. } => tr!("Some words of your own"),
            Shows::Video { .. } => tr!("A video"),
            Shows::Clock { .. } => tr!("A clock (the time now)"),
            Shows::Countdown { .. } => tr!("A countdown"),
            Shows::Web { .. } => tr!("A web page"),
            Shows::Unknown => tr!("Needs a newer RustyPresenter"),
        }
    }

    /// The layer list's name: the kind with what is in it.
    pub fn label(&self) -> String {
        let file = |path: &str| std::path::Path::new(path.trim()).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
        match self {
            Shows::Picture { path } if path.trim().is_empty() => tr!("A picture (none chosen yet)"),
            Shows::Picture { path } => tr!("Picture: {name}", name = file(path)),
            Shows::Video { path } if path.trim().is_empty() => tr!("A video (none chosen yet)"),
            Shows::Video { path } => tr!("Video: {name}", name = file(path)),
            Shows::Colour { colour } => tr!("Colour {hex}", hex = hex_or_black(colour)),
            Shows::Text { text, .. } => tr!("Your words: {text}", text = text.trim()),
            Shows::Clock { .. } => tr!("Clock"),
            Shows::Countdown { to: None, .. } => tr!("Countdown to the service's start"),
            Shows::Countdown { to: Some(t), .. } => tr!("Countdown to {time}", time = t.label()),
            Shows::Web { address } if address.trim().is_empty() => tr!("A web page (no address yet)"),
            Shows::Web { address } => tr!("Web page: {address}", address = address.trim()),
            s => s.kind_label(),
        }
    }

    /// `choice` (from `choices`) keeping what `self` holds when it is the same kind; a new picture, colour or text
    /// starts with nothing, black or "Welcome".
    pub fn changed_to(&self, choice: &Shows) -> Shows {
        use std::mem::discriminant;
        if discriminant(choice) == discriminant(self) && !matches!(self, Shows::Camera { .. }) { return self.clone() }
        match choice {
            Shows::Colour { .. } => Shows::Colour { colour: "#000000".into() },
            Shows::Text { .. } => Shows::Text { text: tr!("Welcome"), look: self.look().cloned().unwrap_or_default() },
            Shows::Clock { .. } => Shows::Clock { seconds: false, h24: false, look: self.look().cloned().unwrap_or_default() },
            Shows::Countdown { .. } => Shows::Countdown { to: None, look: self.look().cloned().unwrap_or_default() },
            c => c.clone(),
        }
    }

    /// The look of a text, clock or countdown layer's words.
    pub fn look(&self) -> Option<&TextLook> {
        match self { Shows::Text { look, .. } | Shows::Clock { look, .. } | Shows::Countdown { look, .. } => Some(look), _ => None }
    }

    pub fn look_mut(&mut self) -> Option<&mut TextLook> {
        match self { Shows::Text { look, .. } | Shows::Clock { look, .. } | Shows::Countdown { look, .. } => Some(look), _ => None }
    }

    /// A colour or the dark or light parts can be taken out of it: cameras, pictures and videos.
    pub fn can_key(&self) -> bool { matches!(self, Shows::Camera { .. } | Shows::Picture { .. } | Shows::Video { .. }) }

    /// Its edges can be cut off: everything with a picture of its own.
    pub fn can_crop(&self) -> bool {
        matches!(self, Shows::Camera { .. } | Shows::Room | Shows::Words | Shows::Picture { .. } | Shows::Video { .. } | Shows::Web { .. })
    }
}

impl Layer {
    /// The layer list's line: what it shows, where, and how see-through.
    pub fn label(&self) -> String {
        let main = tr!("{what} · {place}", what = self.shows.label(), place = self.place.label());
        if self.see_through == 0 { main } else { tr!("{layer} · {n}% see-through", layer = main, n = self.see_through.min(100)) }
    }

    /// Where it goes in the wide (`tall` false) or the tall picture.
    pub fn place_in(&self, tall: bool) -> Place { if tall { self.tall.unwrap_or(self.place) } else { self.place } }
}

impl OwnScene {
    /// A new scene: the audience screen, with the camera on air in the bottom right corner.
    pub fn new(name: String) -> OwnScene {
        OwnScene {
            id: Uuid::new_v4(),
            name,
            layers: vec![
                Layer { shows: Shows::Room, ..Layer::default() },
                Layer { shows: Shows::Camera { number: 0 }, place: Place::Corner { corner: Corner::BottomRight, size: CornerSize::Medium }, ..Layer::default() },
            ],
            ..OwnScene::default()
        }
    }

    /// The name to show (never empty).
    pub fn title(&self) -> String { if self.name.trim().is_empty() { tr!("Untitled scene") } else { self.name.trim().to_string() } }

    pub fn shows_words(&self) -> bool { self.layers.iter().any(|l| l.shows == Shows::Words && l.see_through < 100) }

    /// Whether camera `on_air` (from 0) is in the picture: a layer of the camera on air, or of that camera.
    pub fn shows_camera(&self, on_air: usize) -> bool {
        self.layers.iter().any(|l| l.see_through < 100 && matches!(l.shows, Shows::Camera { number } if number == 0 || usize::from(number) == on_air + 1))
    }
}

/// The scene's name: a built-in scene's own, or the church scene's (`own` is the church's list).
pub fn name(scene: Scene, own: &[OwnScene]) -> String {
    match scene {
        Scene::Own(id) => find(own, id).map_or_else(|| tr!("A scene that was deleted"), OwnScene::title),
        s => s.label(),
    }
}

pub fn find(own: &[OwnScene], id: Uuid) -> Option<&OwnScene> { own.iter().find(|s| s.id == id) }

/// Every scene the operator can pick: the built-in ones, then the church's.
pub fn all(own: &[OwnScene]) -> Vec<Scene> { Scene::ALL.into_iter().chain(own.iter().map(|s| Scene::Own(s.id))).collect() }

/// `scene`, or the church's scene that stands in for it (the first, when more than one does).
pub fn stand_in(scene: Scene, own: &[OwnScene]) -> Scene {
    own.iter().find(|s| s.instead_of == Some(scene)).map_or(scene, |s| Scene::Own(s.id))
}

/// Whether camera `on_air` (from 0) is in the picture of `scene`.
pub fn shows_camera(scene: Scene, on_air: usize, own: &[OwnScene]) -> bool {
    match scene {
        Scene::Own(id) => find(own, id).is_some_and(|s| s.shows_camera(on_air)),
        s => s.needs_camera(),
    }
}

/// How the stream changes to `scene`: its own transition, or the church's default.
pub fn transition_to(scene: Scene, own: &[OwnScene], default: &Transition) -> Transition {
    match scene {
        Scene::Own(id) => find(own, id).and_then(|s| s.transition.clone()).unwrap_or_else(|| default.clone()),
        _ => default.clone(),
    }
}

/// Every video file the church's scenes play, each once.
pub fn videos(own: &[OwnScene]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for l in own.iter().flat_map(|s| &s.layers) {
        if let Shows::Video { path } = &l.shows && !path.trim().is_empty() && !out.iter().any(|p| p == path.trim()) { out.push(path.trim().to_string()) }
    }
    out
}

/// Every web page the church's scenes show, each once.
pub fn web_pages(own: &[OwnScene]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for l in own.iter().flat_map(|s| &s.layers) {
        if let Shows::Web { address } = &l.shows && !address.trim().is_empty() && !out.iter().any(|p| p == address.trim()) { out.push(address.trim().to_string()) }
    }
    out
}

/// `#rrggbb` in small letters; black when it is not a colour.
fn hex_or_black(hex: &str) -> String {
    let h = hex.trim().trim_start_matches('#');
    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) { format!("#{}", h.to_ascii_lowercase()) } else { "#000000".into() }
}

pub(crate) fn rgb(hex: &str) -> [u8; 3] {
    let h = hex_or_black(hex);
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
    [byte(1), byte(3), byte(5)]
}

/// What moves, filled in each frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    Camera(u8),
    Room,
    Words,
    /// `Prepared::moving()[i]`.
    Moving(usize),
}

/// A picture that changes all the time besides the cameras, the room and the words: one for each video file and web
/// page a scene shows, each at the stream's size (`Inputs::more`, in the same order).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Moving {
    /// A video file: opaque, its shape kept with black bars.
    Video(String),
    /// A web page: premultiplied, clear where the page is.
    Web(String),
}

/// A layer made ready. `alpha` 0 to 255 is how much of it shows.
#[derive(Clone)]
enum Ready {
    Live { from: Live, rect: Rect, alpha: u32, crop: Crop, cut: Option<Arc<Cutout>> },
    /// One colour, premultiplied.
    Fill { rect: Rect, px: [u8; 4] },
    /// A picture the size of `rect`, premultiplied, see-through already in it.
    Patch { rect: Rect, rgba: Arc<Vec<u8>> },
}

/// A clock or a countdown: drawn again (as its layer's `Patch`) when its words change.
#[derive(Clone, Debug)]
struct Ticking { layer: usize, tick: Tick, look: TextLook, rect: Rect, alpha: u32 }

#[derive(Clone, Debug, PartialEq)]
enum Tick { Clock { seconds: bool, h24: bool }, Countdown { to: Option<TimeOfDay> } }

/// The time it is and the service's start time, for the clocks and countdowns.
#[derive(Clone, Copy, Debug)]
pub struct Now { pub at: chrono::NaiveDateTime, pub starts_at: Option<TimeOfDay> }

impl Now {
    pub fn local(starts_at: Option<TimeOfDay>) -> Now { Now { at: chrono::Local::now().naive_local(), starts_at } }
}

/// A scene made ready for one picture size.
#[derive(Clone)]
pub struct Prepared {
    pub id: Uuid,
    pub size: (u32, u32),
    layers: Vec<Ready>,
    moving: Vec<Moving>,
    ticking: Vec<Ticking>,
    /// The words the clocks and countdowns show now, in `ticking`'s order.
    texts: Vec<String>,
    /// The church's default look, for drawing the clocks again.
    theme: Theme,
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "Prepared({}, {} layers)", self.id, self.layers.len()) }
}

impl Prepared {
    /// Each camera layer's number (0: the camera on air).
    pub fn cameras(&self) -> impl Iterator<Item = u8> + '_ {
        self.layers.iter().filter_map(|l| match l { Ready::Live { from: Live::Camera(n), .. } => Some(*n), _ => None })
    }

    pub fn shows_room(&self) -> bool { self.layers.iter().any(|l| matches!(l, Ready::Live { from: Live::Room, .. })) }

    /// The videos and web pages it shows, in the order `Inputs::more` gives their pictures.
    pub fn moving(&self) -> &[Moving] { &self.moving }

    /// It has a clock or a countdown.
    pub fn ticks(&self) -> bool { !self.ticking.is_empty() }

    /// The words its clocks and countdowns show at `now`.
    pub fn tick_texts(&self, now: &Now) -> Vec<String> { self.ticking.iter().map(|t| tick_text(&t.tick, now)).collect() }

    /// The same scene with its clocks and countdowns showing `now`: `None` when nothing they show changed.
    pub fn retick(&self, now: &Now, fonts: &mut crate::render::Fonts) -> Option<Prepared> {
        let texts = self.tick_texts(now);
        if texts == self.texts { return None }
        let mut p = self.clone();
        for (t, (text, old)) in self.ticking.iter().zip(texts.iter().zip(&self.texts)) {
            if text == old { continue }
            if let Some(rgba) = text_patch(text, &t.look, self.size, (t.rect.2, t.rect.3), t.alpha, &self.theme, fonts) {
                p.layers[t.layer] = Ready::Patch { rect: t.rect, rgba: Arc::new(rgba) };
            }
        }
        p.texts = texts;
        Some(p)
    }
}

/// A clock's or countdown's words at `now`.
fn tick_text(tick: &Tick, now: &Now) -> String {
    match tick {
        Tick::Clock { seconds, h24 } => crate::stage::view::clock(now.at.time(), *seconds, *h24),
        Tick::Countdown { to } => match to.or(now.starts_at) {
            None => "–:––".to_string(),
            Some(t) => {
                let target = now.at.date().and_hms_opt(u32::from(t.hour), u32::from(t.minute), 0).unwrap_or(now.at);
                // Whole seconds left, rounded up, so it shows 0:00 only at the time.
                let ms = (target - now.at).num_milliseconds().max(0);
                crate::timers::text((ms + 999) / 1000 * 1000)
            }
        },
    }
}

/// `scene` made ready for a `size` wide picture: places worked out, pictures read, colours and text drawn, each with
/// its see-through. Layers that would not be seen are left out. `theme` is the church default's (text layers).
pub fn prepare(scene: &OwnScene, size: (u32, u32), theme: &Theme, fonts: &mut crate::render::Fonts) -> Prepared {
    prepare_at(scene, size, false, theme, fonts, &Now::local(None))
}

/// `prepare` for the wide picture or (`tall`) the scene's tall picture, its clocks and countdowns at `now`.
pub fn prepare_at(scene: &OwnScene, size: (u32, u32), tall: bool, theme: &Theme, fonts: &mut crate::render::Fonts, now: &Now) -> Prepared {
    let mut p = Prepared { id: scene.id, size, layers: vec![], moving: vec![], ticking: vec![], texts: vec![], theme: theme.clone() };
    for l in &scene.layers {
        let rect = l.place_in(tall).rect(size);
        let alpha = (u32::from(100 - l.see_through.min(100)) * 255 + 50) / 100;
        if alpha == 0 || rect.2 == 0 || rect.3 == 0 { continue }
        let cut = if l.shows.can_key() { Cutout::new(&l.key).map(Arc::new) } else { None };
        let crop = if l.shows.can_crop() { l.crop.checked() } else { Crop::default() };
        let live = |from: Live| Ready::Live { from, rect, alpha, crop, cut: cut.clone() };
        let moving = |m: Moving, p: &mut Prepared| {
            let i = p.moving.iter().position(|x| *x == m).unwrap_or_else(|| { p.moving.push(m); p.moving.len() - 1 });
            live(Live::Moving(i))
        };
        let ready = match &l.shows {
            Shows::Camera { number } => live(Live::Camera((*number).min(CAMERAS))),
            Shows::Room => live(Live::Room),
            Shows::Words => live(Live::Words),
            Shows::Video { path } if !path.trim().is_empty() => moving(Moving::Video(path.trim().to_string()), &mut p),
            Shows::Web { address } if !address.trim().is_empty() => moving(Moving::Web(address.trim().to_string()), &mut p),
            Shows::Colour { colour } => {
                let [r, g, b] = rgb(colour);
                let k = |c: u8| ((u32::from(c) * alpha + 127) / 255) as u8;
                Ready::Fill { rect, px: [k(r), k(g), k(b), alpha as u8] }
            }
            Shows::Picture { path } => match picture_patch(path, (rect.2, rect.3), alpha, crop, cut.as_deref()) { Some(rgba) => Ready::Patch { rect, rgba: Arc::new(rgba) }, None => continue },
            Shows::Text { text, look } => match text_patch(text, look, size, (rect.2, rect.3), alpha, theme, fonts) { Some(rgba) => Ready::Patch { rect, rgba: Arc::new(rgba) }, None => continue },
            Shows::Clock { seconds, h24, look } => ticking(&mut p, Tick::Clock { seconds: *seconds, h24: *h24 }, look, rect, alpha, fonts, now),
            Shows::Countdown { to, look } => ticking(&mut p, Tick::Countdown { to: *to }, look, rect, alpha, fonts, now),
            _ => continue,
        };
        p.layers.push(ready);
    }
    p
}

/// A clock or countdown layer, its words at `now`, noted in `p` to be drawn again when they change.
fn ticking(p: &mut Prepared, tick: Tick, look: &TextLook, rect: Rect, alpha: u32, fonts: &mut crate::render::Fonts, now: &Now) -> Ready {
    let text = tick_text(&tick, now);
    // A clear patch until it can be drawn.
    let rgba = text_patch(&text, look, p.size, (rect.2, rect.3), alpha, &p.theme, fonts).unwrap_or_else(|| vec![0; rect.2 as usize * rect.3 as usize * 4]);
    p.ticking.push(Ticking { layer: p.layers.len(), tick, look: look.clone(), rect, alpha });
    p.texts.push(text);
    Ready::Patch { rect, rgba: Arc::new(rgba) }
}

/// Every byte of a premultiplied picture times `alpha` / 255.
fn fade(rgba: &mut [u8], alpha: u32) {
    if alpha >= 255 { return }
    rgba.iter_mut().for_each(|b| *b = ((u32::from(*b) * alpha + 127) / 255) as u8);
}

/// Picture files read lately: the editor draws its picture again at each change, and a picture is read only once.
static PICTURES: std::sync::Mutex<Vec<(String, Option<std::time::SystemTime>, Arc<image::RgbaImage>)>> = std::sync::Mutex::new(Vec::new());

/// The picture file `path` (from the files read lately when it has not changed since).
fn read_picture(path: &str) -> Option<Arc<image::RgbaImage>> {
    let changed = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let mut kept = PICTURES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, _, img)) = kept.iter().find(|(p, t, _)| p == path && *t == changed) { return Some(img.clone()) }
    let img = Arc::new(image::open(path).map_err(|e| eprintln!("stream scene: could not read {path}: {e}")).ok()?.to_rgba8());
    kept.retain(|(p, ..)| p != path);
    kept.push((path.to_string(), changed, img.clone()));
    if kept.len() > 8 { kept.remove(0); }
    Some(img)
}

/// The picture file cropped and fitted inside `(w, h)`, its shape kept, see-through around it and where it is or
/// where `cut` takes it out.
fn picture_patch(path: &str, (w, h): (u32, u32), alpha: u32, crop: Crop, cut: Option<&Cutout>) -> Option<Vec<u8>> {
    if path.trim().is_empty() { return None }
    let whole = read_picture(path.trim())?;
    let (cx, cy, cw, ch) = crop.rect((whole.width(), whole.height()));
    let img = image::imageops::crop_imm(&*whole, cx, cy, cw, ch).to_image();
    let (x0, y0, fw, fh) = crate::record::pump::fit_rect(img.width(), img.height(), w, h);
    if fw == 0 || fh == 0 { return None }
    let mut small = image::imageops::resize(&img, fw, fh, image::imageops::FilterType::Triangle);
    if let Some(c) = cut { c.apply_straight(&mut small) }
    let mut out = vec![0u8; w as usize * h as usize * 4];
    for (y, row) in small.rows().enumerate() {
        let d0 = ((y0 as usize + y) * w as usize + x0 as usize) * 4;
        for (d, p) in out[d0..d0 + fw as usize * 4].chunks_exact_mut(4).zip(row) {
            let a = u32::from(p[3]);
            for c in 0..3 { d[c] = ((u32::from(p[c]) * a + 127) / 255) as u8 }
            d[3] = p[3];
        }
    }
    fade(&mut out, alpha);
    Some(out)
}

/// `text` in the middle of a `(w, h)` box, in the church's font, in `look`'s size and colour (else the church's), as
/// big as it would be on the whole `canvas` (smaller only when it does not fit).
fn text_patch(text: &str, look: &TextLook, canvas: (u32, u32), (w, h): (u32, u32), alpha: u32, theme: &Theme, fonts: &mut crate::render::Fonts) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() { return None }
    let base = if look.size > 0 { u32::from(look.size) } else { theme.size };
    let size = (u64::from(base) * u64::from(canvas.0) / u64::from(w.max(1))).min(2000) as u32;
    let color = if look.colour.trim().is_empty() { theme.color.clone() } else { hex_or_black(&look.colour) };
    let key = RenderKey { text: text.to_string(), footer: None, reference: None, scripture: false, size: 0, passage: Default::default(), parallel: None,
        theme: Theme { position: "center".into(), band: false, size, color, ..theme.clone() }, canvas: (w, h), design: None };
    let r = draw_key(fonts, &key);
    if (r.width, r.height) != (w, h) { return None }
    let mut out = r.rgba_premul.buffer().as_bytes().to_vec();
    fade(&mut out, alpha);
    Some(out)
}

/// Draws the prepared scene into `out` (RGBA of `p.size`), over black. The live pictures in `i` are of `i.size`
/// (the wide stream); the scene may be the tall picture. A camera with no picture shows the audience screen in its
/// place.
pub fn compose(p: &Prepared, i: &Inputs, out: &mut [u8]) {
    black(out);
    if out.len() != p.size.0 as usize * p.size.1 as usize * 4 { return }
    let room = i.ok(Some(i.room));
    for l in &p.layers {
        match l {
            Ready::Fill { rect, px } => fill(out, p.size, *rect, *px),
            Ready::Patch { rect, rgba } => patch(out, p.size, *rect, rgba),
            Ready::Live { from, rect, alpha, crop, cut } => {
                let (src, opaque, cut) = match from {
                    Live::Camera(n) => {
                        let c = if *n == 0 { i.ok(i.camera) } else { i.ok(i.cameras.get(usize::from(*n) - 1).copied().flatten()) };
                        // Without a picture the room stands in, with nothing taken out of it.
                        match c { Some(c) => (Some(c), true, cut.as_deref()), None => (room, true, None) }
                    }
                    Live::Room => (room, true, None),
                    Live::Words => (i.ok(i.words), false, None),
                    Live::Moving(k) => match p.moving.get(*k) {
                        Some(Moving::Video(_)) => (i.ok(i.more.get(*k).copied().flatten()), true, cut.as_deref()),
                        Some(Moving::Web(_)) => (i.ok(i.more.get(*k).copied().flatten()), false, None),
                        None => (None, true, None),
                    },
                };
                if let Some(src) = src { place(src, i.size, p.size, *rect, *alpha, opaque, *crop, cut, out) }
            }
        }
    }
}

fn fill(out: &mut [u8], (w, _): (u32, u32), (x0, y0, rw, rh): Rect, px: [u8; 4]) {
    let a = 255 - u32::from(px[3]);
    for y in y0..y0 + rh {
        let d0 = (y as usize * w as usize + x0 as usize) * 4;
        for d in out[d0..d0 + rw as usize * 4].chunks_exact_mut(4) {
            if a == 0 { d.copy_from_slice(&px); continue }
            for c in 0..3 { d[c] = (u32::from(px[c]) + (u32::from(d[c]) * a + 127) / 255).min(255) as u8 }
        }
    }
}

fn patch(out: &mut [u8], (w, _): (u32, u32), (x0, y0, rw, rh): Rect, rgba: &[u8]) {
    let row = rw as usize * 4;
    for (y, src) in (y0..y0 + rh).zip(rgba.chunks_exact(row)) {
        let d0 = (y as usize * w as usize + x0 as usize) * 4;
        over(&mut out[d0..d0 + row], src);
    }
}

/// A live picture of `src_size`, cropped by `crop`, into `rect` of an `out_size` picture. An opaque one (camera,
/// room, video) fills it, its shape kept, cut off equally on both sides; a premultiplied one (the words, a web page)
/// fits inside it whole. `cut` takes a colour or the dark or light parts out. Nearest pixel.
#[allow(clippy::too_many_arguments)]
fn place(src: &[u8], (w, h): (u32, u32), (ow, _): (u32, u32), rect: Rect, alpha: u32, opaque: bool, crop: Crop, cut: Option<&Cutout>, out: &mut [u8]) {
    let region = crop.rect((w, h));
    if rect == (0, 0, w, h) && ow == w && region == (0, 0, w, h) && alpha >= 255 && cut.is_none() {
        if opaque { out.copy_from_slice(src) } else { over(out, src) }
        return;
    }
    let (gx, gy, gw, gh) = region;
    let (rx, ry, rw, rh) = rect;
    let (from, to) = if opaque {
        // The part of the picture with the place's shape, from its middle.
        let cw = ((u64::from(gh) * u64::from(rw) / u64::from(rh.max(1))) as u32).clamp(1, gw);
        let ch = ((u64::from(gw) * u64::from(rh) / u64::from(rw.max(1))) as u32).clamp(1, gh);
        ((gx + (gw - cw) / 2, gy + (gh - ch) / 2, cw, ch), rect)
    } else {
        let (fx, fy, fw, fh) = crate::record::pump::fit_rect(gw, gh, rw, rh);
        (region, (rx + fx, ry + fy, fw, fh))
    };
    let (sx0, sy0, sw, sh) = from;
    let (dx0, dy0, dw, dh) = to;
    if dw == 0 || dh == 0 { return }
    let sx: Vec<usize> = (0..dw).map(|x| (sx0 + (u64::from(x) * u64::from(sw) / u64::from(dw)) as u32) as usize * 4).collect();
    for dy in 0..dh {
        let sy = (sy0 + (u64::from(dy) * u64::from(sh) / u64::from(dh)) as u32) as usize;
        let row = &src[sy * w as usize * 4..(sy + 1) * w as usize * 4];
        let d0 = ((dy0 + dy) as usize * ow as usize + dx0 as usize) * 4;
        for (d, &o) in out[d0..d0 + dw as usize * 4].chunks_exact_mut(4).zip(&sx) {
            let s = &row[o..o + 4];
            // How much of it shows: the layer's see-through times what the key leaves.
            let k = match cut { Some(c) => (alpha * c.keep(s) + 127) / 255, None => alpha };
            if opaque && k >= 255 { d.copy_from_slice(s); continue }
            let sa = if opaque { k } else { (u32::from(s[3]) * k + 127) / 255 };
            if sa == 0 { continue }
            for c in 0..3 {
                let sc = (u32::from(s[c]) * k + 127) / 255;
                d[c] = (sc + (u32::from(d[c]) * (255 - sa) + 127) / 255).min(255) as u8;
            }
        }
    }
}

/// Stand-in pictures for the editor: a grey picture named for each camera, a blue "Audience screen", sample words, and
/// named pictures for a video and a web page.
pub struct StandIns {
    pub size: (u32, u32),
    pub room: Vec<u8>,
    /// Camera 1 to `CAMERAS`.
    pub cameras: Vec<Vec<u8>>,
    pub words: Option<Vec<u8>>,
    pub video: Vec<u8>,
    /// Premultiplied: clear around its words.
    pub web: Vec<u8>,
}

impl StandIns {
    pub fn new(size: (u32, u32), theme: &Theme, fonts: &mut crate::render::Fonts) -> StandIns {
        let mut named = |colour: [u8; 4], text: String| {
            let mut px = colour.repeat(size.0 as usize * size.1 as usize);
            let key = RenderKey { text, footer: None, reference: None, scripture: false, size: 0, passage: Default::default(), parallel: None,
                theme: Theme { position: "center".into(), band: false, color: "#ffffff".into(), ..theme.clone() }, canvas: size, design: None };
            let r = draw_key(fonts, &key);
            if (r.width, r.height) == size { over(&mut px, r.rgba_premul.buffer().as_bytes()) }
            px
        };
        let room = named([24, 48, 96, 255], tr!("Audience screen"));
        let cameras = (1..=CAMERAS).map(|n| named([92, 92, 92, 255], tr!("Camera {n}", n = n))).collect();
        let video = named([72, 40, 96, 255], tr!("Video"));
        let web = named([0, 0, 0, 0], tr!("Web page"));
        let slide = crate::live::SlideLayer::Text { text: tr!("The words of the song go here"), footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None, theme: None };
        let words = super::lyrics::key_for(&slide, theme, size).map(|k| draw_key(fonts, &k)).filter(|r| (r.width, r.height) == size)
            .map(|r| r.rgba_premul.buffer().as_bytes().to_vec());
        StandIns { size, room, cameras, words, video, web }
    }
}

/// How `scene` looks, with the stand-ins in place of the cameras, the room, the words, videos and web pages: drawn by
/// the same code as the stream. The camera on air is camera 1. `tall`: the scene's tall picture (`stand.size` the
/// other way round).
pub fn preview_at(scene: &OwnScene, stand: &StandIns, tall: bool, theme: &Theme, fonts: &mut crate::render::Fonts, now: &Now) -> Vec<u8> {
    let size = if tall { (stand.size.1, stand.size.0) } else { stand.size };
    let p = prepare_at(scene, size, tall, theme, fonts, now);
    let cameras: Vec<Option<&[u8]>> = stand.cameras.iter().map(|c| Some(c.as_slice())).collect();
    let more: Vec<Option<&[u8]>> = p.moving().iter().map(|m| Some(match m { Moving::Video(_) => stand.video.as_slice(), Moving::Web(_) => stand.web.as_slice() })).collect();
    let i = Inputs { size: stand.size, room: &stand.room, camera: cameras.first().copied().flatten(), words: stand.words.as_deref(), card: None, cameras: &cameras, own: None, more: &more };
    let mut out = vec![0u8; size.0 as usize * size.1 as usize * 4];
    compose(&p, &i, &mut out);
    out
}

/// `preview_at` the wide picture, now.
pub fn preview(scene: &OwnScene, stand: &StandIns, theme: &Theme, fonts: &mut crate::render::Fonts) -> Vec<u8> {
    preview_at(scene, stand, false, theme, fonts, &Now::local(None))
}

/// A shared prepared scene, as the scene worker hands it to the writer.
pub type Shared = Arc<Prepared>;

#[cfg(test)]
#[path = "own_scenes_tests.rs"]
mod tests;
