//! The background picker (`backgroundPicker()`, `operator.js:701-718`; `ui/dialogs/background-picker.slint`):
//! Theme default, six swatches, a custom colour, the six gradients, the church's background library, every other
//! ready picture or video, and "+ Upload…" (FEAT-1 spec §3.4). The choice list and the matching of the current
//! value are pure; `Picker` keeps the thumbnails and the value for one dialog.
#[path = "../../strings/item_edit.rs"]
pub(super) mod strings;

use crate::{
    app::{ImportPurpose, Imported},
    import::CANCELLED,
    model::{AssetId, Background, BackgroundEntry, MediaFile, MediaKind, MediaStatus},
    ui::{BgChoice, operator::{file_input::Filter, files::pick, window::Operator}, thumbs::{self, ThumbJob, ThumbKey, ThumbMedia, Thumbs}},
};
use slint::{Color, Image};
use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc};

/// The six swatches (`operator.js:703`), as data rather than `.slint` literals.
pub const SWATCHES: [&str; 6] = ["#000000", "#0b1a33", "#1d2b1f", "#2b1530", "#3a2412", "#ffffff"];
/// The custom colour's swatch when the value is not a colour (`value: '#123456'`).
pub const CUSTOM_DEFAULT: &str = "#123456";

/// What one tile picks.
#[derive(Clone, Debug, PartialEq)]
pub enum ChoiceKind {
    Default,
    Color(String),
    /// The custom colour: opens the grid; it is not itself a value.
    Custom,
    Image(AssetId),
    Video(AssetId),
    /// A built-in gradient, or a background from the library (any kind).
    Value(Background),
    /// "+ Upload…": imports files.
    Upload,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub kind: ChoiceKind,
    pub label: String,
    pub tooltip: String,
    /// What the tile shows: a picture, or a video's saved frame.
    pub picture: Option<AssetId>,
}

impl Choice {
    fn new(kind: ChoiceKind, label: impl Into<String>, tooltip: impl Into<String>) -> Self { Self { kind, label: label.into(), tooltip: tooltip.into(), picture: None } }
    fn showing(mut self, picture: Option<AssetId>) -> Self { self.picture = picture; self }

    /// The background this tile sets (`Some(None)` is the theme default); `None` for the custom colour and Upload.
    pub fn value(&self) -> Option<Option<Background>> {
        match &self.kind {
            ChoiceKind::Default => Some(None),
            ChoiceKind::Color(c) => Some(Some(Background::Color { color: c.clone() })),
            ChoiceKind::Image(a) => Some(Some(Background::Image { asset: a.clone(), dim: 0 })),
            ChoiceKind::Video(a) => Some(Some(Background::Video { asset: a.clone(), dim: 0 })),
            ChoiceKind::Value(b) => Some(Some(b.clone())),
            ChoiceKind::Custom | ChoiceKind::Upload => None,
        }
    }

    /// The name the `.slint` tile switches on.
    pub fn kind_id(&self) -> &'static str {
        match self.kind {
            ChoiceKind::Default => "default",
            ChoiceKind::Color(_) => "color",
            ChoiceKind::Custom => "custom",
            ChoiceKind::Image(_) => "image",
            ChoiceKind::Video(_) => "video",
            ChoiceKind::Value(Background::Color { .. }) => "color",
            ChoiceKind::Value(Background::Video { .. }) => "video",
            ChoiceKind::Value(_) => "image",
            ChoiceKind::Upload => "upload",
        }
    }
}

/// The tiles in the web's order: "Theme default" (when allowed), the six swatches, the custom colour, every
/// **ready** picture or video in library order (videos captioned "▶ name"; the tooltip is the name), "+ Upload…".
pub fn choices(media: &[MediaFile], allow_default: bool) -> Vec<Choice> {
    choices_with(media, &[], allow_default.then(strings::theme_default))
}

