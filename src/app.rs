//! The app's state and every operator action, independent of how the screens are drawn.
//! The HTTP layer (`server.rs`) and the native screens both drive it.
use crate::{bible, import, live::*, model::*, recovery::state::{self, StateWriter}, scripture::strings::{blocked, passages_removed}, slides::build_item, songselect, store::Store};
use crate::sha256::{Digest, Sha256, digest_hex, hex};
use crate::tr;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    io::{Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak, atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}},
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Why an action failed. `status` follows HTTP (400 bad input, 403 not allowed, 404 unknown action).
#[derive(Debug, Clone, PartialEq)]
pub struct ActionError { pub status: u16, pub message: String }

impl ActionError {
    pub fn bad(msg: impl Into<String>) -> Self { Self { status: 400, message: msg.into() } }
    pub fn forbidden(msg: impl Into<String>) -> Self { Self { status: 403, message: msg.into() } }
}

macro_rules! tryr { ($e:expr) => { $e.map_err(ActionError::bad)? }; }

// `do_action`'s actions, a file for each group (after `tryr!`, which they use).
mod do_songs;
mod do_service;
mod do_bible;
mod do_media;
mod do_settings;
mod do_screens;
mod do_backups;
mod do_phones;

/// Runs work that waits on the disk (a save that is not done until the file is there). Actions and phone requests
/// run on the async runtime's worker threads, where waiting holds up every other request on that thread: there
/// the runtime is told first, and moves them to another thread. Anywhere else the work is simply run.
pub(crate) fn on_disk<T>(work: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(rt) if rt.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => tokio::task::block_in_place(work),
        _ => work(),
    }
}

/// Something every screen may need to react to. Sent on `App::tx`.
#[derive(Clone, Debug)]
pub enum Update {
    Live(LiveUpdate),
    /// Only how far a video or audio has played changed (a player says so twice a second): the last live view
    /// with the new play status. The operator's transport row and the stage's time left follow it; the phones, the
    /// room and auto-follow have nothing to do.
    Playback(LiveUpdate),
    /// Library, service, settings or windows changed: re-read `state_json`.
    Data,
    Job { id: Uuid, percent: u8 },
    /// A paired phone was removed; its remote stream ends.
    Revoked { phone: Uuid },
    /// The timer readout changed (FEAT-2): the only tick update. `App::timers()` returns the same `Arc`.
    Timers(Arc<crate::timers::TimersView>),
    /// Lines drawn on the live slide changed (`live::drawing`). The newest drawing, shared.
    Drawing(Arc<crate::live::drawing::Drawing>),
}

/// One published live view, shared by every receiver of the update.
/// Native screens read the typed view (via `Deref`); web clients read `json()`, which is
/// serialized on first use and then reused, so N connected clients cost one serialization.
#[derive(Clone, Debug)]
pub struct LiveUpdate(Arc<LiveShared>);

#[derive(Debug)]
struct LiveShared { view: LiveView, json: OnceLock<String> }

impl LiveUpdate {
    pub fn new(view: LiveView) -> Self { Self(Arc::new(LiveShared { view, json: OnceLock::new() })) }
    pub fn view(&self) -> &LiveView { &self.0.view }
    /// The wire message: the view's fields plus `"type":"live"`.
    pub fn json(&self) -> &str {
        self.0.json.get_or_init(|| {
            let mut j = serde_json::to_value(&self.0.view).unwrap_or_default();
            if let Value::Object(m) = &mut j { m.insert("type".into(), "live".into()); }
            j.to_string()
        })
    }
}

impl std::ops::Deref for LiveUpdate {
    type Target = LiveView;
    fn deref(&self) -> &LiveView { &self.0.view }
}

/// What is live, as every screen draws it.
#[derive(Serialize, Clone, Debug)]
pub struct LiveView {
    pub version: u64,
    pub committed: u64,
    pub pending: bool,
    pub frame: Frame,
    pub cursor: Option<Cursor>,
    pub item: Option<LiveItemSummary>,
    pub slide: Option<Slide>,
    pub next: Option<NextUp>,
    pub media: Option<PlayStatus>,
    pub audio: Option<PlayStatus>,
    pub notice: Option<String>,
    /// The live song's sections, from the live snapshot (FEAT-4): the section bar and the phone chips.
    pub sections: Vec<crate::sections::OccurrenceView>,
    /// The live item differs from the library in a way the room would show (FEAT-4 banner).
    pub changed: Option<crate::sections::LiveChange>,
    /// The church default's style (stage, old pages, and slides taken live before themes).
    pub theme: Theme,
    pub logo: Option<AssetId>,
    /// The newest clear (`Live::clear_epoch`): an overlay turned on sends it back as `seen_clear`.
    #[serde(default)]
    pub clear_epoch: u64,
    /// The operator's message for the stage screen (spec §12.2). Never in the JSON: it goes to the stage and nowhere else.
    #[serde(skip)]
    pub stage_message: String,
    /// Lines drawn on the live slide from phones (`live::drawing`). Screens only.
    #[serde(skip)]
    pub drawing: Arc<crate::live::drawing::Drawing>,
    /// N29: a slide shown on the stage screen only (the room keeps what it shows). Never in the JSON.
    #[serde(skip)]
    pub stage_only: Option<StageOnly>,
    /// N9: Settings' look fade, in ms (the room fades this long when only the theme changes).
    #[serde(skip)]
    pub look_fade_ms: u32,
    /// 1.3: song auto-follow for the live song (none for anything else): the mode, the light, the line, Hold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow: Option<crate::follow::FollowState>,
}

/// N29: the slide on the stage screen only, and where it came from (the operator marks its thumbnail).
#[derive(Clone, Debug, PartialEq)]
pub struct StageOnly { pub item_id: Uuid, pub index: usize, pub slide: Slide }

#[derive(Serialize, Clone, Debug)]
pub struct LiveItemSummary {
    pub item_id: Uuid, pub title: String, pub kind: String, pub count: usize,
    /// The library song of a live song item (FEAT-4: Edit words, the editor's live banner).
    pub song_id: Option<Uuid>,
}

#[derive(Serialize, Clone, Debug)]
pub struct NextUp { pub title: String, pub slide: Slide }

/// A connected screen as the windowing layer reports it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Monitor {
    /// Stable id used as the room/stage binding.
    pub id: String,
    pub label: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
}

/// The windowing layer: opens and closes the operator, room and stage screens.
pub trait Screens: Send + Sync {
    /// `role` is "operator", or a window name from `ScreenSetup::keyed`: "room", "stage", or either plus ":" and a
    /// screen's id for the extra screens of a group.
    fn open(&self, role: &str, monitor: Option<&str>) -> Result<(), String>;
    /// A plain window of `size` (physical pixels), for a screen that shows in a window.
    fn open_window(&self, role: &str, size: (u32, u32)) -> Result<(), String> { let _ = size; self.open(role, None) }
    fn close(&self, role: &str);
    /// Roles with an open window.
    fn open_roles(&self) -> Vec<String>;
    fn monitors(&self) -> Vec<Monitor>;
    /// Shows `(monitor id, big number, what it is used for)` on each monitor for a few seconds.
    fn identify(&self, marks: Vec<(String, String, String)>) { let _ = marks; }
    /// The sound outputs found. Asks the system, so never call it on the UI thread. Empty when that cannot be told.
    fn audio_devices(&self) -> Vec<crate::video::ffi::AudioDevice> { vec![] }
}

/// A `data.json` save taken while `data` was locked, waiting to be written (`App::write_save`): the data as it
/// was then (a copy that shares the big lists, so taking it is quick) and its number. It is turned into the
/// file's bytes when it is written, after the lock is let go.
struct PendingSave { seq: u64, data: Data }

/// An open pairing window: the PIN to type, and the one-time secret the QR code carries (both die together).
pub struct Pairing { pub(crate) pin: String, pub(crate) secret: String, pub(crate) until: Instant, pub(crate) failures: u32 }

#[derive(Serialize, Deserialize, Clone)]
pub struct Phone {
    pub(crate) id: Uuid, pub(crate) name: String, pub(crate) hash: String, pub(crate) last_seen: u64,
    /// "May draw on slides" (on for new pairings, and for phones paired before drawing existed).
    #[serde(default = "crate::live::drawing::yes")]
    pub(crate) may_draw: bool,
}

use crate::import::strings::fallback_note;

/// `App::job_ends` keeps this many. Far more than can end between two operator reads.
const JOB_ENDS_KEPT: usize = 64;

/// A background job. The fields added for Plan 4 are left out of the JSON when unset, so the web operator's
/// `jobs` stay as they were for an ordinary job.
#[derive(Serialize, Clone, Default)]
pub struct Job {
    pub(crate) id: Uuid,
    pub(crate) label: String,
    pub(crate) percent: u8,
    pub(crate) error: Option<String>,
    /// Shown like `error`, but not red. A finished job with a note stays until it is dismissed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) cancellable: bool,
    /// The id of what the job made, e.g. the new deck.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<Uuid>,
    /// The job ended well and is only still listed for its note.
    #[serde(skip)]
    pub(crate) done: bool,
    /// `fallback_note()` was given; `note` starts with it.
    #[serde(skip)]
    pub(crate) fell_back: bool,
    /// The latest note other than `fallback_note()`.
    #[serde(skip)]
    pub(crate) other_note: Option<String>,
    /// What the job works on, so a list can show its progress in the right row: a Bible download's translation id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) subject: Option<String>,
}

/// Counts a native room or stage window as a connected screen while it is held (like a web page's socket).
pub struct ScreenGuard { app: Weak<App>, role: String }

impl Drop for ScreenGuard {
    fn drop(&mut self) {
        if let Some(app) = self.app.upgrade() && let Some(c) = app.conns(&self.role) { c.fetch_sub(1, Ordering::Relaxed); }
    }
}

