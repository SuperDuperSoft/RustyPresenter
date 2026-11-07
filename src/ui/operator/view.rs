//! What the operator window shows, worked out without Slint: the state snapshot it draws from, item
//! subtitles, the Up next text, the live marker and the transport rows (`web/operator.js:62-250`).
use super::strings as tr;
use crate::{
    app::{App, LiveView, Monitor},
    model::{Data, EndAction, ItemKind, MediaFile, MediaStatus, ServiceItem, Settings, Slide, SlideContent},
    slides::build_item_with_notes,
    store::BackupInfo,
};
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use uuid::Uuid;

/// One service item as the list and the slide grid show it.
#[derive(Clone, Debug)]
/// `notes`: what the operator should know about the item's slides (a passage's missing verses), shown under the grid.
pub struct ItemRow { pub id: Uuid, pub kind: String, pub title: String, pub subtitle: String, pub slides: Vec<Slide>, pub item: ServiceItem, pub notes: Vec<String> }

/// The room pill: showing, opened but not drawing yet, or not open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomStatus { Showing, Starting, Off }

/// A background job (PowerPoint import) as the jobs box shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct JobRow { pub id: Uuid, pub label: String, pub percent: u8, pub error: Option<String>, pub note: Option<String>, pub cancellable: bool, pub done: bool, pub subject: Option<String> }

/// One translation's source for Settings → Bible (`App::bible_sources`; never carries a key).
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct BibleSource {
    pub translation: String,
    /// "offline", "online", "needs_key" or "unavailable".
    pub status: String,
    pub provider: Option<String>,
    pub imported: bool,
    pub copyright: Option<String>,
    /// Key names of the services that can supply this translation.
    pub providers: Vec<String>,
    /// What chips show ("RV1909"); empty for a translation the app only knows by name.
    pub abbr: String,
    pub name: String,
    /// Downloaded from a catalog (eBible), not a file the church imported.
    pub downloaded: bool,
    /// Licence short name and link for the credits ("CC BY-SA 4.0"); empty when the notice is all there is.
    pub licence: String,
    pub licence_url: String,
    /// The language code from the Bible's file ("spa"); empty for built-in and online Bibles.
    pub lang: String,
    /// The language as the picker heads it ("Spanish · Español"); empty when not known.
    pub language: String,
}

impl BibleSource {
    /// How the translation is named in lists: its abbreviation, or the id when it has none.
    pub fn label(&self) -> &str { if self.abbr.is_empty() { &self.translation } else { &self.abbr } }
}

/// An online Bible service a church can connect (`App::bible_providers`).
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct BibleProvider {
    pub id: String,
    pub label: String,
    pub signup_url: String,
    pub note: String,
    pub key_set: bool,
    pub translations: Vec<String>,
}

/// Everything `state_json` holds, typed. Built off the Slint thread (see `read`) and sent to it whole.
#[derive(Clone, Debug)]
pub struct OperatorSnapshot {
    pub service_id: Option<Uuid>,
    pub service_name: String,
    pub items: Vec<ItemRow>,
    pub translations: Vec<String>,
    pub available: Vec<String>,
    pub translation: String,
    pub bible_sources: Vec<BibleSource>,
    pub bible_providers: Vec<BibleProvider>,
    pub room: RoomStatus,
    pub stage_connected: bool,
    pub stage_configured: bool,
    pub phones: usize,
    pub jobs: Vec<JobRow>,
    /// How the last jobs ended: (job, what it made).
    pub job_ends: Vec<(Uuid, Option<Uuid>)>,
    pub settings: Settings,
    pub data: Arc<Data>,
    pub monitors: Vec<Monitor>,
    pub windows: Vec<String>,
    pub lan_url: Option<String>,
    pub player_missing: bool,
    pub backups: Vec<BackupInfo>,
    pub phones_list: Vec<(Uuid, String)>,
    pub data_folder: String,
    pub version: String,
    /// Which full read this is (`read_number`); a patch keeps the number of the read it changed.
    pub read: u64,
}

/// Full reads, numbered in the order they start.
static READS: AtomicU64 = AtomicU64::new(0);

/// The number of the newest full read started so far: a snapshot whose `read` is greater began after this call.
pub fn read_number() -> u64 { READS.load(Ordering::SeqCst) }

// The follower thread builds it and sends it to the Slint thread.
const _: fn() = || { fn send<T: Send + Sync>() {} send::<OperatorSnapshot>(); };