/// `choices` with the six built-in gradients after the custom colour, then the background library's ready entries
/// (a picture or video already there is not listed again from the media), and the first tile named `default`
/// ("Theme default", "From theme", "Standard background") when there is one.
pub fn choices_with(media: &[MediaFile], library: &[BackgroundEntry], default: Option<String>) -> Vec<Choice> {
    let mut out = Vec::new();
    if let Some(label) = default { out.push(Choice::new(ChoiceKind::Default, label, "")); }
    out.extend(SWATCHES.iter().map(|c| Choice::new(ChoiceKind::Color((*c).into()), "", "")));
    out.push(Choice::new(ChoiceKind::Custom, "", ""));
    for (name, b) in crate::theme::backgrounds::builtin_backgrounds().into_iter().filter(|(_, b)| matches!(b, Background::Gradient { .. })) {
        out.push(Choice::new(ChoiceKind::Value(b), "", name));
    }
    for e in library.iter().filter(|e| e.pickable()) {
        let picture = match &e.background {
            Background::Image { asset, .. } => Some(asset.clone()),
            Background::Video { asset, .. } => e.thumb.clone().or_else(|| media.iter().find(|m| m.asset == *asset).and_then(|m| m.thumb.clone())),
            _ => None,
        };
        let label = if matches!(e.background, Background::Video { .. }) { strings::video_choice(&e.name) } else { e.name.clone() };
        out.push(Choice::new(ChoiceKind::Value(e.background.clone().with_dim(0)), label, e.name.clone()).showing(picture));
    }
    let in_library = |a: &AssetId| library.iter().any(|e| e.background.asset() == Some(a));
    for m in media.iter().filter(|m| m.status == MediaStatus::Ready && !in_library(&m.asset)) {
        match m.kind {
            MediaKind::Image => out.push(Choice::new(ChoiceKind::Image(m.asset.clone()), m.name.clone(), m.name.clone()).showing(Some(m.asset.clone()))),
            MediaKind::Video => out.push(Choice::new(ChoiceKind::Video(m.asset.clone()), strings::video_choice(&m.name), m.name.clone()).showing(m.thumb.clone())),
            MediaKind::Audio => {}
        }
    }
    out.push(Choice::new(ChoiceKind::Upload, strings::upload(), ""));
    out
}

/// The tile drawn "on": the first whose value equals `value` (the web compares the JSON), whatever its Darken.
pub fn selected(choices: &[Choice], value: &Option<Background>) -> Option<usize> {
    choices.iter().position(|c| match (c.value(), value) {
        (Some(Some(a)), Some(b)) => a.same_source(b),
        (Some(None), None) => true,
        _ => false,
    })
}

/// Darken applies to pictures, videos and gradients (spec §3.4 step 6).
pub fn darken_shown(value: &Option<Background>) -> bool {
    matches!(value, Some(Background::Image { .. } | Background::Video { .. } | Background::Gradient { .. }))
}

/// `#rrggbb` (or `rrggbb`, any case) as the lower-case `#rrggbb` the web's colour input gives; `None` otherwise.
pub fn parse_hex(text: &str) -> Option<String> {
    let t = text.trim();
    let h = t.strip_prefix('#').unwrap_or(t);
    (h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| format!("#{}", h.to_ascii_lowercase()))
}