pub struct App {
    pub store: Store,
    /// The library, the services and the settings. Its revision (`data_rev`) moves on with every change.
    pub data: crate::tracked::Tracked<Data>,
    pub live: Mutex<Live>,
    pub tx: broadcast::Sender<Update>,
    /// Held from reading a live view until it is sent, so views go out in the order they were read. Keeps the last
    /// one sent, which a playback report is made from (`broadcast_playback`).
    live_order: Mutex<Option<LiveUpdate>>,
    pub port: u16,
    pub lan_url: Option<String>,
    /// Set once the windowing layer is running; windows can only be opened after that.
    screens: OnceLock<Arc<dyn Screens>>,
    /// The output windows as last opened or left alone on purpose, by window name (`sync_screens`).
    applied: Mutex<HashMap<String, ScreenWindow>>,
    pub(crate) pairing: Mutex<Option<Pairing>>,
    pub(crate) phones: Mutex<Vec<Phone>>,
    pub(crate) attempts: Mutex<HashMap<IpAddr, Instant>>,
    pub(crate) jobs: Mutex<Vec<Job>>,
    /// How the last jobs ended: (job, what it made). A job that ended well is gone from `jobs` at once, so this
    /// is how the operator learns which deck to add.
    pub(crate) job_ends: Mutex<VecDeque<(Uuid, Option<Uuid>)>>,
    /// The cancel token of each running cancellable job; dropped when the job ends.
    cancels: Mutex<HashMap<Uuid, CancellationToken>>,
    pub room_conns: AtomicUsize,
    pub stage_conns: AtomicUsize,
    /// See `LiveView::stage_message`. Gone when the app closes.
    stage_message: Mutex<String>,
    /// See `LiveView::stage_only`. Gone when the app closes.
    stage_only: Mutex<Option<StageOnly>>,
    pub remote_conns: AtomicUsize,
    /// N31: stage view sign-ins and the TCP control.
    pub(crate) net: crate::network::NetState,
    state_writer: Mutex<Option<StateWriter>>,
    /// Numbers saved states, newest highest.
    state_seq: AtomicU64,
    /// Where restored players start, by playback instance.
    resume: Mutex<HashMap<u64, f64>>,
    /// The player that probes use; set once at start-up. Holds `None` when libmpv is missing.
    probe_mpv: OnceLock<Option<Arc<crate::video::ffi::Mpv>>>,
    /// The deck converters; set once at start-up by `import::init`.
    converters: OnceLock<Arc<import::deck::Converters>>,
    /// Held by the Bible download under way, so downloads run one at a time (Bible library spec §3.1).
    bible_downloads: tokio::sync::Mutex<()>,
    /// The data was saved by a newer RustyPresenter: live control works, nothing is saved (FEAT-1 spec §4.7).
    read_only: AtomicBool,
    /// YouTube downloads: their helper programs and lookups.
    pub youtube: crate::youtube::Hub,
    /// The song use log for CCLI reporting (`usage.json`).
    pub usage: crate::usage::Usage,
    /// Edit → Undo / Redo (`crate::undo`). Locked after `data`, never before.
    pub(crate) history: Mutex<crate::undo::History>,
    /// Numbers each `data.json` save, taken while `data` is locked, so saves are numbered in the order the changes
    /// were made.
    save_seq: AtomicU64,
    /// The newest save number on the disk. Held while writing, so an older save never lands after a newer one.
    saved_seq: Mutex<u64>,
    /// FEAT-2's memory-only state. Lock order: `data` → `live` → `automation`.
    pub automation: Mutex<crate::automation::runtime::AutomationRuntime>,
    /// Wakes the automation ticker (`automation::task`).
    pub(crate) ticker_wake: tokio::sync::Notify,
    /// The Automation dialog is open, so stopped countdowns to a time keep their readout fresh.
    pub(crate) automation_watching: AtomicBool,
    /// The livestream's scene the operator chose (a scene button, a slide action); Automatic by default.
    pub stream_choice: Mutex<crate::stream::scene::Choice>,
    /// The phones' camera tally lights (`stream::tally`), kept by the operator window while it streams.
    pub tally: Mutex<crate::stream::tally::Tally>,
    /// A slide action or macro asked to start (true) or end a clip of the recording; the operator takes it.
    pub clip_request: Mutex<Option<bool>>,
    /// A thread is taking faded ink away (`live::drawing::actions`).
    pub(crate) fade_ticking: AtomicBool,
    /// Bible keyword search indexes, made on the first search (FEAT-5).
    pub(crate) search: OnceLock<crate::scripture::search::SearchService>,
    /// Listen (N21): the sound goes to the operator's headphones only. Never saved, so every start has it off.
    pub(crate) listening: AtomicBool,
    /// The audience screen's size (width << 32 | height), so the phone draws on a picture of the same shape.
    room_canvas: AtomicU64,
    /// 1.2: decks that run in PowerPoint (`pplive`).
    pub(crate) pplive: crate::pplive::app::Hub,
    /// 1.3 song auto-follow, once started (`start_follower`).
    follower: OnceLock<crate::follow::Follower>,
    /// The sound input auto-follow listens to (`start_follow_if_wanted`).
    listen: Mutex<Option<crate::follow::Capture>>,
    /// The practice tape (`follow::tape`): off until Settings → Auto-follow → Practice starts one.
    pub(crate) tape: Arc<crate::follow::tape::Tape>,
}

pub struct DataPlan<'a>(pub &'a Data);

impl Plan for DataPlan<'_> {
    fn saved_overlay(&self, id: Uuid) -> Option<(u32, crate::theme::overlay::Overlay)> {
        self.0.overlays.iter().position(|o| o.id == id).map(|i| (i as u32, self.0.overlays[i].clone()))
    }
    fn logo(&self) -> Option<AssetId> { self.0.settings.logo.clone() }
    fn automation(&self) -> Option<&crate::automation::Automation> { Some(&self.0.automation) }
    fn theme_style(&self, id: Uuid) -> Option<crate::theme::Theme> { self.0.catalog().find(id).map(|t| t.style) }
    fn order(&self) -> Vec<Uuid> { self.0.service().map(|s| s.items.iter().map(|i| i.id).collect()).unwrap_or_default() }
    fn build(&self, id: Uuid) -> Option<LiveItem> {
        let service = self.0.service()?;
        let item = service.items.iter().find(|i| i.id == id)?;
        let (title, kind, slides) = build_item(self.0, item);
        let resolved = crate::theme::resolve(self.0, Some(service), item);
        Some(LiveItem { item_id: id, title, kind: kind.into(), slides, background: resolved.background, theme: resolved.style })
    }
}

/// The library song of service item `item_id`, if it is a song.
fn song_of(data: &Data, item_id: Uuid) -> Option<Uuid> {
    data.service()?.items.iter().find(|i| i.id == item_id).and_then(|i| match i.kind { ItemKind::Song { song_id, .. } => Some(song_id), _ => None })
}

pub(crate) const SESSION_DAYS: u64 = 30;
/// Phones paired at once; one more pushes out the one unseen longest. Room for remotes, phone cameras and
/// microphones together (it was 5: a church with two cameras, a microphone and two remotes lost one).
pub(crate) const MAX_PHONES: usize = 16;

pub(crate) fn now_secs() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }

impl App {
    /// The audience screen's size in pixels (1920×1080 until a audience screen opens).
    pub fn room_canvas(&self) -> (u32, u32) {
        let v = self.room_canvas.load(Ordering::Relaxed);
        ((v >> 32) as u32, v as u32)
    }

    /// The audience screen tells its size; an empty size is ignored.
    pub fn set_room_canvas(&self, (w, h): (u32, u32)) {
        if w > 0 && h > 0 { self.room_canvas.store((w as u64) << 32 | h as u64, Ordering::Relaxed) }
    }

