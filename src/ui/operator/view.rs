//! What the operator window shows, worked out without Slint: the state snapshot it draws from, item
//! subtitles, the Up next text, the live marker and the transport rows (`web/operator.js:62-250`).
use super::strings as tr;
use crate::{
    app::{App, LiveView, Monitor},
    model::{Data, EndAction, ItemKind, MediaFile, MediaStatus, ServiceItem, Settings, Slide, SlideContent},
    slides::build_item,
    store::BackupInfo,
};
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use uuid::Uuid;

/// One service item as the list and the slide grid show it.
#[derive(Clone, Debug)]
pub struct ItemRow { pub id: Uuid, pub kind: String, pub title: String, pub subtitle: String, pub slides: Vec<Slide>, pub item: ServiceItem }

/// The room pill: showing, opened but not drawing yet, or not open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomStatus { Showing, Starting, Off }

/// A background job (PowerPoint import) as the jobs box shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct JobRow { pub id: Uuid, pub label: String, pub percent: u8, pub error: Option<String>, pub note: Option<String>, pub cancellable: bool, pub done: bool }

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
    pub settings: Settings,
    pub data: Arc<Data>,
    pub monitors: Vec<Monitor>,
    pub windows: Vec<String>,
    pub lan_url: Option<String>,
    pub ffmpeg: bool,
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
    /// Reads the same sources as `App::state_json`. **Never call it on the Slint thread:** it parses every backup,
    /// searches PATH for ffmpeg and asks `Screens` for monitors, which `NativeScreens` answers on the Slint thread.
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
            let (title, kind, slides) = build_item(&data, i);
            ItemRow { id: i.id, kind: kind.into(), title, subtitle: String::new(), slides, item: i.clone() }
        }).collect()).unwrap_or_default();
        let mut snap = Self {
            service_id: data.service().map(|s| s.id),
            service_name: data.service().map_or(tr::SERVICE, |s| s.name.as_str()).into(),
            items,
            translations,
            available,
            translation,
            bible_sources: typed(app.bible_sources()),
            bible_providers: typed(app.bible_providers()),
            room,
            stage_connected: app.stage_conns.load(Ordering::Relaxed) > 0,
            stage_configured: data.settings.displays.stage.is_some(),
            phones: app.remote_conns.load(Ordering::Relaxed),
            jobs: app.jobs.lock().unwrap().iter().map(|j| JobRow { id: j.id, label: j.label.clone(), percent: j.percent, error: j.error.clone(), note: j.note.clone(), cancellable: j.cancellable, done: j.done }).collect(),
            settings: data.settings.clone(),
            monitors: app.monitors(),
            windows,
            lan_url: app.lan_url.clone(),
            ffmpeg: crate::media::ffmpeg().is_some(),
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
        if player_missing && row.kind != "image" { return tr::player_missing(name); }
        match media.iter().find(|m| Some(m.id) == media_id).map(|m| &m.status) {
            Some(MediaStatus::Converting { percent }) => return tr::converting(name, *percent),
            Some(MediaStatus::Unconverted { .. }) => return tr::needs_converter(name),
            Some(MediaStatus::Failed { .. }) => return tr::conversion_failed(name),
            _ => {}
        }
        return format!("{name}{}", if end == EndAction::Loop || looped { tr::LOOPS } else { "" });
    }
    tr::slide_count(name, row.slides.len())
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
    if f.logo { return tr::LOGO.into(); }
    if f.slide.is_some() || f.media.is_some() {
        let Some(item) = &view.item else { return String::new() };
        return match &view.slide { Some(s) => format!("{} · {}", item.title, s.label), None => item.title.clone() };
    }
    match &f.audio { Some(a) => tr::audio_title(&a.v.title), None => tr::CLEAR.into() }
}

