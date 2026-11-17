//! The church's own livestream scenes (design `docs/superpowers/specs/2026-11-17-own-scenes-design.md`): a name and
//! layers, back to front, each a camera, the room screen, the words, a picture file, a colour or a short text, in a
//! place and as see-through as the church wants. `Prepared` is a scene made ready for one picture size: what does not
//! move is drawn once, so putting a frame together (`compose`) is quick enough for 1080p at 30 a second on an old PC.
use super::compose::{Corner, CornerSize, Inputs, Layout, black, corner_rect, over};
use super::scene::Scene;
use crate::model::Theme;
use crate::render::cache::{RenderKey, draw_key};
use crate::tr;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// The cameras a layer can name (besides the camera on air).
pub const CAMERAS: u8 = 4;

/// The size the editor draws its picture at.
pub const PREVIEW: (u32, u32) = (640, 360);

/// One of the church's scenes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
}

fn is_zero(v: &u8) -> bool { *v == 0 }

/// What a layer shows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shows {
    /// `number` 0: the camera on air (the one the operator switched to); 1: the first camera, and so on.
    Camera { #[serde(default)] number: u8 },
    /// What the room screen shows.
    #[default]
    Room,
    /// The live slide's words, as over the camera.
    Words,
    /// A picture file.
    Picture { #[serde(default)] path: String },
    /// A colour, `#rrggbb`.
    Colour { #[serde(default)] colour: String },
    /// A few words of the church's own, such as "Welcome".
    Text { #[serde(default)] text: String },
    /// Made by a newer RustyPresenter: shows nothing here.
    #[serde(other)]
    Unknown,
}

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

    /// The same place as a box in percent (to start a box from where the layer was).
    pub fn as_box(&self) -> Place {
        if let Place::Box { .. } = self { return *self }
        let (w, h) = (1920u32, 1080u32);
        let (x, y, rw, rh) = self.rect((w, h));
        let pct = |v: u32, of: u32| ((u64::from(v) * 100 + u64::from(of) / 2) / u64::from(of)) as u8;
        Place::Box { x: pct(x, w), y: pct(y, h), w: pct(rw, w), h: pct(rh, h) }
    }

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
    /// What the editor's "Shows" list offers, in order (a picture, colour or text with nothing in it yet).
    pub fn choices() -> Vec<Shows> {
        let mut out: Vec<Shows> = (0..=CAMERAS).map(|number| Shows::Camera { number }).collect();
        out.extend([Shows::Room, Shows::Words, Shows::Picture { path: String::new() }, Shows::Colour { colour: String::new() }, Shows::Text { text: String::new() }]);
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
            Shows::Unknown => return None,
        })
    }

    /// The list's names.
    pub fn kind_label(&self) -> String {
        match self {
            Shows::Camera { number: 0 } => tr!("The camera on air"),
            Shows::Camera { number } => tr!("Camera {n}", n = number),
            Shows::Room => tr!("The room screen"),
            Shows::Words => tr!("The words"),
            Shows::Picture { .. } => tr!("A picture"),
            Shows::Colour { .. } => tr!("A colour"),
            Shows::Text { .. } => tr!("Some words of your own"),
            Shows::Unknown => tr!("Needs a newer RustyPresenter"),
        }
    }

    /// The layer list's name: the kind with what is in it.
    pub fn label(&self) -> String {
        match self {
            Shows::Picture { path } if path.trim().is_empty() => tr!("A picture (none chosen yet)"),
            Shows::Picture { path } => {
                let name = std::path::Path::new(path.trim()).file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
                tr!("Picture: {name}", name = name)
            }
            Shows::Colour { colour } => tr!("Colour {hex}", hex = hex_or_black(colour)),
            Shows::Text { text } => tr!("Your words: {text}", text = text.trim()),
            s => s.kind_label(),
        }
    }

    /// `choice` (from `choices`) keeping what `self` holds when it is the same kind; a new picture, colour or text
    /// starts with nothing, black or "Welcome".
    pub fn changed_to(&self, choice: &Shows) -> Shows {
        match (choice, self) {
            (Shows::Picture { .. }, Shows::Picture { .. }) | (Shows::Colour { .. }, Shows::Colour { .. }) | (Shows::Text { .. }, Shows::Text { .. }) => self.clone(),
            (Shows::Colour { .. }, _) => Shows::Colour { colour: "#000000".into() },
            (Shows::Text { .. }, _) => Shows::Text { text: tr!("Welcome") },
            (c, _) => c.clone(),
        }
    }
}