impl OperatorSnapshot {
    /// Reads the same sources as `App::state_json`. **Never call it on the Slint thread:** it parses every backup
    /// and asks `Screens` for monitors, which `NativeScreens` answers on the Slint thread.
    /// The core does not know whether libmpv loaded, so `player_missing` starts false; see `with_player_missing`.
    pub fn read(app: &App) -> Self {
        let read = READS.fetch_add(1, Ordering::SeqCst) + 1;
        let data = app.data.lock().unwrap().clone();
        let translations = app.translations();
        let available = app.available_translations();
        // A saved translation may be an imported Bible that has since been removed (`operator.js:97`).
        let translation = if translations.contains(&data.settings.translation) { data.settings.translation.clone() }
            else { available.first().cloned().unwrap_or_else(|| "KJV".into()) };
        let windows = app.open_roles();
        let room = if app.room_conns.load(Ordering::Relaxed) > 0 { RoomStatus::Showing }
            else if windows.iter().any(|w| w == "room") { RoomStatus::Starting } else { RoomStatus::Off };
        let items = data.service().map(|s| s.items.iter().map(|i| {
            let (title, kind, slides, notes) = build_item_with_notes(&data, i);
            ItemRow { id: i.id, kind: kind.into(), title, subtitle: String::new(), slides, item: i.clone(), notes }
        }).collect()).unwrap_or_default();
        let mut snap = Self {
            service_id: data.service().map(|s| s.id),
            service_name: data.service().map_or_else(tr::service, |s| s.name.clone()).into(),
            items,
            translations,
            available,
            translation,
            bible_sources: typed(app.bible_sources()),
            bible_providers: typed(app.bible_providers()),
            room,
            stage_connected: app.stage_conns.load(Ordering::Relaxed) > 0,
            stage_configured: data.settings.screens.stage.on && !data.settings.screens.keyed("stage").is_empty(),
            phones: app.remote_conns.load(Ordering::Relaxed),
            jobs: app.jobs.lock().unwrap().iter().map(|j| JobRow { id: j.id, label: j.label.clone(), percent: j.percent, error: j.error.clone(), note: j.note.clone(), cancellable: j.cancellable, done: j.done, subject: j.subject.clone() }).collect(),
            job_ends: app.job_ends.lock().unwrap().iter().copied().collect(),
            settings: data.settings.clone(),
            monitors: app.monitors(),
            windows,
            lan_url: app.lan_url.clone(),
            player_missing: false,
            backups: app.store.backups(),
            phones_list: app.phones.lock().unwrap().iter().map(|p| (p.id, p.name.clone())).collect(),
            data_folder: app.store.root.display().to_string(),
            version: env!("CARGO_PKG_VERSION").into(),
            data: Arc::new(data),
            read,
        };
        snap.refresh_subtitles();
        snap
    }

    /// Sets whether libmpv is missing (the windowing layer knows) and rewrites the subtitles to match.
    pub fn with_player_missing(mut self, missing: bool) -> Self {
        self.player_missing = missing;
        self.refresh_subtitles();
        self
    }

    /// Recomputes every row's subtitle, e.g. after a conversion progress patch to `data.media`.
    pub fn refresh_subtitles(&mut self) {
        for row in &mut self.items { row.subtitle = subtitle(row, &self.data.media, self.player_missing); }
    }
}

/// Parses `state_json`-shaped rows; a row that does not fit is dropped rather than failing the whole read.
fn typed<T: serde::de::DeserializeOwned>(rows: Vec<serde_json::Value>) -> Vec<T> {
    rows.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()
}

/// The stage pill is shown when a stage screen is chosen or one is connected; `Some(on)` (`operator.js:105`).
pub fn stage_pill(snap: &OperatorSnapshot) -> Option<bool> {
    (snap.stage_configured || snap.stage_connected).then_some(snap.stage_connected)
}

/// "1 phone", "N phones", or no pill (`operator.js:106`).
pub fn phones_label(n: usize) -> Option<String> { (n > 0).then(|| tr::phones(n)) }

/// The line under an item's title (`operator.js:111-121`).
pub fn subtitle(row: &ItemRow, media: &[MediaFile], player_missing: bool) -> String {
    let name = tr::kind_name(&row.kind);
    if matches!(row.kind.as_str(), "video" | "image" | "audio") {
        let (media_id, end, looped) = match &row.item.kind {
            ItemKind::Media { media_id, end, looped, .. } => (Some(*media_id), *end, *looped),
            _ => (None, EndAction::Stop, false),
        };
        if player_missing && row.kind != "image" { return tr::player_missing(&name); }
        match media.iter().find(|m| Some(m.id) == media_id).map(|m| &m.status) {
            Some(MediaStatus::Failed { .. }) => return tr::cant_be_played(&name),
            // `Converting` and `Unconverted` only come from an old `data.json`, and `upgrade_media` removes them.
            _ => {}
        }
        return format!("{name}{}", if end == EndAction::Loop || looped { tr::loops() } else { String::new() });
    }
    tr::slide_count(&name, row.slides.len())
}