    pub fn new(store: Store, data: Data, port: u16, lan_url: Option<String>) -> Arc<Self> {
        let (tx, _) = broadcast::channel(256);
        let phones: Vec<Phone> = std::fs::read(store.root.join("phones.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        // A download cut off by a crash or a forced quit leaves nothing behind.
        bible::catalog::install::clean_partials(&store.root);
        let youtube = crate::youtube::Hub::new(&store.root);
        let usage = crate::usage::Usage::open(&store.root);
        let app = Arc::new(Self {
            store, data: crate::tracked::Tracked::new(data), live: Mutex::new(Live::new()), tx, live_order: Mutex::new(None), port, lan_url,
            screens: OnceLock::new(), applied: Mutex::default(), pairing: Mutex::new(None), phones: Mutex::new(phones),
            attempts: Mutex::new(HashMap::new()), jobs: Mutex::new(vec![]), job_ends: Mutex::default(), cancels: Mutex::new(HashMap::new()),
            room_conns: AtomicUsize::new(0), stage_conns: AtomicUsize::new(0), stage_message: Mutex::default(), stage_only: Mutex::default(), remote_conns: AtomicUsize::new(0), net: Default::default(),
            state_writer: Mutex::new(None), state_seq: AtomicU64::new(0), resume: Mutex::new(HashMap::new()), probe_mpv: OnceLock::new(), converters: OnceLock::new(),
            bible_downloads: tokio::sync::Mutex::new(()), fade_ticking: AtomicBool::new(false), youtube, read_only: AtomicBool::new(false), usage, search: OnceLock::new(),
            history: Mutex::default(), save_seq: AtomicU64::new(0), saved_seq: Mutex::new(0),
            automation: Mutex::default(), ticker_wake: tokio::sync::Notify::new(), automation_watching: AtomicBool::new(false), stream_choice: Mutex::default(), tally: Mutex::default(), clip_request: Mutex::default(),
            listening: AtomicBool::new(false), room_canvas: AtomicU64::new(1920 << 32 | 1080), pplive: Default::default(), follower: OnceLock::new(),
            listen: Mutex::default(), tape: Default::default(),
        });
        crate::sections::look::set_look(&app.data().settings.sections);
        {
            let data = app.data();
            let mut live = app.live();
            live.keep_drawing = data.settings.keep_drawings;
            crate::live::drawing::actions::sync_fade(&mut live, &data.settings);
        }
        app.publish_timers(chrono::Utc::now(), true);
        app.refresh_bible_credits();
        app.online_text_from_cache();
        app
    }

    /// Puts back the online Bible text that saved data leaves out, from the capped cache alone, and pins the open
    /// service's verses there so they are evicted last (Nov 5 spec §5). Never uses the network.
    pub fn online_text_from_cache(&self) {
        use crate::scripture::online::online_only;
        let root = &self.store.root;
        let mut data = self.data();
        let before = data.clone();
        let open = data.current_service;
        let mut pins: BTreeMap<String, Vec<crate::scripture::VerseId>> = BTreeMap::new();
        for s in &mut data.services {
            for item in &mut s.items {
                let ItemKind::Scripture { reference, translation, verses, also, .. } = &mut item.kind else { continue };
                for (t, vs) in std::iter::once((&*translation, verses)).chain(also.iter_mut().map(|a| (&a.translation, &mut a.verses))) {
                    if !online_only(root, t) { continue }
                    if vs.is_empty() && let Some(found) = bible::sources::cached(root, t, reference) { *vs = found }
                    if Some(s.id) == open { pins.entry(t.clone()).or_default().extend(vs.iter().filter_map(|v| v.id)) }
                }
            }
        }
        for (t, ids) in pins { bible::sources::pin(&t, ids) }
        // Text the app filled in by itself: Undo keeps it.
        self.history.lock().unwrap().carry(&before, &data);
    }

    /// The open service's online passages that have no text yet, looked up now (cache, then the internet), so the
    /// service is ready offline afterwards (Nov 5 spec §5: opening a service fetches what's missing). Quiet on failure:
    /// the passage's note and preflight already say so.
    pub async fn fetch_online_text(self: &Arc<Self>) {
        use crate::scripture::online::online_only;
        let wanted: Vec<(String, String)> = {
            let data = self.data();
            data.service().map(|s| s.items.iter().flat_map(|i| match &i.kind {
                ItemKind::Scripture { reference, translation, verses, also, .. } => std::iter::once((translation, verses.is_empty())).chain(also.iter().map(|a| (&a.translation, a.verses.is_empty())))
                    .filter(|(t, empty)| *empty && online_only(&self.store.root, t)).map(|(t, _)| (t.clone(), reference.clone())).collect::<Vec<_>>(),
                _ => vec![],
            }).collect()).unwrap_or_default()
        };
        if wanted.is_empty() { return }
        let mut got = false;
        for (t, reference) in wanted { got |= self.lookup_passage(&t, &reference).await.is_ok() }
        if got {
            self.online_text_from_cache();
            self.broadcast_data();
            self.broadcast_live();
        }
    }

    pub fn live_view(&self) -> LiveView {
        let data = self.data();
        let live = self.live();
        let plan = DataPlan(&data);
        LiveView {
            version: live.version,
            committed: live.committed_version,
            pending: live.pending(),
            frame: live.frame.clone(),
            cursor: live.cursor.clone(),
            item: live.item.as_ref().map(|i| LiveItemSummary { item_id: i.item_id, title: i.title.clone(), kind: i.kind.clone(), count: i.slides.len(), song_id: song_of(&data, i.item_id) }),
            slide: live.cursor.as_ref().and_then(|c| live.item.as_ref().and_then(|i| i.slides.get(c.slide))).cloned(),
            next: live.next_preview(&plan).map(|(title, slide)| NextUp { title, slide }),
            media: live.media_status.clone(),
            audio: live.audio_status.clone(),
            notice: live.notice.clone(),
            sections: live.item.as_ref().map(|i| crate::sections::occurrence_views(&i.slides, live.cursor.as_ref().map(|c| c.slide))).unwrap_or_default(),
            changed: live.changed.clone(),
            theme: crate::theme::church_default(&data).style,
            logo: data.settings.logo.clone(),
            clear_epoch: live.clear_epoch,
            stage_message: self.stage_message.lock().unwrap().clone(),
            drawing: live.drawing.clone(),
            stage_only: self.stage_only.lock().unwrap().clone(),
            look_fade_ms: data.settings.look_fade_ms,
            follow: self.follow_view(&data, &live),
        }
    }

    /// 1.3: auto-follow as the live song shows it. The mode and Hold are read here (the worker may not have
    /// looked yet); the light, line and suggestion are the worker's.
    pub(crate) fn follow_view(&self, data: &Data, live: &crate::live::Live) -> Option<crate::follow::FollowState> {
        use crate::follow::{Light, Mode};
        let item = live.item.as_ref().filter(|i| i.kind == "song")?;
        let mode = data.automation.follow.get(&item.item_id).copied().unwrap_or(data.settings.follow.mode);
        let f = self.follower.get();
        let mut s = f.map(|f| f.state()).unwrap_or_default();
        s.holding = f.is_some_and(|f| f.holding());
        if mode != s.mode { s.light = Light::Off }
        if mode != Mode::Suggest { s.suggest = None }
        if mode == Mode::Manual { s.light = Light::Off } else if s.holding { s.light = Light::Holding }
        s.mode = mode;
        Some(s)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Update> { self.tx.subscribe() }

    /// Two threads can get here at once: the room's commit and an operator's Next. Sent out of order, the older view
    /// would replace the newer one in the room's `latest`, and the room would never see the slide it was asked for.
    pub fn broadcast_live(&self) {
        {
            let mut last = self.live_order.lock().unwrap();
            let update = LiveUpdate::new(self.live_view());
            *last = Some(update.clone());
            let _ = self.tx.send(Update::Live(update));
        }
        // Whatever moved the live slide (a click, a phone, a macro, an edit), auto-follow looks again.
        if let Some(f) = self.follower.get() { f.on_live_change() }
        self.tape_screen();
    }

    /// A player said how far it has played and nothing else changed. A whole new live view (the frame, the slides,
    /// the next item, the sections) twice a second would make every screen and phone do its work again for
    /// nothing: this sends the last view with the new play status, as `Update::Playback`.
    fn broadcast_playback(&self) {
        let mut last = self.live_order.lock().unwrap();
        let (version, media, audio) = { let live = self.live(); (live.version, live.media_status.clone(), live.audio_status.clone()) };
        match last.as_ref().filter(|sent| sent.view().version == version) {
            Some(sent) => {
                let update = LiveUpdate::new(LiveView { media, audio, ..sent.view().clone() });
                *last = Some(update.clone());
                let _ = self.tx.send(Update::Playback(update));
            }
            // Nothing sent yet, or the screen moved on since: a whole view.
            None => { drop(last); self.broadcast_live() }
        }
    }

    /// The screen as a practice tape keeps it, while one runs.
    fn tape_screen(&self) {
        if !self.tape.on() { return }
        let screen = {
            let data = self.data();
            let live = self.live();
            let (Some(item), Some(cursor)) = (live.item.as_ref(), live.cursor.as_ref()) else { return };
            let song = item.kind == "song";
            let words = |s: &crate::model::Slide| match &s.content { crate::model::SlideContent::Text { text, .. } => text.clone(), _ => String::new() };
            crate::follow::tape::Screen {
                item: item.item_id, title: item.title.clone(), song,
                slides: if song { item.slides.iter().map(words).collect() } else { vec![] },
                slide: cursor.slide, words_up: live.frame.slide.is_some(),
                mode: data.automation.follow.get(&item.item_id).copied().unwrap_or(data.settings.follow.mode),
                hand: live.manual_version,
                video: live.frame.media.as_ref().is_some_and(|m| matches!(m.v, crate::live::MediaLayer::Video { .. })),
                hold: self.follower.get().is_some_and(|f| f.holding()),
            }
        };
        self.tape.screen(&screen);
    }

    /// Starts a practice tape (`follow::tape`) and the listening it needs, whatever mode the songs are in. The
    /// tape's folder. Runs FFmpeg, so not on the UI thread.
    pub fn start_tape(self: &Arc<Self>) -> Result<std::path::PathBuf, String> {
        let about = {
            let d = self.data();
            serde_json::json!({ "app": env!("CARGO_PKG_VERSION"), "follow": d.settings.follow, "lines_per_slide": d.settings.lines_per_slide })
        };
        let dir = self.tape.start(&self.store.root.join(crate::follow::tape::FOLDER), chrono::Local::now().naive_local(), about)?;
        self.start_follow_if_wanted();
        if !self.listen.lock().unwrap().as_ref().is_some_and(|c| c.running()) {
            self.tape.stop();
            let _ = std::fs::remove_dir_all(&dir);
            return Err(crate::tr!("No sound input to record. Choose one in Settings → Auto-follow."));
        }
        self.tape_screen();
        Ok(dir)
    }

    /// Ends the practice tape. Its folder, when one was running. Listening stops with it when no song follows.
    pub fn stop_tape(&self) -> Option<std::path::PathBuf> {
        let dir = self.tape.stop();
        if !self.follow_wanted() { self.listen.lock().unwrap().take(); }
        dir
    }

    /// Learns the order each song was sung in from the practice tape in `dir`, and how long each slide was up
    /// (`follow::order`), for the songs that are still in a service: each one's title and order. A song the tape
    /// has for a minute or more with nothing to learn from (one slide) loses what it had. Orders of items that
    /// are gone are dropped.
    pub fn learn_orders(&self, dir: &std::path::Path) -> Result<Vec<(String, Vec<usize>)>, String> {
        use crate::follow::order;
        let takes = crate::follow::review::read(dir)?;
        let lessons = order::lessons(&takes);
        self.edit_untracked(|d| {
            let items: std::collections::BTreeSet<Uuid> = d.services.iter().flat_map(|s| s.items.iter().map(|i| i.id)).collect();
            d.automation.follow_order.retain(|id, _| items.contains(id));
            for take in takes.iter().filter(|t| t.until - t.from >= 60.0) {
                if let Some(item) = take.item.filter(|i| !lessons.iter().any(|l| l.0 == *i)) { d.automation.follow_order.remove(&item); }
            }
            let mut out = vec![];
            for (item, title, lesson) in lessons {
                if !items.contains(&item) { continue }
                out.push((title, lesson.slides.clone()));
                d.automation.follow_order.insert(item, lesson);
            }
            Ok(out)
        })
    }

    /// Forgets every learned song order. How many there were.
    pub fn forget_orders(&self) -> Result<usize, String> {
        self.edit_untracked(|d| Ok(std::mem::take(&mut d.automation.follow_order).len()))
    }

    /// Starts song auto-follow's worker (once).
    pub fn start_follower(self: &Arc<Self>) -> &crate::follow::Follower {
        self.follower.get_or_init(|| crate::follow::Follower::start(Arc::downgrade(self)))
    }

    /// Song auto-follow, once started.
    pub fn follower(&self) -> Option<&crate::follow::Follower> { self.follower.get() }

    /// Auto-follow is wanted: Settings → Auto-follow or a song item is set to Suggest or Automatic.
    pub fn follow_wanted(&self) -> bool {
        // A practice tape records the room whatever mode the songs are in.
        if self.tape.on() { return true }
        let d = self.data();
        d.settings.follow.mode != crate::follow::Mode::Manual || d.automation.follow.values().any(|m| *m != crate::follow::Mode::Manual)
    }

    /// Starts auto-follow and its sound input when wanted and not running: at start, and when a song is first set
    /// to Suggest or Automatic. Runs FFmpeg, so not on the UI thread. Without an input the follower stays in Manual
    /// and tells the operator why.
    pub fn start_follow_if_wanted(self: &Arc<Self>) {
        if !self.follow_wanted() { return }
        let f = self.start_follower();
        let mut listen = self.listen.lock().unwrap();
        if listen.as_ref().is_some_and(|c| c.running()) { return }
        let chosen = self.data().settings.follow.input.clone();
        let device = chosen.filter(|d| !d.is_empty()).or_else(|| crate::follow::input_devices().into_iter().next());
        let started = match device {
            Some(d) => crate::follow::Capture::start_tapped(&d, crate::follow::room_ear(f.lexicon()), f.sender(), Some(self.tape.clone())),
            None => Err(crate::tr!("no microphone or line-in found")),
        };
        match started {
            Ok(c) => *listen = Some(c),
            Err(e) => { *listen = None; f.feed(crate::follow::FollowEvent::InputLost(e)) }
        }
    }

    /// Settings → Auto-follow's sound input changed: listen to the new one. Runs FFmpeg, so not on the UI thread.
    pub fn listen_again(self: &Arc<Self>) {
        self.listen.lock().unwrap().take();
        self.start_follow_if_wanted();
    }
    pub fn broadcast_data(&self) { let _ = self.tx.send(Update::Data); }

    /// The data, locked. A thread that panicked while it held this lock leaves it "poisoned"; taking it with
    /// `unwrap` would then panic on every screen, phone request and the follower, and one small bug in a rarely
    /// used action would take the whole app down in the middle of a service. The data is taken as it is instead:
    /// an edit that panicked was put back as it was (`edit_kept`), and everything else only reads or writes whole
    /// values. Lock order: `data` → `live` → `automation`.
    pub fn data(&self) -> crate::tracked::TrackedGuard<'_, Data> { self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) }

    /// The live engine, locked; like `data`, never refused because another thread panicked while holding it.
    pub fn live(&self) -> std::sync::MutexGuard<'_, Live> { self.live.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) }

    /// A number that is different after every change to the data (`Tracked::rev`). Whoever draws from the data
    /// keeps the number it last read: the same number means there is nothing to read again.
    pub fn data_rev(&self) -> u64 { self.data.rev() }

    pub(crate) fn save(&self, data: &Data) -> Result<(), String> { self.write_save(self.take_save(data)?) }

    /// The first half of a save: the data as it is now and its save number. Call it with `data` locked. Nothing is
    /// serialised here: a slide change waiting for the lock never waits for the whole library to be written out
    /// as JSON.
    fn take_save(&self, data: &Data) -> Result<PendingSave, String> {
        if self.read_only() { return Err(crate::theme::strings::newer_data().into()) }
        Ok(PendingSave { seq: self.save_seq.fetch_add(1, Ordering::Relaxed) + 1, data: data.clone() })
    }

    /// The second half: the disk write, best after `data` is unlocked so live control never waits on the disk.
    /// A save older than the one already written is dropped; the newer one holds its changes too. The write waits
    /// until the file is on the disk; on one of the async workers the runtime is told first (`on_disk`).
    fn write_save(&self, save: PendingSave) -> Result<(), String> {
        on_disk(|| {
            let mut saved = self.saved_seq.lock().unwrap_or_else(|e| e.into_inner());
            if save.seq <= *saved { return Ok(()) }
            let bytes = self.store.encode(&save.data).map_err(|e| tr!("Could not save: {error}", error = e))?;
            self.store.write_data(&bytes).map_err(|e| tr!("Could not save: {error}", error = e))?;
            *saved = save.seq;
            Ok(())
        })
    }

    /// The data was saved by a newer RustyPresenter (`Store::load_full`): every edit is refused with
    /// `strings::themes::newer_data()`, live control keeps working. The operator shows it as a banner.
    pub fn read_only(&self) -> bool { self.read_only.load(Ordering::Relaxed) }
    pub fn set_read_only(&self, on: bool) { self.read_only.store(on, Ordering::Relaxed) }

    /// Mutate data, save, and tell every screen. The live item is checked against the new data, and every screen is
    /// told, even when the save failed: memory has changed (FEAT-4 spec §5.4), and the operator window must not go
    /// on showing the old data while the room and the phones use the new.
    /// Edit → Undo can take it back (`crate::undo`).
    pub(crate) fn edit<T>(&self, f: impl FnOnce(&mut Data) -> Result<T, String>) -> Result<T, String> { self.edit_kept(true, f) }

    /// `edit` for a change the app makes by itself (what an import found about a file): not a step of its own for
    /// Undo, and carried into the steps kept.
    pub(crate) fn edit_untracked<T>(&self, f: impl FnOnce(&mut Data) -> Result<T, String>) -> Result<T, String> { self.edit_kept(false, f) }

    fn edit_kept<T>(&self, step: bool, f: impl FnOnce(&mut Data) -> Result<T, String>) -> Result<T, String> {
        if self.read_only() { return Err(crate::theme::strings::newer_data().into()) }
        let r = {
            let mut data = self.data();
            let before = data.clone();
            // A change that panics half way is put back like one that fails, and the lock is let go clean before
            // the panic carries on to be reported (`recovery::panic`): the data is never left half changed.
            let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut data)));
            let made = match done {
                Ok(made) => made,
                Err(panic) => {
                    *data = before;
                    drop(data);
                    std::panic::resume_unwind(panic);
                }
            };
            let r = match made.and_then(|r| self.take_save(&data).map(|s| (r, s))) {
                Ok(r) => r,
                Err(e) => {
                    *data = before;
                    return Err(e);
                }
            };
            let mut history = self.history.lock().unwrap();
            if step { history.record(&before, &data) } else { history.carry(&before, &data) }
            r
        };
        let r = self.write_save(r.1).map(|()| r.0);
        self.after_edit();
        r
    }

