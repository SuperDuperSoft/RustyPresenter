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
    /// A full-screen deck slide image (PowerPoint, images), with the slide's own transition from the file or
    /// Item edit (`None`: the normal crossfade).
    Image {
        asset: AssetId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transition: Option<crate::design::Transition>,
    },
    /// Foreground video with its own audio.
    Video { asset: AssetId, autoplay: bool, end: EndAction },
    /// Full-screen still image on the media layer.
    Still { asset: AssetId },
    /// Audio-only item (walk-in music). Does not change visuals.
    Audio { asset: AssetId, looped: bool },
    /// A designed slide (N26) as shown after `step` of its `steps` clicks (0: before any). `play`: the click group
    /// that animates into it (for step 0, the steps that play by themselves as it comes up).
    Design {
        design: crate::design::Design,
        #[serde(default)]
        step: u16,
        #[serde(default)]
        steps: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        play: Option<Box<crate::design::Play>>,
    },
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
    /// A song slide's label (N19): "Instrumental", from a `{Instrumental}` line on top of it in the lyrics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// N10: a song slide's words with the chords typed in the lyrics (`[G]Amazing [C]grace`), for the stage only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chords: Option<String>,
    /// A deck slide's own auto-advance time in milliseconds (PowerPoint's "After").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advance_ms: Option<u32>,
}

impl Slide {
    /// A slide with no notes, no section and no label.
    pub fn new(label: impl Into<String>, content: SlideContent) -> Self { Self { label: label.into(), content, notes: String::new(), section: None, tag: None, chords: None, advance_ms: None } }
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
    /// Who sings it (N36).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub artist: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub publisher: String,
    /// The copyright year, as typed ("2006").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub year: String,
    /// Lyrics with section labels on their own lines (`Verse 1`, `[Chorus]`).
    /// A blank line inside a section forces a new slide.
    pub lyrics: String,
    /// Section labels in performance order. Empty = the order they are written.
    #[serde(default)]
    pub arrangement: Vec<String>,
    /// Lines on each slide for this song; 0 = the number in Settings → Songs.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub lines_per_slide: u8,
    /// This song's largest text size in pixels on a 1080p screen; 0 = automatic (the theme's size).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub text_size: u32,
    /// The key the song is written in, as typed or read from a song file ("G", "Bb", "F#m"); empty = not known.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// How far the stage screen moves the chords, in semitones (−11 … +11; 0 = as written). The words and the
    /// chords in `lyrics` never change (`crate::transpose`).
    #[serde(default, skip_serializing_if = "is_zero_i8")]
    pub transpose: i8,
}

fn is_zero_i8(n: &i8) -> bool { *n == 0 }

fn is_zero_u8(n: &u8) -> bool { *n == 0 }

impl Song {
    /// The range of a song's own lines per slide (0 = Settings').
    pub const MAX_LINES: u8 = 8;
    /// The range of a song's own text size (0 = automatic).
    pub const MIN_TEXT: u32 = 24;
    pub const MAX_TEXT: u32 = 200;

    /// Lines per slide for this song: its own number, else `settings`'.
    pub fn lines(&self, settings: &Settings) -> usize {
        (if self.lines_per_slide > 0 { self.lines_per_slide } else { settings.lines_per_slide }).max(1) as usize
    }
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
    /// Speaker notes by slide, from the presentation or typed in Item edit. Shorter than `slides` (or empty)
    /// when the last slides have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Each slide's own transition, from the presentation or Item edit (`None`: the normal crossfade). Shorter
    /// than `slides` (or empty) when the last slides have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<Option<crate::design::Transition>>,
    /// Each slide's own auto-advance time in milliseconds, from PowerPoint's "After" or Item edit (`None`: the
    /// operator moves on). Shorter than `slides` (or empty) when the last slides have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advance_ms: Vec<Option<u32>>,
    /// 1.2: the presentation file itself, kept so it can run in PowerPoint (`pplive`). `None` for decks made
    /// only of pictures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<DeckShow>,
}

/// A deck's own presentation file (1.2 "Run in PowerPoint"). The slide pictures stay too: the operator, stage
/// and phone show them, and the room shows them when PowerPoint cannot run.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DeckShow {
    /// The file in the asset store (with its extension, so PowerPoint knows what it is).
    pub file: AssetId,
    /// Runs in PowerPoint on the audience screen. False: shown as pictures, the file kept for switching back.
    #[serde(default)]
    pub run: bool,
}

