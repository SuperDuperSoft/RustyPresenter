//! The app's state and every operator action, independent of how the screens are drawn.
//! The HTTP layer (`server.rs`) and the native screens both drive it.
use crate::{bible, import, live::*, media, model::*, recovery::state::{self, StateWriter}, slides::build_item, songselect, store::Store};
use crate::sha256::{Digest, Sha256, digest_hex, hex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
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
    Convert { id: Uuid, percent: u8 },
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
    fn open(&self, role: &str, monitor: Option<&str>) -> Result<(), String>;
    fn close(&self, role: &str);
    /// Roles with an open window.
    fn open_roles(&self) -> Vec<String>;
    fn monitors(&self) -> Vec<Monitor>;
}

pub struct Pairing { pub(crate) pin: String, pub(crate) until: Instant, pub(crate) failures: u32 }

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
        Arc::new(Self {
            store, data: Mutex::new(data), live: Mutex::new(Live::new()), tx, live_order: Mutex::new(()), port, lan_url,
            screens: OnceLock::new(), pairing: Mutex::new(None), phones: Mutex::new(phones),
            attempts: Mutex::new(HashMap::new()), jobs: Mutex::new(vec![]), job_ends: Mutex::default(), cancels: Mutex::new(HashMap::new()),
            room_conns: AtomicUsize::new(0), stage_conns: AtomicUsize::new(0), remote_conns: AtomicUsize::new(0),
            state_writer: Mutex::new(None), state_seq: AtomicU64::new(0), resume: Mutex::new(HashMap::new()), probe_mpv: OnceLock::new(), converters: OnceLock::new(),
        })
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

    pub(crate) fn monitors(&self) -> Vec<Monitor> { self.screens.get().map(|s| s.monitors()).unwrap_or_default() }

    /// Roles with an open window (`state_json`'s `windows`).
    pub(crate) fn open_roles(&self) -> Vec<String> { self.screens.get().map(|s| s.open_roles()).unwrap_or_default() }

    /// The standard list plus any other Bible the church imported.
    pub fn translations(&self) -> Vec<String> {
        let mut out: Vec<String> = bible::TRANSLATIONS.iter().map(|t| t.to_string()).collect();
        for b in bible::import::all(&self.store.root) { if !out.contains(&b.abbreviation) { out.push(b.abbreviation.clone()); } }
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
                return json!({ "translation": t, "status": "offline", "provider": format!("{name} — built in, public domain") });
            }
            if let Some(b) = bible::import::get(root, t) {
                return json!({ "translation": t, "status": "offline", "imported": true, "provider": format!("{} — imported file", b.name), "copyright": b.copyright });
            }
            match source_with(t, &secrets) {
                Some(s) => json!({
                    "translation": t,
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
            "ffmpeg": media::ffmpeg().is_some(),
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
        // The web operator's raw-body import; it always keeps both.
        "song/import-txt" => {
            let (id, title, duplicate_of) = tryr!(app.import_song(&String::from_utf8_lossy(body), SongImportMode::KeepBoth));
            Ok(json!({ "id": id, "title": title, "duplicate_of": duplicate_of }))
        }
        // ---- service ----
        "service/add" => {
            #[derive(Deserialize)] struct Add { item: Value, position: Option<usize> }
            let Add { mut item, position } = parse(body)?;
            if item.get("id").is_none() { item["id"] = json!(Uuid::new_v4()); }
            let mut item: ServiceItem = serde_json::from_value(item).map_err(|e| ActionError::bad(e.to_string()))?;
            item.id = Uuid::new_v4();
            let id = item.id;
            tryr!(app.edit(|d| {
                let s = d.service_mut().ok_or("No service is open.")?;
                let pos = position.unwrap_or(s.items.len()).min(s.items.len());
                s.items.insert(pos, item);
                Ok(())
            }));
            Ok(json!({ "id": id }))
        }
        "service/update" => {
            let item: ServiceItem = parse(body)?;
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
            #[derive(Deserialize)] struct L { reference: String, translation: String }
            let L { reference, translation } = parse(body)?;
            // Built in, then a file the church imported (works offline, wins over any online source), then online.
            let (label, verses) = if let Some(b) = bible::built_in(&translation) { tryr!(b.lookup(&reference)) }
                else if let Some(b) = bible::import::get(&app.store.root, &translation) { tryr!(b.lookup(&reference)) }
                else { tryr!(bible::sources::lookup(&app.store.root, &translation, &reference).await) };
            Ok(json!({ "label": label, "verses": verses, "translation": translation }))
        }
        "bible/paste" => {
            // Verses typed or pasted from the church's own Bible, for a translation with no source here (e.g. HCSB).
            #[derive(Deserialize)] struct P { reference: String, translation: String, text: String }
            let P { reference, translation, text } = parse(body)?;
            let (label, verses) = tryr!(bible::pasted(&reference, &text));
            Ok(json!({ "label": label, "verses": verses, "translation": translation.trim().to_uppercase() }))
        }
        "bible/key" => {
            // An empty key removes it. A non-empty key is checked with one lookup before it is saved.
            #[derive(Deserialize)] struct K { provider: String, key: String }
            let K { provider, key } = parse(body)?;
            let p = bible::sources::Provider::from_key_name(&provider).ok_or_else(|| ActionError::bad("Unknown Bible service."))?;
            tryr!(bible::sources::set_key(&app.store.root, p, &key).await);
            app.broadcast_data();
            Ok(json!({}))
        }
        // The body is the raw file. `bible/import/-` keeps the file's own short name.
        a if a.starts_with("bible/import/") => {
            let abbr = a["bible/import/".len()..].trim_matches('-').to_string();
            let (root, bytes) = (app.store.root.clone(), body.to_vec());
            let b = tokio::task::spawn_blocking(move || bible::import::import(&root, &bytes, &abbr)).await.map_err(|e| ActionError::bad(e.to_string()))?;
            let b = tryr!(b);
            app.broadcast_data();
            Ok(json!({ "abbreviation": b.abbreviation, "name": b.name, "copyright": b.copyright, "verses": b.len() }))
        }
        "bible/import-remove" => {
            #[derive(Deserialize)] struct R { translation: String }
            let R { translation } = parse(body)?;
            tryr!(bible::import::remove(&app.store.root, &translation).map_err(|e| e.to_string()));
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
        "media/convert" => {
            let Id { id } = parse(body)?;
            let m = app.data.lock().unwrap().media.iter().find(|m| m.id == id).cloned().ok_or_else(|| ActionError::bad("That file is gone."))?;
            let src = m.original_asset.clone().unwrap_or(m.asset.clone());
            let ext = src.rsplit('.').next().unwrap_or("").to_string();
            let path = app.store.asset_path(&src);
            let plan = if m.kind == MediaKind::Audio { media::plan_audio(&path, &ext).await } else { media::plan_video(&path, &ext).await };
            let plan = if plan == media::Plan::Ready { media::Plan::Transcode } else { plan };
            if let media::Plan::Unknown(r) = plan { return Err(ActionError::bad(r)); }
            spawn_conversion(app.clone(), id, src, plan);
            Ok(json!({}))
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
            tryr!(app.edit(|d| { d.settings = s; Ok(()) }));
            Ok(json!({}))
        }
        "settings/translation" => {
            #[derive(Deserialize)] struct T { translation: String }
            let T { translation } = parse(body)?;
            if !app.translations().contains(&translation) { return Err(ActionError::bad("Unknown translation.")); }
            tryr!(app.edit(|d| { d.settings.translation = translation; Ok(()) }));
            Ok(json!({}))
        }
        // ---- displays ----
        "display/open" => {
            #[derive(Deserialize)] struct O { role: String, monitor: Option<String> }
            let O { role, monitor } = parse(body)?;
            if !matches!(role.as_str(), "room" | "stage" | "operator") { return Err(ActionError::bad("Unknown screen.")); }
            if let Some(m) = monitor.as_ref().filter(|_| role != "operator") {
                let m = m.clone();
                let r = role.clone();
                tryr!(app.edit(|d| { if r == "room" { d.settings.displays.room = Some(m); } else { d.settings.displays.stage = Some(m); } Ok(()) }));
            }
            tryr!(app.open_window(&role, monitor.as_deref()));
            app.broadcast_data();
            Ok(json!({}))
        }
        "display/close" => {
            #[derive(Deserialize)] struct C { role: String }
            let C { role } = parse(body)?;
            if !matches!(role.as_str(), "room" | "stage" | "operator") { return Err(ActionError::bad("Unknown screen.")); }
            app.close_window(&role);
            app.broadcast_data();
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
            let restored = tryr!(app.store.restore(&name));
            *app.data.lock().unwrap() = restored;
            app.broadcast_data();
            app.broadcast_live();
            Ok(json!({}))
        }
        // ---- phone remote ----
        "pair/start" => {
            let pin = random_pin();
            *app.pairing.lock().unwrap() = Some(Pairing { pin: pin.clone(), until: Instant::now() + Duration::from_secs(120), failures: 0 });
            let url = app.lan_url.clone().unwrap_or_default();
            let qr = qrcode::QrCode::with_error_correction_level(&url, qrcode::EcLevel::M).ok().filter(|_| !url.is_empty())
                .map(|c| c.render::<qrcode::render::svg::Color>().quiet_zone(true).build());
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

pub(crate) fn random_pin() -> String {
    let b = Uuid::new_v4().into_bytes();
    let n = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[9], b[10]]);
    format!("{:06}", n % 1_000_000)
}

#[derive(Deserialize)]
struct Id { id: Uuid }

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ActionError> {
    serde_json::from_slice(body).map_err(|e| ActionError::bad(format!("Bad request: {e}")))
}

pub(crate) fn spawn_conversion(app: Arc<App>, id: Uuid, src: AssetId, plan: media::Plan) {
    tokio::spawn(async move {
        let kind = app.data.lock().unwrap().media.iter().find(|m| m.id == id).map(|m| m.kind);
        let Some(kind) = kind else { return };
        let (k, ext) = if kind == MediaKind::Audio { ("audio", "m4a") } else { ("video", "mp4") };
        let input = app.store.asset_path(&src);
        let out = app.store.tmp().join(format!("{}.{ext}", Uuid::new_v4()));
        let set = |app: &App, st: MediaStatus| { if let Some(m) = app.data.lock().unwrap().media.iter_mut().find(|m| m.id == id) { m.status = st; } };
        set(&app, MediaStatus::Converting { percent: 0 });
        app.broadcast_data();
        let mut last = 0u8;
        let app2 = app.clone();
        let result = media::convert(&input, &out, k, &plan, |p| {
            if p >= last + 2 || p == 100 {
                last = p;
                set(&app2, MediaStatus::Converting { percent: p });
                let _ = app2.tx.send(Update::Convert { id, percent: p });
            }
        }).await;
        let final_status = match result {
            Ok(()) => match app.store.import_file(&out, ext) {
                Ok(new_asset) => {
                    let _ = std::fs::remove_file(&out);
                    let _ = app.edit(|d| {
                        if let Some(m) = d.media.iter_mut().find(|m| m.id == id) { m.original_asset = Some(src.clone()); m.asset = new_asset; m.status = MediaStatus::Ready; }
                        Ok(())
                    });
                    return;
                }
                Err(e) => MediaStatus::Failed { reason: e.to_string() },
            },
            Err(e) => MediaStatus::Failed { reason: e },
        };
        let _ = std::fs::remove_file(&out);
        let _ = app.edit(|d| { if let Some(m) = d.media.iter_mut().find(|m| m.id == id) { m.status = final_status; } Ok(()) });
    });
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
                    slides: vec![Slide { label: "s".into(), content: SlideContent::Text { text: "hello".into(), footer: None, reference: None }, notes: String::new() }] })
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
    async fn bible_paste_splits_verses() {
        let v = test_app().do_action("bible/paste", br#"{"reference":"John 3:16-17","translation":"hcsb","text":"16 For God loved. 17 For God sent."}"#).await.unwrap();
        assert_eq!(v["translation"], "HCSB");
        assert_eq!(v["verses"].as_array().unwrap().len(), 2);
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
    async fn import_txt_action_still_keeps_both_for_the_web_operator() {
        let app = test_app();
        let first = app.do_action("song/import-txt", SONG_V1.as_bytes()).await.unwrap();
        let v = app.do_action("song/import-txt", SONG_V2.as_bytes()).await.unwrap();
        assert_eq!(v["duplicate_of"], first["id"]);
        assert_eq!(app.data.lock().unwrap().songs.len(), 2);
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
}
