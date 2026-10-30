//! HTTP host with live updates over Server-Sent Events. Operator, room and stage pages are served only to this
//! computer (loopback). The LAN only sees the phone remote, behind PIN pairing.
use crate::{bible, displays, live::*, media, model::*, slides::build_item, songselect, store::Store};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{ConnectInfo, DefaultBodyLimit, Path as UrlPath, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response, sse::{Event, KeepAlive, Sse}},
    routing::{get, post},
};
use futures_util::{Stream, StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use crate::sha256::{Digest, Sha256, hex, digest_hex};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}},
    time::{Duration, Instant},
};
use tokio::{io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt}, sync::broadcast};
use uuid::Uuid;

pub struct Pairing { pin: String, until: Instant, failures: u32 }

#[derive(Serialize, Deserialize, Clone)]
pub struct Phone { id: Uuid, name: String, hash: String, last_seen: u64 }

#[derive(Serialize, Clone)]
pub struct Job { id: Uuid, label: String, percent: u8, error: Option<String> }

pub struct App {
    pub store: Store,
    pub data: Mutex<Data>,
    pub live: Mutex<Live>,
    pub tx: broadcast::Sender<Arc<str>>,
    pub port: u16,
    pub lan_url: Option<String>,
    pub windows: Mutex<HashMap<String, displays::Window>>,
    /// Set once Tauri is running; windows can only be opened after that.
    pub tauri: std::sync::OnceLock<tauri::AppHandle>,
    pairing: Mutex<Option<Pairing>>,
    phones: Mutex<Vec<Phone>>,
    attempts: Mutex<HashMap<IpAddr, Instant>>,
    jobs: Mutex<Vec<Job>>,
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

const SESSION_DAYS: u64 = 30;
const MAX_PHONES: usize = 5;

fn now_secs() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }

impl App {
    pub fn new(store: Store, data: Data, port: u16, lan_url: Option<String>) -> Arc<Self> {
        let (tx, _) = broadcast::channel(256);
        let phones: Vec<Phone> = std::fs::read(store.root.join("phones.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Arc::new(Self {
            store, data: Mutex::new(data), live: Mutex::new(Live::new()), tx, port, lan_url,
            windows: Mutex::new(HashMap::new()), tauri: std::sync::OnceLock::new(), pairing: Mutex::new(None), phones: Mutex::new(phones),
            attempts: Mutex::new(HashMap::new()), jobs: Mutex::new(vec![]),
            room_conns: AtomicUsize::new(0), stage_conns: AtomicUsize::new(0), remote_conns: AtomicUsize::new(0),
        })
    }

    pub fn live_json(&self) -> String {
        let data = self.data.lock().unwrap();
        let live = self.live.lock().unwrap();
        let plan = DataPlan(&data);
        let slide = live.cursor.as_ref().and_then(|c| live.item.as_ref().and_then(|i| i.slides.get(c.slide)));
        let next = live.next_preview(&plan).map(|(t, s)| json!({ "title": t, "slide": s }));
        json!({
            "type": "live",
            "version": live.version,
            "committed": live.committed_version,
            "pending": live.pending(),
            "frame": live.frame,
            "cursor": live.cursor,
            "item": live.item.as_ref().map(|i| json!({ "item_id": i.item_id, "title": i.title, "kind": i.kind, "count": i.slides.len() })),
            "slide": slide,
            "next": next,
            "media": live.media_status,
            "audio": live.audio_status,
            "notice": live.notice,
            "theme": data.settings.theme,
            "logo": data.settings.logo,
        }).to_string()
    }

    pub fn broadcast_live(&self) { let _ = self.tx.send(Arc::from(self.live_json())); }
    pub fn broadcast_data(&self) { let _ = self.tx.send(Arc::from(json!({ "type": "data" }).to_string())); }

    fn save(&self, data: &Data) -> Result<(), String> { self.store.save(data).map_err(|e| format!("Could not save: {e}")) }