    /// Edit → Undo (`redo` false) or Redo: the toast's text, or why nothing changed. Saved and checked against the
    /// live item like an edit, so the screen never changes by itself.
    pub fn undo(&self, redo: bool) -> Result<String, String> {
        if self.read_only() { return Err(crate::theme::strings::newer_data().into()) }
        let r = {
            let mut data = self.data();
            let mut history = self.history.lock().unwrap();
            let r = if redo { history.redo(&mut data) } else { history.undo(&mut data) };
            r.and_then(|t| self.take_save(&data).map(|s| (t, s)))
        };
        let r = r.and_then(|(t, s)| self.write_save(s).map(|()| t));
        // A step that could not be taken is gone too, so the menu is told either way.
        self.after_edit();
        r
    }

    /// After memory changed (saved or not): every screen reads the data again.
    fn after_edit(&self) {
        // Settings → Sections: names, colours, keys and heading words follow at once.
        let look = self.data().settings.sections.clone();
        crate::sections::look::set_look(&look);
        let live_changed = self.reconcile_live();
        self.broadcast_data();
        self.broadcast_live();
        if live_changed { self.automation_after(None) }
    }

    /// Compares the live item with the library (data, then live: the one lock order). True when live changed.
    pub(crate) fn reconcile_live(&self) -> bool {
        let data = self.data();
        let mut live = self.live();
        live.keep_drawing = data.settings.keep_drawings;
        crate::live::drawing::actions::sync_fade(&mut live, &data.settings);
        live.reconcile(&DataPlan(&data))
    }

    pub fn command(&self, env: Envelope) -> Outcome {
        // Macros need timers and data as well as the engine (FEAT-2).
        if matches!(env.cmd, Command::RunMacro { .. }) { return self.run_macro(&env).0 }
        // N29: the stage-only slide is the App's, like the stage message; the room never sees it.
        if let Command::StageOnly { item_id, slide } = env.cmd {
            let version = self.live().version;
            return match self.set_stage_only(item_id, slide) {
                Ok(()) => Outcome { ok: true, version, error: None, duplicate: false },
                Err(e) => Outcome { ok: false, version, error: Some(e), duplicate: false },
            };
        }
        // 1.3: auto-follow's mode and Hold are the App's; the room never changes.
        if matches!(env.cmd, Command::FollowMode { .. } | Command::FollowHold { .. }) { return self.follow_command(&env.cmd) }
        // 1.2: Next/Back on a deck running in PowerPoint click there first.
        if let Some(out) = self.show_command(&env) { return out }
        let cmd = env.cmd.clone();
        let out = {
            let data = self.data();
            let mut live = self.live();
            live.apply(env, &DataPlan(&data))
        };
        if out.ok && !out.duplicate {
            self.show_after_command(&cmd);
            self.broadcast_live();
            if self.room_conns.load(Ordering::Relaxed) == 0 { self.note_song_use(false); }
            self.automation_after(None);
        }
        out
    }

    /// 1.3: a song item's auto-follow mode (kept with the service's automation), or Hold on or off.
    fn follow_command(&self, cmd: &Command) -> Outcome {
        let r = match *cmd {
            Command::FollowMode { item_id, mode } => self.edit_untracked(|d| {
                if !d.services.iter().any(|s| s.items.iter().any(|i| i.id == item_id)) { return Err(tr!("That item isn't in the service any more.")) }
                match mode {
                    Some(m) => { d.automation.follow.insert(item_id, m); }
                    None => { d.automation.follow.remove(&item_id); }
                }
                Ok(())
            }),
            Command::FollowHold { on } => { if let Some(f) = self.follower.get() { f.set_hold(on) } Ok(()) }
            _ => Ok(()),
        };
        let version = self.live().version;
        // The Live column, stage and phones show the new mode or Hold at once; the follower looks again.
        if r.is_ok() { self.broadcast_live() }
        Outcome { ok: r.is_ok(), version, error: r.err(), duplicate: false }
    }

    pub fn set_state_writer(&self, w: StateWriter) { *self.state_writer.lock().unwrap() = Some(w); }

    /// Writes what is waiting and ends the writer. For a normal quit.
    pub fn stop_state_writer(&self) {
        if let Some(w) = self.state_writer.lock().unwrap().take() { w.flush_and_stop(); }
    }

    /// Hands the frame on air to the state writer, with where its players are now.
    pub fn save_live_state(&self) {
        let windows = self.screens.get().map(|s| s.open_roles()).unwrap_or_default();
        let writer = self.state_writer.lock().unwrap();
        let Some(writer) = writer.as_ref() else { return };
        let seq = self.state_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let snapshot = state::snapshot(&self.live(), windows, seq);
        if let Some(s) = snapshot { writer.submit(s); }
    }

    /// Is video or audio on air?
    pub fn playing(&self) -> bool { self.live().committed().is_some_and(|(frame, ..)| frame.plays()) }

    /// Where the restored players should start, in seconds.
    pub fn set_resume(&self, positions: &[(u64, f64)]) { self.resume.lock().unwrap().extend(positions.iter().copied()); }

    /// Kept until the instance goes on air, as a newer plan may prepare it again first; after that a retrigger
    /// starts from the beginning.
    pub fn resume_position(&self, inst: u64) -> Option<f64> { self.resume.lock().unwrap().get(&inst).copied() }

    /// Forgets the resume positions of the instances the room just committed.
    fn resume_on_air(&self) {
        if self.resume.lock().unwrap().is_empty() { return }
        let Some((frame, ..)) = self.live().committed() else { return };
        let on_air: Vec<u64> = frame.instances().collect();
        self.resume.lock().unwrap().retain(|inst, _| !on_air.contains(inst));
    }

    /// The stage screen's message (empty hides it).
    /// The message on the stage screen now (`LiveView::stage_message`), without building a live view for it.
    pub fn stage_message(&self) -> String { self.stage_message.lock().unwrap().clone() }

    /// Something is on the screens now: a slide, a video or picture, or audio.
    pub fn on_air(&self) -> bool {
        let live = self.live();
        live.frame.slide.is_some() || live.frame.media.is_some() || live.frame.audio.is_some()
    }

    pub(crate) fn set_stage_message(&self, text: &str) {
        *self.stage_message.lock().unwrap() = text.to_string();
        self.broadcast_live();
    }

    /// N29: the stage-only slide, without building a whole live view.
    pub fn live_view_stage_only(&self) -> Option<StageOnly> { self.stage_only.lock().unwrap().clone() }

    /// N29: shows slide `index` of service item `item_id` on the stage screen only; `None` takes it down.
    pub(crate) fn set_stage_only(&self, item_id: Option<Uuid>, index: usize) -> Result<(), String> {
        let shown = match item_id {
            None => None,
            Some(id) => {
                let data = self.data();
                let item = DataPlan(&data).build(id).ok_or_else(|| tr!("That item isn't in the service any more."))?;
                let slide = item.slides.get(index).cloned().ok_or_else(|| tr!("That slide isn't there any more."))?;
                Some(StageOnly { item_id: id, index, slide })
            }
        };
        *self.stage_only.lock().unwrap() = shown;
        self.broadcast_live();
        Ok(())
    }

    pub fn set_notice(&self, text: &str) {
        self.live().notice = Some(text.into());
        self.broadcast_live();
    }

    pub fn room_event(&self, ev: RoomEvent) {
        let (changed, committed, only_position) = {
            let data = self.data();
            let mut live = self.live();
            let before = live.committed_version;
            let only_position = live.only_position(&ev);
            let failed = matches!(ev, RoomEvent::Failed { .. });
            let ended = match &ev { RoomEvent::Media { inst, state: PlayState::Ended, .. } => Some(*inst), _ => None };
            let mut changed = live.report(ev);
            // A revert can go back to a snapshot older than a quiet adoption: check it against the library again.
            if changed && failed { live.reconcile(&DataPlan(&data)); }
            // N24 "Go to the next slide": the last frame stays up until the next slide is shown, or for good at the
            // end of the service, where the video is taken down as "Clear the screen" would.
            if let Some(inst) = ended.filter(|i| live.ends_with_next(*i)) {
                let base = Some(live.version);
                let next = live.apply(Envelope { id: Uuid::new_v4().to_string(), base, cmd: Command::Next }, &DataPlan(&data));
                if !next.ok { live.retire_ended(inst); }
                changed = true;
            }
            (changed, live.committed_version != before, only_position)
        };
        if changed { if only_position && !committed { self.broadcast_playback() } else { self.broadcast_live() } }
        if committed { self.resume_on_air(); self.save_live_state(); self.note_song_use(true); }
        if changed || committed {
            let c = committed.then(|| self.live().committed_version);
            self.automation_after(c);
        }
    }

    /// Room window restarted: static layers return, audible media does not (spec §4.2).
    pub fn room_recovered(&self) {
        {
            let data = self.data();
            let mut live = self.live();
            let mut stopped = false;
            if live.frame.media.as_ref().is_some_and(|m| matches!(m.v, MediaLayer::Video { .. })) {
                live.apply(Envelope::new(Command::ClearMedia), &DataPlan(&data));
                stopped = true;
            }
            if live.frame.audio.is_some() {
                live.apply(Envelope::new(Command::ClearAudio), &DataPlan(&data));
                stopped = true;
            }
            live.notice = Some(if stopped { tr!("The audience screen restarted. Video/audio stopped — click it again to replay.") } else { tr!("The audience screen restarted.") });
        }
        self.broadcast_live();
    }

    pub fn set_probe_mpv(&self, mpv: Option<Arc<crate::video::ffi::Mpv>>) {
        let _ = self.probe_mpv.set(mpv);
    }
    pub(crate) fn probe_mpv(&self) -> Option<Arc<crate::video::ffi::Mpv>> {
        self.probe_mpv.get().cloned().flatten()
    }

    pub fn set_converters(&self, converters: Arc<import::deck::Converters>) {
        let _ = self.converters.set(converters);
    }
    pub fn converters(&self) -> Option<Arc<import::deck::Converters>> {
        self.converters.get().cloned()
    }