/// Which item stays selected after the list changes: the same one, else the live one, else the first (`operator.js:73`).
pub fn reselect(selected: Option<Uuid>, items: &[ItemRow], live_item: Option<Uuid>) -> Option<Uuid> {
    let has = |id: Option<Uuid>| id.filter(|id| items.iter().any(|i| i.id == *id));
    has(selected).or(has(live_item)).or(items.first().map(|i| i.id))
}

/// Where a new Bible passage goes: right after the live item, or `None` to append (`operator.js:337-340`).
pub fn insert_position(items: &[ItemRow], live_item: Option<Uuid>) -> Option<usize> {
    let live = live_item?;
    items.iter().position(|i| i.id == live).map(|i| i + 1)
}

/// The "what is live" line over the monitor (`operator.js:224-225`).
pub fn live_what(view: &LiveView) -> String {
    let f = &view.frame;
    if f.logo { return tr::logo().into(); }
    if f.slide.is_some() || f.media.is_some() {
        let Some(item) = &view.item else { return String::new() };
        return match &view.slide { Some(s) => format!("{} · {}", item.title, s.label), None => item.title.clone() };
    }
    match &f.audio { Some(a) => tr::audio_title(&a.v.title), None => tr::clear().into() }
}

/// The Up next box: the small title · label line and the body (`operator.js:231-236`).
pub fn next_up(view: &LiveView) -> (String, String) {
    let Some(n) = &view.next else {
        return (String::new(), if view.item.is_some() { tr::end_of_service() } else { tr::NOTHING.into() });
    };
    let body = match &n.slide.content {
        SlideContent::Text { text, .. } => text.clone(),
        SlideContent::Image { .. } => tr::slide_picture(),
        SlideContent::Video { .. } => tr::video(),
        SlideContent::Still { .. } => tr::picture(),
        SlideContent::Audio { .. } => tr::audio(),
    };
    (format!("{} · {}", n.title, n.slide.label), body)
}

/// The thumbnail to mark live: the cursor's slide when it is on the selected item and something is on air (`markLive`).
pub fn live_index(view: &LiveView, selected: Option<Uuid>) -> Option<usize> {
    let c = view.cursor.as_ref()?;
    let f = &view.frame;
    let on_air = f.slide.is_some() || f.media.is_some() || f.audio.is_some();
    (selected == Some(c.item_id) && view.item.is_some() && on_air).then_some(c.slide)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportKind { Video, Audio }

/// One play/pause row under the monitor (`operator.js:240-261`).
#[derive(Clone, Debug, PartialEq)]
pub struct TransportRow { pub kind: TransportKind, pub inst: u64, pub label: String, pub paused: bool, pub percent: f32, pub time: String }

/// A row for a playing video and one for audio. Room-reported times count only for the instance on air.
pub fn transport(view: &LiveView) -> Vec<TransportRow> {
    let row = |kind, inst, paused, st: Option<&crate::live::PlayStatus>, label: String| {
        let (pos, dur) = st.filter(|s| s.inst == inst).map_or((0.0, 0.0), |s| (s.position, s.duration));
        let percent = if dur > 0.0 { (pos / dur * 100.0).min(100.0) as f32 } else { 0.0 };
        let time = if dur > 0.0 { format!("{} / −{}", fmt_time(pos), fmt_time(dur - pos)) } else { tr::NOTHING.into() };
        TransportRow { kind, inst, label, paused, percent, time }
    };
    let f = &view.frame;
    let mut rows = vec![];
    if let Some(m) = &f.media && let crate::live::MediaLayer::Video { paused, .. } = m.v {
        rows.push(row(TransportKind::Video, m.inst, paused, view.media.as_ref(), tr::video().into()));
    }
    if let Some(a) = &f.audio {
        rows.push(row(TransportKind::Audio, a.inst, a.v.paused, view.audio.as_ref(), tr::audio_title(&a.v.title)));
    }
    rows
}

/// `m:ss`, floored and never below zero (`fmtTime`, `operator.js:42`).
pub fn fmt_time(s: f64) -> String {
    let s = if s.is_finite() { s.max(0.0).floor() as u64 } else { 0 };
    format!("{}:{:02}", s / 60, s % 60)
}

/// "1 item", "N items", or nothing for an empty service (`operator.js:129`).
pub fn count_label(n: usize) -> String { if n == 0 { String::new() } else { tr::items(n) } }

/// How many chips follow the chosen Bible's in the Bible box (spec §7.2: Alt+1 is the chosen one, Alt+2…5 these).
pub const QUICK_CHIPS: usize = 4;

/// One Bible as the Bible box's chips and picker show it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BibleChoice {
    /// What lookups and `settings/translation` take ("KJV", "ebible:spaRV1909").
    pub id: String,
    /// The chip's text: the abbreviation, with the language and then "imported" added where two would read alike.
    pub label: String,
    /// The full name, for the picker and the chip's tip; empty when the app only knows the abbreviation.
    pub name: String,
    /// Not connected yet (a licensed Bible with no key): shown faded, still pickable.
    pub off: bool,
    pub favourite: bool,
}

