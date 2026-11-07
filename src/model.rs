//! Library, service plan, settings and the slide types every screen renders.
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::collections::BTreeMap;

/// A content-addressed asset file name: `<sha256>.<ext>`.
pub type AssetId = String;

/// A colour, picture, video or gradient, each with a Darken value (FEAT-1 spec §4.3).
pub use crate::theme::backgrounds::{Background, BackgroundEntry};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EndAction {
    #[default]
    Stop,
    Loop,
    Hold,
    /// Hold the last frame and go to the next slide (N24).
    Next,
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
        /// More Bibles on this scripture slide; `text` is the main one's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parallel: Option<crate::scripture::Parallel>,
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
    /// The credit line one slide of a passage carries; it takes room from that slide's text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer: Option<String>,
}

/// The passage of an item's slides: every text slide of a scripture item, else none (songs fit slide by slide).
pub fn passage(kind: &str, slides: &[Slide]) -> Vec<PassageSlide> {
    if kind != "scripture" || slides.len() < 2 { return Vec::new() }
    slides.iter().filter_map(|s| match &s.content {
        SlideContent::Text { text, reference, footer, .. } => Some(PassageSlide { text: text.clone(), reference: reference.clone(), footer: footer.clone() }),
        _ => None,
    }).collect()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Slide {
    pub label: String,
    pub content: SlideContent,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Which song section this slide is from (FEAT-4). Some for song slides only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<crate::sections::SectionRef>,
}

impl Slide {
    /// A slide with no notes and no section.
    pub fn new(label: impl Into<String>, content: SlideContent) -> Self { Self { label: label.into(), content, notes: String::new(), section: None } }
}

// ---------- Library ----------

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
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
    /// The media library folder this file is in; empty: none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Deck {
    pub id: Uuid,
    pub title: String,
    pub slides: Vec<AssetId>,
}

// ---------- Service plan ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct VerseText {
    pub reference: String,
    pub text: String,
    /// The verse in English (KJV-style) numbering, when the lookup knew it. Labels in other languages can't be
    /// parsed reliably, so this is what lines Bibles up; items saved before it have none and fall back to the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<crate::scripture::VerseId>,
}

/// A slide's words for the stage screen and the phone: the text, then each Also Bible's, every one headed by its code
/// ("KJV  For God…", a blank line, "WEB  For God…") when the slide shows several Bibles (Nov 5 spec §3).
pub fn all_bibles_text(text: &str, parallel: Option<&crate::scripture::Parallel>) -> String {
    match parallel.filter(|p| !p.also.is_empty()) {
        None => text.to_string(),
        Some(p) => std::iter::once((p.main_code.as_str(), text)).chain(p.also.iter().map(|a| (a.code.as_str(), a.text.as_str())))
            .map(|(code, t)| format!("{code}  {t}")).collect::<Vec<_>>().join("\n\n"),
    }
}