    /// A job with no Cancel button. Every real import has one, so only tests start these.
    #[cfg(test)]
    pub(crate) fn job_start(&self, label: &str) -> Uuid {
        let id = Uuid::new_v4();
        self.jobs.lock().unwrap().push(Job { id, label: label.into(), ..Job::default() });
        self.broadcast_data();
        id
    }
    /// `job_start` for a job with a Cancel button. The work watches the token and, once it has stopped, calls
    /// `job_end_cancelled`.
    pub(crate) fn job_start_cancellable(&self, label: &str) -> (Uuid, CancellationToken) {
        let (id, token) = (Uuid::new_v4(), CancellationToken::new());
        self.cancels.lock().unwrap().insert(id, token.clone());
        self.jobs.lock().unwrap().push(Job { id, label: label.into(), cancellable: true, ..Job::default() });
        self.broadcast_data();
        (id, token)
    }
    /// Asks a job to stop. Unknown or finished ids are ignored.
    pub fn job_cancel(&self, id: Uuid) {
        if let Some(token) = self.cancels.lock().unwrap().get(&id) { token.cancel(); }
    }
    /// Says what a job works on (`Job::subject`).
    pub(crate) fn job_subject(&self, id: Uuid, subject: &str) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) { j.subject = Some(subject.into()); }
        self.broadcast_data();
    }
    /// Changes a running job's label, e.g. to "Waiting for LibreOffice…" and back.
    pub(crate) fn job_label(&self, id: Uuid, label: &str) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) { j.label = label.into(); }
        self.broadcast_data();
    }
    /// Sets the job's note. The last one wins, except that `fallback_note()` and any other note are both kept, the
    /// fallback first. So the deck pipeline sends "Imported with LibreOffice" only when it did not fall back.
    pub(crate) fn job_note(&self, id: Uuid, note: String) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
            if note == fallback_note() { j.fell_back = true } else { j.other_note = Some(note) }
            j.note = match (j.fell_back, &j.other_note) {
                (true, Some(other)) => Some(format!("{} {other}", fallback_note())),
                (true, None) => Some(fallback_note().into()),
                (false, other) => other.clone(),
            };
        }
        self.broadcast_data();
    }
    pub(crate) fn job_result(&self, id: Uuid, made: Uuid) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) { j.result = Some(made); }
    }
    pub(crate) fn job_progress(&self, id: Uuid, p: u8) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) { if j.percent == p { return; } j.percent = p; }
        let _ = self.tx.send(Update::Job { id, percent: p });
    }
    /// Ends a job. One that ended well goes away, unless it has a note: then it stays, at 100%, until `job/dismiss`.
    /// A failed one stays with its error.
    pub(crate) fn job_end(&self, id: Uuid, error: Option<String>) {
        self.cancels.lock().unwrap().remove(&id);
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(j) = jobs.iter_mut().find(|j| j.id == id) {
            self.job_ended(id, if error.is_none() { j.result } else { None });
            if error.is_none() && j.note.is_none() {
                jobs.retain(|j| j.id != id);
            } else {
                j.done = error.is_none();
                j.error = error;
                j.percent = 100;
                j.cancellable = false;
            }
        }
        drop(jobs);
        self.broadcast_data();
    }
    /// Ends a job that stopped because it was cancelled: it leaves no error and no note.
    pub(crate) fn job_end_cancelled(&self, id: Uuid) {
        self.cancels.lock().unwrap().remove(&id);
        self.job_ended(id, None);
        self.jobs.lock().unwrap().retain(|j| j.id != id);
        self.broadcast_data();
    }
    fn job_ended(&self, id: Uuid, made: Option<Uuid>) {
        let mut ends = self.job_ends.lock().unwrap();
        if ends.len() == JOB_ENDS_KEPT { ends.pop_front(); }
        ends.push_back((id, made));
    }

    /// Writes the list of paired phones. A phone's own request can end up here (its sign-in was just used again),
    /// so the wait for the disk is taken off the async workers (`on_disk`).
    pub(crate) fn save_phones(&self, phones: &[Phone]) {
        let saved = serde_json::to_vec(phones).map_err(std::io::Error::other)
            .and_then(|b| on_disk(|| crate::store::write_atomic(&self.store.root.join("phones.json"), &b)));
        // Without the file every paired phone is forgotten at the next start: said in the operator window (with
        // the next look at the screen; callers hold the phone list, so nothing is sent from here).
        if let Err(e) = saved { self.live().notice = Some(tr!("The list of paired phones could not be saved: {reason}", reason = e)); }
    }
    pub fn set_screens(&self, s: Arc<dyn Screens>) { let _ = self.screens.set(s); }

    pub fn open_window(&self, role: &str, monitor: Option<&str>) -> Result<(), String> {
        self.screens.get().ok_or_else(|| tr!("RustyPresenter is still starting. Try again in a moment."))?.open(role, monitor)
    }

    pub fn close_window(&self, role: &str) { if let Some(s) = self.screens.get() { s.close(role); } }

    /// Remembers that these windows were dealt with at start, so `sync_screens` leaves them alone.
    pub fn screens_applied(&self, windows: Vec<ScreenWindow>) {
        *self.applied.lock().unwrap() = windows.into_iter().map(|w| (w.key.clone(), w)).collect();
    }

    /// Makes the open windows match the setup: closes what it no longer asks for, and opens what is new or changed.
    /// A window that is already right is left alone, so nothing on air blinks. Says which screens could not open.
    pub fn sync_screens(&self) -> Result<(), String> {
        let want = self.data().settings.screens.windows();
        let mut applied = std::mem::take(&mut *self.applied.lock().unwrap());
        let wanted = |key: &str| want.iter().any(|w| w.key == key);
        for key in self.open_roles().into_iter().filter(|r| r != "operator" && !wanted(r)) { self.close_window(&key) }
        applied.retain(|key, _| wanted(key));
        let mut failed = vec![];
        for w in &want {
            if applied.get(&w.key) == Some(w) { continue }
            let opened = match &w.target {
                ScreenTarget::Monitor(m) => self.open_window(&w.key, Some(m)),
                ScreenTarget::Window => self.screens.get().ok_or_else(|| tr!("RustyPresenter is still starting. Try again in a moment.")).and_then(|s| s.open_window(&w.key, w.size)),
                ScreenTarget::None | ScreenTarget::Placeholder => continue,
            };
            match opened { Ok(()) => { applied.insert(w.key.clone(), w.clone()); } Err(e) => failed.push(e) }
        }
        *self.applied.lock().unwrap() = applied;
        failed.dedup();
        if failed.is_empty() { Ok(()) } else { Err(failed.join(" ")) }
    }

    /// The first monitor that is free for a group's screen: not the operator's, and not used by another screen.
    fn free_monitor(&self, screens: &ScreenSetup) -> Option<Monitor> {
        let used: Vec<&str> = ScreenSetup::GROUPS.iter().flat_map(|g| screens.monitors(g)).collect();
        self.monitors().into_iter().find(|m| !m.primary && !used.contains(&m.id.as_str()))
    }

    /// Big number for each monitor, and the screens that show on it.
    fn identify_screens(&self) {
        let Some(screens) = self.screens.get() else { return };
        let setup = self.data().settings.screens.clone();
        let marks = screens.monitors().into_iter().enumerate().map(|(i, m)| {
            let names: Vec<&str> = ScreenSetup::GROUPS.iter().filter_map(|g| setup.group(g)).flat_map(|g| &g.screens)
                .filter(|s| s.output().target == ScreenTarget::Monitor(m.id.clone())).map(|s| s.name.as_str()).collect();
            let what = if names.is_empty() { if m.primary { tr!("This computer") } else { tr!("Not used") } } else { names.join(", ") };
            (m.id, (i + 1).to_string(), what)
        }).collect();
        screens.identify(marks);
    }

    /// Spec §13, for the current service. Looks at every file it needs, so call it off the UI thread.
    pub fn preflight(&self) -> Vec<String> {
        // A copy for the check, so the lock is not held while every file is looked at. The copy shares the songs,
        // media, decks and services (`Shared`): the status pill's look every 30 seconds copies no library.
        let data = self.data().clone();
        let has_asset = |id: &str| std::fs::metadata(self.store.asset_path(id)).is_ok_and(|m| m.is_file() && m.len() > 0);
        let audio: Vec<String> = self.audio_devices().into_iter().map(|d| d.name).collect();
        let root = &self.store.root;
        let cached = |t: &str, reference: &str| crate::scripture::online::stored(root, t) || bible::sources::cached(root, t, reference).is_some();
        crate::preflight::check(&data, &self.monitors(), &audio, &has_asset, &cached)
    }

    pub fn audio_devices(&self) -> Vec<crate::video::ffi::AudioDevice> { self.screens.get().map(|s| s.audio_devices()).unwrap_or_default() }

    /// Where the room's sound goes now: the main output, or the headphones while Listen is on (N21).
    pub fn sound_out(&self) -> crate::media_prefs::SoundOut {
        let s = &self.data().settings;
        crate::media_prefs::sound_out(&s.audio_device, &s.media, self.listening.load(Ordering::Relaxed))
    }

    /// One look at the chosen sound output. Asks the system, so never call it on the UI thread.
    pub fn watch_sound(&self) {
        use crate::{preflight::strings::sound_gone, sound};
        let chosen = self.sound_out().device;
        let told = || self.live().notice.as_deref() == Some(&*sound_gone());
        let heard = || sound::pauses(&self.live().frame);
        if !told() && (chosen.is_empty() || heard().is_empty()) { return }
        let found = if chosen.is_empty() { vec![] } else { self.audio_devices() };
        if !chosen.is_empty() && found.is_empty() { return } // The list could not be read: nothing has changed.
        if !sound::missing(&chosen, &found) {
            let cleared = self.live().notice.take_if(|n| *n == sound_gone()).is_some();
            if cleared { self.broadcast_live(); }
            return;
        }
        // Read again: the list takes a moment, and what plays may have changed. A stale pause is refused anyway.
        let pauses = heard();
        if pauses.is_empty() { return }
        for cmd in pauses { self.command(Envelope::new(cmd)); }
        self.set_notice(&sound_gone());
    }

    pub(crate) fn monitors(&self) -> Vec<Monitor> { self.screens.get().map(|s| s.monitors()).unwrap_or_default() }

    /// Roles with an open window (`state_json`'s `windows`).
    pub(crate) fn open_roles(&self) -> Vec<String> { self.screens.get().map(|s| s.open_roles()).unwrap_or_default() }

    /// The standard list plus any other Bible the church imported.
    /// A passage in one Bible: built in, then a file the church imported or downloaded (offline, and it wins over
    /// any online source), then online.
    pub async fn lookup_passage(&self, translation: &str, reference: &str) -> Result<(String, Vec<VerseText>), String> {
        if let Some(b) = bible::built_in(translation) { return b.lookup(reference) }
        if let Some(b) = bible::import::get(&self.store.root, translation) {
            // A Bible numbered another way takes its own numbers first. A reference it does not have, typed the
            // English way ("Malachi 4:1" in the Hebrew Bible), is found under its own number instead.
            let own = b.lookup(reference);
            let its = bible::import::meta(&self.store.root, translation).map_or(bible::library::Vsys::English, |m| m.versification);
            if own.is_ok() || its == bible::library::Vsys::English { return own }
            let there = bible::kjv().lookup(reference).ok().and_then(|(_, verses)| {
                let ids: Vec<_> = verses.iter().filter_map(|v| v.id)
                    .filter_map(|id| bible::versify::convert(id, bible::library::Vsys::English, its)).collect();
                Self::verse_range(&ids)
            });
            return there.and_then(|range| b.lookup(&range).ok()).map_or(own, Ok);
        }
        let found = bible::sources::lookup(&self.store.root, translation, reference).await?;
        // API.Bible sends its notice with the first passage; the slides credit it from then on.
        self.refresh_bible_credits();
        Ok(found)
    }

    /// `reference` in each of `ids` (tidied against `main`): the ones found, and the ids left out (not available, not
    /// found, or blocked).
    pub async fn lookup_also(&self, main: &str, ids: &[String], reference: &str) -> (Vec<crate::scripture::AlsoShow>, Vec<String>) {
        let mut ids = ids.to_vec();
        let mut left_out = crate::scripture::also::tidy_ids(main, &mut ids, &|t| bible::blocklist::blocked(t, &[], &[], &[t]));
        let mut found = vec![];
        let numbering = |t: &str| bible::import::meta(&self.store.root, t).map_or(bible::library::Vsys::English, |m| m.versification);
        let main_numbering = numbering(main);
        // An online Bible says nothing about how it numbers its verses. One in a language whose Bibles often number
        // the Psalms and some Old Testament chapters their own way is not shown beside another Bible where the
        // numbers can differ: better left out than shown against the wrong verse.
        let unsure = |t: &str| bible::online::get(&self.store.root, t).is_some_and(|b| bible::versify::language_may_number_its_own_way(&b.lang));
        let moves = if unsure(main) || ids.iter().any(|t| unsure(t)) {
            // The verses asked for: the main Bible's, or (when it cannot be read just now) the reference as an
            // English Bible reads it.
            let asked = match self.lookup_passage(main, reference).await { Ok(found) => Some(found), Err(_) => bible::kjv().lookup(reference).ok() };
            asked.is_some_and(|(_, verses)| verses.iter().filter_map(|v| v.id).any(bible::versify::moves))
        } else { false };
        for t in ids {
            if moves && (unsure(main) || unsure(&t)) { left_out.push(t); continue }
            // A Bible numbered another way (the Hebrew Bible beside an English one) is asked for the same verses
            // under its own numbers, so the two line up.
            let its = numbering(&t);
            let looked_up = if its == main_numbering { self.lookup_passage(&t, reference).await }
                else { self.lookup_renumbered(main, main_numbering, &t, its, reference).await };
            match looked_up {
                Ok((_, verses)) if !bible::blocklist::blocked_passage(&t, &verses) => found.push(crate::scripture::AlsoShow { translation: t, verses }),
                _ => left_out.push(t),
            }
        }
        (found, left_out)
    }

    /// The reference that covers these verses, first to last ("Malachi 3:19-20").
    fn verse_range(ids: &[crate::scripture::VerseId]) -> Option<String> {
        let (first, last) = (*ids.iter().min()?, *ids.iter().max()?);
        let book = bible::display_name(first.book());
        Some(if first == last { format!("{book} {}:{}", first.chapter(), first.verse()) }
            else if first.chapter() == last.chapter() { format!("{book} {}:{}-{}", first.chapter(), first.verse(), last.verse()) }
            else { format!("{book} {}:{}-{}:{}", first.chapter(), first.verse(), last.chapter(), last.verse()) })
    }

    /// `reference` (in `main`'s numbering) from a Bible that numbers its verses another way (spec §5.4): each of the
    /// main Bible's verses is found under the other Bible's own number. The verse keeps its own label ("Malachi
    /// 3:19") and takes the main verse's identity, which is what lines the columns up on the slide. A verse the
    /// other Bible does not have (a psalm's title beside an English Bible) is left out, never shifted. Where two
    /// verses of one Bible are one verse of the other (Psalm 13:5-6 in English is 13:6 in Hebrew), the one verse
    /// shows once and stands for both, and the two show together beside the one.
    async fn lookup_renumbered(&self, main: &str, from: bible::library::Vsys, other: &str, to: bible::library::Vsys, reference: &str)
        -> Result<(String, Vec<VerseText>), String> {
        use crate::scripture::VerseId;
        let (label, verses) = self.lookup_passage(main, reference).await?;
        let pairs: Vec<(VerseId, Vec<VerseId>)> = verses.iter().filter_map(|v| v.id)
            .map(|id| (id, bible::versify::convert_all(id, from, to))).filter(|(_, there)| !there.is_empty()).collect();
        let Some(range) = Self::verse_range(&pairs.iter().flat_map(|p| p.1.iter().copied()).collect::<Vec<_>>()) else { return Err(tr!("No verses found.")) };
        let (_, theirs) = self.lookup_passage(other, &range).await?;
        // The other Bible's verses already shown, and where in `out`.
        let mut used: std::collections::BTreeMap<VerseId, usize> = std::collections::BTreeMap::new();
        let mut out: Vec<VerseText> = vec![];
        for (id, there) in &pairs {
            // Two of the main Bible's verses can be one verse there (Psalm 13:5-6): it shows once, beside the
            // first, and stands for both.
            if let Some(&at) = there.first().and_then(|t| used.get(t)) { out[at].through = Some(*id); continue }
            let found: Vec<&VerseText> = there.iter().filter_map(|t| theirs.iter().find(|v| v.id == Some(*t))).collect();
            let Some(first) = found.first() else { continue };
            // One of the main Bible's verses can be two verses there: both show beside it, under one label.
            let reference = if found.len() > 1 { Self::verse_range(there).unwrap_or_else(|| first.reference.clone()) } else { first.reference.clone() };
            let text = found.iter().map(|v| v.text.as_str()).collect::<Vec<_>>().join(" ");
            for t in there { used.insert(*t, out.len()); }
            out.push(VerseText { reference, text, id: Some(*id), through: None });
        }
        if out.is_empty() { return Err(tr!("No verses found.")) }
        Ok((label, out))
    }

    /// Gives a new passage the church's Also Bibles and layout. Returns the toast to show when some were left out.
    pub async fn fill_also(&self, item: &mut ServiceItem) -> Option<String> {
        let (ids, layout) = { let d = self.data(); (d.settings.also_show.clone(), d.settings.parallel_layout) };
        let ItemKind::Scripture { reference, translation, .. } = &item.kind else { return None };
        if ids.is_empty() { return None }
        let (main, reference) = (translation.clone(), reference.clone());
        let (found, left_out) = self.lookup_also(&main, &ids, &reference).await;
        let toast = (!left_out.is_empty()).then(|| {
            let kept: Vec<String> = std::iter::once(main.as_str()).chain(found.iter().map(|a| a.translation.as_str())).map(|t| self.abbr_of(t)).collect();
            let left: Vec<String> = left_out.iter().map(|t| self.abbr_of(t)).collect();
            crate::scripture::strings::also_left_out(&left.join(" · "), &kept.join(" · "))
        });
        if let ItemKind::Scripture { also, layout: l, .. } = &mut item.kind { *also = found; *l = layout; }
        toast
    }

    /// The short name a Bible shows under: its credit's abbreviation, else its id.
    pub fn abbr_of(&self, translation: &str) -> String {
        self.data().bible_credit(translation).map(|c| c.abbr.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| translation.to_string())
    }

    pub fn translations(&self) -> Vec<String> {
        let (secrets, now) = (bible::sources::Secrets::load(&self.store.root), now_secs());
        let own_key = |t: &str| bible::sources::sources(t).any(|s| !s.provider.closed(now) && secrets.own_key(s.provider).is_some());
        let mut out: Vec<String> = bible::TRANSLATIONS.iter().filter(|t| !bible::OWN_KEY_ONLY.contains(t) || own_key(t)).map(|t| t.to_string()).collect();
        for b in bible::import::list(&self.store.root).iter() { if !out.contains(&b.id) { out.push(b.id.clone()); } }
        for b in bible::online::list(&self.store.root) { if !out.contains(&b.id) { out.push(b.id); } }
        out
    }

    /// Translations that can be looked up now: built in, imported, or a licensed source whose key is saved.
    pub fn available_translations(&self) -> Vec<String> {
        self.bible_sources().into_iter().filter(|s| s["status"] != "needs_key" && s["status"] != "unavailable")
            .filter_map(|s| s["translation"].as_str().map(String::from)).collect()
    }

    /// Per-translation source details for Settings → Bible. Never includes the keys themselves.
    pub(crate) fn bible_sources(&self) -> Vec<Value> { self.bible_sources_of(&self.translations()) }

    /// `bible_sources` for a list of translations already in hand (`translations()`), so one look at the Bible
    /// lists makes that list once.
    pub(crate) fn bible_sources_of(&self, translations: &[String]) -> Vec<Value> {
        use bible::sources::{Secrets, source_with};
        let root = &self.store.root;
        let secrets = Secrets::load(root);
        let online = bible::online::list(root);
        translations.iter().map(|t| {
            if let Some((_, name)) = bible::BUILT_IN.iter().find(|b| b.0 == t) {
                return json!({ "translation": t, "abbr": t, "name": name, "status": "offline", "provider": tr!("{name} — built in, public domain", name = name), "language": "English" });
            }
            if let Some(b) = bible::import::meta(root, t) {
                use bible::library::Source;
                let downloaded = matches!(b.source, Source::Ebible { .. });
                let provider = if downloaded { bible::catalog::strings::from_ebible(&b.name) } else { tr!("{name} — imported file", name = b.name) };
                let abbr = if b.abbr.is_empty() { b.id.clone() } else { b.abbr.clone() };
                return json!({
                    "translation": t, "abbr": abbr, "name": b.name, "status": "offline", "imported": true, "downloaded": downloaded,
                    "provider": provider, "copyright": b.copyright, "licence": b.licence.short(), "licence_url": b.licence.link(), "lang": b.lang,
                    "language": if b.lang.is_empty() || b.lang == "und" { String::new() } else { bible::catalog::strings::language(&b.lang_name_en, &b.lang_name, &b.lang) },
                });
            }
            // Added from YouVersion's catalog: read over the internet with the YouVersion key.
            if let Some(b) = online.iter().find(|b| b.id == *t) {
                let has_key = secrets.key(bible::sources::Provider::YouVersion).is_some();
                return json!({
                    "translation": t, "abbr": if b.abbr.is_empty() { &b.id } else { &b.abbr }, "name": b.name,
                    "status": if has_key { "online" } else { "needs_key" }, "provider": bible::catalog::strings::from_youversion(&b.name),
                    "providers": [bible::sources::Provider::YouVersion.key_name()], "copyright": b.copyright, "lang": b.lang,
                    "language": if b.lang.is_empty() { String::new() } else { bible::catalog::strings::language(&b.lang_name_en, &b.lang_name, &b.lang) },
                });
            }
            match source_with(t, &secrets) {
                Some(s) => json!({
                    "translation": t,
                    "language": "English",
                    "status": if secrets.key(s.provider).is_some() { "online" } else { "needs_key" },
                    "provider": s.provider.label(),
                    "providers": bible::sources::sources(t).map(|s| s.provider.key_name()).collect::<Vec<_>>(),
                    // API.Bible sends its own notice with each passage; show it once one has been fetched.
                    "copyright": (s.provider == bible::sources::Provider::ApiBible).then(|| bible::api_bible::copyright(t)).flatten()
                        .unwrap_or_else(|| s.copyright.to_string()),
                }),
                None => json!({ "translation": t, "status": "unavailable" }),
            }
        }).collect()
    }

    /// Each Bible's credit for its scripture slides (Bible library spec §10), from the rows Settings → Bible shows:
    /// its abbreviation, its notice with the licence's short name, and whether the credit must show. Only a
    /// public-domain text may go without one: built in, labelled public domain, or a notice that says so.
    pub(crate) fn bible_credits(&self) -> BTreeMap<String, BibleCredit> {
        self.bible_sources().iter().filter_map(|s| {
            let id = s["translation"].as_str()?;
            let abbr = s["abbr"].as_str().filter(|a| !a.is_empty()).unwrap_or(id);
            let notice = s["copyright"].as_str().unwrap_or("").trim();
            let licence = s["licence"].as_str().unwrap_or("");
            let line = if notice.is_empty() && licence.is_empty() { String::new() }
                else { bible::catalog::strings::credit(abbr, notice, licence) };
            let built_in = bible::BUILT_IN.iter().any(|b| b.0 == id);
            let public = built_in || licence == bible::library::Licence::Pd.short()
                || (licence.is_empty() && notice.to_lowercase().contains("public domain"));
            Some((id.to_string(), BibleCredit { abbr: abbr.to_string(), required: !public && !line.is_empty(), line }))
        }).collect()
    }

    /// Works the slide credits out again; call it when a Bible is added, removed or its notice is learnt.
    pub(crate) fn refresh_bible_credits(&self) {
        let credits = self.bible_credits();
        self.data().bible_credits = credits;
    }

    /// The online services a church can connect, for Settings → Bible.
    pub(crate) fn bible_providers(&self) -> Vec<Value> {
        let secrets = bible::sources::Secrets::load(&self.store.root);
        // Tyndale's NLT API takes no new churches and closes on January 1, 2027: it is shown only to a church that
        // already saved its key, until then.
        let now = now_secs();
        bible::sources::PROVIDERS.iter().filter(|p| **p != bible::sources::Provider::Nlt || (secrets.own_key(**p).is_some() && !p.closed(now))).map(|p| json!({
            "id": p.key_name(),
            "label": p.label(),
            "signup_url": p.signup_url(),
            "note": p.setup_note(),
            "key_set": secrets.own_key(*p).is_some(),
            "translations": bible::sources::SOURCES.iter().filter(|s| s.provider == *p).map(|s| s.translation).collect::<Vec<_>>(),
        })).collect()
    }

    fn conns(&self, role: &str) -> Option<&AtomicUsize> {
        match role { "room" => Some(&self.room_conns), "stage" => Some(&self.stage_conns), _ => None }
    }

    /// Marks `role` ("room" or "stage") connected until the guard is dropped. Other roles count nothing.
    pub fn screen_guard(self: &Arc<Self>, role: &str) -> ScreenGuard {
        if let Some(c) = self.conns(role) { c.fetch_add(1, Ordering::Relaxed); }
        ScreenGuard { app: Arc::downgrade(self), role: role.into() }
    }

    /// The whole state as the web operator page once read it (`/api/state`, long gone). No route serves it and
    /// nothing in the app asks for it: it is kept for the tests that read the state through it.
    #[cfg(test)]
    pub fn state_json(&self) -> Value {
        let data = self.data();
        let items: Vec<Value> = data.service().map(|s| s.items.iter().map(|i| {
            let (title, kind, slides) = build_item(&data, i);
            json!({ "id": i.id, "title": title, "kind": kind, "slides": slides, "item": i })
        }).collect()).unwrap_or_default();
        let windows = self.open_roles();
        json!({
            "data": *data,
            "service_id": data.service().map(|s| s.id),
            "items": items,
            "monitors": self.monitors(),
            "windows": windows,
            "lan_url": self.lan_url,
            "translations": self.translations(),
            "available_translations": self.available_translations(),
            "bible_sources": self.bible_sources(),
            "bible_providers": self.bible_providers(),
            "backups": self.store.backups(),
            "phones": self.phones.lock().unwrap().iter().map(|p| json!({ "id": p.id, "name": p.name, "may_draw": p.may_draw })).collect::<Vec<_>>(),
            "jobs": *self.jobs.lock().unwrap(),
            "read_only": self.read_only(),
            "room_connected": self.room_conns.load(Ordering::Relaxed) > 0,
            "stage_connected": self.stage_conns.load(Ordering::Relaxed) > 0,
            "remotes_connected": self.remote_conns.load(Ordering::Relaxed),
            "data_folder": self.store.root.display().to_string(),
            "app_version": env!("CARGO_PKG_VERSION"),
            // The language shown (I18N): the phone remote reads its words again when this changes.
            "lang": crate::i18n::current(),
        })
    }

        /// Every operator action. The native UI calls this in-process; the HTTP `/api/do/*` route forwards to it.
    pub async fn do_action(self: &Arc<Self>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    let app = self;
    match action {
        "command" => {
            let env: Envelope = parse(body)?;
            Ok(serde_json::to_value(app.command(env)).unwrap())
        }
        // What the room shows, for tests and local automation.
        "live" => Ok(serde_json::to_value(app.live_view()).unwrap()),
        // The rest, by what they are about: each group of actions has its own file under `app/`.
        a if do_songs::handles(a) => do_songs::run(app, a, body).await,
        a if do_service::handles(a) => do_service::run(app, a, body).await,
        a if do_bible::handles(a) => do_bible::run(app, a, body).await,
        a if do_media::handles(a) => do_media::run(app, a, body).await,
        a if do_settings::handles(a) => do_settings::run(app, a, body).await,
        a if do_screens::handles(a) => do_screens::run(app, a, body).await,
        a if do_backups::handles(a) => do_backups::run(app, a, body).await,
        a if do_phones::handles(a) => do_phones::run(app, a, body).await,
        a if a.starts_with("usage/") => app.usage_action(a, body).await,
        a if crate::live::drawing::actions::handles(a) => crate::live::drawing::actions::run(app, a, body),
        a if crate::scripture::actions::handles(a) => crate::scripture::actions::run(app, a, body).await,
        a if crate::planning::actions::handles(a) => crate::planning::actions::run(app, a, body).await,
        a if crate::stage::actions::handles(a) => crate::stage::actions::run(app, a, body).await,
        a if crate::automation::actions::handles(a) => crate::automation::actions::run(app, a, body).await,
        a if crate::transfer::handles(a) => crate::transfer::run(app, a, body).await,
        a if crate::media_care::handles(a) => crate::media_care::run(app, a, body).await,
        a if crate::network::actions::handles(a) => crate::network::actions::run(app, a, body).await,
        a if crate::song_edit::actions::handles(a) => crate::song_edit::actions::run(app, a, body).await,
        // Themes, backgrounds, overlays and the logo (FEAT-1): `theme::actions` lists them.
        a => match crate::theme::actions::run(app, a, body).await {
            Some(r) => r,
            None => Err(ActionError { status: 404, message: "unknown action".into() }),
        },
    }
}
}