impl Deck {
    /// The file to run in PowerPoint, when this deck is set to run there.
    pub fn runs_in_powerpoint(&self) -> Option<&AssetId> { self.show.as_ref().filter(|s| s.run).map(|s| &s.file) }
}

/// The longest auto-advance time a slide can have (one hour).
pub const ADVANCE_MAX_MS: u32 = 3_600_000;

/// Auto-advance times for a deck of `slides` slides, kept short like `deck_transitions`, each no longer than an hour.
pub fn deck_advance(mut t: Vec<Option<u32>>, slides: usize) -> Vec<Option<u32>> {
    t.truncate(slides);
    for x in t.iter_mut().flatten() { *x = (*x).min(ADVANCE_MAX_MS) }
    while t.last().is_some_and(Option::is_none) { t.pop(); }
    t
}

/// Transitions for a deck of `slides` slides, kept short like `deck_notes`, each no longer than the editor offers.
pub fn deck_transitions(mut t: Vec<Option<crate::design::Transition>>, slides: usize) -> Vec<Option<crate::design::Transition>> {
    t.truncate(slides);
    for x in t.iter_mut().flatten() { x.ms = x.ms.min(crate::design::TRANSITION_MAX_MS) }
    while t.last().is_some_and(Option::is_none) { t.pop(); }
    t
}

