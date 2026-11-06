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
        /// Largest text size in pixels on a 1080p screen (a passage's own size); 0 = the theme's.
        #[serde(default, skip_serializing_if = "is_zero")]
        size: u32,
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

fn is_zero(n: &u32) -> bool { *n == 0 }

/// One slide of a passage, as far as fitting its text goes: all slides of a passage share one text size, the
/// largest that fits every one of them, so the text does not jump size between slides.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PassageSlide {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

/// The passage of an item's slides: every text slide of a scripture item, else none (songs fit slide by slide).
pub fn passage(kind: &str, slides: &[Slide]) -> Vec<PassageSlide> {
    if kind != "scripture" || slides.len() < 2 { return Vec::new() }
    slides.iter().filter_map(|s| match &s.content {
        SlideContent::Text { text, reference, .. } => Some(PassageSlide { text: text.clone(), reference: reference.clone() }),
        _ => None,
    }).collect()
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
    Converting { percent: u8 },
    /// Kept as uploaded; the browser engine may not play it.
    Unconverted { reason: String },
    Failed { reason: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaFile {
    pub id: Uuid,
    pub name: String,
    pub kind: MediaKind,
    pub asset: AssetId,
    pub status: MediaStatus,
    #[serde(default)]
    pub original_asset: Option<AssetId>,
    /// The media library folder this file is in; empty: none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
    /// A video's picture (a JPEG asset from a frame mpv grabbed), for the media library; `None` until grabbed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<AssetId>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Deck {
    pub id: Uuid,
    pub title: String,
    pub slides: Vec<AssetId>,
}

// ---------- Service plan ----------

/// Another Bible shown with a passage: its verses line up with the first Bible's (empty text where it has none).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AlsoShow {
    pub translation: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub abbr: String,
    pub verses: Vec<VerseText>,
}

/// How many Bibles a passage can show at once.
pub const MAX_BIBLES: usize = 3;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct VerseText {
    pub reference: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemKind {
    Song { song_id: Uuid },
    /// `per_slide`: verses per slide, 0 = automatic (by length). `text_size`: the passage's largest text size in
    /// pixels on a 1080p screen, 0 = automatic (Settings → Look's scripture size).
    /// `abbr` is the label slides show when the translation id is not one (`yv:149` → "RVR1960"); saved with the
    /// item so it survives the Bible being removed or the service moving to another computer. Empty: `translation`.
    Scripture {
        reference: String, translation: String, verses: Vec<VerseText>, #[serde(default)] per_slide: u8,
        #[serde(default)] text_size: u32,
        #[serde(default, skip_serializing_if = "String::is_empty")] abbr: String,
        /// Up to two more Bibles shown with this one, verse for verse, under it on each slide.
        #[serde(default, skip_serializing_if = "Vec::is_empty")] also: Vec<AlsoShow>,
    },
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
    /// Verses per slide for new Bible passages: 0 = automatic (by length), else 1–4.
    pub bible_per_slide: u8,
    /// Bibles shown under the chosen one with every new passage (at most two: three Bibles in all).
    pub also_show: Vec<String>,
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
            bible_per_slide: 0,
            also_show: Vec::new(),
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
    /// Media library folders, so one can exist before anything is in it.
    pub media_folders: Vec<String>,
    pub decks: Vec<Deck>,
    pub services: Vec<Service>,
    pub current_service: Option<Uuid>,
}

impl Default for Data {
    fn default() -> Self {
        let service = Service::new("Sunday Service");
        Self { format: 1, settings: Settings::default(), songs: vec![], media: vec![], media_folders: vec![], decks: vec![], current_service: Some(service.id), services: vec![service] }
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
            if let Some(o) = &m.original_asset { set.insert(o.clone()); }
            if let Some(t) = &m.thumb { set.insert(t.clone()); }
        }
        for d in &self.decks { set.extend(d.slides.iter().cloned()); }
        for s in &self.services { for i in &s.items { if let Some(b) = &i.background { bg(b, &mut set); } } }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_data_without_bible_sizing_loads_as_automatic() {
        let old = r#"{"format":1,"settings":{"lines_per_slide":2,"theme":{"scripture_size":70}},"services":[{"id":"6f7b3c1e-1a2b-4c3d-8e9f-001122334455","name":"Sunday","items":[
            {"id":"6f7b3c1e-1a2b-4c3d-8e9f-001122334466","kind":"scripture","reference":"John 3:16","translation":"KJV","verses":[],"per_slide":2}]}]}"#;
        let d: Data = serde_json::from_str(old).unwrap();
        assert_eq!(d.settings.bible_per_slide, 0);
        assert_eq!(d.settings.theme.scripture_size, 70);
        match &d.services[0].items[0].kind {
            ItemKind::Scripture { per_slide, text_size, .. } => assert_eq!((*per_slide, *text_size), (2, 0)),
            k => panic!("{k:?}"),
        }
    }

    #[test]
    fn bible_sizing_survives_json() {
        let mut d = Data::default();
        d.settings.bible_per_slide = 3;
        d.services[0].items.push(ServiceItem {
            id: Uuid::new_v4(), background: None, notes: String::new(),
            kind: ItemKind::Scripture { reference: "Psalm 23".into(), translation: "KJV".into(), verses: vec![], per_slide: 1, text_size: 96, abbr: String::new(), also: vec![] },
        });
        let back: Data = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back.settings.bible_per_slide, 3);
        assert!(matches!(back.services[0].items[0].kind, ItemKind::Scripture { per_slide: 1, text_size: 96, .. }));
    }

    #[test]
    fn a_slide_without_its_own_size_writes_no_size() {
        let c = SlideContent::Text { text: "a".into(), footer: None, reference: None, size: 0 };
        assert_eq!(serde_json::to_string(&c).unwrap(), r#"{"type":"text","text":"a"}"#);
        let back: SlideContent = serde_json::from_str(r#"{"type":"text","text":"a"}"#).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn passage_is_every_scripture_slide_and_none_for_songs() {
        let s = |t: &str| Slide { label: t.into(), content: SlideContent::Text { text: t.into(), footer: None, reference: Some(format!("{t} ref")), size: 0 }, notes: String::new() };
        let slides = [s("a"), s("b")];
        assert_eq!(passage("scripture", &slides), vec![
            PassageSlide { text: "a".into(), reference: Some("a ref".into()) },
            PassageSlide { text: "b".into(), reference: Some("b ref".into()) },
        ]);
        assert!(passage("song", &slides).is_empty(), "songs fit slide by slide");
        assert!(passage("scripture", &slides[..1]).is_empty(), "one slide fits itself");
    }
}