/// What a file is being imported for. Picks what `App::import_staged` does with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportPurpose { Media, Asset, PowerPoint, Lyrics, /** A .pptx as editable designed slides (1.1). */ Slides, /** Pictures, and the file kept to run in PowerPoint (1.2). */ Show }

/// What to do when an imported song has the same CCLI # as one in the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SongImportMode {
    /// Save nothing and report the match, so the operator can choose.
    Ask,
    /// Replace the existing song's words and credits. It keeps its id, so service items still point at it.
    Update,
    KeepBoth,
}

/// Why a service was not saved to a file.
#[derive(Clone, Debug, PartialEq)]
pub enum ExportError {
    /// One line per thing the service needs that is not on this computer.
    Missing(Vec<String>),
    Failed(String),
}

/// What an import made: a media library entry, a bare asset (logo, background), a PowerPoint job, or a song.
/// A song with no `id` was not saved: it matches `duplicate_of` and the operator has to choose.
#[derive(Clone, Debug)]
pub enum Imported {
    Media(MediaFile),
    Asset(AssetId),
    Job(Uuid),
    Song { id: Option<Uuid>, title: String, duplicate_of: Option<Uuid> },
}

impl App {
    /// Import a file from this computer (native file dialog or desktop drop). The file is copied into the
    /// store's tmp, never moved: it is the user's file. `progress` gets 0..=100 as bytes are copied and hashed.
    pub async fn import_path(self: &Arc<Self>, path: &Path, purpose: ImportPurpose, progress: impl Fn(u8) + Send + Sync + 'static) -> Result<Imported, ActionError> {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // A Keynote package is a folder. There is nothing to copy, and `start` says what to do about it.
        if purpose == ImportPurpose::PowerPoint && path.is_dir() {
            return Ok(Imported::Job(import::deck::start(self.clone(), path.to_path_buf(), name)));
        }
        let tmp = self.store.tmp().join(format!("{}.part", Uuid::new_v4()));
        let (src, dst, shown) = (path.to_path_buf(), tmp.clone(), name.clone());
        let sha = tokio::task::spawn_blocking(move || -> Result<String, String> {
            let disk_full = |_| tr!("Could not save the file (disk full?).");
            let mut input = std::fs::File::open(&src).map_err(|e| tr!("Could not open “{file}”: {error}", file = shown, error = e))?;
            let total = input.metadata().map(|m| m.len()).unwrap_or(0);
            let mut out = std::fs::File::create(&dst).map_err(disk_full)?;
            let (mut hasher, mut buf, mut done, mut last) = (Sha256::new(), vec![0u8; 1 << 16], 0u64, 0u8);
            progress(0);
            let copied = (|| -> Result<(), String> {
                loop {
                    let n = input.read(&mut buf).map_err(|e| tr!("Could not read “{file}”: {error}", file = shown, error = e))?;
                    if n == 0 { return Ok(()); }
                    hasher.update(&buf[..n]);
                    out.write_all(&buf[..n]).map_err(disk_full)?;
                    done += n as u64;
                    // 100 is sent once, after the copy is complete.
                    let p = (done * 99 / total.max(done).max(1)) as u8;
                    if p > last { last = p; progress(p); }
                }
            })();
            drop(out);
            if let Err(e) = copied { let _ = std::fs::remove_file(&dst); return Err(e); }
            progress(100);
            Ok(hex(&hasher.finalize()))
        }).await.map_err(|e| ActionError::bad(e.to_string()))?.map_err(ActionError::bad)?;
        self.import_staged(tmp, &name, &sha, purpose).await
    }