/// What each translation is called on chips (spec §7.2). Abbreviations that read alike, ignoring case, get the
/// language code, then "imported" for a file the church imported, so "RV1909 · spa" and "RV1909 · spa · imported"
/// can sit side by side. A label does not change with what is next to it.
pub fn bible_labels(s: &OperatorSnapshot) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;
    let source = |t: &str| s.bible_sources.iter().find(|b| b.translation == t);
    let base = |t: &str| source(t).map_or(t, BibleSource::label).to_string();
    let mut count: HashMap<String, usize> = HashMap::new();
    for t in &s.translations { *count.entry(base(t).to_lowercase()).or_default() += 1 }
    let mut labels: HashMap<String, String> = s.translations.iter().map(|t| {
        let mut label = base(t);
        if count[&label.to_lowercase()] > 1 && let Some(b) = source(t) && !b.lang.is_empty() { label = format!("{label} · {}", b.lang) }
        (t.clone(), label)
    }).collect();
    let mut count: HashMap<String, usize> = HashMap::new();
    for l in labels.values() { *count.entry(l.to_lowercase()).or_default() += 1 }
    for t in &s.translations {
        let l = &labels[t];
        if count[&l.to_lowercase()] > 1 && source(t).is_some_and(|b| b.imported && !b.downloaded) {
            let l = format!("{l} · {}", tr::bible_imported());
            labels.insert(t.clone(), l);
        }
    }
    labels
}

/// The translation `t` as a chip or picker row.
fn bible_choice(s: &OperatorSnapshot, labels: &std::collections::HashMap<String, String>, t: &str) -> BibleChoice {
    let source = s.bible_sources.iter().find(|b| b.translation == t);
    BibleChoice {
        id: t.to_string(),
        label: labels.get(t).cloned().unwrap_or_else(|| t.to_string()),
        name: source.map(|b| b.name.clone()).unwrap_or_default(),
        off: !s.available.iter().any(|a| a == t),
        favourite: s.settings.favourites.iter().any(|f| crate::bible::id::same_id(f, t)),
    }
}

/// The ids in `list` that are still installed (or known), each once, leaving out `skip`.
fn installed<'a>(s: &'a OperatorSnapshot, list: &'a [String], skip: &'a [String]) -> impl Iterator<Item = &'a String> + 'a {
    use crate::bible::id::same_id;
    list.iter().enumerate()
        .filter(|(i, t)| !list[..*i].iter().any(|u| same_id(u, t)))
        .filter_map(|(_, t)| s.translations.iter().find(|u| same_id(u, t)))
        .filter(move |t| !skip.iter().any(|u| same_id(u, t)))
}

/// The Bible box's chips (spec §7.2): the chosen Bible first, then up to `QUICK_CHIPS` more: favourites, then the
/// ones picked lately. While there are fewer than that (a new install has none), the Bibles that work now fill the
/// rest in list order, so Alt+2… and one-click switching work from the first day.
pub fn bible_chips(s: &OperatorSnapshot) -> Vec<BibleChoice> {
    let labels = bible_labels(s);
    let mut ids: Vec<String> = vec![s.translation.clone()];
    let more: Vec<String> = installed(s, &s.settings.favourites, &ids).cloned()
        .chain(installed(s, &s.settings.recent, &ids).cloned())
        .chain(s.translations.iter().filter(|t| s.available.contains(t)).cloned())
        .collect();
    for t in more {
        if ids.len() > QUICK_CHIPS { break }
        if !ids.iter().any(|u| crate::bible::id::same_id(u, &t)) { ids.push(t) }
    }
    ids.iter().map(|t| bible_choice(s, &labels, t)).collect()
}

/// One line of the Bible picker: a heading, or a Bible.
#[derive(Clone, Debug, PartialEq)]
pub enum PickerLine { Heading(String), Bible(BibleChoice) }