/// The Up next box: the small title · label line and the body (`operator.js:231-236`).
pub fn next_up(view: &LiveView) -> (String, String) {
    let Some(n) = &view.next else {
        return (String::new(), if view.item.is_some() { tr::END_OF_SERVICE } else { tr::NOTHING }.into());
    };
    let body = match &n.slide.content {
        SlideContent::Text { text, .. } => text.as_str(),
        SlideContent::Image { .. } => tr::SLIDE_PICTURE,
        SlideContent::Video { .. } => tr::VIDEO,
        SlideContent::Still { .. } => tr::PICTURE,
        SlideContent::Audio { .. } => tr::AUDIO,
    };
    (format!("{} · {}", n.title, n.slide.label), body.into())
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
        rows.push(row(TransportKind::Video, m.inst, paused, view.media.as_ref(), tr::VIDEO.into()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{LiveItemSummary, NextUp},
        live::{AudioLayer, Cursor, Frame, Inst, MediaLayer, PlayState, PlayStatus, SlideLayer},
        model::{Background, MediaKind, Theme},
    };

    fn media_file(kind: MediaKind, status: MediaStatus) -> MediaFile {
        MediaFile { id: Uuid::new_v4(), name: "clip".into(), kind, asset: "a.mp4".into(), status, original_asset: None }
    }

    fn media_row(m: &MediaFile, kind: &str, end: EndAction, looped: bool) -> ItemRow {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Media { media_id: m.id, autoplay: true, end, looped }, background: None, notes: String::new() };
        ItemRow { id: item.id, kind: kind.into(), title: m.name.clone(), subtitle: String::new(), slides: vec![text_slide("clip", "")], item }
    }

    fn text_slide(label: &str, text: &str) -> Slide {
        Slide { label: label.into(), content: SlideContent::Text { text: text.into(), footer: None, reference: None, size: 0 }, notes: String::new() }
    }

    fn row(kind: &str, slides: usize) -> ItemRow {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "T".into(), body: String::new() }, background: None, notes: String::new() };
        ItemRow { id: item.id, kind: kind.into(), title: "T".into(), subtitle: String::new(), slides: (0..slides).map(|i| text_slide(&format!("S{i}"), "x")).collect(), item }
    }

    fn view() -> LiveView {
        LiveView {
            version: 1, committed: 1, pending: false, frame: Frame::default(), cursor: None, item: None, slide: None, next: None,
            media: None, audio: None, notice: None, theme: Theme::default(), logo: None,
        }
    }

    fn summary(title: &str) -> Option<LiveItemSummary> {
        Some(LiveItemSummary { item_id: Uuid::new_v4(), title: title.into(), kind: "song".into(), count: 3 })
    }

    fn text_layer() -> Option<SlideLayer> { Some(SlideLayer::Text { text: "Amazing grace".into(), footer: None, reference: None, scripture: false, size: 0, passage: vec![] }) }

    fn snap(configured: bool, connected: bool) -> OperatorSnapshot {
        let app = crate::app::test_app();
        let mut s = OperatorSnapshot::read(&app);
        s.stage_configured = configured;
        s.stage_connected = connected;
        s
    }

    #[test]
    fn subtitle_for_each_kind_and_media_state() {
        let conv = media_file(MediaKind::Video, MediaStatus::Converting { percent: 40 });
        let unconv = media_file(MediaKind::Video, MediaStatus::Unconverted { reason: "codec".into() });
        let failed = media_file(MediaKind::Video, MediaStatus::Failed { reason: "bad".into() });
        let ready = media_file(MediaKind::Video, MediaStatus::Ready);
        let pic = media_file(MediaKind::Image, MediaStatus::Ready);
        let audio = media_file(MediaKind::Audio, MediaStatus::Ready);
        let media = vec![conv.clone(), unconv.clone(), failed.clone(), ready.clone(), pic.clone(), audio.clone()];
        let sub = |r: ItemRow| subtitle(&r, &media, false);
        assert_eq!(sub(media_row(&conv, "video", EndAction::Stop, false)), "Video · converting 40%");
        assert_eq!(sub(media_row(&unconv, "video", EndAction::Stop, false)), "Video · may not play — needs converter");
        assert_eq!(sub(media_row(&failed, "video", EndAction::Stop, false)), "Video · conversion failed");
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
        let placeholder = |content| { let mut v = view(); v.next = Some(NextUp { title: "X".into(), slide: Slide { label: "L".into(), content, notes: String::new() } }); next_up(&v).1 };
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
            d.service_mut().unwrap().items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Hi".into() }, background: bg, notes: String::new() });
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