    /// Mutate data, save, and tell every screen.
    fn edit<T>(&self, f: impl FnOnce(&mut Data) -> Result<T, String>) -> Result<T, String> {
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

    fn room_event(&self, ev: RoomEvent) {
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

    fn job_start(&self, label: &str) -> Uuid {
        let id = Uuid::new_v4();
        self.jobs.lock().unwrap().push(Job { id, label: label.into(), percent: 0, error: None });
        self.broadcast_data();
        id
    }
    fn job_progress(&self, id: Uuid, p: u8) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) { if j.percent == p { return; } j.percent = p; }
        let _ = self.tx.send(Arc::from(json!({ "type": "job", "id": id, "percent": p }).to_string()));
    }
    fn job_end(&self, id: Uuid, error: Option<String>) {
        let mut jobs = self.jobs.lock().unwrap();
        match error {
            None => jobs.retain(|j| j.id != id),
            Some(e) => if let Some(j) = jobs.iter_mut().find(|j| j.id == id) { j.error = Some(e); j.percent = 100; },
        }
        drop(jobs);
        self.broadcast_data();
    }

    fn save_phones(&self, phones: &[Phone]) {
        if let Ok(b) = serde_json::to_vec(phones) { let _ = crate::store::write_atomic(&self.store.root.join("phones.json"), &b); }
    }

    pub fn open_window(&self, role: &str, monitor: Option<&str>) -> Result<(), String> {
        let (path, title) = match role {
            "room" => ("/output", "RustyPresenter — Room screen"),
            "stage" => ("/stage", "RustyPresenter — Stage display"),
            "operator" => ("/", "RustyPresenter"),
            _ => return Err("unknown window".into()),
        };
        let handle = self.tauri.get().ok_or("RustyPresenter is still starting. Try again in a moment.")?;
        // A second launch just brings the operator window forward.
        if role == "operator" {
            let label = self.windows.lock().unwrap().get(role).map(|w| w.label.clone());
            if let Some(w) = label.and_then(|l| tauri::Manager::get_webview_window(handle, &l)) {
                let _ = w.unminimize();
                let _ = w.set_focus();
                return Ok(());
            }
        }
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        let mons = displays::monitors();
        let mon = match monitor {
            Some(id) => Some(mons.iter().find(|m| m.id == id).ok_or("That screen is not connected right now.")?),
            None => None,
        };
        if let Some(old) = self.windows.lock().unwrap().remove(role) { old.close(handle); }
        let profile = self.store.root.join("webview").join(role);
        let w = displays::open_app_window(handle, role, title, &url, &profile, mon)?;
        self.windows.lock().unwrap().insert(role.to_string(), w);
        Ok(())
    }

    pub fn close_window(&self, role: &str) {
        let Some(handle) = self.tauri.get() else { return };
        if let Some(w) = self.windows.lock().unwrap().remove(role) { w.close(handle); }
    }

