//! The app's state and every operator action, independent of how the screens are drawn.
//! The HTTP layer (`server.rs`) and the native screens both drive it.
use crate::{bible, import, live::*, model::*, recovery::state::{self, StateWriter}, scripture::strings::{BLOCKED, passages_removed}, slides::build_item, songselect, store::Store};
use crate::sha256::{Digest, Sha256, digest_hex, hex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    io::{Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak, atomic::{AtomicU64, AtomicUsize, Ordering}},
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

/// Something every screen may need to react to. Sent on `App::tx`.
#[derive(Clone, Debug)]
pub enum Update {
    Live(LiveUpdate),
    /// Library, service, settings or windows changed: re-read `state_json`.
    Data,
    Job { id: Uuid, percent: u8 },
    /// A paired phone was removed; its remote stream ends.
    Revoked { phone: Uuid },
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
    pub theme: Theme,
    pub logo: Option<AssetId>,
    /// The operator's message for the stage screen (spec §12.2). Never in the JSON: it goes to the stage and nowhere else.
    #[serde(skip)]
    pub stage_message: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct LiveItemSummary { pub item_id: Uuid, pub title: String, pub kind: String, pub count: usize }

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

/// An open pairing window: the PIN to type, and the one-time secret the QR code carries (both die together).
pub struct Pairing { pub(crate) pin: String, pub(crate) secret: String, pub(crate) until: Instant, pub(crate) failures: u32 }

#[derive(Serialize, Deserialize, Clone)]
pub struct Phone { pub(crate) id: Uuid, pub(crate) name: String, pub(crate) hash: String, pub(crate) last_seen: u64 }

use crate::import::strings::FALLBACK_NOTE;

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
    /// `FALLBACK_NOTE` was given; `note` starts with it.
    #[serde(skip)]
    pub(crate) fell_back: bool,
    /// The latest note other than `FALLBACK_NOTE`.
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
    pub data: Mutex<Data>,
    pub live: Mutex<Live>,
    pub tx: broadcast::Sender<Update>,
    /// Held from reading a live view until it is sent, so views go out in the order they were read.
    live_order: Mutex<()>,
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
    pub remote_conns: AtomicUsize,
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
    /// YouTube downloads: their helper programs and lookups.
    pub youtube: crate::youtube::Hub,
}

pub struct DataPlan<'a>(pub &'a Data);

impl Plan for DataPlan<'_> {
    fn order(&self) -> Vec<Uuid> { self.0.service().map(|s| s.items.iter().map(|i| i.id).collect()).unwrap_or_default() }
    fn build(&self, id: Uuid) -> Option<LiveItem> {
        let item = self.0.service()?.items.iter().find(|i| i.id == id)?;
        let (title, kind, slides) = build_item(self.0, item);
        Some(LiveItem { item_id: id, title, kind: kind.into(), slides, background: item.background.clone().unwrap_or_else(|| self.0.settings.background.clone()) })
    }
}

pub(crate) const SESSION_DAYS: u64 = 30;
pub(crate) const MAX_PHONES: usize = 5;

pub(crate) fn now_secs() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }

impl App {
    pub fn new(store: Store, data: Data, port: u16, lan_url: Option<String>) -> Arc<Self> {
        let (tx, _) = broadcast::channel(256);
        let phones: Vec<Phone> = std::fs::read(store.root.join("phones.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        // A download cut off by a crash or a forced quit leaves nothing behind.
        bible::catalog::install::clean_partials(&store.root);
        let youtube = crate::youtube::Hub::new(&store.root);
        let app = Arc::new(Self {
            store, data: Mutex::new(data), live: Mutex::new(Live::new()), tx, live_order: Mutex::new(()), port, lan_url,
            screens: OnceLock::new(), applied: Mutex::default(), pairing: Mutex::new(None), phones: Mutex::new(phones),
            attempts: Mutex::new(HashMap::new()), jobs: Mutex::new(vec![]), job_ends: Mutex::default(), cancels: Mutex::new(HashMap::new()),
            room_conns: AtomicUsize::new(0), stage_conns: AtomicUsize::new(0), stage_message: Mutex::default(), remote_conns: AtomicUsize::new(0),
            state_writer: Mutex::new(None), state_seq: AtomicU64::new(0), resume: Mutex::new(HashMap::new()), probe_mpv: OnceLock::new(), converters: OnceLock::new(),
            bible_downloads: tokio::sync::Mutex::new(()), youtube,
        });
        app.refresh_bible_credits();
        app
    }

    pub fn live_view(&self) -> LiveView {
        let data = self.data.lock().unwrap();
        let live = self.live.lock().unwrap();
        let plan = DataPlan(&data);
        LiveView {
            version: live.version,
            committed: live.committed_version,
            pending: live.pending(),
            frame: live.frame.clone(),
            cursor: live.cursor.clone(),
            item: live.item.as_ref().map(|i| LiveItemSummary { item_id: i.item_id, title: i.title.clone(), kind: i.kind.clone(), count: i.slides.len() }),
            slide: live.cursor.as_ref().and_then(|c| live.item.as_ref().and_then(|i| i.slides.get(c.slide))).cloned(),
            next: live.next_preview(&plan).map(|(title, slide)| NextUp { title, slide }),
            media: live.media_status.clone(),
            audio: live.audio_status.clone(),
            notice: live.notice.clone(),
            theme: data.settings.theme.clone(),
            logo: data.settings.logo.clone(),
            stage_message: self.stage_message.lock().unwrap().clone(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Update> { self.tx.subscribe() }

    /// Two threads can get here at once: the room's commit and an operator's Next. Sent out of order, the older view
    /// would replace the newer one in the room's `latest`, and the room would never see the slide it was asked for.
    pub fn broadcast_live(&self) {
        let _order = self.live_order.lock().unwrap();
        let _ = self.tx.send(Update::Live(LiveUpdate::new(self.live_view())));
    }
    pub fn broadcast_data(&self) { let _ = self.tx.send(Update::Data); }

    pub(crate) fn save(&self, data: &Data) -> Result<(), String> { self.store.save(data).map_err(|e| format!("Could not save: {e}")) }

    /// Mutate data, save, and tell every screen.
    pub(crate) fn edit<T>(&self, f: impl FnOnce(&mut Data) -> Result<T, String>) -> Result<T, String> {
        let r = {
            let mut data = self.data.lock().unwrap();
            let r = f(&mut data)?;
            self.save(&data)?;
            r
        };
        self.broadcast_data();
        self.broadcast_live();
        Ok(r)
    }

    pub fn command(&self, env: Envelope) -> Outcome {
        let out = {
            let data = self.data.lock().unwrap();
            let mut live = self.live.lock().unwrap();
            live.apply(env, &DataPlan(&data))
        };
        if out.ok && !out.duplicate { self.broadcast_live(); }
        out
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
        let snapshot = state::snapshot(&self.live.lock().unwrap(), windows, seq);
        if let Some(s) = snapshot { writer.submit(s); }
    }

    /// Is video or audio on air?
    pub fn playing(&self) -> bool { self.live.lock().unwrap().committed().is_some_and(|(frame, ..)| frame.plays()) }

    /// Where the restored players should start, in seconds.
    pub fn set_resume(&self, positions: &[(u64, f64)]) { self.resume.lock().unwrap().extend(positions.iter().copied()); }

    /// Kept until the instance goes on air, as a newer plan may prepare it again first; after that a retrigger
    /// starts from the beginning.
    pub fn resume_position(&self, inst: u64) -> Option<f64> { self.resume.lock().unwrap().get(&inst).copied() }

    /// Forgets the resume positions of the instances the room just committed.
    fn resume_on_air(&self) {
        if self.resume.lock().unwrap().is_empty() { return }
        let Some((frame, ..)) = self.live.lock().unwrap().committed() else { return };
        let on_air: Vec<u64> = frame.instances().collect();
        self.resume.lock().unwrap().retain(|inst, _| !on_air.contains(inst));
    }

    pub fn set_notice(&self, text: &str) {
        self.live.lock().unwrap().notice = Some(text.into());
        self.broadcast_live();
    }

    pub fn room_event(&self, ev: RoomEvent) {
        let (changed, committed) = {
            let mut live = self.live.lock().unwrap();
            let before = live.committed_version;
            (live.report(ev), live.committed_version != before)
        };
        if changed { self.broadcast_live(); }
        if committed { self.resume_on_air(); self.save_live_state(); }
    }

    /// Room window restarted: static layers return, audible media does not (spec §4.2).
    pub fn room_recovered(&self) {
        {
            let data = self.data.lock().unwrap();
            let mut live = self.live.lock().unwrap();
            let mut stopped = false;
            if live.frame.media.as_ref().is_some_and(|m| matches!(m.v, MediaLayer::Video { .. })) {
                live.apply(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::ClearMedia }, &DataPlan(&data));
                stopped = true;
            }
            if live.frame.audio.is_some() {
                live.apply(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::ClearAudio }, &DataPlan(&data));
                stopped = true;
            }
            live.notice = Some(if stopped { "The room screen restarted. Video/audio stopped — click it again to replay." } else { "The room screen restarted." }.into());
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
    /// Sets the job's note. The last one wins, except that `FALLBACK_NOTE` and any other note are both kept, the
    /// fallback first. So the deck pipeline sends "Imported with LibreOffice" only when it did not fall back.
    pub(crate) fn job_note(&self, id: Uuid, note: String) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
            if note == FALLBACK_NOTE { j.fell_back = true } else { j.other_note = Some(note) }
            j.note = match (j.fell_back, &j.other_note) {
                (true, Some(other)) => Some(format!("{FALLBACK_NOTE} {other}")),
                (true, None) => Some(FALLBACK_NOTE.into()),
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

    pub(crate) fn save_phones(&self, phones: &[Phone]) {
        if let Ok(b) = serde_json::to_vec(phones) { let _ = crate::store::write_atomic(&self.store.root.join("phones.json"), &b); }
    }
    pub fn set_screens(&self, s: Arc<dyn Screens>) { let _ = self.screens.set(s); }

    pub fn open_window(&self, role: &str, monitor: Option<&str>) -> Result<(), String> {
        self.screens.get().ok_or("RustyPresenter is still starting. Try again in a moment.")?.open(role, monitor)
    }

    pub fn close_window(&self, role: &str) { if let Some(s) = self.screens.get() { s.close(role); } }

    /// Remembers that these windows were dealt with at start, so `sync_screens` leaves them alone.
    pub fn screens_applied(&self, windows: Vec<ScreenWindow>) {
        *self.applied.lock().unwrap() = windows.into_iter().map(|w| (w.key.clone(), w)).collect();
    }

    /// Makes the open windows match the setup: closes what it no longer asks for, and opens what is new or changed.
    /// A window that is already right is left alone, so nothing on air blinks. Says which screens could not open.
    pub fn sync_screens(&self) -> Result<(), String> {
        let want = self.data.lock().unwrap().settings.screens.windows();
        let mut applied = std::mem::take(&mut *self.applied.lock().unwrap());
        let wanted = |key: &str| want.iter().any(|w| w.key == key);
        for key in self.open_roles().into_iter().filter(|r| r != "operator" && !wanted(r)) { self.close_window(&key) }
        applied.retain(|key, _| wanted(key));
        let mut failed = vec![];
        for w in &want {
            if applied.get(&w.key) == Some(w) { continue }
            let opened = match &w.target {
                ScreenTarget::Monitor(m) => self.open_window(&w.key, Some(m)),
                ScreenTarget::Window => self.screens.get().ok_or_else(|| "RustyPresenter is still starting. Try again in a moment.".to_string()).and_then(|s| s.open_window(&w.key, w.size)),
                ScreenTarget::None => continue,
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
        let setup = self.data.lock().unwrap().settings.screens.clone();
        let marks = screens.monitors().into_iter().enumerate().map(|(i, m)| {
            let names: Vec<&str> = ScreenSetup::GROUPS.iter().filter_map(|g| setup.group(g)).flat_map(|g| &g.screens)
                .filter(|s| s.output().target == ScreenTarget::Monitor(m.id.clone())).map(|s| s.name.as_str()).collect();
            let what = if names.is_empty() { if m.primary { "This computer" } else { "Not used" }.to_string() } else { names.join(", ") };
            (m.id, (i + 1).to_string(), what)
        }).collect();
        screens.identify(marks);
    }

    /// Spec §13, for the current service. Looks at every file it needs, so call it off the UI thread.
    pub fn preflight(&self) -> Vec<String> {
        let data = self.data.lock().unwrap().clone();
        let has_asset = |id: &str| std::fs::metadata(self.store.asset_path(id)).is_ok_and(|m| m.is_file() && m.len() > 0);
        let audio: Vec<String> = self.audio_devices().into_iter().map(|d| d.name).collect();
        crate::preflight::check(&data, &self.monitors(), &audio, &has_asset)
    }

    pub fn audio_devices(&self) -> Vec<crate::video::ffi::AudioDevice> { self.screens.get().map(|s| s.audio_devices()).unwrap_or_default() }

    /// One look at the chosen sound output. Asks the system, so never call it on the UI thread.
    pub fn watch_sound(&self) {
        use crate::{preflight::strings::SOUND_GONE, sound};
        let chosen = self.data.lock().unwrap().settings.audio_device.clone();
        let told = || self.live.lock().unwrap().notice.as_deref() == Some(SOUND_GONE);
        let heard = || sound::pauses(&self.live.lock().unwrap().frame);
        if !told() && (chosen.is_empty() || heard().is_empty()) { return }
        let found = if chosen.is_empty() { vec![] } else { self.audio_devices() };
        if !chosen.is_empty() && found.is_empty() { return } // The list could not be read: nothing has changed.
        if !sound::missing(&chosen, &found) {
            let cleared = self.live.lock().unwrap().notice.take_if(|n| n == SOUND_GONE).is_some();
            if cleared { self.broadcast_live(); }
            return;
        }
        // Read again: the list takes a moment, and what plays may have changed. A stale pause is refused anyway.
        let pauses = heard();
        if pauses.is_empty() { return }
        for cmd in pauses { self.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd }); }
        self.set_notice(SOUND_GONE);
    }

    pub(crate) fn monitors(&self) -> Vec<Monitor> { self.screens.get().map(|s| s.monitors()).unwrap_or_default() }

    /// Roles with an open window (`state_json`'s `windows`).
    pub(crate) fn open_roles(&self) -> Vec<String> { self.screens.get().map(|s| s.open_roles()).unwrap_or_default() }

    /// The standard list plus any other Bible the church imported.
    /// A passage in one Bible: built in, then a file the church imported or downloaded (offline, and it wins over
    /// any online source), then online.
    pub async fn lookup_passage(&self, translation: &str, reference: &str) -> Result<(String, Vec<VerseText>), String> {
        if let Some(b) = bible::built_in(translation) { return b.lookup(reference) }
        if let Some(b) = bible::import::get(&self.store.root, translation) { return b.lookup(reference) }
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
        for t in ids {
            match self.lookup_passage(&t, reference).await {
                Ok((_, verses)) if !bible::blocklist::blocked_passage(&t, &verses) => found.push(crate::scripture::AlsoShow { translation: t, verses }),
                _ => left_out.push(t),
            }
        }
        (found, left_out)
    }

    /// Gives a new passage the church's Also Bibles and layout. Returns the toast to show when some were left out.
    pub async fn fill_also(&self, item: &mut ServiceItem) -> Option<String> {
        let (ids, layout) = { let d = self.data.lock().unwrap(); (d.settings.also_show.clone(), d.settings.parallel_layout) };
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
        self.data.lock().unwrap().bible_credit(translation).map(|c| c.abbr.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| translation.to_string())
    }

    pub fn translations(&self) -> Vec<String> {
        let mut out: Vec<String> = bible::TRANSLATIONS.iter().map(|t| t.to_string()).collect();
        for b in bible::import::list(&self.store.root).iter() { if !out.contains(&b.id) { out.push(b.id.clone()); } }
        out
    }

    /// Translations that can be looked up now: built in, imported, or a licensed source whose key is saved.
    pub fn available_translations(&self) -> Vec<String> {
        self.bible_sources().into_iter().filter(|s| s["status"] != "needs_key" && s["status"] != "unavailable")
            .filter_map(|s| s["translation"].as_str().map(String::from)).collect()
    }

    /// Per-translation source details for Settings → Bible. Never includes the keys themselves.
    pub(crate) fn bible_sources(&self) -> Vec<Value> {
        use bible::sources::{Secrets, source_with};
        let root = &self.store.root;
        let secrets = Secrets::load(root);
        self.translations().iter().map(|t| {
            if let Some((_, name)) = bible::BUILT_IN.iter().find(|b| b.0 == t) {
                return json!({ "translation": t, "abbr": t, "name": name, "status": "offline", "provider": format!("{name} — built in, public domain"), "language": "English" });
            }
            if let Some(b) = bible::import::meta(root, t) {
                use bible::library::Source;
                let downloaded = matches!(b.source, Source::Ebible { .. });
                let provider = if downloaded { bible::catalog::strings::from_ebible(&b.name) } else { format!("{} — imported file", b.name) };
                let abbr = if b.abbr.is_empty() { b.id.clone() } else { b.abbr.clone() };
                return json!({
                    "translation": t, "abbr": abbr, "name": b.name, "status": "offline", "imported": true, "downloaded": downloaded,
                    "provider": provider, "copyright": b.copyright, "licence": b.licence.short(), "licence_url": b.licence.link(), "lang": b.lang,
                    "language": if b.lang.is_empty() || b.lang == "und" { String::new() } else { bible::catalog::strings::language(&b.lang_name_en, &b.lang_name, &b.lang) },
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
        self.data.lock().unwrap().bible_credits = credits;
    }

    /// The online services a church can connect, for Settings → Bible.
    pub(crate) fn bible_providers(&self) -> Vec<Value> {
        let secrets = bible::sources::Secrets::load(&self.store.root);
        bible::sources::PROVIDERS.iter().map(|p| json!({
            "id": p.key_name(),
            "label": p.label(),
            "signup_url": p.signup_url(),
            "note": p.setup_note(),
            "key_set": secrets.key(*p).is_some(),
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

    pub fn state_json(&self) -> Value {
        let data = self.data.lock().unwrap();
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
            "phones": self.phones.lock().unwrap().iter().map(|p| json!({ "id": p.id, "name": p.name })).collect::<Vec<_>>(),
            "jobs": *self.jobs.lock().unwrap(),
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
        // ---- songs ----
        "song/save" => {
            let mut song: Song = parse(body)?;
            if song.title.trim().is_empty() { return Err(ActionError::bad("Give the song a title.")); }
            if song.id.is_nil() { song.id = Uuid::new_v4(); }
            let id = song.id;
            tryr!(app.edit(|d| { match d.songs.iter_mut().find(|s| s.id == id) { Some(s) => *s = song, None => d.songs.push(song) } Ok(()) }));
            Ok(json!({ "id": id }))
        }
        "song/delete" => {
            let Id { id } = parse(body)?;
            tryr!(app.edit(|d| {
                let used = d.services.iter().any(|s| s.items.iter().any(|i| matches!(i.kind, ItemKind::Song { song_id } if song_id == id)));
                if used { return Err("This song is in a service. Remove it from the service first.".into()); }
                d.songs.retain(|s| s.id != id);
                Ok(())
            }));
            Ok(json!({}))
        }
        "song/import" => {
            #[derive(Deserialize)] struct Import { text: String, mode: SongImportMode }
            let Import { text, mode } = parse(body)?;
            let (id, title, duplicate_of) = tryr!(app.import_song(&text, mode));
            Ok(json!({ "id": id, "title": title, "duplicate_of": duplicate_of }))
        }
        // ---- service ----
        "service/add" => {
            #[derive(Deserialize)] struct Add { item: Value, position: Option<usize> }
            let Add { mut item, position } = parse(body)?;
            if item.get("id").is_none() { item["id"] = json!(Uuid::new_v4()); }
            // A passage added without a choice gets the church's verses per slide (Settings → Look).
            if item.get("kind").and_then(Value::as_str) == Some("scripture") && item.get("per_slide").is_none() {
                item["per_slide"] = json!(app.data.lock().unwrap().settings.bible_per_slide);
            }
            // ... and the church's Also Bibles (Settings → Bible), looked up here.
            let fill_also = item.get("kind").and_then(Value::as_str) == Some("scripture") && item.get("also").is_none();
            let mut item: ServiceItem = serde_json::from_value(item).map_err(|e| ActionError::bad(e.to_string()))?;
            let mut toast = None;
            if fill_also { toast = app.fill_also(&mut item).await; }
            refuse_blocked(&item)?;
            item.id = Uuid::new_v4();
            let id = item.id;
            tryr!(app.edit(|d| {
                let s = d.service_mut().ok_or("No service is open.")?;
                let pos = position.unwrap_or(s.items.len()).min(s.items.len());
                s.items.insert(pos, item);
                Ok(())
            }));
            Ok(json!({ "id": id, "toast": toast }))
        }
        "service/update" => {
            let item: ServiceItem = parse(body)?;
            refuse_blocked(&item)?;
            tryr!(app.edit(|d| {
                let s = d.service_mut().ok_or("No service is open.")?;
                let slot = s.items.iter_mut().find(|i| i.id == item.id).ok_or("That item is gone.")?;
                *slot = item;
                Ok(())
            }));
            Ok(json!({}))
        }
        "service/remove" => {
            let Id { id } = parse(body)?;
            tryr!(app.edit(|d| { if let Some(s) = d.service_mut() { s.items.retain(|i| i.id != id); } Ok(()) }));
            Ok(json!({}))
        }
        "service/move" => {
            #[derive(Deserialize)] struct Mv { id: Uuid, to: usize }
            let Mv { id, to } = parse(body)?;
            tryr!(app.edit(|d| {
                let s = d.service_mut().ok_or("No service is open.")?;
                let from = s.items.iter().position(|i| i.id == id).ok_or("That item is gone.")?;
                let it = s.items.remove(from);
                s.items.insert(to.min(s.items.len()), it);
                Ok(())
            }));
            Ok(json!({}))
        }
        "service/new" => {
            #[derive(Deserialize)] struct N { name: String, copy_from: Option<Uuid> }
            let N { name, copy_from } = parse(body)?;
            let id = tryr!(app.edit(|d| {
                let mut s = Service::new(if name.trim().is_empty() { "New Service" } else { name.trim() });
                if let Some(src) = copy_from.and_then(|c| d.services.iter().find(|x| x.id == c)) {
                    s.items = src.items.iter().cloned().map(|mut i| { i.id = Uuid::new_v4(); i }).collect();
                }
                let id = s.id;
                d.services.push(s);
                d.current_service = Some(id);
                Ok(id)
            }));
            Ok(json!({ "id": id }))
        }
        "service/select" => {
            let Id { id } = parse(body)?;
            tryr!(app.edit(|d| { if !d.services.iter().any(|s| s.id == id) { return Err("That service is gone.".into()); } d.current_service = Some(id); Ok(()) }));
            Ok(json!({}))
        }
        "service/rename" => {
            #[derive(Deserialize)] struct R { id: Uuid, name: String }
            let R { id, name } = parse(body)?;
            tryr!(app.edit(|d| { if let Some(s) = d.services.iter_mut().find(|s| s.id == id) { s.name = name; } Ok(()) }));
            Ok(json!({}))
        }
        "service/delete" => {
            let Id { id } = parse(body)?;
            tryr!(app.edit(|d| {
                if d.services.len() <= 1 { return Err("You need at least one service.".into()); }
                d.services.retain(|s| s.id != id);
                if d.current_service == Some(id) { d.current_service = d.services.first().map(|s| s.id); }
                Ok(())
            }));
            Ok(json!({}))
        }
        // ---- bible ----
        "bible/lookup" => {
            #[derive(Deserialize)] struct L { reference: String, translation: String, #[serde(default)] also: Vec<String> }
            let L { reference, translation, also } = parse(body)?;
            // Built in, then a file the church imported (works offline, wins over any online source), then online.
            let (label, verses) = tryr!(app.lookup_passage(&translation, &reference).await);
            // More Bibles shown with it (Nov 5 spec §3): one that cannot be looked up is left out, never the passage.
            let (also, left_out) = app.lookup_also(&translation, &also, &reference).await;
            Ok(json!({ "label": label, "verses": verses, "translation": translation, "also": also, "left_out": left_out }))
        }
        "bible/paste" => {
            // Verses typed or pasted from the church's own Bible, for a translation with no source here (e.g. HCSB).
            #[derive(Deserialize)] struct P { reference: String, translation: String, text: String }
            let P { reference, translation, text } = parse(body)?;
            let (label, verses) = tryr!(bible::pasted(&reference, &text));
            if bible::blocklist::blocked_passage(translation.trim(), &verses) { return Err(ActionError::bad(BLOCKED)); }
            Ok(json!({ "label": label, "verses": verses, "translation": bible::normalise_id(&translation) }))
        }
        "bible/key" => {
            // An empty key removes it. A non-empty key is checked with one lookup before it is saved.
            #[derive(Deserialize)] struct K { provider: String, key: String }
            let K { provider, key } = parse(body)?;
            let p = bible::sources::Provider::from_key_name(&provider).ok_or_else(|| ActionError::bad("Unknown Bible service."))?;
            tryr!(bible::sources::set_key(&app.store.root, p, &key).await);
            app.refresh_bible_credits();
            app.broadcast_data();
            Ok(json!({}))
        }
        // The body is the raw file. `bible/import/-` keeps the file's own short name.
        a if a.starts_with("bible/import/") => {
            let abbr = a["bible/import/".len()..].trim_matches('-').to_string();
            let (root, bytes) = (app.store.root.clone(), body.to_vec());
            let b = tokio::task::spawn_blocking(move || bible::import::import(&root, &bytes, &abbr)).await.map_err(|e| ActionError::bad(e.to_string()))?;
            let b = tryr!(b);
            app.refresh_bible_credits();
            app.broadcast_data();
            Ok(json!({ "abbreviation": b.abbreviation, "name": b.name, "copyright": b.copyright, "verses": b.len() }))
        }
        "bible/catalog" => {
            #[derive(Deserialize, Default)] #[serde(default)] struct C { force: bool }
            let C { force } = if body.is_empty() { C::default() } else { parse(body)? };
            Ok(tryr!(app.bible_catalog(force).await))
        }
        "bible/install" => {
            let T { translation } = parse(body)?;
            let m = tryr!(app.install_bible(&translation).await);
            Ok(json!({ "id": m.id, "abbr": m.abbr, "name": m.name }))
        }
        "bible/import-remove" => {
            let T { translation } = parse(body)?;
            tryr!(bible::import::remove(&app.store.root, &translation).map_err(|e| e.to_string()));
            app.refresh_bible_credits();
            app.broadcast_data();
            Ok(json!({}))
        }
        // ---- media & decks ----
        "media/rename" => {
            #[derive(Deserialize)] struct R { id: Uuid, name: String }
            let R { id, name } = parse(body)?;
            tryr!(app.edit(|d| { if let Some(m) = d.media.iter_mut().find(|m| m.id == id) { m.name = name; } Ok(()) }));
            Ok(json!({}))
        }
        "media/folder" => {
            #[derive(Deserialize)] struct F { id: Uuid, folder: String }
            let F { id, folder } = parse(body)?;
            let folder = folder.trim().to_string();
            tryr!(app.edit(|d| {
                let m = d.media.iter_mut().find(|m| m.id == id).ok_or("That file is gone.")?;
                m.folder = folder.clone();
                if !folder.is_empty() && !d.media_folders.contains(&folder) { d.media_folders.push(folder); }
                Ok(())
            }));
            Ok(json!({}))
        }
        "media/folder-new" => {
            #[derive(Deserialize)] struct F { name: String }
            let F { name } = parse(body)?;
            let name = name.trim().to_string();
            if name.is_empty() { return Err(ActionError::bad("Give the folder a name.")); }
            tryr!(app.edit(|d| {
                if !d.media_folders.iter().any(|f| f.eq_ignore_ascii_case(&name)) { d.media_folders.push(name); }
                Ok(())
            }));
            Ok(json!({}))
        }
        "media/folder-delete" => {
            #[derive(Deserialize)] struct F { name: String }
            let F { name } = parse(body)?;
            // The files stay in the library; only the folder goes.
            tryr!(app.edit(|d| {
                d.media_folders.retain(|f| *f != name);
                for m in d.media.iter_mut().filter(|m| m.folder == name) { m.folder.clear(); }
                Ok(())
            }));
            Ok(json!({}))
        }
        "media/delete" => {
            let Id { id } = parse(body)?;
            tryr!(app.edit(|d| {
                let used = d.services.iter().any(|s| s.items.iter().any(|i| matches!(i.kind, ItemKind::Media { media_id, .. } if media_id == id)));
                if used { return Err("This is in a service. Remove it from the service first.".into()); }
                d.media.retain(|m| m.id != id);
                Ok(())
            }));
            Ok(json!({}))
        }
        "media/reprobe" => {
            let Id { id } = parse(body)?;
            import::media::reprobe(app.clone(), vec![id]);
            Ok(json!({}))
        }
        // Local automation and tests. Never on the phone's list (`server::remote_command`).
        "import/path" => {
            #[derive(Deserialize)] struct P { path: PathBuf, purpose: String }
            let P { path, purpose } = parse(body)?;
            let purpose = match purpose.as_str() {
                "media" => ImportPurpose::Media,
                "asset" => ImportPurpose::Asset,
                "powerpoint" => ImportPurpose::PowerPoint,
                "lyrics" => ImportPurpose::Lyrics,
                other => return Err(ActionError::bad(format!("Bad request: unknown purpose {other}"))),
            };
            Ok(match app.import_path(&path, purpose, |_| {}).await? {
                Imported::Media(m) => json!({ "media": m }),
                Imported::Asset(asset) => json!({ "asset": asset }),
                Imported::Job(job) => json!({ "job": job }),
                Imported::Song { id, title, duplicate_of } => json!({ "id": id, "title": title, "duplicate_of": duplicate_of }),
            })
        }
        "deck/from-images" => {
            #[derive(Deserialize)] struct D { title: String, assets: Vec<AssetId> }
            let D { title, assets } = parse(body)?;
            if assets.is_empty() { return Err(ActionError::bad("Choose at least one picture.")); }
            for a in &assets { if !app.store.asset_path(a).exists() { return Err(ActionError::bad("A picture is missing. Try uploading again.")); } }
            let id = Uuid::new_v4();
            tryr!(app.edit(|d| { d.decks.push(Deck { id, title, slides: assets }); Ok(()) }));
            Ok(json!({ "id": id }))
        }
        "deck/rename" => {
            #[derive(Deserialize)] struct R { id: Uuid, title: String }
            let R { id, title } = parse(body)?;
            tryr!(app.edit(|d| { if let Some(x) = d.decks.iter_mut().find(|x| x.id == id) { x.title = title; } Ok(()) }));
            Ok(json!({}))
        }
        "deck/delete" => {
            let Id { id } = parse(body)?;
            tryr!(app.edit(|d| {
                let used = d.services.iter().any(|s| s.items.iter().any(|i| matches!(i.kind, ItemKind::Deck { deck_id } if deck_id == id)));
                if used { return Err("These slides are in a service. Remove them from the service first.".into()); }
                d.decks.retain(|x| x.id != id);
                Ok(())
            }));
            Ok(json!({}))
        }
        "job/cancel" => {
            let Id { id } = parse(body)?;
            app.job_cancel(id);
            Ok(json!({}))
        }
        "job/dismiss" => {
            let Id { id } = parse(body)?;
            app.jobs.lock().unwrap().retain(|j| j.id != id);
            app.broadcast_data();
            Ok(json!({}))
        }
        // ---- settings ----
        "settings" => {
            let mut s: Settings = parse(body)?;
            s.lines_per_slide = s.lines_per_slide.clamp(1, 8);
            s.theme.size = s.theme.size.clamp(24, 200);
            s.theme.scripture_size = s.theme.scripture_size.clamp(24, 200);
            s.bible_per_slide = s.bible_per_slide.min(4);
            // The picker owns these (`settings/translation`, `settings/favourite`): a form saved from an older copy
            // must not undo a star or a pick made while it was open.
            tryr!(app.edit(|d| {
                s.favourites = std::mem::take(&mut d.settings.favourites);
                s.recent = std::mem::take(&mut d.settings.recent);
                // Settings → Language applies at once (`settings/language`); an older form must not undo it.
                s.ui_language = std::mem::take(&mut d.settings.ui_language);
                d.settings = s;
                Ok(())
            }));
            Ok(json!({}))
        }
        "settings/translation" => {
            #[derive(Deserialize)] struct T { translation: String }
            let T { translation } = parse(body)?;
            if bible::blocklist::blocked(&translation, &[], &[], &[&translation]) { return Err(ActionError::bad(BLOCKED)); }
            if !app.translations().contains(&translation) { return Err(ActionError::bad("Unknown translation.")); }
            // A main Bible that was also an Also Bible stops being one (Nov 5 spec §3).
            let turned_off = tryr!(app.edit(|d| {
                d.settings.used_translation(&translation);
                let was = d.settings.also_show.iter().any(|t| bible::id::same_id(t, &translation));
                d.settings.also_show.retain(|t| !bible::id::same_id(t, &translation));
                d.settings.translation = translation.clone();
                Ok(was)
            }));
            let toast = turned_off.then(|| crate::scripture::strings::also_turned_off(&app.abbr_of(&translation)));
            Ok(json!({ "toast": toast }))
        }
        // The app's language (I18N spec D6): `""` follows the computer. Rust text changes at once; windows follow
        // on their thread (`i18n::sync_slint`). The toast is in the new language.
        "settings/language" => {
            #[derive(Deserialize)] struct L { code: String }
            let L { code } = parse(body)?;
            if !code.is_empty() && !crate::i18n::is_known(&code) { return Err(ActionError::bad(crate::tr!("Unknown language."))); }
            // Switched before the save, so the update the save sends finds the new language.
            let before = crate::i18n::current();
            let _ = crate::i18n::set_language(&crate::i18n::resolve(&code));
            if let Err(e) = app.edit(|d| { d.settings.ui_language = code.clone(); Ok(()) }) {
                let _ = crate::i18n::set_language(&before);
                return Err(ActionError::bad(e));
            }
            Ok(json!({ "toast": crate::tr!("Language changed") }))
        }
        // The church's Also Bibles for new passages and how they share a slide (Nov 5 spec §2–3).
        "settings/also" => {
            #[derive(Deserialize)] struct A { also_show: Vec<String>, #[serde(default)] parallel_layout: crate::scripture::ParallelLayout }
            let A { mut also_show, parallel_layout } = parse(body)?;
            let known = app.translations();
            if also_show.iter().any(|t| bible::blocklist::blocked(t, &[], &[], &[t])) { return Err(ActionError::bad(BLOCKED)); }
            if also_show.iter().any(|t| !known.contains(t)) { return Err(ActionError::bad("Unknown translation.")); }
            tryr!(app.edit(|d| {
                crate::scripture::also::tidy_ids(&d.settings.translation, &mut also_show, &|_| false);
                d.settings.also_show = also_show;
                d.settings.parallel_layout = parallel_layout;
                Ok(())
            }));
            Ok(json!({}))
        }
        // The picker's star. A Bible no longer installed can still be unstarred.
        "settings/favourite" => {
            #[derive(Deserialize)] struct F { translation: String, on: bool }
            let F { translation, on } = parse(body)?;
            if on {
                if bible::blocklist::blocked(&translation, &[], &[], &[&translation]) { return Err(ActionError::bad(BLOCKED)); }
                if !app.translations().contains(&translation) { return Err(ActionError::bad("Unknown translation.")); }
            }
            tryr!(app.edit(|d| { d.settings.set_favourite(&translation, on); Ok(()) }));
            Ok(json!({}))
        }
        // ---- displays ----
        "display/open" => {
            #[derive(Deserialize)] struct O { role: String, monitor: Option<String> }
            let O { role, monitor } = parse(body)?;
            if !matches!(role.as_str(), "room" | "stage" | "operator") { return Err(ActionError::bad("Unknown screen.")); }
            if role == "operator" { tryr!(app.open_window(&role, None)) } else {
                let target = monitor.map_or(ScreenTarget::Window, ScreenTarget::Monitor);
                let group = if role == "room" { "audience" } else { "stage" };
                tryr!(app.edit(|d| { d.settings.screens.assign(group, target); Ok(()) }));
                tryr!(app.sync_screens());
            }
            app.broadcast_data();
            Ok(json!({}))
        }
        "display/close" => {
            #[derive(Deserialize)] struct C { role: String }
            let C { role } = parse(body)?;
            if !matches!(role.as_str(), "room" | "stage" | "operator") { return Err(ActionError::bad("Unknown screen.")); }
            if role == "operator" { app.close_window(&role) } else {
                let group = if role == "room" { "audience" } else { "stage" };
                tryr!(app.edit(|d| { d.settings.screens.group_mut(group).ok_or("Unknown screen.")?.on = false; Ok(()) }));
                tryr!(app.sync_screens());
            }
            app.broadcast_data();
            Ok(json!({}))
        }
        // ---- the screen setup ----
        "screens/power" => {
            #[derive(Deserialize)] struct P { group: String, on: bool }
            let P { group, on } = parse(body)?;
            let free = app.free_monitor(&app.data.lock().unwrap().settings.screens);
            tryr!(app.edit(|d| {
                let g = d.settings.screens.group_mut(&group).ok_or("Unknown screen.")?;
                // The first switch-on just works when a screen is free for it.
                if on && g.screens.iter().all(|s| s.output().target == ScreenTarget::None) {
                    let free = free.ok_or("Pick an output for this screen first.")?;
                    let first = g.screens.first_mut().ok_or("Add a screen first.")?;
                    *first.output_mut() = ScreenOutput { target: ScreenTarget::Monitor(free.id), width: free.width.max(0) as u32, height: free.height.max(0) as u32 };
                }
                g.on = on;
                Ok(())
            }));
            tryr!(app.sync_screens());
            app.broadcast_data();
            Ok(json!({}))
        }
        "screens/add" => {
            #[derive(Deserialize)] struct A { group: String }
            let A { group } = parse(body)?;
            let id = tryr!(app.edit(|d| {
                let g = d.settings.screens.group_mut(&group).ok_or("Unknown screen.")?;
                let n = g.screens.len() + 1;
                let title = if group == "audience" { "Audience" } else { "Stage Display" };
                let name = (n..).map(|i| format!("{title} {i}")).find(|name| !g.screens.iter().any(|s| s.name == *name)).unwrap_or_default();
                let screen = Screen::new(Uuid::new_v4(), &name);
                let id = screen.id;
                g.screens.push(screen);
                Ok(id)
            }));
            Ok(json!({ "id": id }))
        }
        "screens/remove" => {
            #[derive(Deserialize)] struct R { id: Uuid }
            let R { id } = parse(body)?;
            tryr!(app.edit(|d| {
                let s = &mut d.settings.screens;
                let (group, _) = s.find(id).ok_or("That screen is gone.")?;
                let g = s.group_mut(group).ok_or("Unknown screen.")?;
                if g.screens.len() < 2 { return Err("Each list keeps at least one screen.".into()) }
                g.screens.retain(|s| s.id != id);
                Ok(())
            }));
            tryr!(app.sync_screens());
            app.broadcast_data();
            Ok(json!({}))
        }
        // Any of the name, where it shows and the size of a window. One monitor shows one screen, so taking a monitor
        // takes it from the screen that had it.
        "screens/update" => {
            #[derive(Deserialize)] struct U { id: Uuid, name: Option<String>, target: Option<ScreenTarget>, size: Option<(u32, u32)> }
            let U { id, name, target, size } = parse(body)?;
            let move_windows = target.is_some() || size.is_some();
            let monitors = app.monitors();
            tryr!(app.edit(|d| {
                let setup = &mut d.settings.screens;
                if let Some(ScreenTarget::Monitor(m)) = &target {
                    for g in [&mut setup.audience, &mut setup.stage] {
                        for s in g.screens.iter_mut().filter(|s| s.id != id) {
                            if s.output().target == ScreenTarget::Monitor(m.clone()) { s.output_mut().target = ScreenTarget::None }
                        }
                    }
                }
                let screen = setup.find_mut(id).ok_or("That screen is gone.")?;
                if let Some(name) = name.as_deref().map(str::trim).filter(|n| !n.is_empty()) { screen.name = name.into() }
                let out = screen.output_mut();
                if let Some(t) = target {
                    if let ScreenTarget::Monitor(m) = &t && let Some(m) = monitors.iter().find(|x| x.id == *m) {
                        (out.width, out.height) = (m.width.max(0) as u32, m.height.max(0) as u32);
                    }
                    out.target = t;
                }
                if let Some((w, h)) = size { (out.width, out.height) = (w.clamp(320, 7680), h.clamp(240, 4320)) }
                Ok(())
            }));
            if move_windows { tryr!(app.sync_screens()) }
            app.broadcast_data();
            Ok(json!({}))
        }
        "screens/identify" => {
            app.identify_screens();
            Ok(json!({}))
        }
        "stage/message" => {
            #[derive(Deserialize)] struct M { text: String }
            let M { text } = parse(body)?;
            *app.stage_message.lock().unwrap() = text.trim().to_string();
            app.broadcast_live();
            Ok(json!({}))
        }
        // ---- backups ----
        "backup/now" => {
            let data = app.data.lock().unwrap().clone();
            // Copies every asset: keep it off the runtime's worker threads.
            let a = app.clone();
            let name = tokio::task::spawn_blocking(move || a.store.backup(&data)).await
                // Only a panic inside the backup lands here: a server fault, not the user's.
                .map_err(|_| ActionError { status: 500, message: "Backup failed.".into() })?;
            let name = tryr!(name.map_err(|e| e.to_string()));
            app.broadcast_data();
            Ok(json!({ "name": name }))
        }
        "backup/restore" => {
            #[derive(Deserialize)] struct B { name: String }
            let B { name } = parse(body)?;
            let mut restored = tryr!(app.store.restore(&name));
            let removed = bible::blocklist::strip(&mut restored);
            if removed > 0 {
                tryr!(app.save(&restored));
                app.live.lock().unwrap().notice = Some(passages_removed(removed));
            }
            *app.data.lock().unwrap() = restored;
            app.refresh_bible_credits();
            app.broadcast_data();
            app.broadcast_live();
            Ok(json!({}))
        }
        // ---- phone remote ----
        "pair/start" => {
            let (pin, secret) = (random_pin(), random_token());
            let url = app.lan_url.clone().unwrap_or_default();
            let qr = (!url.is_empty()).then(|| pair_qr_svg(&pair_link(&url, &secret))).flatten();
            *app.pairing.lock().unwrap() = Some(Pairing { pin: pin.clone(), secret, until: Instant::now() + Duration::from_secs(120), failures: 0 });
            Ok(json!({ "pin": pin, "url": url, "qr": qr, "seconds": 120 }))
        }
        "pair/stop" => { *app.pairing.lock().unwrap() = None; Ok(json!({})) }
        "phone/revoke" => {
            let Id { id } = parse(body)?;
            let mut phones = app.phones.lock().unwrap();
            phones.retain(|p| p.id != id);
            app.save_phones(&phones);
            drop(phones);
            let _ = app.tx.send(Update::Revoked { phone: id });
            app.broadcast_data();
            Ok(json!({}))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}
}

/// What a file is being imported for. Picks what `App::import_staged` does with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportPurpose { Media, Asset, PowerPoint, Lyrics }

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

/// What importing another program's song library did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LibraryImport {
    pub added: usize,
    /// Titles of the songs already in the library, which were left as they are.
    pub duplicates: Vec<String>,
    /// (title, reason) for what could not become a song.
    pub skipped: Vec<(String, String)>,
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
            let disk_full = |_| "Could not save the file (disk full?).".to_string();
            let mut input = std::fs::File::open(&src).map_err(|e| format!("Could not open “{shown}”: {e}"))?;
            let total = input.metadata().map(|m| m.len()).unwrap_or(0);
            let mut out = std::fs::File::create(&dst).map_err(disk_full)?;
            let (mut hasher, mut buf, mut done, mut last) = (Sha256::new(), vec![0u8; 1 << 16], 0u64, 0u8);
            progress(0);
            let copied = (|| -> Result<(), String> {
                loop {
                    let n = input.read(&mut buf).map_err(|e| format!("Could not read “{shown}”: {e}"))?;
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
        let ext = crate::store::sanitize_ext(name.rsplit('.').next().unwrap_or(""));
        let base_name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let display = base_name.rsplit_once('.').map(|(a, _)| a).unwrap_or(base_name).to_string();

        // Anything else dropped on the Media tab goes to the probe, which decides what it is.
        let purpose = if purpose == ImportPurpose::Media && ext == "txt" { ImportPurpose::Lyrics } else { purpose };
        match purpose {
            ImportPurpose::Lyrics => {
                let bytes = tokio::fs::read(&tmp).await;
                let _ = tokio::fs::remove_file(&tmp).await;
                let bytes = bytes.map_err(|e| ActionError::bad(e.to_string()))?;
                let (id, title, duplicate_of) = app.import_song(&String::from_utf8_lossy(&bytes), SongImportMode::Ask).map_err(ActionError::bad)?;
                return Ok(Imported::Song { id, title, duplicate_of });
            }
            ImportPurpose::PowerPoint => return Ok(Imported::Job(import::deck::start(app.clone(), tmp, display))),
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
        let data = self.data.lock().unwrap().clone();
        let app = self.clone();
        let check = tokio::task::spawn_blocking(move || bundle::gather(&data, id, &|a| app.store.asset_path(a).is_file()));
        let (contents, assets) = check.await.map_err(|e| ExportError::Failed(e.to_string()))?.map_err(ExportError::Missing)?;
        let (job, cancel) = self.job_start_cancellable(bundle::strings::SAVING);
        let app = self.clone();
        let work = tokio::task::spawn_blocking(move || bundle::write(&app.store, &contents, &assets, &dest, &cancel, &|p| app.job_progress(job, p)));
        self.job_finished(job, work.await.map_err(|e| e.to_string()).and_then(|r| r)).map_err(ExportError::Failed)
    }

    /// Opens a saved service file, adds what is new to the library, and opens the service (spec §11). A bad file
    /// adds nothing and its error stays on the job.
    pub async fn import_service(self: &Arc<Self>, path: PathBuf) -> Result<crate::bundle::Opened, String> {
        use crate::bundle;
        let (job, cancel) = self.job_start_cancellable(bundle::strings::OPENING);
        let app = self.clone();
        let work = tokio::task::spawn_blocking(move || bundle::read(&app.store, &path, &cancel, &|p| app.job_progress(job, p)));
        let contents = self.job_finished(job, work.await.map_err(|e| e.to_string()).and_then(|r| r))?;
        self.edit(|d| Ok(bundle::merge(d, contents)))
    }

    /// The eBible catalog as Settings → Bibles shows it, with what is installed and what has a newer edition.
    /// Asks eBible at most once a day unless `force`.
    pub async fn bible_catalog(&self, force: bool) -> Result<serde_json::Value, String> {
        use bible::catalog::{CatalogSource, Ctx, LicenceState};
        let mut ctx = Ctx::new(self.store.root.clone(), CatalogSource::Ebible);
        ctx.force = force;
        let listing = CatalogSource::Ebible.refresh(&ctx).await.map_err(|e| e.message())?;
        let installed = bible::import::list(&self.store.root);
        // Grouped by language, the church's first. There is no church language setting yet, so English.
        let all: Vec<&bible::catalog::CatalogRow> = listing.rows.iter().collect();
        let groups = bible::catalog::by_language(&all, "eng");
        let rows: Vec<serde_json::Value> = groups.iter().flat_map(|g| g.rows.iter().copied()).map(|r| {
            let have = installed.iter().find(|m| m.id == r.id());
            let update = have.is_some_and(|m| !matches!(&m.source, bible::library::Source::Ebible { update_date } if update_date == r.update_date()));
            json!({
                "id": r.id(), "abbr": r.abbr(), "title": r.title(), "lang": r.lang(), "lang_name": r.lang_name(),
                "lang_name_en": r.lang_name_en(), "scope": r.scope().label(), "apocrypha": r.has_dc(),
                "licence_on_download": *r.licence() == LicenceState::OnDownload, "installed": have.is_some(), "update": update,
            })
        }).collect();
        Ok(json!({ "offline": listing.offline, "rows": rows }))
    }

    /// Downloads a Bible from the eBible catalog, or updates the installed copy, as a job with Cancel. Downloads
    /// wait their turn. A failed update leaves the installed copy working.
    pub async fn install_bible(self: &Arc<Self>, id: &str) -> Result<bible::library::BibleMeta, String> {
        use bible::catalog::{CatalogSource, Ctx, strings};
        let ctx = Ctx::new(self.store.root.clone(), CatalogSource::Ebible);
        let listing = CatalogSource::Ebible.refresh(&ctx).await.map_err(|e| e.message())?;
        if listing.offline { return Err(strings::OFFLINE_NO_ADD.into()); }
        let row = listing.rows.into_iter().find(|r| r.id() == id).ok_or(strings::NOT_IN_LIST)?;
        let name = row.title().to_string();
        let update = bible::import::meta(&self.store.root, id).is_some();
        let (job, cancel) = self.job_start_cancellable(&strings::WAITING.replace("{name}", &name));
        self.job_subject(job, id);
        let turn = tokio::select! {
            t = self.bible_downloads.lock() => Some(t),
            _ = cancel.cancelled() => None,
        };
        let Some(_turn) = turn else { self.job_end_cancelled(job); return Err(import::CANCELLED.into()) };
        self.job_label(job, &strings::DOWNLOADING.replace("{name}", &name));
        let app = self.clone();
        let r = row.source().install(&ctx, &row, &cancel, &move |p| app.job_progress(job, p)).await
            .map_err(|e| { eprintln!("Bible download {id}: {e:?}"); e.message(&name, update) });
        let r = self.job_finished(job, r);
        if r.is_ok() { self.refresh_bible_credits(); self.broadcast_data(); }
        r
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

    /// Reads an EasyWorship library and saves its new songs in one step (spec §9.3).
    pub async fn import_easyworship(&self, folder: PathBuf) -> Result<LibraryImport, String> {
        let read = tokio::task::spawn_blocking(move || crate::import::easyworship::read_library(&folder));
        let crate::import::ImportReport { songs, skipped } = read.await.map_err(|e| e.to_string())??;
        self.edit(|d| {
            let marks = crate::import::classify(&d.songs, &songs);
            let mut done = LibraryImport { skipped, ..Default::default() };
            for (song, mark) in songs.into_iter().zip(marks) {
                if mark == crate::import::Dedupe::New { d.songs.push(song); done.added += 1 } else { done.duplicates.push(song.title) }
            }
            Ok(done)
        })
    }

    /// Import a SongSelect lyric file. Returns the saved song's id (`None` if `Ask` found a match), its title,
    /// and the library song with the same CCLI #. `Update` with no match adds the song: the old one may have
    /// been deleted while the question was open.
    fn import_song(&self, text: &str, mode: SongImportMode) -> Result<(Option<Uuid>, String, Option<Uuid>), String> {
        let song = songselect::import_txt(text)?;
        let title = song.title.clone();
        // A question saves nothing, so it neither writes data.json nor tells the screens.
        if mode == SongImportMode::Ask {
            let data = self.data.lock().unwrap();
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
            Err(ActionError::bad(BLOCKED)),
        _ => Ok(()),
    }
}

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ActionError> {
    serde_json::from_slice(body).map_err(|e| ActionError::bad(format!("Bad request: {e}")))
}

#[cfg(test)]
pub(crate) fn test_app() -> Arc<App> {
    let root = std::env::temp_dir().join(format!("rp-test-{}", Uuid::new_v4()));
    let store = Store::open(root).unwrap();
    let data = store.load().unwrap();
    App::new(store, data, 0, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<F: std::future::Future>(f: F) -> F::Output { tokio::runtime::Runtime::new().unwrap().block_on(f) }

    #[tokio::test]
    async fn a_passage_added_without_a_choice_gets_the_settings_verses_per_slide() {
        let app = test_app();
        app.data.lock().unwrap().settings.bible_per_slide = 3;
        let add = |per_slide: &str| format!(r#"{{"item":{{"kind":"scripture","reference":"John 3:16","translation":"KJV","verses":[]{per_slide}}}}}"#);
        app.do_action("service/add", add("").as_bytes()).await.unwrap();
        app.do_action("service/add", add(r#","per_slide":1"#).as_bytes()).await.unwrap();
        let d = app.data.lock().unwrap();
        let per: Vec<u8> = d.services[0].items.iter().map(|i| match i.kind { ItemKind::Scripture { per_slide, .. } => per_slide, _ => 9 }).collect();
        assert_eq!(per, [3, 1], "a choice the client made is kept");
    }

    #[tokio::test]
    async fn do_action_saves_a_song_and_returns_its_id() {
        let app = test_app();
        let v = app.do_action("song/save", br#"{"id":"00000000-0000-0000-0000-000000000000","title":"Amazing Grace","lyrics":"Verse 1\nAmazing grace"}"#).await.unwrap();
        let id: Uuid = serde_json::from_value(v["id"].clone()).unwrap();
        assert!(!id.is_nil());
        assert_eq!(app.data.lock().unwrap().songs[0].title, "Amazing Grace");
    }

    #[tokio::test]
    async fn do_action_rejects_untitled_song_with_400() {
        let err = test_app().do_action("song/save", br#"{"id":"00000000-0000-0000-0000-000000000000","title":" ","lyrics":""}"#).await.unwrap_err();
        assert_eq!((err.status, err.message.as_str()), (400, "Give the song a title."));
    }

    fn on_air(app: &App) {
        let id = Uuid::new_v4();
        let mut live = app.live.lock().unwrap();
        struct One(Uuid);
        impl Plan for One {
            fn order(&self) -> Vec<Uuid> { vec![self.0] }
            fn build(&self, id: Uuid) -> Option<LiveItem> {
                Some(LiveItem { item_id: id, title: "T".into(), kind: "song".into(), background: Background::Color { color: "#000".into() },
                    slides: vec![Slide { label: "s".into(), content: SlideContent::Text { text: "hello".into(), footer: None, reference: None, size: 0, parallel: None }, notes: String::new() }] })
            }
        }
        live.apply(Envelope { id: id.to_string(), base: None, cmd: Command::Show { item_id: id, slide: 0 } }, &One(id));
    }

    fn wait_for_state(root: &std::path::Path, seq: u64) -> state::LiveState {
        for _ in 0..200 {
            if let Some(s) = state::load(root) && s.seq >= seq { return s; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("no saved state with seq {seq}");
    }

    #[test]
    fn app_writes_state_after_commit() {
        let app = test_app();
        app.set_state_writer(state::StateWriter::start(app.store.root.clone()));
        on_air(&app);
        let version = app.live.lock().unwrap().version;
        assert_eq!(state::load(&app.store.root), None, "nothing is on air yet");
        app.room_event(RoomEvent::Committed { version });
        let s = wait_for_state(&app.store.root, 1);
        assert!(matches!(s.frame.slide, Some(crate::live::SlideLayer::Text { ref text, .. }) if text == "hello"));
        assert!(s.saved_at.len() > 8);
    }

    #[test]
    fn a_report_that_commits_nothing_saves_nothing() {
        let app = test_app();
        app.set_state_writer(state::StateWriter::start(app.store.root.clone()));
        on_air(&app);
        let version = app.live.lock().unwrap().version;
        app.room_event(RoomEvent::Committed { version });
        app.room_event(RoomEvent::Committed { version: 99 });
        app.room_event(RoomEvent::Committed { version });
        app.stop_state_writer();
        assert_eq!(state::load(&app.store.root).unwrap().seq, 1);
    }

    #[test]
    fn saving_records_the_open_windows_and_a_newer_seq() {
        struct Roles;
        impl Screens for Roles {
            fn open(&self, _: &str, _: Option<&str>) -> Result<(), String> { Ok(()) }
            fn close(&self, _: &str) {}
            fn open_roles(&self) -> Vec<String> { vec!["room".into(), "stage".into()] }
            fn monitors(&self) -> Vec<Monitor> { vec![] }
        }
        let app = test_app();
        app.set_screens(Arc::new(Roles));
        app.set_state_writer(state::StateWriter::start(app.store.root.clone()));
        on_air(&app);
        let version = app.live.lock().unwrap().version;
        app.room_event(RoomEvent::Committed { version });
        let first = wait_for_state(&app.store.root, 1);
        app.save_live_state();
        let second = wait_for_state(&app.store.root, first.seq + 1);
        assert_eq!(second.windows, ["room", "stage"]);
    }

    #[test]
    fn saving_with_nothing_committed_writes_nothing() {
        let app = test_app();
        app.set_state_writer(state::StateWriter::start(app.store.root.clone()));
        app.save_live_state();
        app.stop_state_writer();
        assert_eq!(state::load(&app.store.root), None);
    }

    #[test]
    fn playing_means_committed_video_or_audio() {
        let app = test_app();
        let item_id = Uuid::new_v4();
        let kind = ItemKind::Text { title: "t".into(), body: "text".into() };
        app.data.lock().unwrap().services[0].items.push(ServiceItem { id: item_id, kind, background: None, notes: String::new() });
        let version = app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::Show { item_id, slide: 0 } }).version;
        app.room_event(RoomEvent::Committed { version });
        assert!(!app.playing(), "a text slide does not play");

        let mut saved = state::snapshot(&app.live.lock().unwrap(), vec![], 1).unwrap();
        saved.frame.audio = Some(Inst { inst: 9, v: AudioLayer { asset: "a.mp3".into(), title: "A".into(), looped: false, paused: false } });
        app.live.lock().unwrap().restore(&saved, true);
        assert!(!app.playing(), "not on air until the room commits");
        let version = app.live.lock().unwrap().version;
        app.room_event(RoomEvent::Committed { version });
        assert!(app.playing());
    }

    #[test]
    fn saving_without_a_writer_does_nothing() {
        let app = test_app();
        on_air(&app);
        let version = app.live.lock().unwrap().version;
        app.room_event(RoomEvent::Committed { version });
        app.save_live_state();
        app.stop_state_writer();
        assert_eq!(state::load(&app.store.root), None);
    }

    #[test]
    fn resume_position_lasts_until_on_air() {
        let app = test_app();
        on_air(&app);
        let (version, inst) = {
            let live = app.live.lock().unwrap();
            (live.version, live.frame.instances().next().expect("the shown frame has an instance"))
        };
        app.set_resume(&[(inst, 12.5), (inst + 100, 3.0)]);
        assert_eq!(app.resume_position(inst), Some(12.5));
        // A newer plan may prepare the same instance again before the first one went on air.
        assert_eq!(app.resume_position(inst), Some(12.5));
        assert_eq!(app.resume_position(inst + 1000), None);
        app.room_event(RoomEvent::Committed { version });
        assert_eq!(app.resume_position(inst), None, "a later replay starts from the beginning");
        assert_eq!(app.resume_position(inst + 100), Some(3.0), "only what went on air is forgotten");
    }

    #[test]
    fn a_stopped_writer_saves_nothing_more() {
        let app = test_app();
        app.set_state_writer(state::StateWriter::start(app.store.root.clone()));
        on_air(&app);
        app.stop_state_writer();
        assert_eq!(state::load(&app.store.root), None, "a requested frame is not saved");
        // A clean quit stops the writer, then clears: a later commit must not bring the file back.
        state::clear(&app.store.root);
        let version = app.live.lock().unwrap().version;
        app.room_event(RoomEvent::Committed { version });
        app.stop_state_writer();
        assert!(!state::path(&app.store.root).exists());
    }

    #[tokio::test]
    async fn set_notice_shows_on_the_live_view() {
        let app = test_app();
        let mut rx = app.subscribe();
        app.set_notice("RustyPresenter restarted after a problem.");
        assert_eq!(app.live.lock().unwrap().notice.as_deref(), Some("RustyPresenter restarted after a problem."));
        assert!(matches!(rx.try_recv().unwrap(), Update::Live(_)));
    }

    #[tokio::test]
    async fn a_live_command_publishes_one_live_update() {
        let app = test_app();
        let mut rx = app.subscribe();
        app.command(Envelope { id: "c1".into(), base: None, cmd: Command::ClearAll });
        assert!(matches!(rx.try_recv().unwrap(), Update::Live(_)));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn live_views_go_out_in_the_order_they_were_read() {
        use tokio::sync::broadcast::error::TryRecvError;
        let app = test_app();
        let mut rx = app.subscribe();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let other = {
            let (app, stop) = (app.clone(), stop.clone());
            std::thread::spawn(move || while !stop.load(Ordering::Relaxed) { app.broadcast_live() })
        };
        let mut newest = 0;
        for i in 0..2000 {
            app.command(Envelope { id: format!("c{i}"), base: None, cmd: Command::Logo });
            loop {
                match rx.try_recv() {
                    Ok(Update::Live(u)) => {
                        assert!(u.version >= newest, "view {} went out after view {newest}", u.version);
                        newest = u.version;
                    }
                    Ok(_) | Err(TryRecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        }
        stop.store(true, Ordering::Relaxed);
        other.join().unwrap();
        assert!(newest > 1000, "only reached version {newest}");
    }

    #[test]
    fn a_live_update_is_serialized_once_however_many_clients_receive_it() {
        let app = test_app();
        let (mut a, mut b) = (app.subscribe(), app.subscribe());
        app.broadcast_live();
        let (Update::Live(x), Update::Live(y)) = (a.try_recv().unwrap(), b.try_recv().unwrap()) else { panic!("expected live updates") };
        // Both receivers read the JSON; the second must get the first one's string, not a fresh serialization.
        assert!(std::ptr::eq(x.json(), y.json()), "each client re-serialized the live view");
        assert!(std::ptr::eq(x.json(), x.json()));
        // The typed view stays available to native screens.
        assert_eq!(x.version, app.live_view().version);
    }

    #[test]
    fn preflight_looks_at_the_real_files() {
        let app = test_app();
        let good = app.store.import_file(&write_temp(b"picture"), "png").unwrap();
        let empty = app.store.import_file(&write_temp(b""), "jpg").unwrap();
        let check = |logo: &str| {
            app.data.lock().unwrap().settings.logo = Some(logo.to_string());
            app.preflight().contains(&crate::preflight::strings::LOGO_MISSING.to_string())
        };
        assert!(!check(&good));
        assert!(check(&empty), "an empty file cannot be shown");
        assert!(check("gone.png"));
    }

    fn write_temp(bytes: &[u8]) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rp-preflight-{}", Uuid::new_v4()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[tokio::test]
    async fn stage_message_goes_to_the_stage_view_at_once_and_trimmed() {
        let app = test_app();
        let mut rx = app.subscribe();
        app.do_action("stage/message", br#"{"text":"  Wrap up, please \n"}"#).await.unwrap();
        assert_eq!(app.live_view().stage_message, "Wrap up, please");
        let Ok(Update::Live(u)) = rx.try_recv() else { panic!("no live update") };
        assert_eq!(u.stage_message, "Wrap up, please");
    }

    #[tokio::test]
    async fn empty_stage_message_clears_it() {
        let app = test_app();
        app.do_action("stage/message", br#"{"text":"Wrap up"}"#).await.unwrap();
        app.do_action("stage/message", br#"{"text":"   "}"#).await.unwrap();
        assert_eq!(app.live_view().stage_message, "");
    }

    #[tokio::test]
    async fn stage_message_never_reaches_the_json_phones_and_pages_read() {
        let app = test_app();
        app.do_action("stage/message", br#"{"text":"Wrap up"}"#).await.unwrap();
        let v = serde_json::to_string(&app.live_view()).unwrap();
        assert!(!v.contains("Wrap up"), "{v}");
        assert!(!app.state_json().to_string().contains("Wrap up"));
    }

    #[test]
    fn live_view_serializes_every_field_the_screens_read() {
        let v = serde_json::to_value(test_app().live_view()).unwrap();
        for k in ["version", "committed", "pending", "frame", "cursor", "item", "slide", "next", "media", "audio", "notice", "theme", "logo"] {
            assert!(v.get(k).is_some(), "{k}");
        }
    }

    #[tokio::test]
    async fn bible_lookup_built_in_and_needs_key() {
        let app = test_app();
        let v = app.do_action("bible/lookup", br#"{"reference":"John 3:16","translation":"BSB"}"#).await.unwrap();
        assert!(v["verses"][0]["text"].as_str().unwrap().starts_with("For God so loved"));
        let err = app.do_action("bible/lookup", br#"{"reference":"John 3:16","translation":"ESV"}"#).await.unwrap_err();
        assert!(err.message.contains("Settings → Bible"), "{}", err.message);
        let s = app.state_json();
        assert_eq!(s["available_translations"], json!(["KJV", "WEB", "BSB"]));
        let esv = s["bible_sources"].as_array().unwrap().iter().find(|b| b["translation"] == "ESV").unwrap();
        assert_eq!(esv["status"], "needs_key");
    }

    #[tokio::test]
    async fn saved_keys_never_reach_the_state() {
        let app = test_app();
        let mut s = bible::sources::Secrets::default();
        s.keys.insert("esv".into(), "secret-key-123".into());
        s.save(&app.store.root).unwrap();
        let state = app.state_json();
        assert!(!state.to_string().contains("secret-key-123"));
        assert!(state["available_translations"].as_array().unwrap().contains(&json!("ESV")));
        let esv = state["bible_providers"].as_array().unwrap().iter().find(|p| p["id"] == "esv").unwrap();
        assert_eq!(esv["key_set"], true);
        assert!(!app.store.data_path().exists() || !std::fs::read_to_string(app.store.data_path()).unwrap().contains("secret-key-123"));
    }

    #[tokio::test]
    async fn bible_key_rejects_unknown_service_and_empty_key_removes() {
        let app = test_app();
        assert_eq!(app.do_action("bible/key", br#"{"provider":"nope","key":"x"}"#).await.unwrap_err().message, "Unknown Bible service.");
        let mut s = bible::sources::Secrets::default();
        s.keys.insert("esv".into(), "k".into());
        s.save(&app.store.root).unwrap();
        app.do_action("bible/key", br#"{"provider":"esv","key":"  "}"#).await.unwrap();
        assert!(bible::sources::Secrets::load(&app.store.root).keys.is_empty());
    }

    #[tokio::test]
    async fn imported_bible_is_looked_up_offline_and_removed() {
        let app = test_app();
        let file = br#"<?xml version="1.0"?><XMLBIBLE biblename="Test"><INFORMATION><title>Holman Test</title><identifier>HCSB</identifier><rights>(c) Holman</rights></INFORMATION>
            <BIBLEBOOK bnumber="43"><CHAPTER cnumber="3"><VERS vnumber="16">For God loved.</VERS><VERS vnumber="17">For God sent.</VERS></CHAPTER></BIBLEBOOK></XMLBIBLE>"#;
        let v = app.do_action("bible/import/HCSB", file).await.unwrap();
        assert_eq!(v["abbreviation"], "HCSB");
        let v = app.do_action("bible/lookup", br#"{"reference":"John 3:16-17","translation":"HCSB"}"#).await.unwrap();
        assert_eq!(v["verses"][1]["text"], "For God sent.");
        let hcsb = app.state_json()["bible_sources"].as_array().unwrap().iter().find(|b| b["translation"] == "HCSB").unwrap().clone();
        assert_eq!((hcsb["status"].as_str(), hcsb["imported"].as_bool()), (Some("offline"), Some(true)));
        assert!(app.do_action("bible/import/KJV", file).await.unwrap_err().message.contains("already built in"));
        app.do_action("bible/import-remove", br#"{"translation":"HCSB"}"#).await.unwrap();
        assert!(app.do_action("bible/lookup", br#"{"reference":"John 3:16","translation":"HCSB"}"#).await.is_err());
    }

    #[tokio::test]
    async fn blocked_translations_are_refused_on_every_action() {
        let app = test_app();
        let refused = |action: &'static str, body: String| { let app = app.clone(); async move { app.do_action(action, body.as_bytes()).await.unwrap_err().message } };
        for t in ["TPT", "nwt", "JST"] {
            assert_eq!(refused("bible/lookup", format!(r#"{{"reference":"John 3:16","translation":"{t}"}}"#)).await, BLOCKED, "{t}");
            assert_eq!(refused("bible/paste", format!(r#"{{"reference":"John 3:16","translation":"{t}","text":"16 For God loved."}}"#)).await, BLOCKED, "{t}");
            assert_eq!(refused("settings/translation", format!(r#"{{"translation":"{t}"}}"#)).await, BLOCKED, "{t}");
        }
        let passage = |t: &str| format!(r#""kind":"scripture","reference":"John 3:16","translation":"{t}","verses":[]"#);
        assert_eq!(refused("service/add", format!(r#"{{"item":{{{}}}}}"#, passage("TPT"))).await, BLOCKED);
        let item = format!(r#"{{"id":"{}",{}}}"#, Uuid::new_v4(), passage("NWT"));
        assert_eq!(refused("service/update", item).await, BLOCKED);
        assert!(app.data.lock().unwrap().services.iter().all(|s| s.items.is_empty()), "nothing went in");
    }

    #[tokio::test]
    async fn a_blocked_text_under_another_name_is_refused() {
        let app = test_app();
        let text = "1 In the beginning was the Word, and the Word was with God, and the Word was a god.";
        let body = json!({ "reference": "John 1:1", "translation": "ABC", "text": text });
        assert_eq!(app.do_action("bible/paste", body.to_string().as_bytes()).await.unwrap_err().message, BLOCKED);
        let body = json!({ "item": { "kind": "scripture", "reference": "John 1:1", "translation": "ABC",
            "verses": [{ "reference": "John 1:1", "text": text }] } });
        assert_eq!(app.do_action("service/add", body.to_string().as_bytes()).await.unwrap_err().message, BLOCKED);
    }

    #[tokio::test]
    async fn restoring_a_backup_leaves_blocked_passages_behind() {
        let app = test_app();
        let mut service = Service::new("Sunday");
        service.items.push(ServiceItem { id: Uuid::new_v4(), background: None, notes: String::new(),
            kind: ItemKind::Scripture { reference: "John 3:16".into(), translation: "TPT".into(), verses: vec![], per_slide: 0, text_size: 0, also: vec![], layout: Default::default() } });
        let data = Data { services: vec![service], ..Data::default() };
        let name = app.store.backup(&data).unwrap();
        app.do_action("backup/restore", json!({ "name": name }).to_string().as_bytes()).await.unwrap();
        assert!(app.data.lock().unwrap().services[0].items.is_empty());
        assert!(app.store.load().unwrap().services[0].items.is_empty(), "saved as well");
        assert_eq!(app.live.lock().unwrap().notice.as_deref(), Some(passages_removed(1).as_str()));
    }

    #[tokio::test]
    async fn bible_paste_splits_verses() {
        let v = test_app().do_action("bible/paste", br#"{"reference":"John 3:16-17","translation":"hcsb","text":"16 For God loved. 17 For God sent."}"#).await.unwrap();
        assert_eq!(v["translation"], "HCSB");
        assert_eq!(v["verses"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_prefixed_id_goes_through_paste_settings_and_a_saved_service() {
        let app = test_app();
        let id = "ebible:spaRV1909";
        let mut b = bible::import::parse(br#"<XMLBIBLE biblename="Reina Valera"><BIBLEBOOK bnumber="43"><CHAPTER cnumber="3"><VERS vnumber="16">De tal manera.</VERS></CHAPTER></BIBLEBOOK></XMLBIBLE>"#).unwrap();
        b.abbreviation = id.into();
        bible::import::save(&app.store.root, &b).unwrap();

        let v = app.do_action("bible/lookup", json!({ "reference": "John 3:16", "translation": id }).to_string().as_bytes()).await.unwrap();
        assert_eq!((v["translation"].as_str(), v["verses"][0]["text"].as_str()), (Some(id), Some("De tal manera.")));
        let v = app.do_action("bible/paste", json!({ "reference": "John 3:16", "translation": id, "text": "16 De tal." }).to_string().as_bytes()).await.unwrap();
        assert_eq!(v["translation"], id, "not uppercased");

        app.do_action("settings/translation", json!({ "translation": id }).to_string().as_bytes()).await.unwrap();
        assert_eq!(app.store.load().unwrap().settings.translation, id);
        let add = json!({ "item": { "kind": "scripture", "reference": "John 3:16", "translation": id, "verses": v["verses"] } });
        app.do_action("service/add", add.to_string().as_bytes()).await.unwrap();
        let saved = app.store.load().unwrap();
        let item = saved.services.iter().flat_map(|s| &s.items).next().unwrap();
        assert!(matches!(&item.kind, ItemKind::Scripture { translation, .. } if translation == id));
        assert!(app.state_json()["available_translations"].as_array().unwrap().contains(&json!(id)));
    }

    /// A downloaded RV1909 under CC BY-SA, saved straight into `app`'s Bible folder (no network).
    fn downloaded_rv1909(app: &App, licence: bible::library::Licence, notice: &str) {
        let id = "ebible:spaRV1909";
        let mut b = bible::import::parse(br#"<XMLBIBLE biblename="Reina Valera"><BIBLEBOOK bnumber="19"><CHAPTER cnumber="23"><VERS vnumber="1">Jehova es mi pastor.</VERS><VERS vnumber="2">En lugares.</VERS></CHAPTER></BIBLEBOOK></XMLBIBLE>"#).unwrap();
        b.abbreviation = id.into();
        b.copyright = notice.into();
        b.meta = Some(bible::library::BibleMeta {
            id: id.into(), abbr: "RV1909".into(), name: "Reina Valera 1909".into(), lang: "spa".into(), copyright: notice.into(), licence,
            source: bible::library::Source::Ebible { update_date: "2026-01-01".into() }, ..Default::default()
        });
        bible::import::install(&app.store.root, b).unwrap();
    }

    #[tokio::test]
    async fn a_downloaded_bible_credits_its_passage_and_names_it_by_abbreviation() {
        let app = test_app();
        let cc = bible::library::Licence::Cc { kind: "by-sa".into(), version: "4.0".into(), url: "https://creativecommons.org/licenses/by-sa/4.0/".into() };
        downloaded_rv1909(&app, cc, "© Sociedad Bíblica");
        assert!(app.data.lock().unwrap().bible_credit("ebible:spaRV1909").is_none(), "not known until the Bibles are read again");
        app.refresh_bible_credits();
        app.data.lock().unwrap().settings.credits = "none".into();
        let v = app.do_action("bible/lookup", br#"{"reference":"Psalm 23:1-2","translation":"ebible:spaRV1909"}"#).await.unwrap();
        let add = json!({ "item": { "kind": "scripture", "reference": v["label"], "translation": "ebible:spaRV1909", "verses": v["verses"], "per_slide": 1 } });
        app.do_action("service/add", add.to_string().as_bytes()).await.unwrap();
        let state = app.state_json();
        let item = state["items"].as_array().unwrap().last().unwrap();
        assert!(item["title"].as_str().unwrap().ends_with(" (RV1909)"), "{}", item["title"]);
        let slides = item["slides"].as_array().unwrap();
        assert_eq!(slides.len(), 2);
        assert!(slides[0]["content"]["reference"].as_str().unwrap().ends_with("23:1 (RV1909)"), "{}", slides[0]["content"]);
        assert_eq!(slides[0]["content"]["footer"], "RV1909: © Sociedad Bíblica (CC BY-SA 4.0)", "CC BY-SA requires the credit, even with credits off");
        assert!(slides[1]["content"].get("footer").is_none(), "once per passage");
    }

    #[tokio::test]
    async fn which_bibles_must_show_their_credit() {
        let app = test_app();
        downloaded_rv1909(&app, bible::library::Licence::Pd, "Public domain");
        app.refresh_bible_credits();
        let data = app.data.lock().unwrap();
        let rv = data.bible_credit("ebible:spaRV1909").unwrap();
        assert!(!rv.required, "public domain: {rv:?}");
        assert_eq!(rv.abbr, "RV1909");
        let kjv = data.bible_credit("KJV").unwrap();
        assert_eq!((kjv.line.as_str(), kjv.required), ("", false), "built in: nothing to credit");
        let esv = data.bible_credit("ESV").unwrap();
        assert!(esv.required && esv.line.contains("ESV"), "{esv:?}");
    }

    #[tokio::test]
    async fn removing_a_bible_drops_its_credit() {
        let app = test_app();
        downloaded_rv1909(&app, bible::library::Licence::Pd, "Public domain");
        app.refresh_bible_credits();
        app.do_action("bible/import-remove", br#"{"translation":"ebible:spaRV1909"}"#).await.unwrap();
        assert!(app.data.lock().unwrap().bible_credit("ebible:spaRV1909").is_none());
    }

    #[tokio::test]
    async fn settings_translation_saves_and_rejects_unknown() {
        let app = test_app();
        app.do_action("settings/translation", br#"{"translation":"NIV"}"#).await.unwrap();
        assert_eq!(app.data.lock().unwrap().settings.translation, "NIV");
        assert_eq!(app.store.load().unwrap().settings.translation, "NIV", "saved to disk");
        assert_eq!(app.do_action("settings/translation", br#"{"translation":"XYZ"}"#).await.unwrap_err().message, "Unknown translation.");
        assert_eq!(app.data.lock().unwrap().settings.translation, "NIV");
    }

    #[tokio::test]
    async fn picking_a_bible_puts_it_first_in_recent() {
        let app = test_app();
        for t in ["NIV", "WEB", "NIV", "BSB", "ESV", "NLT", "CSB", "HCSB", "KJV", "WEB"] {
            app.do_action("settings/translation", json!({ "translation": t }).to_string().as_bytes()).await.unwrap();
        }
        let recent = app.store.load().unwrap().settings.recent;
        assert_eq!(recent, ["WEB", "KJV", "HCSB", "CSB", "NLT", "ESV", "BSB", "NIV"], "newest first, once each, saved");
        assert_eq!(recent.len(), crate::model::RECENT_MAX);
        app.do_action("settings/translation", br#"{"translation":"XYZ"}"#).await.unwrap_err();
        assert_eq!(app.data.lock().unwrap().settings.recent[0], "WEB", "a refused pick changes nothing");
    }

    #[tokio::test]
    async fn language_is_saved_switched_and_kept_by_a_settings_save() {
        let app = test_app();
        assert_eq!(crate::i18n::current(), "en");
        let r = app.do_action("settings/language", br#"{"code":"es"}"#).await.unwrap();
        assert_eq!(app.store.load().unwrap().settings.ui_language, "es");
        assert_eq!(crate::i18n::current(), "es");
        assert_eq!(app.state_json()["lang"], "es", "the phone is told");
        assert_eq!(r["toast"], crate::i18n::tr("Language changed"), "the toast is in the new language");
        let err = app.do_action("settings/language", br#"{"code":"xx"}"#).await.unwrap_err();
        assert_eq!(err.message, crate::i18n::tr("Unknown language."));
        assert_eq!((app.data.lock().unwrap().settings.ui_language.as_str(), crate::i18n::current().as_str()), ("es", "es"), "a refused code changes nothing");
        // A Settings form opened before the change is saved: the language stays.
        app.do_action("settings", serde_json::to_string(&crate::model::Settings::default()).unwrap().as_bytes()).await.unwrap();
        assert_eq!(app.store.load().unwrap().settings.ui_language, "es");
        app.do_action("settings/language", br#"{"code":"en"}"#).await.unwrap();
        assert_eq!((crate::i18n::current().as_str(), app.state_json()["lang"].as_str()), ("en", Some("en")));
        assert_eq!(app.do_action("settings/language", br#"{"code":"en"}"#).await.unwrap()["toast"], "Language changed");
    }

    #[tokio::test]
    async fn favourites_star_unstar_and_survive_a_settings_save() {
        let app = test_app();
        let star = |t: &str, on: bool| json!({ "translation": t, "on": on }).to_string();
        app.do_action("settings/favourite", star("ESV", true).as_bytes()).await.unwrap();
        app.do_action("settings/favourite", star("KJV", true).as_bytes()).await.unwrap();
        app.do_action("settings/favourite", star("esv", true).as_bytes()).await.unwrap_err();
        app.do_action("settings/favourite", star("ESV", true).as_bytes()).await.unwrap();
        assert_eq!(app.store.load().unwrap().settings.favourites, ["ESV", "KJV"], "once each, in the order starred");
        assert_eq!(app.do_action("settings/favourite", star("XYZ", true).as_bytes()).await.unwrap_err().message, "Unknown translation.");
        assert_eq!(app.do_action("settings/favourite", star("NWT", true).as_bytes()).await.unwrap_err().message, BLOCKED);
        // A form opened before the star is saved: the star stays.
        let mut form = crate::model::Settings::default();
        form.lines_per_slide = 3;
        app.do_action("settings", serde_json::to_string(&form).unwrap().as_bytes()).await.unwrap();
        let s = app.data.lock().unwrap().settings.clone();
        assert_eq!((s.lines_per_slide, s.favourites.as_slice()), (3, ["ESV".to_string(), "KJV".to_string()].as_slice()));
        app.do_action("settings/favourite", star("ESV", false).as_bytes()).await.unwrap();
        app.do_action("settings/favourite", star("GONE", false).as_bytes()).await.unwrap();
        assert_eq!(app.data.lock().unwrap().settings.favourites, ["KJV"], "unstarring needs no installed Bible");
    }

    #[test]
    fn blocked_bibles_leave_favourites_and_recent() {
        let mut d = Data::default();
        d.settings.favourites = vec!["KJV".into(), "NWT".into()];
        d.settings.recent = vec!["TPT".into(), "WEB".into()];
        bible::blocklist::strip(&mut d);
        assert_eq!((d.settings.favourites, d.settings.recent), (vec!["KJV".to_string()], vec!["WEB".to_string()]));
    }

    #[tokio::test]
    async fn display_close_rejects_unknown_role() {
        let err = test_app().do_action("display/close", br#"{"role":"projector"}"#).await.unwrap_err();
        assert_eq!((err.status, err.message.as_str()), (400, "Unknown screen."));
    }

    #[tokio::test]
    async fn backup_now_writes_a_backup() {
        let app = test_app();
        let v = app.do_action("backup/now", b"{}").await.unwrap();
        assert!(app.store.backups().iter().any(|b| b.name == v["name"].as_str().unwrap()));
    }

    #[test]
    fn fifty_installed_bibles_are_listed_without_reading_their_verses() {
        // Library spec §14, lazy loading: a church with many Bibles must not pay for parsing them on every update.
        use bible::import::{self, ImportedBible};
        let seed = crate::store::test_dir();
        assert!(import::list(&seed).is_empty());
        let ids: Vec<String> = (0..50).map(|i| format!("B{i:02}")).collect();
        for id in &ids {
            let mut verses = bible::import::Verses::new();
            verses.insert((42, 3, 16), format!("{id} John 3:16"));
            import::save(&seed, &ImportedBible { abbreviation: id.clone(), name: format!("{id} Bible"), verses, ..Default::default() }).unwrap();
        }
        // A fresh data folder holding those files and their index, as after a restart.
        let root = std::env::temp_dir().join(format!("rp-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("bibles")).unwrap();
        for e in std::fs::read_dir(seed.join("bibles")).unwrap().flatten() {
            if e.path().is_file() { std::fs::copy(e.path(), root.join("bibles").join(e.file_name())).unwrap(); }
        }
        let store = Store::open(root.clone()).unwrap();
        let data = store.load().unwrap();
        let app = App::new(store, data, 0, None);
        let state = app.state_json();
        let available = app.available_translations();
        app.refresh_bible_credits();
        for id in &ids { assert!(available.contains(id), "{id} is listed from the index"); }
        assert_eq!(state["available_translations"].as_array().unwrap().len(), available.len());
        assert_eq!(import::in_memory(&root), 0, "no Bible was parsed for the snapshot, the sources or the credits");
        assert!(import::get(&root, "B07").is_some());
        assert_eq!(import::in_memory(&root), 1, "a lookup is what loads one");
    }

    #[test]
    fn available_translations_are_the_bundled_ones() {
        let app = test_app();
        assert_eq!(app.available_translations(), ["KJV", "WEB", "BSB"]);
        assert_eq!(app.state_json()["available_translations"], json!(["KJV", "WEB", "BSB"]));
    }

    #[test]
    fn screen_guard_marks_room_connected_until_dropped() {
        let app = test_app();
        let g = app.screen_guard("room");
        assert_eq!(app.state_json()["room_connected"], true);
        assert_eq!(app.state_json()["stage_connected"], false);
        drop(g);
        assert_eq!(app.state_json()["room_connected"], false);
        let g = app.screen_guard("stage");
        assert_eq!(app.state_json()["stage_connected"], true);
        drop(g);
        assert_eq!(app.state_json()["stage_connected"], false);
    }

    #[tokio::test]
    async fn import_path_adds_a_picture_to_the_media_library() {
        let app = test_app();
        let png = std::env::temp_dir().join(format!("{}.png", Uuid::new_v4()));
        image::RgbaImage::new(4, 4).save(&png).unwrap();
        let Imported::Media(m) = app.import_path(&png, ImportPurpose::Media, |_| {}).await.unwrap() else { panic!() };
        assert_eq!((m.kind, m.status.clone()), (MediaKind::Image, MediaStatus::Ready));
        assert!(png.exists(), "the user's file is copied, never moved");
        assert_eq!(app.data.lock().unwrap().media.len(), 1);
        assert!(app.store.asset_path(&m.asset).exists());
    }

    #[tokio::test]
    async fn import_path_leaves_unknown_extensions_to_the_probe() {
        let p = std::env::temp_dir().join(format!("{}.xyz", Uuid::new_v4()));
        std::fs::write(&p, b"x").unwrap();
        let e = test_app().import_path(&p, ImportPurpose::Media, |_| {}).await.unwrap_err();
        assert_eq!(e.message, crate::video::ffi::MISSING);
    }

    #[tokio::test]
    async fn import_path_lyrics_imports_a_songselect_song() {
        let app = test_app();
        let p = std::env::temp_dir().join(format!("{}.txt", Uuid::new_v4()));
        std::fs::write(&p, "A Test Song\r\n\r\nVerse 1\r\nOriginal test line one\r\nOriginal test line two\r\n\r\nTest Writer\r\nCCLI Song # 12345\r\n© 2026 Test Music\r\nCCLI License # 98765").unwrap();
        let Imported::Song { id, title, duplicate_of } = app.import_path(&p, ImportPurpose::Lyrics, |_| {}).await.unwrap() else { panic!() };
        assert_eq!((title.as_str(), duplicate_of), ("A Test Song", None));
        let id = id.unwrap();
        assert_eq!(app.data.lock().unwrap().songs[0].id, id);
        // The same song again saves nothing: the operator asks what to do.
        let Imported::Song { id: second, duplicate_of, .. } = app.import_path(&p, ImportPurpose::Lyrics, |_| {}).await.unwrap() else { panic!() };
        assert_eq!((second, duplicate_of), (None, Some(id)));
        assert_eq!(app.data.lock().unwrap().songs.len(), 1);
        assert!(p.exists());
    }

    const SONG_V1: &str = "A Test Song\n\nVerse 1\nOriginal test line one\n\nChorus\nOriginal test chorus\n\nTest Writer\nCCLI Song # 12345\n© 2026 Test Music\nCCLI License # 98765";
    const SONG_V2: &str = "A Test Song (New Key)\n\nVerse 1\nChanged test line one\n\nNew Writer\nCCLI Song # 12345\n© 2027 Test Music\nCCLI License # 98765";

    fn song_import_body(text: &str, mode: &str) -> Vec<u8> { json!({ "text": text, "mode": mode }).to_string().into_bytes() }

    #[tokio::test]
    async fn import_ask_without_duplicate_saves() {
        let app = test_app();
        let v = app.do_action("song/import", &song_import_body(SONG_V1, "ask")).await.unwrap();
        let songs = app.data.lock().unwrap().songs.clone();
        assert_eq!(songs.len(), 1);
        assert_eq!(v, json!({ "id": songs[0].id, "title": "A Test Song", "duplicate_of": null }));
    }

    #[tokio::test]
    async fn import_ask_with_duplicate_saves_nothing() {
        let app = test_app();
        let first = app.do_action("song/import", &song_import_body(SONG_V1, "ask")).await.unwrap();
        let mut rx = app.subscribe();
        let v = app.do_action("song/import", &song_import_body(SONG_V2, "ask")).await.unwrap();
        assert_eq!(v, json!({ "id": null, "title": "A Test Song (New Key)", "duplicate_of": first["id"] }));
        assert!(matches!(rx.try_recv(), Err(broadcast::error::TryRecvError::Empty)), "the screens were told of a change");
        let songs = app.data.lock().unwrap().songs.clone();
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].title, "A Test Song");
    }

    #[tokio::test]
    async fn import_ask_never_matches_on_an_empty_ccli_number() {
        let app = test_app();
        let text = "No Number\n\nVerse 1\nA line";
        app.do_action("song/import", &song_import_body(text, "ask")).await.unwrap();
        let v = app.do_action("song/import", &song_import_body(text, "ask")).await.unwrap();
        assert!(v["id"].is_string());
        assert_eq!(v["duplicate_of"], Value::Null);
        assert_eq!(app.data.lock().unwrap().songs.len(), 2);
    }

    #[tokio::test]
    async fn import_update_keeps_song_id_and_service_items() {
        let app = test_app();
        let first = app.do_action("song/import", &song_import_body(SONG_V1, "ask")).await.unwrap();
        let id: Uuid = serde_json::from_value(first["id"].clone()).unwrap();
        app.edit(|d| {
            let song = d.songs.iter_mut().find(|s| s.id == id).unwrap();
            song.arrangement = vec!["Chorus".into(), "Verse 1".into()];
            let service = d.service_mut().ok_or("no service")?;
            service.items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: id }, background: None, notes: String::new() });
            Ok(())
        }).unwrap();
        let v = app.do_action("song/import", &song_import_body(SONG_V2, "update")).await.unwrap();
        assert_eq!(v["id"], first["id"]);
        let d = app.data.lock().unwrap();
        assert_eq!(d.songs.len(), 1);
        let song = &d.songs[0];
        assert_eq!((song.id, song.title.as_str(), song.ccli.as_str()), (id, "A Test Song (New Key)", "12345"));
        assert_eq!(song.writers, "New Writer");
        assert!(song.copyright.contains("2027"));
        assert_eq!(song.arrangement, vec!["Chorus".to_string(), "Verse 1".to_string()]);
        let item_song = match d.service().unwrap().items[0].kind { ItemKind::Song { song_id } => song_id, _ => panic!() };
        let shown = d.songs.iter().find(|s| s.id == item_song).unwrap();
        let settings = Settings { credits: "none".into(), ..Default::default() };
        let texts: Vec<_> = crate::slides::song_slides(shown, &settings).into_iter().map(|s| match s.content { SlideContent::Text { text, .. } => text, _ => panic!() }).collect();
        // "Chorus" is no longer a section, so the core drops that name from the order.
        assert_eq!(texts, vec!["Changed test line one"]);
    }

    #[tokio::test]
    async fn import_update_of_a_song_deleted_meanwhile_adds_it() {
        let app = test_app();
        let v = app.do_action("song/import", &song_import_body(SONG_V1, "update")).await.unwrap();
        assert!(v["id"].is_string());
        assert_eq!(v["duplicate_of"], Value::Null);
        assert_eq!(app.data.lock().unwrap().songs.len(), 1);
    }

    #[tokio::test]
    async fn import_keep_both_adds_second_song() {
        let app = test_app();
        let first = app.do_action("song/import", &song_import_body(SONG_V1, "ask")).await.unwrap();
        let v = app.do_action("song/import", &song_import_body(SONG_V2, "keep_both")).await.unwrap();
        assert_ne!(v["id"], first["id"]);
        assert_eq!(v["duplicate_of"], first["id"]);
        assert_eq!(app.data.lock().unwrap().songs.len(), 2);
    }

    #[tokio::test]
    async fn song_import_rejects_an_unknown_mode() {
        let e = test_app().do_action("song/import", &song_import_body(SONG_V1, "replace")).await.unwrap_err();
        assert_eq!(e.status, 400);
    }

    #[tokio::test]
    async fn import_path_action_imports_a_file_on_this_computer() {
        let app = test_app();
        let png = std::env::temp_dir().join(format!("{}.png", Uuid::new_v4()));
        image::RgbaImage::new(2, 2).save(&png).unwrap();
        let body = json!({ "path": png, "purpose": "media" }).to_string();
        let v = app.do_action("import/path", body.as_bytes()).await.unwrap();
        assert!(v["media"]["asset"].is_string(), "{v}");
        assert_eq!(app.data.lock().unwrap().media.len(), 1);
        let body = json!({ "path": png, "purpose": "other" }).to_string();
        assert_eq!(app.do_action("import/path", body.as_bytes()).await.unwrap_err().status, 400);
    }

    #[tokio::test]
    async fn import_path_asset_returns_the_stored_asset() {
        let app = test_app();
        let png = std::env::temp_dir().join(format!("{}.png", Uuid::new_v4()));
        image::RgbaImage::new(2, 2).save(&png).unwrap();
        let Imported::Asset(a) = app.import_path(&png, ImportPurpose::Asset, |_| {}).await.unwrap() else { panic!() };
        assert!(a.ends_with(".png") && app.store.asset_path(&a).exists());
        assert!(app.data.lock().unwrap().media.is_empty(), "an asset is not a media library entry");
    }

    #[tokio::test]
    async fn import_path_reports_progress_up_to_100() {
        let app = test_app();
        let png = std::env::temp_dir().join(format!("{}.png", Uuid::new_v4()));
        image::RgbaImage::new(600, 600).save(&png).unwrap();
        let seen = Arc::new(Mutex::new(Vec::<u8>::new()));
        let s = seen.clone();
        app.import_path(&png, ImportPurpose::Media, move |p| s.lock().unwrap().push(p)).await.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.last(), Some(&100));
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "progress only goes up: {seen:?}");
    }

    #[tokio::test]
    async fn import_path_missing_file_is_an_error() {
        let p = std::env::temp_dir().join(format!("{}.png", Uuid::new_v4()));
        assert_eq!(test_app().import_path(&p, ImportPurpose::Media, |_| {}).await.unwrap_err().status, 400);
    }

    #[test]
    fn note_survives_job_end_until_dismissed() {
        let app = test_app();
        let id = app.job_start("Importing");
        app.job_note(id, "Imported with LibreOffice — check the slides.".into());
        app.job_end(id, None);
        {
            let jobs = app.jobs.lock().unwrap();
            assert_eq!(jobs.len(), 1);
            assert_eq!(jobs[0].note.as_deref(), Some("Imported with LibreOffice — check the slides."));
            assert_eq!(jobs[0].percent, 100);
            assert!(jobs[0].done && !jobs[0].cancellable);
        }
        do_dismiss(&app, id);
        assert!(app.jobs.lock().unwrap().is_empty());
    }

    fn do_dismiss(app: &Arc<App>, id: Uuid) {
        tokio::runtime::Runtime::new().unwrap().block_on(app.do_action("job/dismiss", json!({ "id": id }).to_string().as_bytes())).unwrap();
    }

    #[test]
    fn job_without_note_is_removed_on_end() {
        let app = test_app();
        let id = app.job_start("Importing");
        app.job_end(id, None);
        assert!(app.jobs.lock().unwrap().is_empty());
    }

    #[test]
    fn cancel_token_fires_and_job_disappears() {
        let app = test_app();
        let (id, token) = app.job_start_cancellable("Importing");
        assert!(app.jobs.lock().unwrap()[0].cancellable);
        assert!(!token.is_cancelled());
        app.job_cancel(id);
        assert!(token.is_cancelled());
        assert_eq!(app.jobs.lock().unwrap().len(), 1, "the job stays until the work has stopped");
        app.job_note(id, "A note from before the cancel.".into());
        app.job_end_cancelled(id);
        assert!(app.jobs.lock().unwrap().is_empty(), "no error and no note left behind");
        assert!(app.cancels.lock().unwrap().is_empty());
    }

    #[test]
    fn every_end_drops_the_cancel_token() {
        let app = test_app();
        let (ok, _) = app.job_start_cancellable("a");
        let (failed, _) = app.job_start_cancellable("b");
        let (noted, _) = app.job_start_cancellable("c");
        app.job_note(noted, "Note".into());
        app.job_end(ok, None);
        app.job_end(failed, Some("Nope".into()));
        app.job_end(noted, None);
        assert!(app.cancels.lock().unwrap().is_empty());
        assert!(app.jobs.lock().unwrap().iter().all(|j| !j.cancellable), "nothing left to cancel");
    }

    #[tokio::test]
    async fn job_cancel_action_is_wired() {
        let app = test_app();
        let (id, token) = app.job_start_cancellable("Importing");
        app.do_action("job/cancel", json!({ "id": Uuid::new_v4() }).to_string().as_bytes()).await.expect("an unknown id is ignored");
        assert!(!token.is_cancelled());
        app.do_action("job/cancel", json!({ "id": id }).to_string().as_bytes()).await.unwrap();
        assert!(token.is_cancelled());
    }

    #[test]
    fn job_label_broadcasts() {
        let app = test_app();
        let id = app.job_start("Importing");
        let mut rx = app.tx.subscribe();
        app.job_label(id, "Waiting for LibreOffice…");
        assert_eq!(app.jobs.lock().unwrap()[0].label, "Waiting for LibreOffice…");
        assert!(matches!(rx.try_recv(), Ok(Update::Data)));
    }

    #[test]
    fn job_note_broadcasts_and_the_last_one_wins() {
        let app = test_app();
        let id = app.job_start("Importing");
        let mut rx = app.tx.subscribe();
        app.job_note(id, "First".into());
        assert!(matches!(rx.try_recv(), Ok(Update::Data)));
        app.job_note(id, "Second".into());
        assert_eq!(app.jobs.lock().unwrap()[0].note.as_deref(), Some("Second"));
    }

    #[test]
    fn fallback_note_and_another_are_joined_fallback_first() {
        let count = "12 slides were expected but 11 were imported.";
        for notes in [[FALLBACK_NOTE, count], [count, FALLBACK_NOTE]] {
            let app = test_app();
            let id = app.job_start("Importing");
            for n in notes { app.job_note(id, n.into()); }
            assert_eq!(app.jobs.lock().unwrap()[0].note.as_deref(), Some(format!("{FALLBACK_NOTE} {count}").as_str()));
        }
        let app = test_app();
        let id = app.job_start("Importing");
        app.job_note(id, FALLBACK_NOTE.into());
        app.job_note(id, FALLBACK_NOTE.into());
        assert_eq!(app.jobs.lock().unwrap()[0].note.as_deref(), Some(FALLBACK_NOTE), "not doubled");
    }

    #[test]
    fn fallback_note_survives_later_notes_and_is_never_doubled() {
        let count = "12 slides were expected but 11 were imported.";
        let other = "Imported with LibreOffice — check the slides.";
        let note = |notes: &[&str]| {
            let app = test_app();
            let id = app.job_start("Importing");
            for n in notes { app.job_note(id, (*n).into()); }
            app.jobs.lock().unwrap()[0].note.clone().unwrap()
        };
        assert_eq!(note(&[FALLBACK_NOTE, count, other]), format!("{FALLBACK_NOTE} {other}"));
        assert_eq!(note(&[FALLBACK_NOTE, count, FALLBACK_NOTE]), format!("{FALLBACK_NOTE} {count}"));
        assert_eq!(note(&[count, FALLBACK_NOTE, other]), format!("{FALLBACK_NOTE} {other}"));
    }

    #[test]
    fn exporting_a_service_runs_as_a_job_that_goes_away() {
        let app = test_app();
        let dest = app.store.root.join("s.rpservice");
        let id = app.data.lock().unwrap().service().unwrap().id;
        block_on(app.export_service(id, dest.clone())).unwrap();
        assert!(dest.exists());
        assert!(app.jobs.lock().unwrap().is_empty());
    }

    #[test]
    fn importing_a_bad_service_file_leaves_the_error_on_its_job() {
        let app = test_app();
        let bad = app.store.root.join("x.rpservice");
        std::fs::write(&bad, b"no").unwrap();
        assert_eq!(block_on(app.import_service(bad)).unwrap_err(), crate::bundle::strings::NOT_A_SERVICE_FILE);
        assert_eq!(app.data.lock().unwrap().services.len(), 1);
        assert_eq!(app.jobs.lock().unwrap()[0].error.as_deref(), Some(crate::bundle::strings::NOT_A_SERVICE_FILE));
    }

    #[test]
    fn export_with_something_missing_makes_no_job() {
        let app = test_app();
        let dest = app.store.root.join("s.rpservice");
        let id = {
            let mut d = app.data.lock().unwrap();
            let s = d.service_mut().unwrap();
            s.items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: Uuid::new_v4() }, background: None, notes: String::new() });
            s.id
        };
        let e = block_on(app.export_service(id, dest.clone())).unwrap_err();
        assert!(matches!(e, ExportError::Missing(ref lines) if lines.len() == 1), "{e:?}");
        assert!(!dest.exists());
        assert!(app.jobs.lock().unwrap().is_empty());
    }

    #[test]
    fn job_result_is_kept() {
        let app = test_app();
        let (id, deck) = (app.job_start("Importing"), Uuid::new_v4());
        app.job_result(id, deck);
        assert_eq!(app.jobs.lock().unwrap()[0].result, Some(deck));
    }

    #[test]
    fn job_json_unchanged_without_new_fields() {
        let app = test_app();
        app.job_start("Importing");
        let job = serde_json::to_value(&app.jobs.lock().unwrap()[0]).unwrap();
        let mut keys: Vec<_> = job.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["error", "id", "label", "percent"]);
    }

    #[tokio::test]
    async fn do_action_unknown_action_is_404() {
        assert_eq!(test_app().do_action("nope", b"{}").await.unwrap_err().status, 404);
    }

    #[tokio::test]
    async fn lookup_with_also_returns_each_and_left_out() {
        let app = test_app();
        let v = app.do_action("bible/lookup", br#"{"reference":"John 3:16","translation":"KJV","also":["WEB","ESV","KJV"]}"#).await.unwrap();
        assert_eq!(v["also"][0]["translation"], "WEB");
        assert!(v["also"][0]["verses"][0]["text"].as_str().unwrap().starts_with("For God so loved"));
        assert_eq!(v["also"][0]["verses"][0]["id"], json!([42, 3, 16]));
        assert_eq!(v["left_out"], json!(["ESV"]), "no ESV key; KJV is the main one, so not an Also Bible");
    }

    #[tokio::test]
    async fn main_equal_to_also_turns_also_off() {
        let app = test_app();
        app.do_action("settings/also", br#"{"also_show":["WEB"],"parallel_layout":"stacked"}"#).await.unwrap();
        assert_eq!(app.data.lock().unwrap().settings.parallel_layout, crate::scripture::ParallelLayout::Stacked);
        let r = app.do_action("settings/translation", br#"{"translation":"WEB"}"#).await.unwrap();
        assert_eq!(r["toast"], "WEB turned off (same as main).");
        assert!(app.data.lock().unwrap().settings.also_show.is_empty());
    }

    #[tokio::test]
    async fn settings_also_keeps_two_and_refuses_blocked() {
        let app = test_app();
        app.do_action("settings/also", br#"{"also_show":["WEB","KJV","BSB","WEB"]}"#).await.unwrap();
        assert_eq!(app.data.lock().unwrap().settings.also_show, ["WEB", "BSB"]);
        let err = app.do_action("settings/also", br#"{"also_show":["TPT"]}"#).await.unwrap_err();
        assert_eq!(err.message, BLOCKED);
    }

    #[tokio::test]
    async fn add_with_unavailable_also_adds_the_rest_and_warns() {
        let app = test_app();
        app.edit(|d| { d.settings.also_show = vec!["WEB".into(), "ESV".into()]; Ok(()) }).unwrap();
        let r = app.do_action("service/add", br#"{"item":{"kind":"scripture","reference":"John 3:16","translation":"KJV","verses":[{"reference":"John 3:16","text":"For God"}]}}"#).await.unwrap();
        assert_eq!(r["toast"], "ESV isn't available right now, so only KJV · WEB was added.");
        let d = app.data.lock().unwrap();
        let item = d.service().unwrap().items.last().unwrap();
        let ItemKind::Scripture { also, .. } = &item.kind else { panic!() };
        assert_eq!(also.iter().map(|a| a.translation.as_str()).collect::<Vec<_>>(), ["WEB"]);
    }
}