    /// Finish an import from a file already in the store's tmp (copied by `import_path`, or streamed there by
    /// the HTTP upload). `name` is the original file name and `sha` the file's hash. Takes ownership of `tmp`.
    pub async fn import_staged(self: &Arc<Self>, tmp: PathBuf, name: &str, sha: &str, purpose: ImportPurpose) -> Result<Imported, ActionError> {
        let app = self;
        let base_name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let file_name = std::path::Path::new(base_name);
        let ext = crate::store::sanitize_ext(file_name.extension().and_then(|s| s.to_str()).unwrap_or(""));
        let display = file_name.file_stem().and_then(|s| s.to_str()).unwrap_or(base_name).to_string();

        // Anything else dropped on the Media tab goes to the probe, which decides what it is.
        let song_file = ext == "txt" || ["usr", "chordpro", "cho", "chopro"].contains(&ext.as_str());
        let purpose = if purpose == ImportPurpose::Media && song_file { ImportPurpose::Lyrics } else { purpose };
        match purpose {
            ImportPurpose::Lyrics => {
                let bytes = tokio::fs::read(&tmp).await;
                let _ = tokio::fs::remove_file(&tmp).await;
                let bytes = bytes.map_err(|e| ActionError::bad(e.to_string()))?;
                let (id, title, duplicate_of) = app.import_song(base_name, &bytes, SongImportMode::Ask).map_err(ActionError::bad)?;
                return Ok(Imported::Song { id, title, duplicate_of });
            }
            ImportPurpose::PowerPoint => return Ok(Imported::Job(import::deck::start(app.clone(), tmp, display))),
            ImportPurpose::Show => return Ok(Imported::Job(import::deck::start_as(app.clone(), tmp, display, true))),
            ImportPurpose::Slides => return Ok(Imported::Job(import::pptx_design::start(app.clone(), tmp, display))),
            ImportPurpose::Asset | ImportPurpose::Media => {}
        }

        if purpose == ImportPurpose::Asset {
            if let Err(e) = import::media::check_picture(&tmp).await {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(ActionError::bad(e));
            }
            return Ok(Imported::Asset(app.store.publish(&tmp, sha, &ext).map_err(|e| ActionError::bad(e.to_string()))?));
        }
        import::media::import_media(app, &tmp, sha, &ext, display).await.map(Imported::Media).map_err(ActionError::bad)
    }

    /// Saves a service and everything it needs to one file (spec §11). Something missing stops it before a job
    /// starts; a save under way shows as a job with Cancel.
    pub async fn export_service(self: &Arc<Self>, id: Uuid, dest: PathBuf) -> Result<(), ExportError> {
        use crate::bundle;
        let data = self.data().clone();
        let app = self.clone();
        let check = tokio::task::spawn_blocking(move || bundle::gather(&data, id, &|a| app.store.asset_path(a).is_file()));
        let (contents, assets) = check.await.map_err(|e| ExportError::Failed(e.to_string()))?.map_err(ExportError::Missing)?;
        let (job, cancel) = self.job_start_cancellable(&bundle::strings::saving());
        let app = self.clone();
        let work = tokio::task::spawn_blocking(move || bundle::write(&app.store, &contents, &assets, &dest, &cancel, &|p| app.job_progress(job, p)));
        self.job_finished(job, work.await.map_err(|e| e.to_string()).and_then(|r| r)).map_err(ExportError::Failed)
    }

    /// Opens a saved service file, adds what is new to the library, and opens the service (spec §11). A bad file
    /// adds nothing and its error stays on the job.
    pub async fn import_service(self: &Arc<Self>, path: PathBuf) -> Result<crate::bundle::Opened, String> {
        use crate::bundle;
        let (job, cancel) = self.job_start_cancellable(&bundle::strings::opening());
        let app = self.clone();
        let file = path.clone();
        let work = tokio::task::spawn_blocking(move || bundle::read(&app.store, &file, &cancel, &|p| app.job_progress(job, p)));
        let contents = self.job_finished(job, work.await.map_err(|e| e.to_string()).and_then(|r| r))?;
        let fonts = contents.fonts.clone();
        let missing = tokio::task::spawn_blocking(move || {
            bundle::missing_fonts(&fonts, &|f| crate::render::Fonts::installed(f))
        }).await.unwrap_or_default();
        let opened = self.edit(|d| {
            d.settings.opened_file(&path);
            Ok(bundle::Opened { fonts_missing: missing, ..bundle::merge(d, contents) })
        })?;
        self.online_text_from_cache();
        let me = self.clone();
        tokio::spawn(async move { me.fetch_online_text().await });
        Ok(opened)
    }