    /// Reopen outputs that were closed unexpectedly, and reload ones whose page crashed.
    pub async fn watchdog(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let Some(handle) = self.tauri.get() else { continue };
            let mut dead = vec![];
            let mut reloaded = vec![];
            {
                let mut w = self.windows.lock().unwrap();
                for (role, win) in w.iter_mut() {
                    if role == "operator" { continue; }
                    if !win.alive(handle) { dead.push((role.clone(), win.monitor.clone(), win.opened.elapsed())); continue; }
                    // The window is open but its page is not connected: the renderer crashed or hung.
                    let conns = if role == "room" { &self.room_conns } else { &self.stage_conns };
                    if conns.load(Ordering::Relaxed) > 0 || win.opened.elapsed() < Duration::from_secs(15) { win.quiet_since = None; continue; }
                    let since = *win.quiet_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= Duration::from_secs(6) && win.reload(handle) { reloaded.push(role.clone()); }
                }
                for (r, _, _) in &dead { w.remove(r); }
            }
            for role in reloaded {
                eprintln!("{role} page stopped responding; reloaded it");
                if role == "room" { self.room_recovered(); }
            }
            for (role, mon, lived) in dead {
                if lived < Duration::from_secs(10) {
                    // Closed right away: don't fight the user or loop on a window that cannot open.
                    eprintln!("{role} window closed right after opening; not reopening automatically");
                    continue;
                }
                eprintln!("{role} window closed unexpectedly; reopening");
                let present = mon.as_ref().is_none_or(|id| displays::monitors().iter().any(|m| &m.id == id));
                if present && self.open_window(&role, mon.as_deref()).is_ok() && role == "room" { self.room_recovered(); }
            }
        }
    }

    pub fn state_json(&self) -> Value {
        let data = self.data.lock().unwrap();
        let items: Vec<Value> = data.service().map(|s| s.items.iter().map(|i| {
            let (title, kind, slides) = build_item(&data, i);
            json!({ "id": i.id, "title": title, "kind": kind, "slides": slides, "item": i })
        }).collect()).unwrap_or_default();
        let windows: Vec<String> = self.windows.lock().unwrap().keys().cloned().collect();
        json!({
            "data": *data,
            "service_id": data.service().map(|s| s.id),
            "items": items,
            "monitors": displays::monitors(),
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
}

// ---------- security helpers ----------

fn local_ok(app: &App, addr: &SocketAddr, headers: &HeaderMap) -> bool {
    let host_ok = headers.get(header::HOST).and_then(|h| h.to_str().ok())
        .is_some_and(|h| h == format!("127.0.0.1:{}", app.port) || h == format!("localhost:{}", app.port));
    let origin_ok = headers.get(header::ORIGIN).and_then(|h| h.to_str().ok())
        .is_none_or(|o| o == format!("http://127.0.0.1:{}", app.port) || o == format!("http://localhost:{}", app.port));
    addr.ip().is_loopback() && host_ok && origin_ok
}

fn forbidden() -> Response { (StatusCode::FORBIDDEN, "This page only opens on the presentation computer.").into_response() }
fn bad(msg: impl Into<String>) -> Response { (StatusCode::BAD_REQUEST, Json(json!({ "error": msg.into() }))).into_response() }
fn ok(v: Value) -> Response { Json(v).into_response() }

fn hash_token(t: &str) -> String { digest_hex(t.as_bytes()) }

/// 256 bits from the OS random generator (via two v4 UUIDs, 244 random bits).
fn random_token() -> String { format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()) }

fn random_pin() -> String {
    let b = Uuid::new_v4().into_bytes();
    let n = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[9], b[10]]);
    format!("{:06}", n % 1_000_000)
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix("rp_session=").map(str::to_string))
}

// ---------- static pages ----------

macro_rules! page { ($f:literal) => { include_str!(concat!("../web/", $f)) }; }

fn asset_response(body: &'static str, ctype: &'static str) -> Response {
    ([(header::CONTENT_TYPE, ctype), (header::CACHE_CONTROL, "no-store")], body).into_response()
}