/// Notes for a deck of `slides` slides, kept short: none past the last slide, no empty ones at the end.
pub fn deck_notes(mut notes: Vec<String>, slides: usize) -> Vec<String> {
    notes.truncate(slides);
    while notes.last().is_some_and(|n| n.trim().is_empty()) { notes.pop(); }
    notes
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
    /// `layout`: which of its theme's layouts the slides use (N25); Text when not given.
    Text { title: String, body: String, #[serde(default, skip_serializing_if = "crate::theme::LayoutKind::is_text")] layout: crate::theme::LayoutKind },
    /// Slides made in the slide editor (N26): text boxes, shapes, pictures… placed freely.
    Design { title: String, #[serde(default)] slides: Vec<crate::design::Design> },
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
    /// Set up now, connected later (N20): kept with its size and fixes, and never opened.
    Placeholder,
}

impl ScreenTarget {
    /// Whether a window opens for it.
    pub fn opens(&self) -> bool { matches!(self, ScreenTarget::Window | ScreenTarget::Monitor(_)) }
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
pub struct Screen {
    pub id: Uuid,
    pub name: String,
    pub outputs: Vec<ScreenOutput>,
    /// Colour, corner pin and delay (N20).
    #[serde(skip_serializing_if = "crate::screen_fix::ScreenFix::is_default")]
    pub fix: crate::screen_fix::ScreenFix,
    /// A test pattern instead of the picture (N20). Turned off at start and when Screen Configuration closes.
    #[serde(skip_serializing_if = "crate::screen_fix::TestPattern::is_off")]
    pub test: crate::screen_fix::TestPattern,
    /// A stage screen that shows what the audience sees instead of a stage layout (N20).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mirror: bool,
    /// N12: the shape an audience screen's picture is cut to.
    pub mask: crate::render::mask::Mask,
}

impl Default for Screen {
    fn default() -> Self { Self::new(Uuid::new_v4(), "Screen") }
}

impl Screen {
    pub fn new(id: Uuid, name: &str) -> Self {
        Self { id, name: name.into(), outputs: vec![ScreenOutput::default()], fix: Default::default(), test: Default::default(), mirror: false, mask: Default::default() }
    }
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
            audience: ScreenGroup { on: false, screens: vec![Screen::new(Uuid::from_u128(1), "Audience screen")] },
            stage: ScreenGroup { on: false, screens: vec![Screen::new(Uuid::from_u128(2), "Stage screen")] },
        }
    }
}

impl ScreenSetup {
    pub const GROUPS: [&str; 2] = ["audience", "stage"];

    /// Older versions named new screens "Audience", "Audience 2", "Stage Display", "Stage Display 2"; the app now
    /// says audience screen and stage screen everywhere. Only those exact names change: a name typed by hand stays.
    /// True when one changed.
    pub fn rename_old_defaults(&mut self) -> bool {
        let mut changed = false;
        // "Room screen" was the default for a few days of test builds (November 17, 2026) before Jake chose "audience
        // screen".
        let renames = [(true, "Audience", "Audience screen"), (true, "Room screen", "Audience screen"), (false, "Stage Display", "Stage screen")];
        for (audience, old, new) in renames {
            let group = if audience { &mut self.audience } else { &mut self.stage };
            for s in &mut group.screens {
                let Some(rest) = s.name.strip_prefix(old) else { continue };
                let number = rest.strip_prefix(' ').filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
                let renamed = match (rest.is_empty(), number) { (true, _) => new.to_string(), (_, Some(n)) => format!("{new} {n}"), _ => continue };
                s.name = renamed;
                changed = true;
            }
        }
        changed
    }

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

    /// The window name of each of the group's screens that opens a window. The first is plain "room" or "stage": the
    /// room is the one that plays sound and tells the live engine what is on air. The others carry their id. A stage
    /// screen set to show the audience picture (`mirror`) is an extra audience window, "room:<id>".
    pub fn keyed<'a>(&'a self, group: &str) -> Vec<(String, &'a Screen)> {
        let (role, screens) = match group { "audience" => ("room", &self.audience.screens), _ => ("stage", &self.stage.screens) };
        let mirrored = |s: &Screen| role == "stage" && s.mirror;
        let opening = || screens.iter().filter(|s| s.output().target.opens());
        let own = opening().filter(|s| !mirrored(s)).enumerate().map(|(i, s)| (if i == 0 { role.to_string() } else { format!("{role}:{}", s.id) }, s));
        let mirrors = opening().filter(|s| mirrored(s)).map(|s| (format!("room:{}", s.id), s));
        let mut all: Vec<_> = own.chain(mirrors).collect();
        all.sort_by_key(|(_, s)| screens.iter().position(|x| x.id == s.id));
        all
    }

    /// The screen a window name stands for.
    pub fn screen_for(&self, key: &str) -> Option<&Screen> {
        Self::GROUPS.into_iter().flat_map(|g| self.keyed(g)).find(|(k, _)| k == key).map(|(_, s)| s)
    }

    /// Turns every test pattern off; true when one was on.
    pub fn clear_tests(&mut self) -> bool {
        let mut any = false;
        for s in self.audience.screens.iter_mut().chain(self.stage.screens.iter_mut()) {
            any |= !s.test.is_off();
            s.test = Default::default();
        }
        any
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
    /// "first", "last", "both" (first and last slide) or "none"
    pub credits: String,
    /// The song credit line (N36), with `{title}`, `{artist}`, `{author}`, `{publisher}`, `{year}`, `{copyright}`,
    /// `{ccli}` and `{license}`; empty = `slides::DEFAULT_CREDIT_TEMPLATE`.
    pub credit_template: String,
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
    /// N31: phone remote password, stage view on phones, TCP control. Saved only by `network/save`.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub network: crate::network::NetworkSettings,
    /// The Print dialog's choices, remembered.
    pub print: PrintSettings,
    /// How dates are written; `None` = by the computer's region.
    pub date_order: Option<DateOrder>,
    /// Settings → Section keys: C, B, 1–9… jump the live song to a section (FEAT-4).
    pub section_keys: bool,
    /// Service files opened lately, newest first, at most `RECENT_FILES_MAX` (File → Open recent).
    pub recent_files: Vec<std::path::PathBuf>,
    /// Settings → General → Screens and graphics → Graphics, used from the next start. Changed only by `settings/graphics`. A value this
    /// version can't read loads as Auto.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub graphics: Graphics,
    /// Settings → Songs → Sections (N19): the church's section names, colours, keys and slide labels.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub sections: crate::sections::SectionLook,
    /// Settings → Keys: the changed keyboard shortcuts and the clear groups (N18, N28). A value this version can't
    /// read loads as the defaults.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub keys: crate::shortcuts::KeySettings,
    /// Settings → General → Screens and graphics and media: new media's defaults and the sound outputs (N21, N24).
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub media: crate::media_prefs::MediaPrefs,
    /// Settings → Livestream → Recording (N22): picture size, folder and sound of File → Start recording.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub record: crate::record::RecordPrefs,
    /// N33: the frames-a-second and video memory line under the live preview.
    pub show_performance: bool,
    /// N33: the room and stage screens that were on come back on at start; off, they wait for the switch.
    pub screens_at_start: bool,
    /// Settings → General → Screens and graphics: lines drawn from a phone stay when the slide changes (off: each slide starts clean).
    pub keep_drawings: bool,
    /// Settings → General → Screens and graphics: the stage screen shows the live slide with its drawing while one is up.
    pub stage_drawings: bool,
    /// Settings → General → Screens and graphics → "Limit phone drawing" (off: no limits), with the most lines on the slide at once and
    /// the most drawing updates a second from one phone.
    pub drawing_limit: bool,
    pub drawing_max_strokes: u32,
    pub drawing_max_per_second: u32,
    /// Settings → General → Screens and graphics → "Fading ink disappears after: N seconds" (1–30).
    pub fade_secs: u32,
    /// N9: how long the room fades when only the look (theme) of what is up changes, in ms; 0 = cut.
    pub look_fade_ms: u32,
    /// N6: the Bible box's last references, newest first, at most `BIBLE_HISTORY_MAX`.
    pub bible_history: Vec<String>,
    /// 1.3 Settings → Auto-follow: songs follow the singers. A value this version can't read loads as the defaults.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub follow: crate::follow::Settings,
    /// Settings → Songs: songs downloaded from SongSelect come into the library whenever the app is open
    /// (`ui::dialogs::songselect`). Off: only while the "Songs from SongSelect" window is open.
    pub songselect_inbox: bool,
    /// Settings → Songs → "Keep my songs in a folder": one ChordPro file per song (`crate::songs_folder`). Changed
    /// only by `songs-folder/set`. A value this version can't read loads as off.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub songs_folder: crate::songs_folder::Prefs,
    /// The operator window's panels: the arrangement in use and the church's saved layouts (dock panels design).
    /// Saved only by `settings/dock`. A value this version can't read loads as Standard.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub dock: crate::ui::dock::DockPrefs,
}

/// The longest look fade Settings offers (N9).
pub const LOOK_FADE_MAX_MS: u32 = 5000;
/// How many references the Bible box's history keeps (N6).
pub const BIBLE_HISTORY_MAX: usize = 10;

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

    /// `reference` was just added from the Bible box: first in its history, once (N6).
    pub fn used_reference(&mut self, reference: &str) {
        let reference = reference.trim();
        if reference.is_empty() { return }
        self.bible_history.retain(|r| !r.eq_ignore_ascii_case(reference));
        self.bible_history.insert(0, reference.to_string());
        self.bible_history.truncate(BIBLE_HISTORY_MAX);
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
            credits: "both".into(),
            credit_template: String::new(),
            church_ccli: String::new(),
            translation: "KJV".into(),
            displays: DisplayRoles::default(),
            screens: ScreenSetup::default(),
            room_on_top: true,
            keep_drawings: false,
            stage_drawings: true,
            drawing_limit: false,
            drawing_max_strokes: crate::live::drawing::actions::MAX_STROKES_DEFAULT,
            drawing_max_per_second: crate::live::drawing::actions::MAX_PER_SECOND_DEFAULT,
            fade_secs: crate::live::drawing::FADE_SECS_DEFAULT,
            audio_device: String::new(),
            bible_per_slide: 0,
            favourites: Vec::new(),
            recent: Vec::new(),
            also_show: Vec::new(),
            parallel_layout: Default::default(),
            ui_language: String::new(),
            stage: Default::default(),
            network: Default::default(),
            print: PrintSettings::default(),
            date_order: None,
            section_keys: true,
            recent_files: Vec::new(),
            graphics: Graphics::Auto,
            sections: Default::default(),
            keys: Default::default(),
            media: Default::default(),
            record: Default::default(),
            show_performance: false,
            screens_at_start: true,
            look_fade_ms: 1000,
            bible_history: Vec::new(),
            follow: Default::default(),
            songselect_inbox: false,
            songs_folder: Default::default(),
            dock: Default::default(),
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
        // N26: hand-edited designed slides made safe to draw.
        for i in self.services.iter_mut().flat_map(|s| &mut s.items) {
            if let ItemKind::Design { slides, .. } = &mut i.kind { for d in slides { changed |= d.sanitise() } }
        }
        // FEAT-2: the built-in timers and messages, once.
        changed |= crate::automation::seed(&mut self.automation);
        // November 17, 2026 (one word for each thing): screens still named as older versions named them.
        changed |= self.settings.screens.rename_old_defaults();
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
        for d in &self.decks { set.extend(d.slides.iter().cloned().chain(d.show.iter().map(|s| s.file.clone()))); }
        for s in &self.services { for i in &s.items { if let Some(b) = &i.background { bg(b, &mut set); } } }
        // N26: pictures and videos on designed slides.
        for i in self.services.iter().flat_map(|s| &s.items).chain(self.templates.iter().flat_map(|t| &t.items)) {
            if let ItemKind::Design { slides, .. } = &i.kind { for d in slides { set.extend(d.assets()) } }
        }
        set.extend(crate::theme::assets::referenced(&self.themes, &self.backgrounds, &self.overlays));
        for t in &self.templates { for i in &t.items { if let Some(b) = &i.background { bg(b, &mut set); } } }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bible_history_keeps_the_last_ten_newest_first_once_each() {
        let mut s = Settings::default();
        for i in 1..=12 { s.used_reference(&format!("John 3:{i}")) }
        assert_eq!(s.bible_history.len(), BIBLE_HISTORY_MAX);
        assert_eq!(s.bible_history[0], "John 3:12");
        s.used_reference(" john 3:5 ");
        assert_eq!(s.bible_history[0], "john 3:5");
        assert_eq!(s.bible_history.iter().filter(|r| r.eq_ignore_ascii_case("John 3:5")).count(), 1);
        s.used_reference("  ");
        assert_eq!(s.bible_history[0], "john 3:5", "nothing typed is not kept");
    }

    #[test]
    fn old_settings_get_a_one_second_look_fade() {
        let s: Settings = serde_json::from_str(r#"{"lines_per_slide":2}"#).unwrap();
        assert_eq!((s.look_fade_ms, s.bible_history.len()), (1000, 0));
    }

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
        assert_eq!(s.find(id).map(|(g, x)| (g, x.name.as_str())), Some(("stage", "Stage screen")));
        assert!(s.find(Uuid::nil()).is_none());
    }

    #[test]
    fn old_default_screen_names_become_audience_and_stage_screen() {
        let mut s = ScreenSetup::default();
        let add = |g: &mut ScreenGroup, name: &str| g.screens.push(Screen::new(Uuid::new_v4(), name));
        s.audience.screens[0].name = "Audience".into();
        add(&mut s.audience, "Audience 2");
        add(&mut s.audience, "Audience overflow");
        add(&mut s.audience, "Room screen 3");
        s.stage.screens[0].name = "Stage Display".into();
        add(&mut s.stage, "Stage Display 3");
        add(&mut s.stage, "Drummer");
        assert!(s.rename_old_defaults());
        let names = |g: &ScreenGroup| g.screens.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&s.audience), ["Audience screen", "Audience screen 2", "Audience overflow", "Audience screen 3"], "a name typed by hand stays");
        assert_eq!(names(&s.stage), ["Stage screen", "Stage screen 3", "Drummer"]);
        assert!(!s.rename_old_defaults(), "once is enough");
    }

    #[test]
    fn a_placeholder_opens_nothing_and_keeps_its_place() {
        let mut s = ScreenSetup::default();
        s.assign("audience", ScreenTarget::Placeholder);
        s.audience.screens[0].fix.red = 0.9;
        assert!(s.windows().is_empty(), "no projector yet");
        let mut lobby = on_monitor("Lobby", "B");
        lobby.mirror = true; // only means something on a stage screen
        let lobby_id = lobby.id;
        s.audience.screens.push(lobby);
        assert_eq!(s.windows().iter().map(|w| w.key.clone()).collect::<Vec<_>>(), ["room"], "the next real one is the room");
        assert_eq!(s.screen_for("room").map(|x| x.id), Some(lobby_id));
        let back: ScreenSetup = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s, "the placeholder and its fix are saved");
    }

    #[test]
    fn a_mirrored_stage_screen_is_an_audience_window() {
        let mut s = ScreenSetup::default();
        s.assign("stage", ScreenTarget::Monitor("A".into()));
        let mut wall = on_monitor("Wall", "B");
        wall.mirror = true;
        let wall_id = wall.id;
        s.stage.screens.push(wall);
        assert_eq!(s.windows().iter().map(|w| w.key.clone()).collect::<Vec<_>>(), ["stage".to_string(), format!("room:{wall_id}")]);
        s.stage.screens[0].mirror = true;
        let first = s.stage.screens[0].id;
        assert_eq!(s.keyed("stage").iter().map(|(k, _)| k.clone()).collect::<Vec<_>>(), [format!("room:{first}"), format!("room:{wall_id}")]);
        assert_eq!(s.screen_for(&format!("room:{wall_id}")).map(|x| x.name.as_str()), Some("Wall"));
    }

    #[test]
    fn test_patterns_are_cleared_together() {
        let mut s = ScreenSetup::default();
        assert!(!s.clear_tests());
        s.stage.screens[0].test = crate::screen_fix::TestPattern::Grid;
        assert!(s.clear_tests());
        assert!(s.stage.screens[0].test.is_off());
        assert!(!serde_json::to_string(&s).unwrap().contains("test"), "an off pattern and an empty fix are not written");
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
        d.services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "t".into(), body: "b".into(), layout: Default::default() },
            background: Some(Background::Gradient { from: "#000".into(), to: "#fff".into(), angle: 90, dim: 0 }), notes: String::new(), theme: None });
        let set = d.referenced_assets();
        for a in ["t.png", "b.mp4", "o.png"] { assert!(set.contains(a), "{a}"); }
    }
}

