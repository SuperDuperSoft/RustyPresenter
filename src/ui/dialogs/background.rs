//! The background picker (`backgroundPicker()`, `operator.js:701-718`; `ui/dialogs/background-picker.slint`):
//! Theme default, six swatches, a custom colour, every ready picture or video, and "+ Upload…". The choice list
//! and the matching of the current value are pure; `Picker` keeps the thumbnails and the value for one dialog.
#[path = "../../strings/item_edit.rs"]
pub(super) mod strings;

use crate::{
    app::{ImportPurpose, Imported},
    import::CANCELLED,
    model::{AssetId, Background, MediaFile, MediaKind, MediaStatus},
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
            ChoiceKind::Image(a) => Some(Some(Background::Image { asset: a.clone() })),
            ChoiceKind::Video(a) => Some(Some(Background::Video { asset: a.clone() })),
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
            ChoiceKind::Upload => "upload",
        }
    }
}

/// The tiles in the web's order: "Theme default" (when allowed), the six swatches, the custom colour, every
/// **ready** picture or video in library order (videos captioned "▶ name"; the tooltip is the name), "+ Upload…".
pub fn choices(media: &[MediaFile], allow_default: bool) -> Vec<Choice> {
    let mut out = Vec::new();
    if allow_default { out.push(Choice::new(ChoiceKind::Default, strings::THEME_DEFAULT, "")); }
    out.extend(SWATCHES.iter().map(|c| Choice::new(ChoiceKind::Color((*c).into()), "", "")));
    out.push(Choice::new(ChoiceKind::Custom, "", ""));
    for m in media.iter().filter(|m| m.status == MediaStatus::Ready) {
        match m.kind {
            MediaKind::Image => out.push(Choice::new(ChoiceKind::Image(m.asset.clone()), m.name.clone(), m.name.clone()).showing(Some(m.asset.clone()))),
            MediaKind::Video => out.push(Choice::new(ChoiceKind::Video(m.asset.clone()), strings::video_choice(&m.name), m.name.clone()).showing(m.thumb.clone())),
            MediaKind::Audio => {}
        }
    }
    out.push(Choice::new(ChoiceKind::Upload, strings::UPLOAD, ""));
    out
}

/// The tile drawn "on": the first whose value equals `value` (the web compares the JSON).
pub fn selected(choices: &[Choice], value: &Option<Background>) -> Option<usize> {
    choices.iter().position(|c| c.value().as_ref() == Some(value))
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
    allow_default: bool,
    media: Vec<MediaFile>,
    choices: Vec<Choice>,
    images: Rc<RefCell<HashMap<AssetId, Image>>>,
    thumbs: Thumbs,
}

/// A tile is 112 px wide; thumbnails are drawn at twice that for high-DPI screens.
const THUMB: (u32, u32) = (224, 126);

impl Picker {
    pub fn new(op: &Operator, value: Option<Background>, media: Vec<MediaFile>, allow_default: bool) -> Self {
        let root = op.app().store.root.clone();
        let thumbs = if op.ops().is_inline() { Thumbs::inline(root) } else { Thumbs::new(root) };
        let choices = choices(&media, allow_default);
        Self { value, allow_default, media, choices, images: Rc::default(), thumbs }
    }

    pub fn choices(&self) -> &[Choice] { &self.choices }

    /// Adds files imported by "+ Upload…" (the web's `refresh()` before `draw()`), so they are pickable at once.
    pub fn add_media(&mut self, files: Vec<MediaFile>) {
        for m in files {
            match self.media.iter_mut().find(|x| x.id == m.id) {
                Some(x) => *x = m,
                None => self.media.push(m),
            }
        }
        self.choices = choices(&self.media, self.allow_default);
    }

    /// The library as a newer read has it (Settings redraws its pages from each read); thumbnails already drawn
    /// are kept.
    pub fn set_media(&mut self, media: Vec<MediaFile>) {
        self.media = media;
        self.choices = choices(&self.media, self.allow_default);
    }

    /// Sets the value from tile `i`; false for the custom colour, Upload or an index out of range.
    pub fn pick(&mut self, i: usize) -> bool {
        let Some(v) = self.choices.get(i).and_then(Choice::value) else { return false };
        self.value = v;
        true
    }

    pub fn set_color(&mut self, hex: String) { self.value = Some(Background::Color { color: hex }); }

    /// The tiles for `.slint`. Pictures not drawn yet are asked for; `redraw` runs when one arrives.
    pub fn rows(&self, redraw: impl Fn() + Clone + 'static) -> Vec<BgChoice> {
        let on = selected(&self.choices, &self.value);
        let custom = match &self.value { Some(Background::Color { color }) => color.as_str(), _ => CUSTOM_DEFAULT };
        self.choices.iter().enumerate().map(|(i, c)| {
            let swatch = match &c.kind { ChoiceKind::Color(hex) => to_color(hex), ChoiceKind::Custom => to_color(custom), _ => Color::default() };
            let image = c.picture.as_ref().map_or_else(Image::default, |a| self.image(a, redraw.clone()));
            BgChoice {
                label: c.label.as_str().into(), swatch, image, kind: c.kind_id().into(),
                on: on == Some(i), tooltip: c.tooltip.as_str().into(),
            }
        }).collect()
    }

    fn image(&self, asset: &AssetId, redraw: impl Fn() + 'static) -> Image {
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
            op.toast(strings::UPLOADED, false);
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
        MediaFile { id: Uuid::new_v4(), name: name.into(), kind, asset: format!("{name}.bin"), status, original_asset: None, duration: None, thumb: None, size: None }
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
        assert_eq!(kinds, ["default", "color", "color", "color", "color", "color", "color", "custom", "image", "video", "upload"]);
        assert_eq!(c[0].label, "Theme default");
        let swatches: Vec<_> = c[1..7].iter().map(|c| c.kind.clone()).collect();
        assert_eq!(swatches, SWATCHES.map(|s| ChoiceKind::Color(s.into())));
        assert_eq!((c[8].label.as_str(), c[8].tooltip.as_str()), ("Sky", "Sky"));
        assert_eq!((c[9].label.as_str(), c[9].tooltip.as_str()), ("▶ Loop", "Loop"));
        assert_eq!(c[9].kind, ChoiceKind::Video("Loop.bin".into()));
        assert_eq!(c[10].label, "+ Upload…");

        let c = choices(&lib, false);
        assert_eq!(c[0].kind, ChoiceKind::Color("#000000".into()), "no Theme default when not allowed");
        assert_eq!(c.len(), 10);
    }

    #[test]
    fn background_picker_video_choice_has_thumb_image() {
        let plain = media("Loop", MediaKind::Video, MediaStatus::Ready);
        let framed = MediaFile { thumb: Some("frame.jpg".into()), ..media("Walk", MediaKind::Video, MediaStatus::Ready) };
        let sky = media("Sky", MediaKind::Image, MediaStatus::Ready);
        let c = choices(&[plain, framed, sky], false);
        let shown: Vec<Option<&str>> = c[7..10].iter().map(|c| c.picture.as_deref()).collect();
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
        assert_eq!(selected(&c, &Some(Background::Image { asset: "Sky.bin".into() })), Some(8));
        assert_eq!(selected(&c, &Some(Background::Video { asset: "Loop.bin".into() })), Some(9));
        assert_eq!(selected(&c, &Some(Background::Image { asset: "Loop.bin".into() })), None, "same asset, other kind");
        assert_eq!(selected(&choices(&lib, false), &None), None);
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