async fn static_file(UrlPath(name): UrlPath<String>) -> Response {
    match name.as_str() {
        "operator.js" => asset_response(page!("operator.js"), "text/javascript; charset=utf-8"),
        "operator.css" => asset_response(page!("operator.css"), "text/css; charset=utf-8"),
        "render.js" => asset_response(page!("render.js"), "text/javascript; charset=utf-8"),
        "output.js" => asset_response(page!("output.js"), "text/javascript; charset=utf-8"),
        "remote.js" => asset_response(page!("remote.js"), "text/javascript; charset=utf-8"),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn operator_page(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    if !local_ok(&app, &addr, &headers) { return redirect_remote(); }
    asset_response(page!("operator.html"), "text/html; charset=utf-8")
}
fn redirect_remote() -> Response { (StatusCode::FOUND, [(header::LOCATION, "/remote")]).into_response() }
async fn output_page(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    if !local_ok(&app, &addr, &headers) { return forbidden(); }
    asset_response(page!("output.html"), "text/html; charset=utf-8")
}
async fn stage_page(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    if !local_ok(&app, &addr, &headers) { return forbidden(); }
    asset_response(page!("stage.html"), "text/html; charset=utf-8")
}
async fn remote_page() -> Response { asset_response(page!("remote.html"), "text/html; charset=utf-8") }

async fn assets(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, UrlPath(name): UrlPath<String>) -> Response {
    if !local_ok(&app, &addr, &headers) { return forbidden(); }
    if name.contains(['/', '\\']) || name.starts_with('.') { return StatusCode::NOT_FOUND.into_response(); }
    serve_file(app.store.asset_path(&name), headers.get(header::RANGE).and_then(|v| v.to_str().ok())).await
}

pub fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "mp4" | "m4v" => "video/mp4", "webm" => "video/webm", "mov" => "video/quicktime", "mkv" => "video/x-matroska", "ogv" => "video/ogg",
        "mp3" => "audio/mpeg", "m4a" | "aac" => "audio/mp4", "wav" => "audio/wav", "ogg" | "oga" | "opus" => "audio/ogg", "flac" => "audio/flac",
        "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "gif" => "image/gif", "webp" => "image/webp", "bmp" => "image/bmp", "avif" => "image/avif", "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// Parse a single `bytes=a-b` range against a file length.
pub fn parse_range(h: &str, len: u64) -> Option<(u64, u64)> {
    let spec = h.trim().strip_prefix("bytes=")?;
    if spec.contains(',') || len == 0 { return None; }
    let (a, b) = spec.split_once('-')?;
    let (start, end) = if a.trim().is_empty() {
        let n: u64 = b.trim().parse().ok()?;
        (len.saturating_sub(n), len - 1)
    } else {
        let start: u64 = a.trim().parse().ok()?;
        let end = if b.trim().is_empty() { len - 1 } else { b.trim().parse::<u64>().ok()?.min(len - 1) };
        (start, end)
    };
    (start <= end && start < len).then_some((start, end))
}

/// Serve a file with HTTP Range support (needed for video seeking and fast starts).
async fn serve_file(path: PathBuf, range: Option<&str>) -> Response {
    let Ok(mut file) = tokio::fs::File::open(&path).await else { return StatusCode::NOT_FOUND.into_response() };
    let Ok(meta) = file.metadata().await else { return StatusCode::NOT_FOUND.into_response() };
    let len = meta.len();
    let ctype = content_type(&path.to_string_lossy());
    let (status, start, end) = match range {
        Some(r) => match parse_range(r, len) {
            Some((a, b)) => (StatusCode::PARTIAL_CONTENT, a, b),
            None => return (StatusCode::RANGE_NOT_SATISFIABLE, [(header::CONTENT_RANGE, format!("bytes */{len}"))]).into_response(),
        },
        None => (StatusCode::OK, 0, len.saturating_sub(1)),
    };
    let count = if len == 0 { 0 } else { end - start + 1 };
    if start > 0 && file.seek(std::io::SeekFrom::Start(start)).await.is_err() { return StatusCode::INTERNAL_SERVER_ERROR.into_response(); }
    let body = stream::unfold((file, count), |(mut f, left)| async move {
        if left == 0 { return None; }
        let mut buf = vec![0u8; (left.min(256 * 1024)) as usize];
        match f.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => { buf.truncate(n); Some((Ok::<_, std::io::Error>(Bytes::from(buf)), (f, left - n as u64))) }
            Err(e) => Some((Err(e), (f, 0))),
        }
    });
    let mut resp = Response::new(Body::from_stream(body));
    *resp.status_mut() = status;
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(ctype));
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(count));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable"));
    if status == StatusCode::PARTIAL_CONTENT {
        h.insert(header::CONTENT_RANGE, HeaderValue::from_str(&format!("bytes {start}-{end}/{len}")).unwrap());
    }
    resp
}

// ---------- local API ----------

async fn get_state(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    if !local_ok(&app, &addr, &headers) { return forbidden(); }
    ok(app.state_json())
}

#[derive(Deserialize)]
struct Id { id: Uuid }

fn parse<T: serde::de::DeserializeOwned>(body: &Bytes) -> Result<T, Response> {
    serde_json::from_slice(body).map_err(|e| bad(format!("Bad request: {e}")))
}

async fn api(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, UrlPath(action): UrlPath<String>, body: Bytes) -> Response {
    if !local_ok(&app, &addr, &headers) || headers.get("x-rp").is_none() { return forbidden(); }
    match api_inner(&app, &action, &body).await { Ok(v) => ok(v), Err(r) => r }
}

macro_rules! tryr { ($e:expr) => { $e.map_err(bad)? }; }