#[cfg(test)]
mod deck_notes_tests {
    use super::*;

    #[test]
    fn a_deck_saved_without_notes_still_loads_and_saves_none() {
        let id = Uuid::new_v4();
        let old = serde_json::json!({ "id": id, "title": "Talk", "slides": ["1.png", "2.png"] });
        let deck: Deck = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(deck, Deck { id, title: "Talk".into(), slides: vec!["1.png".into(), "2.png".into()], notes: vec![], transitions: vec![], advance_ms: vec![], show: None });
        assert_eq!(serde_json::to_value(&deck).unwrap(), old, "no notes key when there are none");
        let with = Deck { notes: vec!["Hi".into()], ..deck };
        assert_eq!(serde_json::from_value::<Deck>(serde_json::to_value(&with).unwrap()).unwrap(), with);
    }

    #[test]
    fn deck_notes_are_kept_short() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(deck_notes(n(&["a", "", "b", "c"]), 3), n(&["a", "", "b"]));
        assert_eq!(deck_notes(n(&["a", "", " "]), 3), n(&["a"]));
        assert_eq!(deck_notes(n(&["", ""]), 2), Vec::<String>::new());
    }

    #[test]
    fn deck_advance_times_are_kept_short_and_reach_each_slide() {
        assert_eq!(deck_advance(vec![None, Some(5000), None, Some(9_999_999)], 3), vec![None, Some(5000)]);
        assert_eq!(deck_advance(vec![Some(9_999_999)], 1), vec![Some(ADVANCE_MAX_MS)]);
        assert_eq!(deck_advance(vec![None, None], 2), vec![]);
        let deck = Deck { id: Uuid::new_v4(), title: "Talk".into(), slides: vec!["1.png".into(), "2.png".into()], notes: vec![], transitions: vec![], advance_ms: vec![None, Some(5000)], show: None };
        let json = serde_json::to_value(&deck).unwrap();
        assert_eq!(json["advance_ms"], serde_json::json!([null, 5000]));
        assert_eq!(serde_json::from_value::<Deck>(json).unwrap(), deck);
        let mut data = Data::default();
        data.decks.push(deck.clone());
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Deck { deck_id: deck.id }, background: None, notes: String::new(), theme: None };
        let (_, _, slides) = crate::slides::build_item(&data, &item);
        assert_eq!(slides.iter().map(|s| s.advance_ms).collect::<Vec<_>>(), [None, Some(5000)]);
        assert!(serde_json::to_value(&slides[0]).unwrap().get("advance_ms").is_none());
    }

    #[test]
    fn deck_transitions_are_kept_short_and_reach_each_slide() {
        use crate::design::{Dir, Transition, TransitionKind};
        let wipe = Transition { kind: TransitionKind::Wipe, ms: 99_999, dir: Dir::Down };
        let short = Transition { ms: crate::design::TRANSITION_MAX_MS, ..wipe };
        assert_eq!(deck_transitions(vec![None, Some(wipe), None, Some(wipe)], 3), vec![None, Some(short)]);
        assert_eq!(deck_transitions(vec![None, None], 2), vec![]);
        let deck = Deck { id: Uuid::new_v4(), title: "Talk".into(), slides: vec!["1.png".into(), "2.png".into(), "3.png".into()], notes: vec![], transitions: vec![None, Some(short)], advance_ms: vec![], show: None };
        let json = serde_json::to_value(&deck).unwrap();
        assert_eq!(json["transitions"], serde_json::json!([null, { "kind": "wipe", "ms": crate::design::TRANSITION_MAX_MS, "dir": "down" }]));
        assert_eq!(serde_json::from_value::<Deck>(json).unwrap(), deck);
        let mut data = Data::default();
        data.decks.push(deck.clone());
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Deck { deck_id: deck.id }, background: None, notes: String::new(), theme: None };
        let (_, _, slides) = crate::slides::build_item(&data, &item);
        let t: Vec<_> = slides.iter().map(|s| match &s.content { SlideContent::Image { transition, .. } => *transition, _ => panic!() }).collect();
        assert_eq!(t, vec![None, Some(short), None]);
    }
}
