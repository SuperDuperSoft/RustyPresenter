//! The app's state and every operator action, independent of how the screens are drawn.
//! The HTTP layer (`server.rs`) and the native screens both drive it.
use crate::{bible, live::*, media, model::*, slides::build_item, songselect, store::Store};
use crate::sha256::digest_hex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex, OnceLock, atomic::{AtomicUsize, Ordering}},
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
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

#[derive(Serialize, Clone)]
pub struct Job { id: Uuid, label: String, percent: u8, error: Option<String> }

pub struct App {
    pub store: Store,
    pub data: Mutex<Data>,
    pub live: Mutex<Live>,
    pub tx: broadcast::Sender<Update>,
    pub port: u16,
    pub lan_url: Option<String>,
    /// Set once the windowing layer is running; windows can only be opened after that.
    screens: OnceLock<Arc<dyn Screens>>,
    pub(crate) pairing: Mutex<Option<Pairing>>,
    pub(crate) phones: Mutex<Vec<Phone>>,
    pub(crate) attempts: Mutex<HashMap<IpAddr, Instant>>,
    pub(crate) jobs: Mutex<Vec<Job>>,
    pub room_conns: AtomicUsize,
    pub stage_conns: AtomicUsize,
    pub remote_conns: AtomicUsize,
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
            store, data: Mutex::new(data), live: Mutex::new(Live::new()), tx, port, lan_url,
            screens: OnceLock::new(), pairing: Mutex::new(None), phones: Mutex::new(phones),
            attempts: Mutex::new(HashMap::new()), jobs: Mutex::new(vec![]),
            room_conns: AtomicUsize::new(0), stage_conns: AtomicUsize::new(0), remote_conns: AtomicUsize::new(0),
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

    pub fn broadcast_live(&self) { let _ = self.tx.send(Update::Live(LiveUpdate::new(self.live_view()))); }
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

    pub fn room_event(&self, ev: RoomEvent) {
        let changed = self.live.lock().unwrap().report(ev);
        if changed { self.broadcast_live(); }
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

    pub(crate) fn job_start(&self, label: &str) -> Uuid {
        let id = Uuid::new_v4();
        self.jobs.lock().unwrap().push(Job { id, label: label.into(), percent: 0, error: None });
        self.broadcast_data();
        id
    }
    pub(crate) fn job_progress(&self, id: Uuid, p: u8) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) { if j.percent == p { return; } j.percent = p; }
        let _ = self.tx.send(Update::Job { id, percent: p });
    }
    pub(crate) fn job_end(&self, id: Uuid, error: Option<String>) {
        let mut jobs = self.jobs.lock().unwrap();
        match error {
            None => jobs.retain(|j| j.id != id),
            Some(e) => if let Some(j) = jobs.iter_mut().find(|j| j.id == id) { j.error = Some(e); j.percent = 100; },
        }
        drop(jobs);
        self.broadcast_data();
    }

    pub(crate) fn save_phones(&self, phones: &[Phone]) {
        if let Ok(b) = serde_json::to_vec(phones) { let _ = crate::store::write_atomic(&self.store.root.join("phones.json"), &b); }
    }
    pub fn set_screens(&self, s: Arc<dyn Screens>) { let _ = self.screens.set(s); }

    pub fn open_window(&self, role: &str, monitor: Option<&str>) -> Result<(), String> {
        self.screens.get().ok_or("RustyPresenter is still starting. Try again in a moment.")?.open(role, monitor)
    }

    pub fn close_window(&self, role: &str) { if let Some(s) = self.screens.get() { s.close(role); } }

    fn monitors(&self) -> Vec<Monitor> { self.screens.get().map(|s| s.monitors()).unwrap_or_default() }

    pub fn state_json(&self) -> Value {
        let data = self.data.lock().unwrap();
        let items: Vec<Value> = data.service().map(|s| s.items.iter().map(|i| {
            let (title, kind, slides) = build_item(&data, i);
            json!({ "id": i.id, "title": title, "kind": kind, "slides": slides, "item": i })
        }).collect()).unwrap_or_default();
        let windows: Vec<String> = self.screens.get().map(|s| s.open_roles()).unwrap_or_default();
        json!({
            "data": *data,
            "service_id": data.service().map(|s| s.id),
            "items": items,
            "monitors": self.monitors(),
            "windows": windows,
            "ffmpeg": media::ffmpeg().is_some(),
            "lan_url": self.lan_url,
            "translations": bible::TRANSLATIONS,
            "available_translations": ["KJV"],
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
        "song/import-txt" => {
            let text = String::from_utf8_lossy(body);
            let song = tryr!(songselect::import_txt(&text));
            let dup = app.data.lock().unwrap().songs.iter().find(|s| !song.ccli.is_empty() && s.ccli == song.ccli).map(|s| s.id);
            let id = song.id;
            let title = song.title.clone();
            tryr!(app.edit(|d| { d.songs.push(song); Ok(()) }));
            Ok(json!({ "id": id, "title": title, "duplicate_of": dup }))
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
            if translation != "KJV" {
                return Err(ActionError::bad(format!("{translation} isn't set up yet. It needs a licensed text source; KJV works now.")));
            }
            let (label, verses) = tryr!(bible::kjv().lookup(&reference));
            Ok(json!({ "label": label, "verses": verses, "translation": translation }))
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
            app.close_window(&role);
            app.broadcast_data();
            Ok(json!({}))
        }
        // ---- backups ----
        "backup/now" => {
            let data = app.data.lock().unwrap().clone();
            let name = tryr!(app.store.backup(&data).map_err(|e| e.to_string()));
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

    #[tokio::test]
    async fn a_live_command_publishes_one_live_update() {
        let app = test_app();
        let mut rx = app.subscribe();
        app.command(Envelope { id: "c1".into(), base: None, cmd: Command::ClearAll });
        assert!(matches!(rx.try_recv().unwrap(), Update::Live(_)));
        assert!(rx.try_recv().is_err());
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
    async fn do_action_unknown_action_is_404() {
        assert_eq!(test_app().do_action("nope", b"{}").await.unwrap_err().status, 404);
    }
}