async fn api_inner(app: &Arc<App>, action: &str, body: &Bytes) -> Result<Value, Response> {
    match action {
        "command" => {
            let env: Envelope = parse(body)?;
            Ok(serde_json::to_value(app.command(env)).unwrap())
        }
        // ---- songs ----
        "song/save" => {
            let mut song: Song = parse(body)?;
            if song.title.trim().is_empty() { return Err(bad("Give the song a title.")); }
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
            let mut item: ServiceItem = serde_json::from_value(item).map_err(|e| bad(e.to_string()))?;
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
                return Err(bad(format!("{translation} isn't set up yet. It needs a licensed text source; KJV works now.")));
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
            let m = app.data.lock().unwrap().media.iter().find(|m| m.id == id).cloned().ok_or_else(|| bad("That file is gone."))?;
            let src = m.original_asset.clone().unwrap_or(m.asset.clone());
            let ext = src.rsplit('.').next().unwrap_or("").to_string();
            let path = app.store.asset_path(&src);
            let plan = if m.kind == MediaKind::Audio { media::plan_audio(&path, &ext).await } else { media::plan_video(&path, &ext).await };
            let plan = if plan == media::Plan::Ready { media::Plan::Transcode } else { plan };
            if let media::Plan::Unknown(r) = plan { return Err(bad(r)); }
            spawn_conversion(app.clone(), id, src, plan);
            Ok(json!({}))
        }
        "deck/from-images" => {
            #[derive(Deserialize)] struct D { title: String, assets: Vec<AssetId> }
            let D { title, assets } = parse(body)?;
            if assets.is_empty() { return Err(bad("Choose at least one picture.")); }
            for a in &assets { if !app.store.asset_path(a).exists() { return Err(bad("A picture is missing. Try uploading again.")); } }
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
            if !matches!(role.as_str(), "room" | "stage" | "operator") { return Err(bad("Unknown screen.")); }
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
            let _ = app.tx.send(Arc::from(json!({ "type": "revoked", "id": id }).to_string()));
            app.broadcast_data();
            Ok(json!({}))
        }
        _ => Err((StatusCode::NOT_FOUND, "unknown action").into_response()),
    }
}

// ---------- uploads ----------

#[derive(Deserialize)]
struct UploadQuery { name: String, #[serde(default)] purpose: String }

async fn upload(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Query(q): Query<UploadQuery>, body: Body) -> Response {
    if !local_ok(&app, &addr, &headers) || headers.get("x-rp").is_none() { return forbidden(); }
    let ext = crate::store::sanitize_ext(q.name.rsplit('.').next().unwrap_or(""));
    let base_name = q.name.rsplit(['/', '\\']).next().unwrap_or(&q.name);
    let display = base_name.rsplit_once('.').map(|(a, _)| a).unwrap_or(base_name).to_string();
    // Stream to a temp file while hashing.
    let tmp = app.store.tmp().join(format!("{}.part", Uuid::new_v4()));
    let mut file = match tokio::fs::File::create(&tmp).await { Ok(f) => f, Err(e) => return bad(e.to_string()) };
    let mut hasher = Sha256::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { let _ = tokio::fs::remove_file(&tmp).await; return bad("The upload was interrupted."); };
        hasher.update(&chunk);
        if file.write_all(&chunk).await.is_err() { let _ = tokio::fs::remove_file(&tmp).await; return bad("Could not save the file (disk full?)."); }
    }
    let _ = file.flush().await;
    drop(file);
    let sha = hex(&hasher.finalize());

