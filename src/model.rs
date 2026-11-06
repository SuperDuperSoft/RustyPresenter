//! Library, service plan, settings and the slide types every screen renders.
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::collections::BTreeMap;

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
    /// The media library folder this file is in; empty: none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
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
    /// `per_slide`: verses per slide, 0 = automatic (by length). `text_size`: the passage's largest text size in
    /// pixels on a 1080p screen, 0 = automatic (Settings → Look's scripture size).
    Scripture { reference: String, translation: String, verses: Vec<VerseText>, #[serde(default)] per_slide: u8, #[serde(default)] text_size: u32 },
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
}

/// How many recently picked Bibles `Settings.recent` keeps: enough that four remain once the chosen one and the
/// favourites are left out of the chips.
pub const RECENT_MAX: usize = 8;

impl Settings {
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
        let service = Service::new("Sunday Service");
        Self { format: 1, settings: Settings::default(), songs: vec![], media: vec![], media_folders: vec![], decks: vec![], current_service: Some(service.id), services: vec![service], bible_credits: BTreeMap::new() }
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
            kind: ItemKind::Scripture { reference: "Psalm 23".into(), translation: "KJV".into(), verses: vec![], per_slide: 1, text_size: 96 },
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