pub fn to_color(hex: &str) -> Color {
    let h = parse_hex(hex).unwrap_or_else(|| CUSTOM_DEFAULT.into());
    let v = u32::from_str_radix(&h[1..], 16).unwrap_or(0);
    Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub fn to_hex(c: Color) -> String { format!("#{:02x}{:02x}{:02x}", c.red(), c.green(), c.blue()) }

/// The custom colour's grid: a row of greys, then twelve hues in four shades.
pub fn palette() -> Vec<Color> {
    let mut out: Vec<Color> = (0..12).map(|i| { let v = (i * 255 / 11) as u8; Color::from_rgb_u8(v, v, v) }).collect();
    for (s, v) in [(0.9, 0.35), (0.75, 0.6), (0.6, 0.85), (0.35, 0.95)] {
        for h in 0..12 { out.push(hsv(h as f32 * 30.0, s, v)); }
    }
    out
}

fn hsv(h: f32, s: f32, v: f32) -> Color {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 { 0 => (c, x, 0.0), 1 => (x, c, 0.0), 2 => (0.0, c, x), 3 => (0.0, x, c), 4 => (x, 0.0, c), _ => (c, 0.0, x) };
    let m = v - c;
    let to = |f: f32| ((f + m) * 255.0).round() as u8;
    Color::from_rgb_u8(to(r), to(g), to(b))
}

/// The picker of one open dialog: the library's media as this dialog knows it, the chosen value and the tiles'
/// thumbnails. Thumbnails are drawn on a worker of the picker's own (`Thumbs`), so a slide grid redraw never
/// cancels them.
pub struct Picker {
    pub value: Option<Background>,
    default: Option<String>,
    media: Vec<MediaFile>,
    library: Vec<BackgroundEntry>,
    choices: Vec<Choice>,
    stills: Stills,
}

/// Small pictures of stills and video frames, drawn on a worker of their own (`Thumbs`), kept by asset.
pub struct Stills {
    images: Rc<RefCell<HashMap<AssetId, Image>>>,
    thumbs: Thumbs,
}

/// A tile is 112 px wide; thumbnails are drawn at twice that for high-DPI screens.
const THUMB: (u32, u32) = (224, 126);

impl Picker {
    pub fn new(op: &Operator, value: Option<Background>, media: Vec<MediaFile>, allow_default: bool) -> Self {
        Self::with_default(op, value, media, allow_default.then(strings::theme_default))
    }

    /// A picker whose first tile is named `default` (or has none), listing the newest background library too.
    pub fn with_default(op: &Operator, value: Option<Background>, media: Vec<MediaFile>, default: Option<String>) -> Self {
        let library = op.state.borrow().snap.as_ref().map(|s| s.data.backgrounds.clone()).unwrap_or_default();
        let choices = choices_with(&media, &library, default.clone());
        Self { value, default, media, library, choices, stills: Stills::new(op) }
    }

    fn rebuild(&mut self) { self.choices = choices_with(&self.media, &self.library, self.default.clone()) }

    pub fn choices(&self) -> &[Choice] { &self.choices }

    /// Adds files imported by "+ Upload…" (the web's `refresh()` before `draw()`), so they are pickable at once.
    pub fn add_media(&mut self, files: Vec<MediaFile>) {
        for m in files {
            match self.media.iter_mut().find(|x| x.id == m.id) {
                Some(x) => *x = m,
                None => self.media.push(m),
            }
        }
        self.rebuild();
    }

    /// The library as a newer read has it (Settings redraws its pages from each read); thumbnails already drawn
    /// are kept.
    pub fn set_media(&mut self, media: Vec<MediaFile>) {
        self.media = media;
        self.rebuild();
    }

    /// The background library as a newer read has it.
    pub fn set_library(&mut self, library: Vec<BackgroundEntry>) {
        self.library = library;
        self.rebuild();
    }

    /// Sets the value from tile `i`, keeping the Darken already chosen; false for the custom colour, Upload or an
    /// index out of range.
    pub fn pick(&mut self, i: usize) -> bool {
        let Some(v) = self.choices.get(i).and_then(Choice::value) else { return false };
        let dim = self.value.as_ref().map_or(0, Background::dim);
        self.value = v.map(|b| b.with_dim(dim));
        true
    }

    /// The Darken value of a picture, video or gradient (0–80 %).
    pub fn set_dim(&mut self, dim: u8) {
        if let Some(v) = self.value.take() { self.value = Some(v.with_dim(dim)) }
    }

    pub fn set_color(&mut self, hex: String) { self.value = Some(Background::Color { color: hex }); }

    /// The tiles for `.slint`. Pictures not drawn yet are asked for; `redraw` runs when one arrives.
    pub fn rows(&self, redraw: impl Fn() + Clone + 'static) -> Vec<BgChoice> {
        let on = selected(&self.choices, &self.value);
        let custom = match &self.value { Some(Background::Color { color }) => color.as_str(), _ => CUSTOM_DEFAULT };
        self.choices.iter().enumerate().map(|(i, c)| {
            let swatch = match &c.kind {
                ChoiceKind::Color(hex) | ChoiceKind::Value(Background::Color { color: hex }) => to_color(hex),
                ChoiceKind::Custom => to_color(custom),
                _ => Color::default(),
            };
            let image = match (&c.picture, &c.kind) {
                (Some(a), _) => self.stills.image(a, redraw.clone()),
                (None, ChoiceKind::Value(b @ Background::Gradient { .. })) => gradient_image(b, (THUMB.0 / 2, THUMB.1 / 2)),
                _ => Image::default(),
            };
            BgChoice {
                label: c.label.as_str().into(), swatch, image, kind: c.kind_id().into(),
                on: on == Some(i), tooltip: c.tooltip.as_str().into(),
            }
        }).collect()
    }

}

/// A gradient drawn as the room draws it, at `size`.
pub fn gradient_image(b: &Background, size: (u32, u32)) -> Image {
    Image::from_rgba8_premultiplied(crate::render::background::background_pixels(b, None, size).rgba_premul.buffer())
}

impl Stills {
    pub fn new(op: &Operator) -> Self {
        let root = op.app().store.root.clone();
        let thumbs = if op.ops().is_inline() { Thumbs::inline(root) } else { Thumbs::new(root) };
        Self { images: Rc::default(), thumbs }
    }

    /// The picture of `asset`, black until drawn; `redraw` runs when it arrives.
    pub fn image(&self, asset: &AssetId, redraw: impl Fn() + 'static) -> Image {
        if let Some(img) = self.images.borrow().get(asset) { return img.clone() }
        let job = ThumbJob { background: None, media: Some(ThumbMedia::Still { asset: asset.clone() }), picture: None, text: None };
        let key = ThumbKey { render: None, layers: serde_json::to_string(&job).unwrap_or_default(), size: THUMB };
        let (images, a) = (self.images.clone(), asset.clone());
        // Marked as asked for, so a redraw before it arrives does not ask again.
        images.borrow_mut().insert(a.clone(), Image::default());
        let flag = Rc::new(std::cell::Cell::new(true));
        let during = flag.clone();
        self.thumbs.request(key, job, move |px: thumbs::Pixels| {
            images.borrow_mut().insert(a, Image::from_rgba8(px));
            // Drawn at once (cache or inline mode): the caller is still building the rows, which read the map.
            if !during.get() { redraw() }
        });
        flag.set(false);
        self.images.borrow().get(asset).cloned().unwrap_or_default()
    }
}

/// "+ Upload…": the picture-and-video picker (several files), then `upload_files`.
pub fn upload(op: &Rc<Operator>, done: impl FnOnce(Vec<MediaFile>) + 'static) {
    let me = Rc::downgrade(op);
    pick(op, Filter::PicturesAndVideos, true, move |paths| {
        if let Some(op) = me.upgrade() && !paths.is_empty() { upload_files(&op, paths, done) }
    });
}

/// Imports every file in turn with `Ops::import(Media)` (`operator.js:711-713`), then calls `done` with what was
/// added and toasts "Added to the media library — pick it here". The first failure is an error toast with the
/// core's message, and the rest are not imported (the web's `guard`).
pub fn upload_files(op: &Rc<Operator>, paths: Vec<PathBuf>, done: impl FnOnce(Vec<MediaFile>) + 'static) {
    fn next(op: &Rc<Operator>, mut rest: std::vec::IntoIter<PathBuf>, mut got: Vec<MediaFile>, done: Box<dyn FnOnce(Vec<MediaFile>)>) {
        let Some(path) = rest.next() else {
            done(got);
            op.toast(strings::uploaded(), false);
            return;
        };
        let me = Rc::downgrade(op);
        op.ops().import(path, ImportPurpose::Media, |_| {}, move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(Imported::Media(m)) => { got.push(m); next(&op, rest, got, done) }
                Ok(_) => next(&op, rest, got, done),
                // The operator pressed Cancel on the card: nothing to say, and nothing more to add.
                Err(e) if e == CANCELLED => {}
                Err(e) => op.toast(e, true),
            }
        });
    }
    next(op, paths.into_iter(), Vec::new(), Box::new(done));
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn media(name: &str, kind: MediaKind, status: MediaStatus) -> MediaFile {
        MediaFile { id: Uuid::new_v4(), name: name.into(), kind, asset: format!("{name}.bin"), status, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() }
    }

    #[test]
    fn background_choices_order_and_ready_only() {
        let lib = [
            media("Sky", MediaKind::Image, MediaStatus::Ready),
            media("Waves", MediaKind::Video, MediaStatus::Converting { percent: 10 }),
            media("Loop", MediaKind::Video, MediaStatus::Ready),
            media("Hymn", MediaKind::Audio, MediaStatus::Ready),
            media("Odd", MediaKind::Video, MediaStatus::Unconverted { reason: "x".into() }),
            media("Bad", MediaKind::Image, MediaStatus::Failed { reason: "x".into() }),
        ];
        let c = choices(&lib, true);
        let kinds: Vec<&str> = c.iter().map(Choice::kind_id).collect();
        assert_eq!(kinds, ["default", "color", "color", "color", "color", "color", "color", "custom",
            "image", "image", "image", "image", "image", "image", "image", "video", "upload"]);
        let c: Vec<Choice> = c.into_iter().enumerate().filter(|(i, _)| !(8..14).contains(i)).map(|(_, c)| c).collect();
        assert_eq!(c[0].label, "Theme default");
        let swatches: Vec<_> = c[1..7].iter().map(|c| c.kind.clone()).collect();
        assert_eq!(swatches, SWATCHES.map(|s| ChoiceKind::Color(s.into())));
        assert_eq!((c[8].label.as_str(), c[8].tooltip.as_str()), ("Sky", "Sky"));
        assert_eq!((c[9].label.as_str(), c[9].tooltip.as_str()), ("▶ Loop", "Loop"));
        assert_eq!(c[9].kind, ChoiceKind::Video("Loop.bin".into()));
        assert_eq!(c[10].label, "+ Upload…");

        let c = choices(&lib, false);
        assert_eq!(c[0].kind, ChoiceKind::Color("#000000".into()), "no Theme default when not allowed");
        assert_eq!(c.len(), 16);
    }

    #[test]
    fn background_picker_video_choice_has_thumb_image() {
        let plain = media("Loop", MediaKind::Video, MediaStatus::Ready);
        let framed = MediaFile { thumb: Some("frame.jpg".into()), ..media("Walk", MediaKind::Video, MediaStatus::Ready) };
        let sky = media("Sky", MediaKind::Image, MediaStatus::Ready);
        let c = choices(&[plain, framed, sky], false);
        let shown: Vec<Option<&str>> = c[13..16].iter().map(|c| c.picture.as_deref()).collect();
        assert_eq!(shown, [None, Some("frame.jpg"), Some("Sky.bin")]);
    }

    #[test]
    fn selected_choice_matches_current_value() {
        let lib = [media("Sky", MediaKind::Image, MediaStatus::Ready), media("Loop", MediaKind::Video, MediaStatus::Ready)];
        let c = choices(&lib, true);
        assert_eq!(selected(&c, &None), Some(0), "Theme default");
        assert_eq!(selected(&c, &Some(Background::Color { color: "#0b1a33".into() })), Some(2));
        assert_eq!(selected(&c, &Some(Background::Color { color: "#0B1A33".into() })), None, "compared as the web does, exactly");
        assert_eq!(selected(&c, &Some(Background::Color { color: "#123456".into() })), None, "a custom colour is no tile");
        assert_eq!(selected(&c, &Some(Background::Image { asset: "Sky.bin".into(), dim: 0 })), Some(14));
        assert_eq!(selected(&c, &Some(Background::Video { asset: "Loop.bin".into(), dim: 0 })), Some(15));
        assert_eq!(selected(&c, &Some(Background::Image { asset: "Sky.bin".into(), dim: 40 })), Some(14), "Darken is not a new tile");
        assert_eq!(selected(&c, &Some(Background::Image { asset: "Loop.bin".into(), dim: 0 })), None, "same asset, other kind");
        assert_eq!(selected(&choices(&lib, false), &None), None);
    }

    fn entry(name: &str, b: Background, status: MediaStatus) -> BackgroundEntry {
        BackgroundEntry::new(name, b, status, chrono::Utc::now())
    }

    #[test]
    fn picker_lists_gradients_then_library_then_other_media() {
        let sky = media("Sky", MediaKind::Image, MediaStatus::Ready);
        let lake = media("Lake", MediaKind::Image, MediaStatus::Ready);
        let library = [
            entry("Sky", Background::Image { asset: "Sky.bin".into(), dim: 0 }, MediaStatus::Ready),
            entry("Sunrise", Background::Gradient { from: "#ff8800".into(), to: "#000000".into(), angle: 90, dim: 0 }, MediaStatus::Ready),
            entry("Waves", Background::Video { asset: "w.mp4".into(), dim: 0 }, MediaStatus::Converting { percent: 40 }),
        ];
        let c = choices_with(&[sky, lake], &library, Some("Standard background".into()));
        assert_eq!(c[0].label, "Standard background");
        let tips: Vec<&str> = c[8..14].iter().map(|c| c.tooltip.as_str()).collect();
        assert_eq!(tips, ["Midnight", "Deep purple", "Forest", "Ember", "Ocean", "Slate"]);
        assert_eq!(c[14].label, "Sky", "the library first");
        assert_eq!(c[15].tooltip, "Sunrise");
        assert_eq!(c[16].label, "Lake", "then media not in the library; a converting entry is left out");
        assert_eq!(c[17].kind, ChoiceKind::Upload);
        assert_eq!(c.len(), 18);
    }

    #[test]
    fn darken_only_for_picture_video_gradient() {
        assert!(!darken_shown(&None));
        assert!(!darken_shown(&Some(Background::Color { color: "#000000".into() })));
        assert!(darken_shown(&Some(Background::Image { asset: "a".into(), dim: 0 })));
        assert!(darken_shown(&Some(Background::Gradient { from: "#000000".into(), to: "#ffffff".into(), angle: 0, dim: 0 })));
    }

    #[test]
    fn hex_colours() {
        assert_eq!(parse_hex("#AbCdEf").as_deref(), Some("#abcdef"));
        assert_eq!(parse_hex(" 123456 ").as_deref(), Some("#123456"));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#12345g"), None);
        assert_eq!(to_hex(to_color("#3a2412")), "#3a2412");
        assert_eq!(palette().len(), 60);
        assert_eq!(to_hex(palette()[0]), "#000000");
        assert_eq!(to_hex(palette()[11]), "#ffffff");
    }
}