    if q.purpose == "powerpoint" {
        let dir = app.store.tmp().join(format!("ppt-{}", Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        let input = dir.join(format!("input.{ext}"));
        if std::fs::rename(&tmp, &input).is_err() { return bad("Could not stage the presentation."); }
        let job = app.job_start(&format!("Importing “{display}” from PowerPoint"));
        let app2 = app.clone();
        tokio::spawn(async move {
            let result = async {
                let pngs = media::export_powerpoint(&input, &dir.join("out")).await?;
                let mut assets = vec![];
                for (i, p) in pngs.iter().enumerate() {
                    assets.push(app2.store.import_file(p, "png").map_err(|e| e.to_string())?);
                    app2.job_progress(job, ((i + 1) * 100 / pngs.len()) as u8);
                }
                app2.edit(|d| { d.decks.push(Deck { id: Uuid::new_v4(), title: display.clone(), slides: assets }); Ok(()) })
            }.await;
            let _ = std::fs::remove_dir_all(&dir);
            app2.job_end(job, result.err());
        });
        return ok(json!({ "job": job }));
    }

    let asset = match app.store.publish(&tmp, &sha, &ext) { Ok(a) => a, Err(e) => return bad(e.to_string()) };
    if q.purpose == "asset" { return ok(json!({ "asset": asset })); }

    let kind = if media::IMAGE.contains(&ext.as_str()) { MediaKind::Image }
        else if media::VIDEO.contains(&ext.as_str()) && !(ext == "webm" && display.to_lowercase().contains("audio")) { MediaKind::Video }
        else if media::AUDIO.contains(&ext.as_str()) { MediaKind::Audio }
        else { return bad(format!("RustyPresenter doesn't know how to show .{ext} files.")); };
    let path = app.store.asset_path(&asset);
    let plan = match kind {
        MediaKind::Image => media::Plan::Ready,
        MediaKind::Video => media::plan_video(&path, &ext).await,
        MediaKind::Audio => media::plan_audio(&path, &ext).await,
    };
    let status = match &plan {
        media::Plan::Ready => MediaStatus::Ready,
        media::Plan::Unknown(r) => MediaStatus::Unconverted { reason: r.clone() },
        _ => MediaStatus::Converting { percent: 0 },
    };
    let id = Uuid::new_v4();
    let m = MediaFile { id, name: display, kind, asset: asset.clone(), status, original_asset: None };
    if let Err(e) = app.edit(|d| { d.media.push(m.clone()); Ok(()) }) { return bad(e); }
    if matches!(plan, media::Plan::Remux | media::Plan::Transcode) { spawn_conversion(app.clone(), id, asset, plan); }
    ok(json!({ "media": m }))
}

fn spawn_conversion(app: Arc<App>, id: Uuid, src: AssetId, plan: media::Plan) {
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
                let _ = app2.tx.send(Arc::from(json!({ "type": "convert", "id": id, "percent": p }).to_string()));
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

// ---------- live updates (Server-Sent Events) ----------

#[derive(Deserialize)]
struct RoleQuery { role: String }

/// Decrements a connection counter when the stream is dropped (client gone).
struct ConnGuard { app: Arc<App>, role: &'static str }
impl Drop for ConnGuard {
    fn drop(&mut self) {
        match self.role { "room" => { self.app.room_conns.fetch_sub(1, Ordering::Relaxed); } "stage" => { self.app.stage_conns.fetch_sub(1, Ordering::Relaxed); } "remote" => { self.app.remote_conns.fetch_sub(1, Ordering::Relaxed); } _ => {} }
        self.app.broadcast_data();
    }
}
fn conn_guard(app: &Arc<App>, role: &'static str) -> ConnGuard {
    match role { "room" => { app.room_conns.fetch_add(1, Ordering::Relaxed); } "stage" => { app.stage_conns.fetch_add(1, Ordering::Relaxed); } "remote" => { app.remote_conns.fetch_add(1, Ordering::Relaxed); } _ => {} }
    app.broadcast_data();
    ConnGuard { app: app.clone(), role }
}

/// A stream of messages: `first`, then every broadcast passed through `map` (None = skip, Err = end).
fn event_stream(app: Arc<App>, role: &'static str, first: String, map: impl Fn(&App, &str) -> Result<Option<String>, ()> + Send + 'static)
    -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let guard = conn_guard(&app, role);
    let rx = app.tx.subscribe();
    let head = stream::once(async move { Ok(Event::default().data(first)) });
    let tail = stream::unfold((rx, app, guard, map, false), |(mut rx, app, guard, map, done)| async move {
        if done { return None; }
        loop {
            match rx.recv().await {
                Ok(text) => match map(&app, &text) {
                    Ok(Some(out)) => return Some((Ok(Event::default().data(out)), (rx, app, guard, map, false))),
                    Ok(None) => continue,
                    Err(()) => return None,
                },
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Too far behind: send a full snapshot instead of the missed messages.
                    if let Ok(Some(out)) = map(&app, "{\"type\":\"resync\"}") { return Some((Ok(Event::default().data(out)), (rx, app, guard, map, false))); }
                }
                Err(_) => return None,
            }
        }
    });
    head.chain(tail)
}

async fn events(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Query(q): Query<RoleQuery>) -> Response {
    if !local_ok(&app, &addr, &headers) { return forbidden(); }
    let role = match q.role.as_str() { "room" => "room", "stage" => "stage", _ => "operator" };
    let first = app.live_json();
    let s = event_stream(app, role, first, |app, text| Ok(Some(if text.contains("\"resync\"") { app.live_json() } else { text.to_string() })));
    Sse::new(s).keep_alive(KeepAlive::new().interval(Duration::from_secs(10))).into_response()
}

async fn room_event(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, body: Bytes) -> Response {
    if !local_ok(&app, &addr, &headers) || headers.get("x-rp").is_none() { return forbidden(); }
    match serde_json::from_slice::<RoomEvent>(&body) {
        Ok(ev) => { app.room_event(ev); ok(json!({})) }
        Err(e) => bad(e.to_string()),
    }
}

// ---------- phone remote ----------

fn phone_for(app: &App, headers: &HeaderMap) -> Option<Uuid> {
    let token = cookie_token(headers)?;
    let h = hash_token(&token);
    let mut phones = app.phones.lock().unwrap();
    let cutoff = now_secs().saturating_sub(SESSION_DAYS * 86400);
    let before = phones.len();
    phones.retain(|p| p.last_seen >= cutoff);
    let mut touched = false;
    let now = now_secs();
    let id = phones.iter_mut().find(|p| p.hash == h).map(|p| { if now.saturating_sub(p.last_seen) > 3600 { p.last_seen = now; touched = true; } p.id });
    if before != phones.len() || touched { app.save_phones(&phones); }
    id
}

#[derive(Deserialize)]
struct PairReq { pin: String, #[serde(default)] name: String }

async fn remote_pair(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(req): Json<PairReq>) -> Response {
    if !same_origin(&headers) { return forbidden(); }
    {
        let mut att = app.attempts.lock().unwrap();
        let now = Instant::now();
        if att.get(&addr.ip()).is_some_and(|t| now.duration_since(*t) < Duration::from_secs(1)) {
            return bad("Too fast — wait a second and try again.");
        }
        att.insert(addr.ip(), now);
    }
    let mut pairing = app.pairing.lock().unwrap();
    let Some(p) = pairing.as_mut().filter(|p| p.until > Instant::now()) else {
        *pairing = None;
        return bad("Pairing isn't open. On the computer, click “Phone remote” → “Pair a phone”.");
    };
    if req.pin.trim() != p.pin {
        p.failures += 1;
        if p.failures >= 5 { *pairing = None; return bad("Too many wrong codes. Start pairing again on the computer."); }
        return bad("That code isn't right.");
    }
    *pairing = None;
    drop(pairing);
    let token = random_token();
    let mut phones = app.phones.lock().unwrap();
    if phones.len() >= MAX_PHONES { phones.sort_by_key(|p| p.last_seen); phones.remove(0); }
    let name = if req.name.trim().is_empty() { format!("Phone {}", phones.len() + 1) } else { req.name.trim().chars().take(40).collect() };
    phones.push(Phone { id: Uuid::new_v4(), name, hash: hash_token(&token), last_seen: now_secs() });
    app.save_phones(&phones);
    drop(phones);
    app.broadcast_data();
    let cookie = format!("rp_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}", SESSION_DAYS * 86400);
    ([(header::SET_COOKIE, cookie)], Json(json!({ "ok": true }))).into_response()
}

async fn remote_status(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    ok(json!({ "paired": phone_for(&app, &headers).is_some() }))
}

fn same_origin(headers: &HeaderMap) -> bool {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    headers.get(header::ORIGIN).and_then(|h| h.to_str().ok()).is_none_or(|o| o == format!("http://{host}"))
}

async fn remote_events(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let Some(phone) = phone_for(&app, &headers) else { return (StatusCode::UNAUTHORIZED, "pair first").into_response() };
    let first = remote_view(&app);
    let revoked = format!("\"revoked\",\"id\":\"{phone}\"");
    let s = event_stream(app, "remote", first, move |app, text| {
        if text.contains(&revoked) { return Err(()); }
        if text.contains("\"type\":\"job\"") || text.contains("\"type\":\"convert\"") { return Ok(None); }
        Ok(Some(remote_view(app)))
    });
    Sse::new(s).keep_alive(KeepAlive::new().interval(Duration::from_secs(10))).into_response()
}

async fn remote_command(State(app): State<Arc<App>>, headers: HeaderMap, body: Bytes) -> Response {
    if !same_origin(&headers) || !headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json")) { return forbidden(); }
    if phone_for(&app, &headers).is_none() { return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "pair first" }))).into_response(); }
    let Ok(env) = serde_json::from_slice::<Envelope>(&body) else { return bad("Bad command.") };
    let allowed = matches!(env.cmd, Command::Next | Command::Prev | Command::Show { .. } | Command::ClearAll | Command::ClearSlide
        | Command::ClearMedia | Command::ClearAudio | Command::Logo | Command::MediaPause { .. });
    // A phone's "show this slide" must say which screen state it was based on.
    let has_base = !matches!(env.cmd, Command::Show { .. }) || env.base.is_some();
    let out = if allowed && has_base { app.command(env) } else { Outcome { ok: false, version: 0, error: Some("Not allowed from the phone.".into()), duplicate: false } };
    ok(serde_json::to_value(out).unwrap())
}