impl VerseText {
    pub fn new(reference: impl Into<String>, text: impl Into<String>) -> Self {
        VerseText { reference: reference.into(), text: text.into(), id: None }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemKind {
    /// `arrangement`: this service's own order for the song (FEAT-4 D10, or a Planning Center sequence); `None`
    /// plays the song's usual order, and an empty list the written order.
    Song { song_id: Uuid, #[serde(default, skip_serializing_if = "Option::is_none")] arrangement: Option<Vec<String>> },
    /// `per_slide`: verses per slide, 0 = automatic (by length). `text_size`: the passage's largest text size in
    /// pixels on a 1080p screen, 0 = automatic (Settings → Look's scripture size).
    Scripture {
        reference: String,
        translation: String,
        verses: Vec<VerseText>,
        #[serde(default)]
        per_slide: u8,
        #[serde(default)]
        text_size: u32,
        /// Up to `scripture::MAX_ALSO` more Bibles shown with the main one, in order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        also: Vec<crate::scripture::AlsoShow>,
        /// How the Bibles share a slide when there is more than one.
        #[serde(default)]
        layout: crate::scripture::ParallelLayout,
        /// Slide breaks the operator chose between verses (FEAT-5 §3.3).
        #[serde(default, skip_serializing_if = "crate::scripture::VerseBreaks::is_empty")]
        breaks: crate::scripture::VerseBreaks,
    },
    Media { media_id: Uuid, #[serde(default = "yes")] autoplay: bool, #[serde(default)] end: EndAction, #[serde(default)] looped: bool },
    Deck { deck_id: Uuid },
    Text { title: String, body: String },
    /// A part of the service still to be chosen (FEAT-6). Shows no slides; Next and Prev step over it.
    Blank { #[serde(default)] label: String, #[serde(default)] want: BlankKind },
    /// A coloured heading in the service list ("Worship", "Message"). Shows no slides.
    Header { title: String, #[serde(default)] color: String },
}

/// What a blank asks for.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BlankKind { Song, Scripture, Media, Deck, Text, #[default] Any }

fn yes() -> bool { true }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ServiceItem {
    pub id: Uuid,
    #[serde(flatten)]
    pub kind: ItemKind,
    #[serde(default)]
    pub background: Option<Background>,
    #[serde(default)]
    pub notes: String,
    /// A saved theme for this item only (songs, scripture, announcements); `None`: the service's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<crate::theme::ThemeId>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Service {
    pub id: Uuid,
    pub name: String,
    pub items: Vec<ServiceItem>,
    /// A saved theme for the whole service; `None`: the church default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<crate::theme::ThemeId>,
    /// The day it is for, "2026-11-08". Services made before FEAT-6 have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<chrono::NaiveDate>,
    /// The template it was made from (information only; the template may be gone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<Uuid>,
}

impl Service {
    pub fn new(name: &str) -> Self { Self { id: Uuid::new_v4(), name: name.into(), items: vec![], theme: None, date: None, template: None } }
}

/// A saved service shape: kept items and blanks to fill in (FEAT-6).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ServiceTemplate {
    pub id: Uuid,
    pub name: String,
    /// The day services from it are for; `None` = any day.
    #[serde(default = "sunday")]
    pub day: Option<chrono::Weekday>,
    pub items: Vec<ServiceItem>,
}

fn sunday() -> Option<chrono::Weekday> { Some(chrono::Weekday::Sun) }

// ---------- Settings ----------

/// A text style (font, size, colour, weight, outline, text band, …). Standard is `Settings.theme`; saved themes are
/// in `Data.themes` (FEAT-1 spec §4.1).
pub use crate::theme::{TextAlign, Theme};

/// The pre-screen-setup choice of monitors. Only ever read from older saved data (`Data::upgrade`); never written.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct DisplayRoles {
    /// Windows monitor device name, e.g. `\\.\DISPLAY2`.
    pub room: Option<String>,
    pub stage: Option<String>,
}

/// Where a screen's picture goes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase", tag = "kind", content = "monitor")]
pub enum ScreenTarget {
    /// Nowhere yet.
    #[default]
    None,
    /// A plain window on the operator's desktop, for trying things out.
    Window,
    /// Full screen on this monitor (`MonitorId::key`).
    Monitor(String),
}

/// One physical output of a screen. A window uses its size; a monitor's real size is kept here so a list can show it
/// while the monitor is unplugged.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ScreenOutput { pub target: ScreenTarget, pub width: u32, pub height: u32 }

impl Default for ScreenOutput {
    fn default() -> Self { Self { target: ScreenTarget::None, width: 1920, height: 1080 } }
}

/// A named screen. It has exactly one output today; a list, so a screen made of several projectors (edge blending)
/// can be added without changing what is saved.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Screen { pub id: Uuid, pub name: String, pub outputs: Vec<ScreenOutput> }

impl Default for Screen {
    fn default() -> Self { Self::new(Uuid::new_v4(), "Screen") }
}

impl Screen {
    pub fn new(id: Uuid, name: &str) -> Self { Self { id, name: name.into(), outputs: vec![ScreenOutput::default()] } }
    /// Its output. A saved screen without one reads as having nowhere to go.
    pub fn output(&self) -> ScreenOutput { self.outputs.first().cloned().unwrap_or_default() }
    pub fn output_mut(&mut self) -> &mut ScreenOutput {
        if self.outputs.is_empty() { self.outputs.push(ScreenOutput::default()) }
        &mut self.outputs[0]
    }
}

/// The Audience or the Stage list and its one switch.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct ScreenGroup { pub on: bool, pub screens: Vec<Screen> }

/// An output window the setup asks for, under the name the windowing layer knows it by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenWindow { pub key: String, pub target: ScreenTarget, pub size: (u32, u32) }

/// Which screens exist and where they show.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ScreenSetup { pub audience: ScreenGroup, pub stage: ScreenGroup }

impl Default for ScreenSetup {
    fn default() -> Self {
        Self {
            audience: ScreenGroup { on: false, screens: vec![Screen::new(Uuid::from_u128(1), "Audience")] },
            stage: ScreenGroup { on: false, screens: vec![Screen::new(Uuid::from_u128(2), "Stage Display")] },
        }
    }
}

impl ScreenSetup {
    pub const GROUPS: [&str; 2] = ["audience", "stage"];

    pub fn group(&self, name: &str) -> Option<&ScreenGroup> {
        match name { "audience" => Some(&self.audience), "stage" => Some(&self.stage), _ => None }
    }

    pub fn group_mut(&mut self, name: &str) -> Option<&mut ScreenGroup> {
        match name { "audience" => Some(&mut self.audience), "stage" => Some(&mut self.stage), _ => None }
    }

    /// The group holding screen `id`, and the screen.
    pub fn find(&self, id: Uuid) -> Option<(&'static str, &Screen)> {
        Self::GROUPS.into_iter().find_map(|g| Some((g, self.group(g)?.screens.iter().find(|s| s.id == id)?)))
    }

    pub fn find_mut(&mut self, id: Uuid) -> Option<&mut Screen> {
        [&mut self.audience, &mut self.stage].into_iter().flat_map(|g| g.screens.iter_mut()).find(|s| s.id == id)
    }

    /// The window name of each of the group's screens that has an output. The first is plain "room" or "stage": the
    /// room is the one that plays sound and tells the live engine what is on air. The others carry their id.
    pub fn keyed<'a>(&'a self, group: &str) -> Vec<(String, &'a Screen)> {
        let (role, screens) = match group { "audience" => ("room", &self.audience.screens), _ => ("stage", &self.stage.screens) };
        screens.iter().filter(|s| s.output().target != ScreenTarget::None).enumerate()
            .map(|(i, s)| (if i == 0 { role.to_string() } else { format!("{role}:{}", s.id) }, s)).collect()
    }

    /// The windows the switches ask for, every room before the stage.
    pub fn windows(&self) -> Vec<ScreenWindow> {
        Self::GROUPS.into_iter().filter(|g| self.group(g).is_some_and(|g| g.on)).flat_map(|g| self.keyed(g)).map(|(key, s)| {
            let o = s.output();
            ScreenWindow { key, target: o.target, size: (o.width, o.height) }
        }).collect()
    }

    /// The monitors the group's screens are set to.
    pub fn monitors(&self, group: &str) -> Vec<&str> {
        self.group(group).map_or(vec![], |g| g.screens.iter().flat_map(|s| &s.outputs).filter_map(|o| match &o.target {
            ScreenTarget::Monitor(m) => Some(m.as_str()),
            _ => None,
        }).collect())
    }

    /// Points the group's first screen at `target` and turns the group on.
    pub fn assign(&mut self, group: &str, target: ScreenTarget) {
        let Some(g) = self.group_mut(group) else { return };
        if g.screens.is_empty() { g.screens.push(Screen::default()) }
        g.screens[0].output_mut().target = target;
        g.on = true;
    }
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
    /// Older data only: read once into `screens` by `Data::upgrade`.
    #[serde(skip_serializing)]
    pub displays: DisplayRoles,
    pub screens: ScreenSetup,
    /// Keep the room window on top of other windows while it is full screen on a monitor of its own, so Windows
    /// paints it first (smooth fades on the projector while the operator is busy). Never applies when the room
    /// shares the operator's monitor.
    pub room_on_top: bool,
    /// Where sound goes: an mpv device name, as `Mpv::audio_devices` lists it. Empty = the computer's default.
    pub audio_device: String,
    /// Verses per slide for new Bible passages: 0 = automatic (by length), else 1–4.
    pub bible_per_slide: u8,
    /// Bibles starred in the picker (ids), in the order they were starred. Chips show them after the chosen one.
    pub favourites: Vec<String>,
    /// Bibles picked lately (ids), newest first, at most `RECENT_MAX`. Chips show them after the favourites.
    pub recent: Vec<String>,
    /// Bibles shown with the main one in every new passage (church default), at most `scripture::MAX_ALSO`.
    pub also_show: Vec<String>,
    /// How those Bibles share a slide.
    pub parallel_layout: crate::scripture::ParallelLayout,
    /// The language the app is shown in (I18N): `""` follows the computer, else a code such as `es` or `pt-BR`.
    /// Changed only by `settings/language`.
    pub ui_language: String,
    /// The stage screen's layout (FEAT-3). A value this version can't read loads as Classic.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub stage: crate::stage::StageSettings,
    /// The Print dialog's choices, remembered.
    pub print: PrintSettings,
    /// How dates are written; `None` = by the computer's region.
    pub date_order: Option<DateOrder>,
    /// Settings → Section keys: C, B, 1–9… jump the live song to a section (FEAT-4).
    pub section_keys: bool,
    /// Service files opened lately, newest first, at most `RECENT_FILES_MAX` (File → Open recent).
    pub recent_files: Vec<std::path::PathBuf>,
    /// Settings → Video → Graphics, used from the next start. Changed only by `settings/graphics`. A value this
    /// version can't read loads as Auto.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub graphics: Graphics,
    /// Settings → Media: new media's defaults and the sound outputs (N21, N24).
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub media: crate::media_prefs::MediaPrefs,
}

/// How many service files File → Open recent lists.
pub const RECENT_FILES_MAX: usize = 5;

/// How the windows are drawn (`video::graphics`). Auto is FemtoVG on OpenGL, with software if that fails.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Graphics { #[default] Auto, OpenGl, Vulkan, Direct3D, Metal, Software }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Paper { Letter, A4 }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PrintWhat { Order, Lyrics, #[default] Both }

/// Month/day/year, day/month/year or year-month-day.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DateOrder { Mdy, Dmy, Ymd }

/// What to print (FEAT-6b). `paper: None` = by the computer's region.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct PrintSettings { pub what: PrintWhat, pub notes: bool, pub scripture: bool, pub song_per_page: bool, pub paper: Option<Paper> }

impl Default for PrintSettings {
    fn default() -> Self { Self { what: PrintWhat::Both, notes: true, scripture: true, song_per_page: false, paper: None } }
}

/// How many recently picked Bibles `Settings.recent` keeps: enough that four remain once the chosen one and the
/// favourites are left out of the chips.
pub const RECENT_MAX: usize = 8;

impl Settings {
    /// A service file was opened: first in File → Open recent.
    pub fn opened_file(&mut self, path: &std::path::Path) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(RECENT_FILES_MAX);
    }

    /// `translation` was just chosen: it goes to the front of `recent`, once.
    pub fn used_translation(&mut self, translation: &str) {
        self.recent.retain(|t| !crate::bible::id::same_id(t, translation));
        self.recent.insert(0, translation.to_string());
        self.recent.truncate(RECENT_MAX);
    }

    /// Stars `translation` (`on`) or unstars it. Starring one already starred changes nothing.
    pub fn set_favourite(&mut self, translation: &str, on: bool) {
        let starred = self.favourites.iter().any(|t| crate::bible::id::same_id(t, translation));
        if on && !starred { self.favourites.push(translation.to_string()) }
        if !on { self.favourites.retain(|t| !crate::bible::id::same_id(t, translation)) }
    }
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
            screens: ScreenSetup::default(),
            room_on_top: true,
            audio_device: String::new(),
            bible_per_slide: 0,
            favourites: Vec::new(),
            recent: Vec::new(),
            also_show: Vec::new(),
            parallel_layout: Default::default(),
            ui_language: String::new(),
            stage: Default::default(),
            print: PrintSettings::default(),
            date_order: None,
            section_keys: true,
            recent_files: Vec::new(),
            graphics: Graphics::Auto,
            media: Default::default(),
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
    /// The background library (pictures, videos, colours and gradients the church keeps).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub backgrounds: Vec<BackgroundEntry>,
    /// Saved themes (Standard is `settings.theme`, never in here; a built-in's id here is its edited copy).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub themes: Vec<crate::theme::SavedTheme>,
    /// The starred church default; `None` or a theme that is gone: Standard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_theme: Option<crate::theme::ThemeId>,
    /// Saved overlays, in drawing order (last on top).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub overlays: Vec<crate::theme::overlay::Overlay>,
    /// Service templates (FEAT-6).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<ServiceTemplate>,
    /// The church's own stage layouts (FEAT-3); the built-ins are made in code and never stored.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stage_layouts: Vec<crate::stage::StageLayout>,
    /// Timers, messages, macros and auto-advance (FEAT-2).
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub automation: crate::automation::Automation,
    /// Each Bible's credit for its scripture slides, by translation id (`App::refresh_bible_credits`). Worked out
    /// from the Bibles on this computer, so never saved: a Bible added or removed changes it.
    #[serde(skip)]
    pub bible_credits: BTreeMap<String, BibleCredit>,
}

/// What a scripture slide says about its Bible (Bible library spec §10).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BibleCredit {
    /// Shown after the reference ("John 3:16 (RV1909)").
    pub abbr: String,
    /// The credit line: the Bible's notice, then its licence's short name ("RV1909: © … (CC BY-SA 4.0)"). Empty
    /// when there is nothing to credit (the built-in public-domain Bibles).
    pub line: String,
    /// The text is not public domain, so its credit shows even when Settings has credits off.
    pub required: bool,
}

impl Data {
    /// The credit of `translation`'s slides, matched by id (`bible::id::same_id`).
    pub fn bible_credit(&self, translation: &str) -> Option<&BibleCredit> {
        self.bible_credits.get(translation)
            .or_else(|| self.bible_credits.iter().find(|(id, _)| crate::bible::id::same_id(id, translation)).map(|(_, c)| c))
    }
}

impl Default for Data {
    fn default() -> Self {
        // In the app's language: only new data gets it, a saved service keeps its name.
        let service = Service::new(&crate::tr!("Sunday Service"));
        Self {
            format: crate::store::SUPPORTED_FORMAT, settings: Settings::default(), songs: vec![], media: vec![], media_folders: vec![], decks: vec![],
            current_service: Some(service.id), services: vec![service], backgrounds: vec![], themes: vec![], default_theme: None, overlays: vec![], templates: vec![], stage_layouts: vec![],
            automation: crate::automation::Automation::seeded(), bible_credits: BTreeMap::new(),
        }
    }
}

impl Data {
    /// Brings data saved by an older version up to date. A saved monitor was shown at every start, so its group is on.
    pub fn upgrade(mut self) -> Self {
        let old = std::mem::take(&mut self.settings.displays);
        for (group, monitor) in [("audience", old.room), ("stage", old.stage)] {
            if let Some(m) = monitor { self.settings.screens.assign(group, ScreenTarget::Monitor(m)) }
        }
        self
    }