    /// The eBible catalog as Settings → Bibles shows it, with what is installed and what has a newer edition.
    /// Asks eBible at most once a day unless `force`.
    pub async fn bible_catalog(&self, force: bool) -> Result<serde_json::Value, String> {
        use bible::catalog::{CatalogSource, Ctx, LicenceState};
        let mut ctx = Ctx::new(self.store.root.clone(), CatalogSource::Ebible);
        ctx.force = force;
        let ebible = CatalogSource::Ebible.refresh(&ctx).await;
        // Every Bible the YouVersion key may read, beside eBible's. Without a key, or offline with no saved list,
        // there are none, and eBible's list shows alone.
        let mut yv_ctx = Ctx::new(self.store.root.clone(), CatalogSource::YouVersion);
        yv_ctx.force = force;
        let youversion = CatalogSource::YouVersion.refresh(&yv_ctx).await.map(|l| l.rows).unwrap_or_default();
        let mut listing = match ebible {
            Ok(l) => l,
            Err(e) if youversion.is_empty() => return Err(e.message()),
            Err(_) => bible::catalog::Listing { rows: Vec::new(), offline: true },
        };
        listing.rows.extend(youversion);
        let installed = bible::import::list(&self.store.root);
        let online = bible::online::list(&self.store.root);
        let has_youversion = bible::sources::Secrets::load(&self.store.root).key(bible::sources::Provider::YouVersion).is_some();
        // Grouped by language, the church's first. There is no church language setting yet, so English.
        // One row per Bible across sources (Nov 5 spec §4): a Bible eBible has too stays a download that works offline.
        let merged = bible::catalog::merge(listing.rows, &|s| s == CatalogSource::YouVersion && has_youversion);
        let all: Vec<&bible::catalog::CatalogRow> = merged.iter().collect();
        let groups = bible::catalog::by_language(&all, "eng");
        let rows: Vec<serde_json::Value> = groups.iter().flat_map(|g| g.rows.iter().copied()).map(|r| {
            let read_online = !r.source().offline();
            let have = installed.iter().find(|m| m.id == r.id());
            let added = if read_online { online.iter().any(|b| b.id == r.id()) } else { have.is_some() };
            let update = !read_online && have.is_some_and(|m| !matches!(&m.source, bible::library::Source::Ebible { update_date } if update_date == r.update_date()));
            json!({
                "id": r.id(), "abbr": r.abbr(), "title": r.title(), "lang": r.lang(), "lang_name": r.lang_name(),
                "lang_name_en": r.lang_name_en(), "scope": r.scope().label(), "apocrypha": r.has_dc(),
                "licence_on_download": *r.licence() == LicenceState::OnDownload, "installed": added, "update": update, "online": read_online,
                "also_on": bible::catalog::strings::also_on(&r.also_from().iter().map(|s| s.label()).collect::<Vec<_>>()),
            })
        }).collect();
        Ok(json!({ "offline": listing.offline, "rows": rows }))
    }

    /// Downloads a Bible from the eBible catalog, or updates the installed copy, as a job with Cancel. Downloads
    /// wait their turn. A failed update leaves the installed copy working.
    pub async fn install_bible(self: &Arc<Self>, id: &str) -> Result<bible::library::BibleMeta, String> {
        use bible::catalog::{CatalogSource, Ctx, strings};
        if id.starts_with("yv:") { return self.add_online_bible(id).await; }
        let ctx = Ctx::new(self.store.root.clone(), CatalogSource::Ebible);
        let listing = CatalogSource::Ebible.refresh(&ctx).await.map_err(|e| e.message())?;
        if listing.offline { return Err(strings::offline_no_add().into()); }
        let row = listing.rows.into_iter().find(|r| r.id() == id).ok_or(strings::not_in_list())?;
        let name = row.title().to_string();
        let update = bible::import::meta(&self.store.root, id).is_some();
        let (job, cancel) = self.job_start_cancellable(&strings::waiting(&name));
        self.job_subject(job, id);
        let turn = tokio::select! {
            t = self.bible_downloads.lock() => Some(t),
            _ = cancel.cancelled() => None,
        };
        let Some(_turn) = turn else { self.job_end_cancelled(job); return Err(import::CANCELLED.into()) };
        self.job_label(job, &strings::downloading_name(&name));
        let app = self.clone();
        let r = row.source().install(&ctx, &row, &cancel, &move |p| app.job_progress(job, p)).await
            .map_err(|e| { eprintln!("Bible download {id}: {e:?}"); e.message(&name, update) });
        let r = self.job_finished(job, r);
        if r.is_ok() { self.refresh_bible_credits(); self.broadcast_data(); }
        r
    }

    /// Adds a Bible from YouVersion's catalog to the church's list. Nothing is downloaded: its passages are read
    /// over the internet as they are looked up. Its notice comes from the Bible's own entry, as the list has none.
    async fn add_online_bible(&self, id: &str) -> Result<bible::library::BibleMeta, String> {
        use bible::catalog::{CatalogSource, Ctx, strings, youversion};
        let root = &self.store.root;
        let ctx = Ctx::new(root.clone(), CatalogSource::YouVersion);
        let listing = CatalogSource::YouVersion.refresh(&ctx).await.map_err(|e| e.message())?;
        let row = listing.rows.into_iter().find(|r| r.id() == id).ok_or(strings::not_in_list())?;
        let key = bible::sources::Secrets::load(root).key(bible::sources::Provider::YouVersion).map(str::to_string);
        let details = match key { Some(k) => youversion::details(&ctx.base, &k, row.source_id()).await, None => None };
        let notice = details.and_then(|d| d.copyright).map(|c| bible::sources::tidy(&c)).filter(|c| !c.is_empty())
            .unwrap_or_else(|| row.copyright().to_string());
        let b = bible::online::OnlineBible {
            id: row.id().into(), abbr: row.abbr().into(), name: row.title().into(), lang: row.lang().into(),
            lang_name: row.lang_name().into(), lang_name_en: row.lang_name_en().into(), copyright: notice,
        };
        bible::online::add(root, b.clone()).map_err(|e| e.to_string())?;
        self.refresh_bible_credits();
        self.broadcast_data();
        Ok(bible::library::BibleMeta {
            id: b.id, abbr: b.abbr, name: b.name, name_en: row.title_en().into(), lang: b.lang, lang_name: b.lang_name,
            lang_name_en: b.lang_name_en, copyright: b.copyright, licence: bible::library::Licence::Provider,
            source: bible::library::Source::YouVersion, ..Default::default()
        })
    }

    /// Ends a job the way its result says.
    fn job_finished<T>(&self, job: Uuid, r: Result<T, String>) -> Result<T, String> {
        match &r {
            Ok(_) => self.job_end(job, None),
            Err(e) if e == import::CANCELLED => self.job_end_cancelled(job),
            Err(e) => self.job_end(job, Some(e.clone())),
        }
        r
    }

    /// Import a SongSelect song file (lyrics `.txt`, ChordPro or `.usr`; `name` is the file's name, which tells them
    /// apart). Returns the saved song's id (`None` if `Ask` found a match), its title,
    /// and the library song with the same CCLI #. `Update` with no match adds the song: the old one may have
    /// been deleted while the question was open.
    fn import_song(&self, name: &str, bytes: &[u8], mode: SongImportMode) -> Result<(Option<Uuid>, String, Option<Uuid>), String> {
        let song = songselect::inbox::read_song_file(name, bytes)?;
        let title = song.title.clone();
        // A question saves nothing, so it neither writes data.json nor tells the screens.
        if mode == SongImportMode::Ask {
            let data = self.data();
            if let Some(s) = data.songs.iter().find(|s| !song.ccli.is_empty() && s.ccli == song.ccli) {
                return Ok((None, title, Some(s.id)));
            }
        }
        let (id, duplicate_of) = self.edit(|d| {
            let existing = d.songs.iter_mut().find(|s| !song.ccli.is_empty() && s.ccli == song.ccli);
            let duplicate_of = existing.as_ref().map(|s| s.id);
            let id = match (mode, existing) {
                (SongImportMode::Ask, Some(_)) => None,
                (SongImportMode::Update, Some(s)) => {
                    let Song { title, writers, ccli, copyright, lyrics, .. } = song;
                    (s.title, s.writers, s.ccli, s.copyright, s.lyrics) = (title, writers, ccli, copyright, lyrics);
                    Some(s.id)
                }
                _ => { let id = song.id; d.songs.push(song); Some(id) }
            };
            Ok((id, duplicate_of))
        })?;
        Ok((id, title, duplicate_of))
    }

    /// A song the person downloaded from SongSelect, into the library without touching the church's own songs
    /// (`songselect::inbox::place`). Gives the id to add to a service and what happened.
    pub fn bring_in_songselect(&self, song: Song) -> Result<(Uuid, songselect::inbox::Landed), String> {
        self.edit(|d| Ok(songselect::inbox::place(&mut d.songs, song)))
    }
}

pub(crate) fn hash_token(t: &str) -> String { digest_hex(t.as_bytes()) }

/// 256 bits from the OS random generator (via two v4 UUIDs, 244 random bits).
pub(crate) fn random_token() -> String { format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()) }

/// What the QR code opens: the phone page with the one-time secret in the fragment, so it never reaches a
/// request line or a log; `remote.js` reads it, wipes it from the address bar and posts it.
pub(crate) fn pair_link(lan_url: &str, secret: &str) -> String { format!("{lan_url}#pair={secret}") }

pub(crate) fn pair_qr_svg(text: &str) -> Option<String> {
    qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::M).ok()
        .map(|c| c.render::<qrcode::render::svg::Color>().quiet_zone(true).build())
}

/// Compares two secrets without stopping at the first difference.
pub(crate) fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let diff = a.iter().zip(b).fold((a.len() ^ b.len()) as u64, |d, (x, y)| d | u64::from(x ^ y));
    diff == 0
}

pub(crate) fn random_pin() -> String {
    let b = Uuid::new_v4().into_bytes();
    let n = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[9], b[10]]);
    format!("{:06}", n % 1_000_000)
}

#[derive(Deserialize)]
struct Id { id: Uuid }

/// A body that names one Bible.
#[derive(Deserialize)]
struct T { translation: String }

/// Whole items come in through the action API without passing `bible/lookup`, so the blocklist looks at them here.
fn refuse_blocked(item: &ServiceItem) -> Result<(), ActionError> {
    match &item.kind {
        ItemKind::Scripture { translation, verses, also, .. }
            if bible::blocklist::blocked_passage(translation, verses) || also.iter().any(|a| bible::blocklist::blocked_passage(&a.translation, &a.verses)) =>
            Err(ActionError::bad(blocked())),
        _ => Ok(()),
    }
}

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ActionError> {
    serde_json::from_slice(body).map_err(|e| ActionError::bad(tr!("Bad request: {error}", error = e)))
}

/// What the tests leave in the computer's Temp folder: each makes its own folder or file there and few remove it
/// (an app the test made may still hold it), so 14,287 had piled up by November 14, 2026. The first test of a run
/// (`test_app`, used by most) removes what earlier runs left: entries whose names start with one of these and that
/// are older than `SCRAPS_AGE`, so a run going on at the same time keeps its own. "rp-import-" and
/// "rp-scripture-" are not here: the app itself uses those names.
#[cfg(test)]
const TEST_SCRAPS: &[&str] = &["rp-test-", "rp-bench-", "rp-deck-", "rp-ew-", "rp-fetch-", "rp-files-", "rp-follow-", "rp-headless-", "rp-lo-",
    "rp lo ", "rp-model-", "rp-olp-", "rp-panic-", "rp-pdf-", "rp-pplive-", "rp-ppt-", "rp ppt ", "rp-preflight-", "rp-probe-", "rp-record-",
    "rp-report-", "rp-safe-mode-", "rp-settings-", "rp-show-", "rp-songselect-", "rp-state-", "rp-supervisor-", "rp-themes-", "rp-thumbs-", "rp-tools-",
    "rp-upload-", "rp-v", "rp-yt-", "rp-ytui-", "v1-screens-"];
#[cfg(test)]
const SCRAPS_AGE: std::time::Duration = std::time::Duration::from_secs(2 * 3600);

/// Removes earlier test runs' leftovers from `dir` (see `TEST_SCRAPS`); how many went.
#[cfg(test)]
pub(crate) fn sweep_test_scraps(dir: &std::path::Path, older_than: std::time::Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let now = std::time::SystemTime::now();
    let mut gone = 0;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !TEST_SCRAPS.iter().any(|p| name.starts_with(p)) { continue }
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|age| age > older_than);
        if !old { continue }
        let path = e.path();
        let removed = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        if removed.is_ok() { gone += 1 }
    }
    gone
}

#[cfg(test)]
pub(crate) fn test_app() -> Arc<App> {
    static SWEPT: std::sync::Once = std::sync::Once::new();
    SWEPT.call_once(|| { sweep_test_scraps(&std::env::temp_dir(), SCRAPS_AGE); });
    let root = std::env::temp_dir().join(format!("rp-test-{}", Uuid::new_v4()));
    let store = Store::open(root).unwrap();
    let data = store.load().unwrap();
    App::new(store, data, 0, None)
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