fn remote_view(app: &App) -> String {
    let data = app.data.lock().unwrap();
    let live = app.live.lock().unwrap();
    let plan = DataPlan(&data);
    let items: Vec<Value> = data.service().map(|s| s.items.iter().map(|i| {
        let (title, kind, slides) = build_item(&data, i);
        json!({ "id": i.id, "title": title, "kind": kind, "slides": slides.iter().map(|s| json!({ "label": s.label, "text": slide_text(s) })).collect::<Vec<_>>() })
    }).collect()).unwrap_or_default();
    let current = live.cursor.as_ref().and_then(|c| live.item.as_ref().and_then(|i| i.slides.get(c.slide))).map(|s| json!({ "label": s.label, "text": slide_text(s) }));
    let next = live.next_preview(&plan).map(|(t, s)| json!({ "title": t, "label": s.label, "text": slide_text(&s) }));
    json!({
        "type": "remote",
        "version": live.version,
        "service": data.service().map(|s| s.name.clone()),
        "items": items,
        "cursor": live.cursor,
        "current": current,
        "next": next,
        "on_air": { "slide": live.frame.slide.is_some(), "media": live.frame.media.is_some(), "audio": live.frame.audio.is_some(), "logo": live.frame.logo, "background": live.frame.background.is_some() },
        "media": live.frame.media.as_ref().map(|m| json!({ "inst": m.inst, "paused": matches!(m.v, MediaLayer::Video { paused: true, .. }) })),
        "notice": live.notice,
    }).to_string()
}

