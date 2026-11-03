//! Library, service plan, settings and the slide types every screen renders.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A content-addressed asset file name: `<sha256>.<ext>`.
pub type AssetId = String;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Background {
    Color { color: String },
    Image { asset: AssetId },
    Video { asset: AssetId },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EndAction {
    #[default]
    Stop,
    Loop,
    Hold,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SlideContent {
    /// Lyrics, scripture or announcement text over the item's background.
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        footer: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
    },
    /// A full-screen deck slide image (PowerPoint, images).
    Image { asset: AssetId },
    /// Foreground video with its own audio.
    Video { asset: AssetId, autoplay: bool, end: EndAction },
    /// Full-screen still image on the media layer.
    Still { asset: AssetId },
    /// Audio-only item (walk-in music). Does not change visuals.
    Audio { asset: AssetId, looped: bool },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Slide {
    pub label: String,
    pub content: SlideContent,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

// ---------- Library ----------

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Song {
    pub id: Uuid,
    pub title: String,
    #[serde(default)]
    pub writers: String,
    #[serde(default)]
    pub ccli: String,
    #[serde(default)]
    pub copyright: String,
    /// Lyrics with section labels on their own lines (`Verse 1`, `[Chorus]`).
    /// A blank line inside a section forces a new slide.
    pub lyrics: String,
    /// Section labels in performance order. Empty = the order they are written.
    #[serde(default)]
    pub arrangement: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Video,
    Image,
    Audio,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MediaStatus {
    Ready,
    /// Read-only: only an old `data.json` has it. `import::media::upgrade_media` turns it into `Ready`.
    Converting { percent: u8 },
    /// Read-only, as `Converting`. Kept as uploaded; the browser engine may not play it.
    Unconverted { reason: String },
    Failed { reason: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MediaFile {
    pub id: Uuid,
    pub name: String,
    pub kind: MediaKind,
    pub asset: AssetId,
    pub status: MediaStatus,
    #[serde(default)]
    pub original_asset: Option<AssetId>,
    /// Seconds, as far as the probe could tell.
    #[serde(default)]
    pub duration: Option<f64>,
    /// A 320 px wide JPEG. None for audio, and for entries that have not been probed yet.
    #[serde(default)]
    pub thumb: Option<AssetId>,
    /// Pixels, for a video or picture.
    #[serde(default)]
    pub size: Option<(u32, u32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Deck {
    pub id: Uuid,
    pub title: String,
    pub slides: Vec<AssetId>,
}

// ---------- Service plan ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct VerseText {
    pub reference: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemKind {
    Song { song_id: Uuid },
    Scripture { reference: String, translation: String, verses: Vec<VerseText>, #[serde(default)] per_slide: u8 },
    Media { media_id: Uuid, #[serde(default = "yes")] autoplay: bool, #[serde(default)] end: EndAction, #[serde(default)] looped: bool },
    Deck { deck_id: Uuid },
    Text { title: String, body: String },
}

fn yes() -> bool { true }

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ServiceItem {
    pub id: Uuid,
    #[serde(flatten)]
    pub kind: ItemKind,
    #[serde(default)]
    pub background: Option<Background>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Service {
    pub id: Uuid,
    pub name: String,
    pub items: Vec<ServiceItem>,
}

impl Service {
    pub fn new(name: &str) -> Self { Self { id: Uuid::new_v4(), name: name.into(), items: vec![] } }
}

// ---------- Settings ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(default)]
pub struct Theme {
    pub font: String,
    /// Largest text size in pixels on a 1080p screen. Text shrinks to fit.
    pub size: u32,
    pub color: String,
    pub shadow: bool,
    /// "center" or "lower"
    pub position: String,
    pub uppercase: bool,
    pub scripture_size: u32,
}

impl Default for Theme {
    fn default() -> Self {
        Self { font: "Segoe UI".into(), size: 84, color: "#ffffff".into(), shadow: true, position: "center".into(), uppercase: false, scripture_size: 64 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct DisplayRoles {
    /// Windows monitor device name, e.g. `\\.\DISPLAY2`.
    pub room: Option<String>,
    pub stage: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub background: Background,
    pub logo: Option<AssetId>,
    pub lines_per_slide: u8,
    /// "first", "last" or "none"
    pub credits: String,
    pub church_ccli: String,
    pub translation: String,
    pub displays: DisplayRoles,
    /// Keep the room window on top of other windows while it is full screen on a monitor of its own, so Windows
    /// paints it first (smooth fades on the projector while the operator is busy). Never applies when the room
    /// shares the operator's monitor.
    pub room_on_top: bool,
    /// Where sound goes: an mpv device name, as `Mpv::audio_devices` lists it. Empty = the computer's default.
    pub audio_device: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            background: Background::Color { color: "#000000".into() },
            logo: None,
            lines_per_slide: 2,
            credits: "first".into(),
            church_ccli: String::new(),
            translation: "KJV".into(),
            displays: DisplayRoles::default(),
            room_on_top: true,
            audio_device: String::new(),
        }
    }
}

/// Everything persisted in `data.json`.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Data {
    pub format: u32,
    pub settings: Settings,
    pub songs: Vec<Song>,
    pub media: Vec<MediaFile>,
    pub decks: Vec<Deck>,
    pub services: Vec<Service>,
    pub current_service: Option<Uuid>,
}

impl Default for Data {
    fn default() -> Self {
        let service = Service::new("Sunday Service");
        Self { format: 1, settings: Settings::default(), songs: vec![], media: vec![], decks: vec![], current_service: Some(service.id), services: vec![service] }
    }
}

impl Data {
    pub fn service(&self) -> Option<&Service> {
        self.current_service.and_then(|id| self.services.iter().find(|s| s.id == id)).or(self.services.first())
    }
    pub fn service_mut(&mut self) -> Option<&mut Service> {
        let id = self.service()?.id;
        self.services.iter_mut().find(|s| s.id == id)
    }
    /// Every asset the library, services or settings reference.
    pub fn referenced_assets(&self) -> std::collections::BTreeSet<AssetId> {
        let mut set = std::collections::BTreeSet::new();
        let bg = |b: &Background, set: &mut std::collections::BTreeSet<AssetId>| match b {
            Background::Image { asset } | Background::Video { asset } => { set.insert(asset.clone()); }
            Background::Color { .. } => {}
        };
        bg(&self.settings.background, &mut set);
        if let Some(l) = &self.settings.logo { set.insert(l.clone()); }
        for m in &self.media {
            set.insert(m.asset.clone());
            set.extend(m.original_asset.iter().chain(&m.thumb).cloned());
        }
        for d in &self.decks { set.extend(d.slides.iter().cloned()); }
        for s in &self.services { for i in &s.items { if let Some(b) = &i.background { bg(b, &mut set); } } }
        set
    }
}