/// The Bible picker's lines (spec §7.2): Favourites, Recent, then every other installed Bible under its language
/// (English first, then A to Z, then Bibles whose language is not known). Each Bible shows once. The search keeps
/// the Bibles whose abbreviation, name or language holds every word (folded like the catalog's search); headings
/// with nothing under them go.
pub fn picker_lines(s: &OperatorSnapshot, query: &str) -> Vec<PickerLine> {
    use crate::bible::blocklist::normalise;
    let labels = bible_labels(s);
    let words: Vec<String> = query.split_whitespace().map(normalise).filter(|w| !w.is_empty()).collect();
    let language = |t: &str| s.bible_sources.iter().find(|b| b.translation == t).map(|b| b.language.clone()).unwrap_or_default();
    let matches = |c: &BibleChoice| {
        let hay = normalise(&format!("{} {} {} {}", c.label, c.name, c.id, language(&c.id)));
        words.iter().all(|w| hay.contains(w.as_str()))
    };
    let favourites: Vec<String> = installed(s, &s.settings.favourites, &[]).cloned().collect();
    let recent: Vec<String> = installed(s, &s.settings.recent, &favourites).take(QUICK_CHIPS).cloned().collect();
    let mut groups: Vec<(String, Vec<String>)> = vec![(tr::picker_favourites().into(), favourites.clone()), (tr::picker_recent().into(), recent.clone())];
    let shown: Vec<String> = favourites.into_iter().chain(recent).collect();
    let mut by_language: Vec<(String, Vec<String>)> = Vec::new();
    for t in s.translations.iter().filter(|t| !shown.contains(t)) {
        let l = language(t);
        match by_language.iter_mut().find(|(k, _)| *k == l) { Some((_, v)) => v.push(t.clone()), None => by_language.push((l, vec![t.clone()])) }
    }
    by_language.sort_by_key(|(l, _)| (l != "English", l.is_empty(), l.to_lowercase()));
    groups.extend(by_language.into_iter().map(|(l, v)| (if l.is_empty() { tr::picker_other() } else { l }, v)));
    let mut out = Vec::new();
    for (heading, ids) in groups {
        let rows: Vec<BibleChoice> = ids.iter().map(|t| bible_choice(s, &labels, t)).filter(|c| matches(c)).collect();
        if rows.is_empty() { continue }
        out.push(PickerLine::Heading(heading));
        out.extend(rows.into_iter().map(PickerLine::Bible));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{LiveItemSummary, NextUp},
        live::{AudioLayer, Cursor, Frame, Inst, MediaLayer, PlayState, PlayStatus, SlideLayer},
        model::{Background, MediaKind, Theme},
    };

    fn source(id: &str, abbr: &str, lang: &str, imported: bool, downloaded: bool) -> BibleSource {
        BibleSource { translation: id.into(), abbr: abbr.into(), lang: lang.into(), imported, downloaded, status: "offline".into(), ..Default::default() }
    }

    #[test]
    fn alike_abbreviations_get_the_language_then_imported() {
        let mut s = OperatorSnapshot::read(&crate::app::test_app());
        let extra = [
            source("ebible:spaRV1909", "RV1909", "spa", true, true),
            source("RV1909", "RV1909", "spa", true, false),
            source("ebible:ptbRV1909", "rv1909", "ptb", true, true),
            source("ebible:engKJV2", "KJV", "eng", true, true),
        ];
        for b in extra {
            s.translations.push(b.translation.clone());
            s.available.push(b.translation.clone());
            s.bible_sources.push(b);
        }
        let l = bible_labels(&s);
        assert_eq!(l["ebible:spaRV1909"], "RV1909 · spa");
        assert_eq!(l["RV1909"], "RV1909 · spa · imported");
        assert_eq!(l["ebible:ptbRV1909"], "rv1909 · ptb", "case does not tell them apart");
        assert_eq!(l["ebible:engKJV2"], "KJV · eng");
        assert_eq!(l["KJV"], "KJV", "a built-in has no language to add");
        assert_eq!(l["WEB"], "WEB");
    }

    #[test]
    fn chips_skip_removed_and_repeated_bibles() {
        let mut s = OperatorSnapshot::read(&crate::app::test_app());
        s.settings.favourites = vec!["GONE".into(), "web".into(), "BSB".into(), "BSB".into()];
        s.settings.recent = vec!["KJV".into(), "NIV".into()];
        let ids: Vec<String> = bible_chips(&s).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["KJV", "WEB", "BSB", "NIV"], "the chosen one is not repeated; a removed one is skipped");
        let c = &bible_chips(&s)[1];
        assert!(c.favourite && !c.off && c.name == "World English Bible", "{c:?}");
        assert!(bible_chips(&s)[3].off, "NIV needs a key");
    }

    fn media_file(kind: MediaKind, status: MediaStatus) -> MediaFile {
        MediaFile { id: Uuid::new_v4(), name: "clip".into(), kind, asset: "a.mp4".into(), status, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() }
    }

    fn media_row(m: &MediaFile, kind: &str, end: EndAction, looped: bool) -> ItemRow {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Media { media_id: m.id, autoplay: true, end, looped }, background: None, notes: String::new(), theme: None };
        ItemRow { id: item.id, kind: kind.into(), title: m.name.clone(), subtitle: String::new(), slides: vec![text_slide("clip", "")], item, notes: vec![] }
    }

    fn text_slide(label: &str, text: &str) -> Slide {
        Slide::new(label, SlideContent::Text { text: text.into(), footer: None, reference: None, size: 0, parallel: None })
    }

    fn row(kind: &str, slides: usize) -> ItemRow {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "T".into(), body: String::new() }, background: None, notes: String::new(), theme: None };
        ItemRow { id: item.id, kind: kind.into(), title: "T".into(), subtitle: String::new(), slides: (0..slides).map(|i| text_slide(&format!("S{i}"), "x")).collect(), item, notes: vec![] }
    }

    fn view() -> LiveView {
        LiveView {
            version: 1, committed: 1, pending: false, frame: Frame::default(), cursor: None, item: None, slide: None, next: None,
            media: None, audio: None, notice: None, sections: vec![], changed: None, theme: Theme::default(), logo: None, clear_epoch: 0, stage_message: String::new(),
        }
    }

    fn summary(title: &str) -> Option<LiveItemSummary> {
        Some(LiveItemSummary { item_id: Uuid::new_v4(), title: title.into(), kind: "song".into(), count: 3, song_id: None })
    }

    fn text_layer() -> Option<SlideLayer> { Some(SlideLayer::Text { text: "Amazing grace".into(), footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None, theme: None }) }

    fn snap(configured: bool, connected: bool) -> OperatorSnapshot {
        let app = crate::app::test_app();
        let mut s = OperatorSnapshot::read(&app);
        s.stage_configured = configured;
        s.stage_connected = connected;
        s
    }

    #[test]
    fn subtitle_for_each_kind_and_media_state() {
        let failed = media_file(MediaKind::Video, MediaStatus::Failed { reason: "bad".into() });
        let ready = media_file(MediaKind::Video, MediaStatus::Ready);
        let pic = media_file(MediaKind::Image, MediaStatus::Ready);
        let audio = media_file(MediaKind::Audio, MediaStatus::Ready);
        let media = vec![failed.clone(), ready.clone(), pic.clone(), audio.clone()];
        let sub = |r: ItemRow| subtitle(&r, &media, false);
        assert_eq!(sub(media_row(&ready, "video", EndAction::Loop, false)), "Video · loops");
        assert_eq!(sub(media_row(&ready, "video", EndAction::Stop, false)), "Video");
        assert_eq!(sub(media_row(&pic, "image", EndAction::Stop, false)), "Picture");
        assert_eq!(sub(media_row(&audio, "audio", EndAction::Stop, true)), "Audio · loops");
        assert_eq!(sub(row("song", 4)), "Song · 4 slides");
        assert_eq!(sub(row("scripture", 1)), "Scripture · 1 slide");
        assert_eq!(sub(row("deck", 2)), "Slides · 2 slides");
        assert_eq!(sub(row("text", 1)), "Announcement · 1 slide");
    }

    #[test]
    fn failed_video_subtitle_says_cant_be_played() {
        let failed = media_file(MediaKind::Video, MediaStatus::Failed { reason: "bad".into() });
        let media = vec![failed.clone()];
        assert_eq!(subtitle(&media_row(&failed, "video", EndAction::Stop, false), &media, false), "Video · can't be played");
    }

    #[test]
    fn old_conversion_states_read_like_ready() {
        for status in [MediaStatus::Converting { percent: 40 }, MediaStatus::Unconverted { reason: "codec".into() }] {
            let m = media_file(MediaKind::Video, status);
            let media = vec![m.clone()];
            assert_eq!(subtitle(&media_row(&m, "video", EndAction::Stop, false), &media, false), "Video");
        }
    }

    #[test]
    fn subtitle_pluralises_slides() {
        assert_eq!(subtitle(&row("song", 0), &[], false), "Song · 0 slides");
        assert_eq!(subtitle(&row("song", 1), &[], false), "Song · 1 slide");
        assert_eq!(subtitle(&row("song", 2), &[], false), "Song · 2 slides");
    }

    #[test]
    fn subtitle_says_player_missing_for_video_and_audio() {
        let v = media_file(MediaKind::Video, MediaStatus::Converting { percent: 10 });
        let a = media_file(MediaKind::Audio, MediaStatus::Ready);
        let p = media_file(MediaKind::Image, MediaStatus::Ready);
        let media = vec![v.clone(), a.clone(), p.clone()];
        assert_eq!(subtitle(&media_row(&v, "video", EndAction::Stop, false), &media, true), "Video · The video player is missing — reinstall RustyPresenter");
        assert_eq!(subtitle(&media_row(&a, "audio", EndAction::Stop, true), &media, true), "Audio · The video player is missing — reinstall RustyPresenter");
        assert_eq!(subtitle(&media_row(&p, "image", EndAction::Stop, false), &media, true), "Picture");
        assert_eq!(subtitle(&row("song", 2), &media, true), "Song · 2 slides");
    }

    #[test]
    fn stage_pill_shown_when_configured_or_connected() {
        assert_eq!(stage_pill(&snap(false, false)), None);
        assert_eq!(stage_pill(&snap(true, false)), Some(false));
        assert_eq!(stage_pill(&snap(false, true)), Some(true));
        assert_eq!(stage_pill(&snap(true, true)), Some(true));
    }

    #[test]
    fn phones_label_singular_plural_and_hidden() {
        assert_eq!(phones_label(0), None);
        assert_eq!(phones_label(1).as_deref(), Some("1 phone"));
        assert_eq!(phones_label(3).as_deref(), Some("3 phones"));
    }

    #[test]
    fn reselect_keeps_existing_selection() {
        let items = vec![row("song", 1), row("song", 1), row("song", 1)];
        assert_eq!(reselect(Some(items[2].id), &items, Some(items[1].id)), Some(items[2].id));
    }

    #[test]
    fn reselect_falls_back_to_live_then_first_then_none() {
        let items = vec![row("song", 1), row("song", 1)];
        let gone = Some(Uuid::new_v4());
        assert_eq!(reselect(gone, &items, Some(items[1].id)), Some(items[1].id));
        assert_eq!(reselect(gone, &items, Some(Uuid::new_v4())), Some(items[0].id), "a removed live item does not count");
        assert_eq!(reselect(None, &items, None), Some(items[0].id));
        assert_eq!(reselect(gone, &[], gone), None);
    }

    #[test]
    fn insert_position_after_live_item_or_append() {
        let items = vec![row("song", 1), row("song", 1), row("song", 1)];
        assert_eq!(insert_position(&items, Some(items[0].id)), Some(1));
        assert_eq!(insert_position(&items, Some(items[2].id)), Some(3));
        assert_eq!(insert_position(&items, Some(Uuid::new_v4())), None);
        assert_eq!(insert_position(&items, None), None);
    }

    #[test]
    fn live_what_priority_logo_slide_audio_clear() {
        let mut v = view();
        assert_eq!(live_what(&v), "Clear");
        v.frame.audio = Some(Inst { inst: 3, v: AudioLayer { asset: "a.mp3".into(), title: "Walk-in".into(), looped: true, paused: false } });
        assert_eq!(live_what(&v), "♪ Walk-in");
        v.item = summary("Amazing Grace");
        v.slide = Some(text_slide("Verse 1", "Amazing grace"));
        v.frame.slide = text_layer();
        assert_eq!(live_what(&v), "Amazing Grace · Verse 1");
        v.frame.logo = true;
        assert_eq!(live_what(&v), "Logo");
        // Media without a slide shows the item title alone.
        let mut m = view();
        m.item = summary("Clip");
        m.frame.media = Some(Inst { inst: 1, v: MediaLayer::Still { asset: "p.png".into() } });
        assert_eq!(live_what(&m), "Clip");
    }

    #[test]
    fn next_up_text_placeholders_and_end_of_service() {
        let mut v = view();
        assert_eq!(next_up(&v), (String::new(), "—".into()));
        v.item = summary("Amazing Grace");
        assert_eq!(next_up(&v), (String::new(), "End of service".into()));
        v.next = Some(NextUp { title: "Amazing Grace".into(), slide: text_slide("Verse 2", "Twas grace") });
        assert_eq!(next_up(&v), ("Amazing Grace · Verse 2".into(), "Twas grace".into()));
        let placeholder = |content| { let mut v = view(); v.next = Some(NextUp { title: "X".into(), slide: Slide::new("L", content) }); next_up(&v).1 };
        assert_eq!(placeholder(SlideContent::Image { asset: "s.png".into() }), "Slide picture");
        assert_eq!(placeholder(SlideContent::Video { asset: "v.mp4".into(), autoplay: true, end: EndAction::Stop }), "Video");
        assert_eq!(placeholder(SlideContent::Still { asset: "p.png".into() }), "Picture");
        assert_eq!(placeholder(SlideContent::Audio { asset: "a.mp3".into(), looped: false }), "Audio");
    }

    #[test]
    fn live_index_only_when_cursor_on_selected_and_on_air() {
        let mut v = view();
        let (sel, other) = (Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(live_index(&v, Some(sel)), None);
        v.cursor = Some(Cursor { item_id: sel, item_index: 0, slide: 2 });
        v.item = summary("Song");
        assert_eq!(live_index(&v, Some(sel)), None, "nothing on air");
        v.frame.slide = text_layer();
        assert_eq!(live_index(&v, Some(sel)), Some(2));
        assert_eq!(live_index(&v, Some(other)), None);
        assert_eq!(live_index(&v, None), None);
        v.item = None;
        assert_eq!(live_index(&v, Some(sel)), None);
        v.item = summary("Song");
        v.frame.slide = None;
        v.frame.audio = Some(Inst { inst: 1, v: AudioLayer { asset: "a".into(), title: "t".into(), looped: false, paused: false } });
        assert_eq!(live_index(&v, Some(sel)), Some(2));
    }

    #[test]
    fn transport_ignores_status_from_other_instance() {
        let mut v = view();
        assert!(transport(&v).is_empty());
        v.frame.media = Some(Inst { inst: 7, v: MediaLayer::Video { asset: "v.mp4".into(), end: EndAction::Stop, paused: true } });
        v.media = Some(PlayStatus { inst: 6, state: PlayState::Playing, position: 30.0, duration: 60.0 });
        let rows = transport(&v);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].kind, rows[0].inst, rows[0].label.as_str(), rows[0].paused), (TransportKind::Video, 7, "Video", true));
        assert_eq!((rows[0].percent, rows[0].time.as_str()), (0.0, "—"));
        v.media = Some(PlayStatus { inst: 7, state: PlayState::Playing, position: 30.0, duration: 60.0 });
        assert_eq!(transport(&v)[0].percent, 50.0);
        // A still on the media layer has no transport; audio gets its own row.
        v.frame.media = Some(Inst { inst: 8, v: MediaLayer::Still { asset: "p.png".into() } });
        v.frame.audio = Some(Inst { inst: 9, v: AudioLayer { asset: "a".into(), title: "Walk-in".into(), looped: true, paused: false } });
        v.audio = Some(PlayStatus { inst: 9, state: PlayState::Playing, position: 200.0, duration: 100.0 });
        let rows = transport(&v);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].kind, rows[0].label.as_str(), rows[0].percent), (TransportKind::Audio, "♪ Walk-in", 100.0));
    }

    #[test]
    fn transport_time_format_and_dash_without_duration() {
        let mut v = view();
        v.frame.media = Some(Inst { inst: 1, v: MediaLayer::Video { asset: "v.mp4".into(), end: EndAction::Stop, paused: false } });
        v.media = Some(PlayStatus { inst: 1, state: PlayState::Playing, position: 65.4, duration: 185.0 });
        assert_eq!(transport(&v)[0].time, "1:05 / −1:59");
        v.media = Some(PlayStatus { inst: 1, state: PlayState::Playing, position: 3.0, duration: 0.0 });
        assert_eq!(transport(&v)[0].time, "—");
        v.media = None;
        assert_eq!(transport(&v)[0].time, "—");
    }

    #[test]
    fn fmt_time_clamps_and_floors() {
        assert_eq!(fmt_time(-5.0), "0:00");
        assert_eq!(fmt_time(0.0), "0:00");
        assert_eq!(fmt_time(59.99), "0:59");
        assert_eq!(fmt_time(61.0), "1:01");
        assert_eq!(fmt_time(3600.0), "60:00");
        assert_eq!(fmt_time(f64::NAN), "0:00");
    }

    #[test]
    fn count_label_empty_for_zero() {
        assert_eq!(count_label(0), "");
        assert_eq!(count_label(1), "1 item");
        assert_eq!(count_label(5), "5 items");
    }

    #[test]
    fn snapshot_reads_the_service_and_status() {
        let app = crate::app::test_app();
        let s = OperatorSnapshot::read(&app);
        assert_eq!(s.service_name, "Sunday Service");
        assert!(s.items.is_empty());
        assert_eq!(s.room, RoomStatus::Off);
        assert_eq!(s.translation, "KJV");
        assert!(s.translations.contains(&"KJV".to_string()));
        let _guard = app.screen_guard("room");
        assert_eq!(OperatorSnapshot::read(&app).room, RoomStatus::Showing);
        app.edit(|d| {
            d.settings.translation = "GONE".into();
            let bg = Some(Background::Color { color: "#112233".into() });
            d.service_mut().unwrap().items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Hi".into() }, background: bg, notes: String::new(), theme: None });
            Ok(())
        }).unwrap();
        let s = OperatorSnapshot::read(&app);
        assert_eq!((s.items[0].title.as_str(), s.items[0].kind.as_str(), s.items[0].subtitle.as_str()), ("Welcome", "text", "Announcement · 1 slide"));
        assert_eq!(s.translation, s.available[0], "a removed translation falls back to the first available");
    }

    #[test]
    fn snapshot_carries_bible_sources_and_providers_like_state_json() {
        let app = crate::app::test_app();
        let s = OperatorSnapshot::read(&app);
        let state = app.state_json();
        assert_eq!(s.bible_sources.len(), state["bible_sources"].as_array().unwrap().len());
        assert_eq!(s.bible_providers.len(), state["bible_providers"].as_array().unwrap().len());
        let kjv = s.bible_sources.iter().find(|b| b.translation == "KJV").unwrap();
        assert_eq!(kjv.status, "offline");
        let esv = s.bible_sources.iter().find(|b| b.translation == "ESV").unwrap();
        assert_eq!((esv.status.as_str(), esv.providers.contains(&"esv".to_string())), ("needs_key", true));
        let p = s.bible_providers.iter().find(|p| p.id == "esv").unwrap();
        assert!(!p.key_set && p.translations.contains(&"ESV".to_string()) && !p.label.is_empty());
    }
}