impl Layer {
    /// The layer list's line: what it shows, where, and how see-through.
    pub fn label(&self) -> String {
        let main = tr!("{what} · {place}", what = self.shows.label(), place = self.place.label());
        if self.see_through == 0 { main } else { tr!("{layer} · {n}% see-through", layer = main, n = self.see_through.min(100)) }
    }
}

impl OwnScene {
    /// A new scene: the room screen, with the camera on air in the bottom right corner.
    pub fn new(name: String) -> OwnScene {
        OwnScene {
            id: Uuid::new_v4(),
            name,
            layers: vec![
                Layer { shows: Shows::Room, ..Layer::default() },
                Layer { shows: Shows::Camera { number: 0 }, place: Place::Corner { corner: Corner::BottomRight, size: CornerSize::Medium }, see_through: 0 },
            ],
            instead_of: None,
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

/// `#rrggbb` in small letters; black when it is not a colour.
fn hex_or_black(hex: &str) -> String {
    let h = hex.trim().trim_start_matches('#');
    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) { format!("#{}", h.to_ascii_lowercase()) } else { "#000000".into() }
}

fn rgb(hex: &str) -> [u8; 3] {
    let h = hex_or_black(hex);
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
    [byte(1), byte(3), byte(5)]
}

/// What moves, filled in each frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live { Camera(u8), Room, Words }

/// A layer made ready. `alpha` 0 to 255 is how much of it shows.
enum Ready {
    Live { from: Live, rect: Rect, alpha: u32 },
    /// One colour, premultiplied.
    Fill { rect: Rect, px: [u8; 4] },
    /// A picture the size of `rect`, premultiplied, see-through already in it.
    Patch { rect: Rect, rgba: Vec<u8> },
}

/// A scene made ready for one picture size.
pub struct Prepared {
    pub id: Uuid,
    pub size: (u32, u32),
    layers: Vec<Ready>,
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
}

/// `scene` made ready for a `size` picture: places worked out, pictures read, colours and text drawn, each with
/// its see-through. Layers that would not be seen are left out. `theme` is the church default's (text layers).
pub fn prepare(scene: &OwnScene, size: (u32, u32), theme: &Theme, fonts: &mut crate::render::Fonts) -> Prepared {
    let mut layers = vec![];
    for l in &scene.layers {
        let rect = l.place.rect(size);
        let alpha = (u32::from(100 - l.see_through.min(100)) * 255 + 50) / 100;
        if alpha == 0 || rect.2 == 0 || rect.3 == 0 { continue }
        let ready = match &l.shows {
            Shows::Camera { number } => Ready::Live { from: Live::Camera((*number).min(CAMERAS)), rect, alpha },
            Shows::Room => Ready::Live { from: Live::Room, rect, alpha },
            Shows::Words => Ready::Live { from: Live::Words, rect, alpha },
            Shows::Colour { colour } => {
                let [r, g, b] = rgb(colour);
                let k = |c: u8| ((u32::from(c) * alpha + 127) / 255) as u8;
                Ready::Fill { rect, px: [k(r), k(g), k(b), alpha as u8] }
            }
            Shows::Picture { path } => match picture_patch(path, (rect.2, rect.3), alpha) { Some(rgba) => Ready::Patch { rect, rgba }, None => continue },
            Shows::Text { text } => match text_patch(text, size, (rect.2, rect.3), alpha, theme, fonts) { Some(rgba) => Ready::Patch { rect, rgba }, None => continue },
            Shows::Unknown => continue,
        };
        layers.push(ready);
    }
    Prepared { id: scene.id, size, layers }
}

/// Every byte of a premultiplied picture times `alpha` / 255.
fn fade(rgba: &mut [u8], alpha: u32) {
    if alpha >= 255 { return }
    rgba.iter_mut().for_each(|b| *b = ((u32::from(*b) * alpha + 127) / 255) as u8);
}

/// The picture file fitted inside `(w, h)`, its shape kept, see-through around it and where it is.
fn picture_patch(path: &str, (w, h): (u32, u32), alpha: u32) -> Option<Vec<u8>> {
    if path.trim().is_empty() { return None }
    let img = image::open(path.trim()).map_err(|e| eprintln!("stream scene: could not read {path}: {e}")).ok()?.to_rgba8();
    let (x0, y0, fw, fh) = crate::record::pump::fit_rect(img.width(), img.height(), w, h);
    if fw == 0 || fh == 0 { return None }
    let small = image::imageops::resize(&img, fw, fh, image::imageops::FilterType::Triangle);
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

/// `text` in the middle of a `(w, h)` box, in the church's font and colour, as big as it would be on the whole
/// `canvas` (smaller only when it does not fit).
fn text_patch(text: &str, canvas: (u32, u32), (w, h): (u32, u32), alpha: u32, theme: &Theme, fonts: &mut crate::render::Fonts) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() { return None }
    let size = (u64::from(theme.size) * u64::from(canvas.0) / u64::from(w.max(1))).min(2000) as u32;
    let key = RenderKey { text: text.to_string(), footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None,
        theme: Theme { position: "center".into(), band: false, size, ..theme.clone() }, canvas: (w, h), design: None };
    let r = draw_key(fonts, &key);
    if (r.width, r.height) != (w, h) { return None }
    let mut out = r.rgba_premul.buffer().as_bytes().to_vec();
    fade(&mut out, alpha);
    Some(out)
}

/// Draws the prepared scene into `out` (RGBA of `i.size`), over black. A camera with no picture shows the room
/// screen in its place.
pub fn compose(p: &Prepared, i: &Inputs, out: &mut [u8]) {
    black(out);
    if p.size != i.size { return }
    let room = i.ok(Some(i.room));
    for l in &p.layers {
        match l {
            Ready::Fill { rect, px } => fill(out, i.size, *rect, *px),
            Ready::Patch { rect, rgba } => patch(out, i.size, *rect, rgba),
            Ready::Live { from, rect, alpha } => {
                let (src, opaque) = match from {
                    Live::Camera(0) => (i.ok(i.camera).or(room), true),
                    Live::Camera(n) => (i.ok(i.cameras.get(usize::from(*n) - 1).copied().flatten()).or(room), true),
                    Live::Room => (room, true),
                    Live::Words => (i.ok(i.words), false),
                };
                if let Some(src) = src { place(src, i.size, *rect, *alpha, opaque, out) }
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

/// A live picture of the whole `size` into `rect`. An opaque one (camera, room) fills it, its shape kept, cut off
/// equally on both sides; a premultiplied one (the words) fits inside it whole. Nearest pixel.
fn place(src: &[u8], (w, h): (u32, u32), rect: Rect, alpha: u32, opaque: bool, out: &mut [u8]) {
    let whole = (0, 0, w, h);
    if rect == whole && alpha >= 255 {
        if opaque { out.copy_from_slice(src) } else { over(out, src) }
        return;
    }
    let (rx, ry, rw, rh) = rect;
    let (from, to) = if opaque {
        // The part of the picture with the place's shape, from its middle.
        let cw = ((u64::from(h) * u64::from(rw) / u64::from(rh.max(1))) as u32).clamp(1, w);
        let ch = ((u64::from(w) * u64::from(rh) / u64::from(rw.max(1))) as u32).clamp(1, h);
        ((((w - cw) / 2), (h - ch) / 2, cw, ch), rect)
    } else {
        let (fx, fy, fw, fh) = crate::record::pump::fit_rect(w, h, rw, rh);
        (whole, (rx + fx, ry + fy, fw, fh))
    };
    let (sx0, sy0, sw, sh) = from;
    let (dx0, dy0, dw, dh) = to;
    if dw == 0 || dh == 0 { return }
    let sx: Vec<usize> = (0..dw).map(|x| (sx0 + (u64::from(x) * u64::from(sw) / u64::from(dw)) as u32) as usize * 4).collect();
    for dy in 0..dh {
        let sy = (sy0 + (u64::from(dy) * u64::from(sh) / u64::from(dh)) as u32) as usize;
        let row = &src[sy * w as usize * 4..(sy + 1) * w as usize * 4];
        let d0 = ((dy0 + dy) as usize * w as usize + dx0 as usize) * 4;
        for (d, &o) in out[d0..d0 + dw as usize * 4].chunks_exact_mut(4).zip(&sx) {
            let s = &row[o..o + 4];
            if opaque && alpha >= 255 { d.copy_from_slice(s); continue }
            let sa = if opaque { alpha } else { (u32::from(s[3]) * alpha + 127) / 255 };
            if sa == 0 { continue }
            for c in 0..3 {
                let sc = (u32::from(s[c]) * alpha + 127) / 255;
                d[c] = (sc + (u32::from(d[c]) * (255 - sa) + 127) / 255).min(255) as u8;
            }
        }
    }
}

/// Stand-in pictures for the editor: a grey picture named for each camera, a blue "Room screen", sample words.
pub struct StandIns {
    pub size: (u32, u32),
    pub room: Vec<u8>,
    /// Camera 1 to `CAMERAS`.
    pub cameras: Vec<Vec<u8>>,
    pub words: Option<Vec<u8>>,
}

impl StandIns {
    pub fn new(size: (u32, u32), theme: &Theme, fonts: &mut crate::render::Fonts) -> StandIns {
        let mut named = |colour: [u8; 4], text: String| {
            let mut px = colour.repeat(size.0 as usize * size.1 as usize);
            let key = RenderKey { text, footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None,
                theme: Theme { position: "center".into(), band: false, color: "#ffffff".into(), ..theme.clone() }, canvas: size, design: None };
            let r = draw_key(fonts, &key);
            if (r.width, r.height) == size { over(&mut px, r.rgba_premul.buffer().as_bytes()) }
            px
        };
        let room = named([24, 48, 96, 255], tr!("Room screen"));
        let cameras = (1..=CAMERAS).map(|n| named([92, 92, 92, 255], tr!("Camera {n}", n = n))).collect();
        let slide = crate::live::SlideLayer::Text { text: tr!("The words of the song go here"), footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None, theme: None };
        let words = super::lyrics::key_for(&slide, theme, size).map(|k| draw_key(fonts, &k)).filter(|r| (r.width, r.height) == size)
            .map(|r| r.rgba_premul.buffer().as_bytes().to_vec());
        StandIns { size, room, cameras, words }
    }
}

/// How `scene` looks, with the stand-ins in place of the cameras, the room and the words: drawn by the same code as
/// the stream. The camera on air is camera 1.
pub fn preview(scene: &OwnScene, stand: &StandIns, theme: &Theme, fonts: &mut crate::render::Fonts) -> Vec<u8> {
    let p = prepare(scene, stand.size, theme, fonts);
    let cameras: Vec<Option<&[u8]>> = stand.cameras.iter().map(|c| Some(c.as_slice())).collect();
    let i = Inputs { size: stand.size, room: &stand.room, camera: cameras.first().copied().flatten(), words: stand.words.as_deref(), card: None, cameras: &cameras, own: None };
    let mut out = vec![0u8; stand.size.0 as usize * stand.size.1 as usize * 4];
    compose(&p, &i, &mut out);
    out
}

/// A shared prepared scene, as the scene worker hands it to the writer.
pub type Shared = Arc<Prepared>;

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 40;
    const H: u32 = 20;
    const RED: [u8; 4] = [200, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 200, 255];
    const GREEN: [u8; 4] = [0, 200, 0, 255];

    fn solid(c: [u8; 4]) -> Vec<u8> { c.repeat((W * H) as usize) }
    fn px(img: &[u8], x: u32, y: u32) -> [u8; 4] { let i = ((y * W + x) * 4) as usize; img[i..i + 4].try_into().unwrap() }
    fn scene(layers: Vec<Layer>) -> OwnScene { OwnScene { id: Uuid::from_u128(7), name: "Ours".into(), layers, instead_of: None } }
    fn layer(shows: Shows, place: Place) -> Layer { Layer { shows, place, see_through: 0 } }
    fn draw(s: &OwnScene, room: &[u8], camera: Option<&[u8]>, cameras: &[Option<&[u8]>], words: Option<&[u8]>) -> Vec<u8> {
        let mut fonts = crate::render::Fonts::bundled_only();
        let p = prepare(s, (W, H), &Theme::default(), &mut fonts);
        let mut out = vec![7u8; (W * H * 4) as usize];
        compose(&p, &Inputs { size: (W, H), room, camera, words, card: None, cameras, own: None }, &mut out);
        out
    }

    #[test]
    fn layers_go_back_to_front() {
        let s = scene(vec![layer(Shows::Room, Place::Full), layer(Shows::Camera { number: 0 }, Place::RightHalf)]);
        let (room, cam) = (solid(BLUE), solid(RED));
        let out = draw(&s, &room, Some(&cam), &[], None);
        assert_eq!((px(&out, 5, 10), px(&out, 30, 10)), (BLUE, RED), "the room on the left, the camera in front on the right");
        let s = scene(s.layers.into_iter().rev().collect());
        assert_eq!(draw(&s, &room, Some(&cam), &[], None), room, "the room in front covers the camera");
    }

    #[test]
    fn each_camera_by_its_number_and_a_missing_one_shows_the_room() {
        let s = scene(vec![layer(Shows::Camera { number: 1 }, Place::LeftHalf), layer(Shows::Camera { number: 2 }, Place::RightHalf)]);
        let (room, one, two) = (solid(BLUE), solid(RED), solid(GREEN));
        let out = draw(&s, &room, None, &[Some(&one), Some(&two)], None);
        assert_eq!((px(&out, 5, 10), px(&out, 30, 10)), (RED, GREEN));
        let out = draw(&s, &room, None, &[Some(&one), None], None);
        assert_eq!(px(&out, 30, 10), BLUE, "camera 2 sends nothing: the room in its place");
        let s = scene(vec![layer(Shows::Camera { number: 3 }, Place::Full)]);
        assert_eq!(draw(&s, &room, None, &[Some(&one)], None), room, "a camera that is not set up");
    }

    #[test]
    fn see_through_lets_what_is_behind_show() {
        let s = scene(vec![layer(Shows::Room, Place::Full), Layer { see_through: 50, ..layer(Shows::Camera { number: 0 }, Place::Full) }]);
        let out = draw(&s, &solid([200, 0, 0, 255]), Some(&solid([0, 0, 200, 255])), &[], None);
        let p = px(&out, 10, 10);
        assert!((95..=105).contains(&p[0]) && (95..=105).contains(&p[2]) && p[3] == 255, "half of each: {p:?}");
        let gone = scene(vec![layer(Shows::Room, Place::Full), Layer { see_through: 100, ..layer(Shows::Colour { colour: "#ffffff".into() }, Place::Full) }]);
        assert_eq!(draw(&gone, &solid(RED), None, &[], None), solid(RED), "100% see-through is not seen");
    }

    #[test]
    fn a_colour_fills_its_place_and_black_is_behind_everything() {
        let s = scene(vec![layer(Shows::Colour { colour: "#00c800".into() }, Place::Box { x: 50, y: 0, w: 50, h: 50 })]);
        let out = draw(&s, &solid(RED), None, &[], None);
        assert_eq!((px(&out, 30, 2), px(&out, 30, 15), px(&out, 5, 2)), (GREEN, [0, 0, 0, 255], [0, 0, 0, 255]));
    }

    #[test]
    fn the_words_go_over_what_is_behind() {
        let mut words = vec![0u8; (W * H * 4) as usize];
        words[(15 * W * 4) as usize..(16 * W * 4) as usize].fill(255);
        let s = scene(vec![layer(Shows::Camera { number: 0 }, Place::Full), layer(Shows::Words, Place::Full)]);
        let out = draw(&s, &solid(BLUE), Some(&solid(RED)), &[], Some(&words));
        assert_eq!((px(&out, 3, 15), px(&out, 3, 2)), ([255; 4], RED));
        assert_eq!(draw(&s, &solid(BLUE), Some(&solid(RED)), &[], None), solid(RED), "no words live: nothing over the camera");
    }

    #[test]
    fn a_camera_in_a_half_keeps_its_shape() {
        // A picture with a red left half and a blue right half, in the left half of the stream: its middle shows.
        let mut cam = vec![0u8; (W * H * 4) as usize];
        for y in 0..H { for x in 0..W { let i = ((y * W + x) * 4) as usize; cam[i..i + 4].copy_from_slice(if x < W / 2 { &RED } else { &BLUE }) } }
        let s = scene(vec![layer(Shows::Camera { number: 0 }, Place::LeftHalf)]);
        let out = draw(&s, &solid(GREEN), Some(&cam), &[], None);
        assert_eq!((px(&out, 1, 10), px(&out, 18, 10)), (RED, BLUE), "the middle of the picture, not squeezed");
        assert_eq!(px(&out, 30, 10), [0, 0, 0, 255]);
    }

    #[test]
    fn places_stay_inside_the_picture() {
        assert_eq!(Place::Full.rect((1920, 1080)), (0, 0, 1920, 1080));
        assert_eq!(Place::LeftHalf.rect((1920, 1080)), (0, 0, 960, 1080));
        assert_eq!(Place::RightHalf.rect((1921, 1080)), (960, 0, 961, 1080));
        assert_eq!(Place::Box { x: 90, y: 90, w: 50, h: 50 }.rect((1000, 500)), (900, 450, 100, 50));
        assert_eq!(Place::Box { x: 10, y: 20, w: 30, h: 40 }.rect((1000, 500)), (100, 100, 300, 200));
        let corner = Place::Corner { corner: Corner::TopLeft, size: CornerSize::Large };
        assert_eq!(corner.rect((1280, 720)), corner_rect((1280, 720), &Layout { corner: Corner::TopLeft, corner_size: CornerSize::Large, words_top: false }));
        assert_eq!(Place::RightHalf.as_box(), Place::Box { x: 50, y: 0, w: 50, h: 100 });
    }

    #[test]
    fn every_choice_has_its_own_place_in_the_lists() {
        for (i, p) in Place::choices().iter().enumerate() { assert_eq!(p.index(), i, "{p:?}") }
        for (i, s) in Shows::choices().iter().enumerate() { assert_eq!(s.index(), Some(i), "{s:?}") }
        assert_eq!(Place::Box { x: 1, y: 2, w: 3, h: 4 }.index(), Place::choices().len() - 1);
        assert_eq!(Shows::Unknown.index(), None);
    }

    #[test]
    fn changing_what_a_layer_shows_keeps_what_it_held() {
        let pic = Shows::Picture { path: "logo.png".into() };
        assert_eq!(pic.changed_to(&Shows::Picture { path: String::new() }), pic);
        assert_eq!(Shows::Room.changed_to(&Shows::Text { text: String::new() }), Shows::Text { text: "Welcome".into() });
        assert_eq!(Shows::Room.changed_to(&Shows::Colour { colour: String::new() }), Shows::Colour { colour: "#000000".into() });
        assert_eq!(pic.changed_to(&Shows::Camera { number: 2 }), Shows::Camera { number: 2 });
    }

    #[test]
    fn a_text_layer_is_drawn_in_its_place_only() {
        let s = scene(vec![layer(Shows::Text { text: "Welcome".into() }, Place::Box { x: 0, y: 0, w: 50, h: 100 })]);
        let mut fonts = crate::render::Fonts::bundled_only();
        let size = (320, 180);
        let p = prepare(&s, size, &Theme::default(), &mut fonts);
        let room = [0u8, 0, 0, 255].repeat(320 * 180);
        let mut out = vec![0u8; 320 * 180 * 4];
        compose(&p, &Inputs { size, room: &room, camera: None, words: None, card: None, cameras: &[], own: None }, &mut out);
        let lit = |x0: usize, x1: usize| (0..180).any(|y| (x0..x1).any(|x| out[(y * 320 + x) * 4] > 100));
        assert!(lit(0, 160), "the words in the left half");
        assert!(!lit(160, 320), "nothing in the right half");
    }

    #[test]
    fn a_picture_layer_keeps_its_see_through_parts() {
        let dir = std::env::temp_dir().join(format!("rp-test-own-scene-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("logo.png");
        // Left half red, right half clear.
        let img = image::RgbaImage::from_fn(20, 20, |x, _| if x < 10 { image::Rgba([200, 0, 0, 255]) } else { image::Rgba([0, 0, 0, 0]) });
        img.save(&path).unwrap();
        let s = scene(vec![layer(Shows::Room, Place::Full), layer(Shows::Picture { path: path.to_string_lossy().into() }, Place::Box { x: 0, y: 0, w: 50, h: 100 })]);
        let out = draw(&s, &solid(BLUE), None, &[], None);
        assert_eq!(px(&out, 7, 10), RED, "the picture's solid part");
        assert_eq!(px(&out, 13, 10), BLUE, "its clear part shows the room");
        assert_eq!(px(&out, 30, 10), BLUE);
        let missing = scene(vec![layer(Shows::Room, Place::Full), layer(Shows::Picture { path: "C:/no/such.png".into() }, Place::Full)]);
        assert_eq!(draw(&missing, &solid(BLUE), None, &[], None), solid(BLUE), "a picture that cannot be read is left out");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_church_scene_stands_in_only_where_it_is_asked_to() {
        let mut ours = OwnScene::new("Songs".into());
        ours.instead_of = Some(Scene::CameraWords);
        let list = vec![ours.clone()];
        assert_eq!(stand_in(Scene::CameraWords, &list), Scene::Own(ours.id));
        assert_eq!(stand_in(Scene::Camera, &list), Scene::Camera);
        assert_eq!(name(Scene::Own(ours.id), &list), "Songs");
        assert_eq!(name(Scene::Own(Uuid::from_u128(9)), &list), "A scene that was deleted");
        assert_eq!(all(&list).last(), Some(&Scene::Own(ours.id)));
        assert_eq!(all(&list).len(), Scene::ALL.len() + 1);
    }

    #[test]
    fn which_camera_a_scene_shows() {
        let s = scene(vec![layer(Shows::Camera { number: 2 }, Place::Full)]);
        assert!(s.shows_camera(1) && !s.shows_camera(0));
        let list = vec![s.clone()];
        assert!(shows_camera(Scene::Own(s.id), 1, &list));
        assert!(!shows_camera(Scene::Own(s.id), 0, &list));
        assert!(shows_camera(Scene::Camera, 0, &list));
        assert!(OwnScene::new("x".into()).shows_camera(3), "the camera on air, whichever it is");
    }

    #[test]
    fn scenes_are_kept_and_read_back_and_newer_kinds_do_not_break_them() {
        let s = OwnScene::new("Welcome".into());
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["layers"][1], serde_json::json!({ "shows": { "kind": "camera", "number": 0 }, "place": { "at": "corner", "corner": "bottom_right", "size": "medium" } }));
        assert_eq!(serde_json::from_value::<OwnScene>(v).unwrap(), s);
        let newer = serde_json::json!({ "id": Uuid::from_u128(1), "name": "x", "layers": [{ "shows": { "kind": "hologram" }, "place": { "at": "spiral" }, "see_through": 10 }] });
        let s: OwnScene = serde_json::from_value(newer).unwrap();
        assert_eq!((s.layers[0].shows.clone(), s.layers[0].place), (Shows::Unknown, Place::Full));
    }

    #[test]
    fn the_preview_draws_the_stand_ins() {
        let mut fonts = crate::render::Fonts::bundled_only();
        let stand = StandIns::new((160, 90), &Theme::default(), &mut fonts);
        let out = preview(&OwnScene::new("x".into()), &stand, &Theme::default(), &mut fonts);
        assert_eq!(out.len(), 160 * 90 * 4);
        assert_eq!(&out[..3], &stand.room[..3], "the room screen behind");
        let (x, y, _, ch) = corner_rect((160, 90), &Layout::default());
        let i = (((y + ch - 1) * 160 + x + 1) * 4) as usize;
        assert_eq!(&out[i..i + 3], &[92, 92, 92], "the grey camera in the corner");
    }
}