pub fn slide_text(s: &Slide) -> String {
    match &s.content {
        SlideContent::Text { text, .. } => text.clone(),
        SlideContent::Image { .. } => "(slide picture)".into(),
        SlideContent::Video { .. } => "(video)".into(),
        SlideContent::Still { .. } => "(picture)".into(),
        SlideContent::Audio { .. } => "(audio)".into(),
    }
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/", get(operator_page))
        .route("/output", get(output_page))
        .route("/stage", get(stage_page))
        .route("/static/{name}", get(static_file))
        .route("/assets/{name}", get(assets))
        .route("/api/state", get(get_state))
        .route("/api/upload", post(upload).layer(DefaultBodyLimit::disable()))
        .route("/api/do/{*action}", post(api).layer(DefaultBodyLimit::max(64 * 1024 * 1024)))
        .route("/events", get(events))
        .route("/api/room-event", post(room_event))
        .route("/remote", get(remote_page))
        .route("/remote/pair", post(remote_pair))
        .route("/remote/status", get(remote_status))
        .route("/remote/events", get(remote_events))
        .route("/remote/command", post(remote_command))
        .with_state(app)
}

pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("RUSTYPRESENTER_DATA").map(PathBuf::from).or_else(|| dirs::data_local_dir().map(|p| p.join("RustyPresenter")))
}