    /// The one load hook (FEAT-1 spec §4.7): every fix-up that runs on loaded data, in a fixed order. True when
    /// something changed, so the caller saves. Later features add their fix-ups here.
    pub fn after_load(&mut self) -> bool {
        let mut changed = false;
        // FEAT-1: overlay text formatting that a hand edit left outside its text.
        for o in &mut self.overlays {
            if let crate::theme::overlay::OverlaySource::Text { text, styles, .. } = &mut o.source {
                let tidy = crate::theme::richtext::normalize(text, styles.clone());
                if tidy != *styles { *styles = tidy; changed = true; }
            }
        }
        // FEAT-3: hand-edited stage layouts made safe to draw.
        for l in &mut self.stage_layouts { changed |= crate::stage::geom::sanitise_layout(l); }
        // FEAT-2: the built-in timers and messages, once.
        changed |= crate::automation::seed(&mut self.automation);
        changed
    }

    /// The themes as resolution sees them: Standard (`settings`), the saved ones, the built-ins and the default.
    pub fn catalog(&self) -> crate::theme::Catalog<'_> {
        crate::theme::Catalog::new(&self.settings.theme, &self.settings.background, &self.themes, self.default_theme)
    }

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
        let bg = |b: &Background, set: &mut std::collections::BTreeSet<AssetId>| { set.extend(b.asset().cloned()); };
        bg(&self.settings.background, &mut set);
        if let Some(l) = &self.settings.logo { set.insert(l.clone()); }
        for m in &self.media {
            set.insert(m.asset.clone());
            set.extend(m.original_asset.iter().chain(&m.thumb).cloned());
        }
        for d in &self.decks { set.extend(d.slides.iter().cloned()); }
        for s in &self.services { for i in &s.items { if let Some(b) = &i.background { bg(b, &mut set); } } }
        set.extend(crate::theme::assets::referenced(&self.themes, &self.backgrounds, &self.overlays));
        for t in &self.templates { for i in &t.items { if let Some(b) = &i.background { bg(b, &mut set); } } }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_data_without_item_arrangement_loads() {
        let it: ServiceItem = serde_json::from_str(r#"{"id":"00000000-0000-0000-0000-000000000001","kind":"song","song_id":"00000000-0000-0000-0000-000000000002"}"#).unwrap();
        assert!(matches!(it.kind, ItemKind::Song { arrangement: None, .. }));
    }

    #[test]
    fn item_arrangement_none_is_not_serialized_and_some_round_trips() {
        let mut it = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: Uuid::new_v4(), arrangement: None }, background: None, notes: String::new(), theme: None };
        assert!(serde_json::to_value(&it).unwrap().get("arrangement").is_none());
        it.kind = ItemKind::Song { song_id: Uuid::new_v4(), arrangement: Some(vec!["Chorus".into(), "Verse 1".into()]) };
        let back: ServiceItem = serde_json::from_value(serde_json::to_value(&it).unwrap()).unwrap();
        assert!(matches!(back.kind, ItemKind::Song { arrangement: Some(a), .. } if a == ["Chorus", "Verse 1"]));
    }

    #[test]
    fn old_data_follows_the_computers_language() {
        let d: Data = serde_json::from_str(r#"{"format":1,"settings":{"lines_per_slide":2}}"#).unwrap();
        assert_eq!(d.settings.ui_language, "");
        let mut s = Settings::default();
        s.ui_language = "pt-BR".into();
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.ui_language, "pt-BR");
    }

    #[test]
    fn graphics_is_auto_unless_saved_and_an_unknown_one_loads_as_auto() {
        let d: Data = serde_json::from_str(r#"{"format":1,"settings":{"lines_per_slide":2}}"#).unwrap();
        assert_eq!(d.settings.graphics, Graphics::Auto);
        let d: Data = serde_json::from_str(r#"{"format":1,"settings":{"graphics":"hologram"}}"#).unwrap();
        assert_eq!(d.settings.graphics, Graphics::Auto);
        for (g, name) in [(Graphics::OpenGl, "opengl"), (Graphics::Vulkan, "vulkan"), (Graphics::Direct3D, "direct3d"), (Graphics::Metal, "metal"), (Graphics::Software, "software")] {
            let s = Settings { graphics: g, ..Settings::default() };
            let json = serde_json::to_value(&s).unwrap();
            assert_eq!(json["graphics"], name);
            assert_eq!(serde_json::from_value::<Settings>(json).unwrap().graphics, g);
        }
    }

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
            id: Uuid::new_v4(), background: None, notes: String::new(), theme: None,
            kind: ItemKind::Scripture { reference: "Psalm 23".into(), translation: "KJV".into(), verses: vec![], per_slide: 1, text_size: 96, also: vec![], layout: Default::default(), breaks: Default::default() },
        });
        let back: Data = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back.settings.bible_per_slide, 3);
        assert!(matches!(back.services[0].items[0].kind, ItemKind::Scripture { per_slide: 1, text_size: 96, .. }));
    }

    #[test]
    fn a_slide_without_its_own_size_writes_no_size() {
        let c = SlideContent::Text { text: "a".into(), footer: None, reference: None, size: 0, parallel: None };
        assert_eq!(serde_json::to_string(&c).unwrap(), r#"{"type":"text","text":"a"}"#);
        let back: SlideContent = serde_json::from_str(r#"{"type":"text","text":"a"}"#).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn passage_is_every_scripture_slide_and_none_for_songs() {
        let s = |t: &str| Slide::new(t, SlideContent::Text { text: t.into(), footer: None, reference: Some(format!("{t} ref")), size: 0, parallel: None });
        let slides = [s("a"), s("b")];
        assert_eq!(passage("scripture", &slides), vec![
            PassageSlide { text: "a".into(), reference: Some("a ref".into()), footer: None },
            PassageSlide { text: "b".into(), reference: Some("b ref".into()), footer: None },
        ]);
        assert!(passage("song", &slides).is_empty(), "songs fit slide by slide");
        assert!(passage("scripture", &slides[..1]).is_empty(), "one slide fits itself");
    }
}

#[cfg(test)]
mod screen_tests {
    use super::*;

    fn on_monitor(name: &str, m: &str) -> Screen {
        let mut s = Screen::new(Uuid::new_v4(), name);
        s.output_mut().target = ScreenTarget::Monitor(m.into());
        s
    }

    #[test]
    fn older_data_keeps_its_monitors_and_stays_on() {
        let old = r#"{"settings":{"displays":{"room":"A","stage":null}}}"#;
        let d = serde_json::from_str::<Data>(old).unwrap().upgrade();
        assert!(d.settings.screens.audience.on);
        assert_eq!(d.settings.screens.monitors("audience"), ["A"]);
        assert!(!d.settings.screens.stage.on, "no stage was chosen");
        assert_eq!(d.settings.displays, DisplayRoles::default());
        assert!(!serde_json::to_string(&d).unwrap().contains("displays"), "the old field is never written again");
    }

    #[test]
    fn saved_screens_survive_a_new_load() {
        let mut d = Data::default();
        d.settings.screens.assign("stage", ScreenTarget::Window);
        let again = serde_json::from_str::<Data>(&serde_json::to_string(&d).unwrap()).unwrap().upgrade();
        assert_eq!(again.settings.screens, d.settings.screens);
    }

    #[test]
    fn data_with_no_screens_gets_the_two_lists() {
        let d = serde_json::from_str::<Data>("{}").unwrap().upgrade();
        assert_eq!(d.settings.screens, ScreenSetup::default());
        assert_eq!(d.settings.screens.windows(), vec![]);
    }

    #[test]
    fn the_first_screen_with_an_output_is_the_room() {
        let mut s = ScreenSetup::default();
        let (lobby, wall) = (on_monitor("Lobby", "B"), on_monitor("Wall", "C"));
        let wall_id = wall.id;
        s.audience.screens.extend([lobby, wall]); // the default one has no output
        s.audience.on = true;
        let w = s.windows();
        assert_eq!(w.iter().map(|w| w.key.clone()).collect::<Vec<_>>(), ["room".to_string(), format!("room:{wall_id}")]);
        assert_eq!(w[0].target, ScreenTarget::Monitor("B".into()));
    }

    #[test]
    fn a_switched_off_group_asks_for_no_windows() {
        let mut s = ScreenSetup::default();
        s.assign("audience", ScreenTarget::Window);
        s.assign("stage", ScreenTarget::Window);
        s.stage.on = false;
        assert_eq!(s.windows().iter().map(|w| w.key.as_str()).collect::<Vec<_>>(), ["room"]);
    }

    #[test]
    fn find_names_the_group() {
        let s = ScreenSetup::default();
        let id = s.stage.screens[0].id;
        assert_eq!(s.find(id).map(|(g, x)| (g, x.name.as_str())), Some(("stage", "Stage Display")));
        assert!(s.find(Uuid::nil()).is_none());
    }
}

#[cfg(test)]
mod feat1_tests {
    use super::*;
    use crate::theme::overlay::{ImageSource, Overlay, OverlaySource, Placement};
    use crate::theme::richtext::StyleSpan;

    #[test]
    fn after_load_normalizes_stored_spans() {
        let mut d = Data::default();
        assert!(!d.after_load(), "nothing to do");
        let span: StyleSpan = serde_json::from_str(r#"{"start":2,"end":99,"bold":true}"#).unwrap();
        d.overlays.push(Overlay { id: Uuid::new_v4(), name: "Wi-Fi".into(), source: OverlaySource::Text { text: "Hello".into(), styles: vec![span], look: Default::default() }, place: Placement::default() });
        assert!(d.after_load());
        let OverlaySource::Text { styles, .. } = &d.overlays[0].source else { panic!() };
        assert_eq!((styles[0].start, styles[0].end), (2, 5));
        assert!(!d.after_load(), "once");
    }

    #[test]
    fn referenced_assets_include_themes_backgrounds_overlays() {
        let mut d = Data::default();
        d.themes.push(crate::theme::SavedTheme { id: Uuid::new_v4(), name: "T".into(), style: Theme::default(), background: Some(Background::Image { asset: "t.png".into(), dim: 10 }) });
        d.backgrounds.push(BackgroundEntry::new("B", Background::Video { asset: "b.mp4".into(), dim: 0 }, MediaStatus::Ready, chrono::Utc::now()));
        d.overlays.push(Overlay { id: Uuid::new_v4(), name: "Bug".into(), source: OverlaySource::Image { source: ImageSource::Asset { asset: "o.png".into() }, opacity: 100 }, place: Placement::default() });
        d.services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "t".into(), body: "b".into() },
            background: Some(Background::Gradient { from: "#000".into(), to: "#fff".into(), angle: 90, dim: 0 }), notes: String::new(), theme: None });
        let set = d.referenced_assets();
        for a in ["t.png", "b.mp4", "o.png"] { assert!(set.contains(a), "{a}"); }
    }
}
