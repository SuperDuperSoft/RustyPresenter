//! HTTP host with live updates over Server-Sent Events. Operator, room and stage pages are served only to this
//! computer (loopback). The LAN only sees the phone remote, behind PIN pairing.
use crate::app::{App, DataPlan, ImportPurpose, Imported, LiveUpdate, Update, MAX_PHONES, Phone, SESSION_DAYS, hash_token, now_secs, random_token};
use crate::{live::*, model::*, slides::build_item};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{ConnectInfo, DefaultBodyLimit, Path as UrlPath, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response, sse::{Event, KeepAlive, Sse}},
    routing::{get, post},
};
use futures_util::{Stream, StreamExt, stream};
use serde::Deserialize;
use serde_json::{Value, json};
use crate::sha256::{Digest, Sha256, hex};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tokio::{io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt}, sync::broadcast};
use uuid::Uuid;

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


async fn api(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, UrlPath(action): UrlPath<String>, body: Bytes) -> Response {
    if !local_ok(&app, &addr, &headers) || headers.get("x-rp").is_none() { return forbidden(); }
    match app.do_action(&action, &body).await {
        Ok(v) => ok(v),
        Err(e) => (StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_REQUEST), Json(json!({ "error": e.message }))).into_response(),
    }
}



// ---------- uploads ----------

#[derive(Deserialize)]
struct UploadQuery { name: String, #[serde(default)] purpose: String }

async fn upload(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Query(q): Query<UploadQuery>, body: Body) -> Response {
    if !local_ok(&app, &addr, &headers) || headers.get("x-rp").is_none() { return forbidden(); }
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

    let purpose = match q.purpose.as_str() { "powerpoint" => ImportPurpose::PowerPoint, "asset" => ImportPurpose::Asset, "lyrics" => ImportPurpose::Lyrics, _ => ImportPurpose::Media };
    match app.import_staged(tmp, &q.name, &sha, purpose).await {
        Ok(Imported::Media(m)) => ok(json!({ "media": m })),
        Ok(Imported::Asset(asset)) => ok(json!({ "asset": asset })),
        Ok(Imported::Job(job)) => ok(json!({ "job": job })),
        Ok(Imported::Song { id, title, duplicate_of }) => ok(json!({ "id": id, "title": title, "duplicate_of": duplicate_of })),
        Err(e) => bad(e.message),
    }
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

/// The JSON the web pages and phone remote receive for an update.
pub fn update_json(u: &Update) -> String {
    match u {
        Update::Live(v) => v.json().to_owned(),
        Update::Data => json!({ "type": "data" }).to_string(),
        Update::Job { id, percent } => json!({ "type": "job", "id": id, "percent": percent }).to_string(),
        Update::Convert { id, percent } => json!({ "type": "convert", "id": id, "percent": percent }).to_string(),
        Update::Revoked { phone } => json!({ "type": "revoked", "id": phone }).to_string(),
    }
}

/// A stream of messages: `first`, then every update passed through `map` (None = skip, Err = end).
/// `map` gets `None` when this client fell behind and needs a full snapshot instead of the missed updates.
fn event_stream(app: Arc<App>, role: &'static str, first: String, map: impl Fn(&App, Option<&Update>) -> Result<Option<String>, ()> + Send + 'static)
    -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let guard = conn_guard(&app, role);
    let rx = app.subscribe();
    let head = stream::once(async move { Ok(Event::default().data(first)) });
    let tail = stream::unfold((rx, app, guard, map, false), |(mut rx, app, guard, map, done)| async move {
        if done { return None; }
        loop {
            match rx.recv().await {
                Ok(update) => match map(&app, Some(&update)) {
                    Ok(Some(out)) => return Some((Ok(Event::default().data(out)), (rx, app, guard, map, false))),
                    Ok(None) => continue,
                    Err(()) => return None,
                },
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    if let Ok(Some(out)) = map(&app, None) { return Some((Ok(Event::default().data(out)), (rx, app, guard, map, false))); }
                }
                Err(_) => return None,
            }
        }
    });
    head.chain(tail)
}

fn live_json(app: &App) -> String { LiveUpdate::new(app.live_view()).json().to_owned() }

async fn events(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Query(q): Query<RoleQuery>) -> Response {
    if !local_ok(&app, &addr, &headers) { return forbidden(); }
    let role = match q.role.as_str() { "room" => "room", "stage" => "stage", _ => "operator" };
    let first = live_json(&app);
    let s = event_stream(app, role, first, |app, u| Ok(Some(match u { Some(u) => update_json(u), None => live_json(app) })));
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

/// What a phone's remote stream sends for each update: a fresh remote view, nothing for import
/// progress, and the end of the stream when this phone is unpaired.
fn remote_map(phone: Uuid) -> impl Fn(&App, Option<&Update>) -> Result<Option<String>, ()> + Send + 'static {
    move |app, u| match u {
        Some(Update::Revoked { phone: p }) if *p == phone => Err(()),
        Some(Update::Job { .. } | Update::Convert { .. }) => Ok(None),
        _ => Ok(Some(remote_view(app))),
    }
}

async fn remote_events(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let Some(phone) = phone_for(&app, &headers) else { return (StatusCode::UNAUTHORIZED, "pair first").into_response() };
    let first = remote_view(&app);
    let s = event_stream(app, "remote", first, remote_map(phone));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{LiveUpdate, Update, test_app};

    #[test]
    fn live_update_json_matches_the_old_wire_format() {
        let app = test_app();
        let v: Value = serde_json::from_str(&update_json(&Update::Live(LiveUpdate::new(app.live_view())))).unwrap();
        assert_eq!(v["type"], "live");
        for k in ["version", "committed", "pending", "frame", "cursor", "item", "slide", "next", "media", "audio", "notice", "theme", "logo"] {
            assert!(v.get(k).is_some(), "{k}");
        }
    }

    #[test]
    fn live_update_json_is_byte_identical_to_the_serialized_view_plus_type() {
        let view = test_app().live_view();
        let mut expected = serde_json::to_value(&view).unwrap();
        expected.as_object_mut().unwrap().insert("type".into(), "live".into());
        assert_eq!(update_json(&Update::Live(LiveUpdate::new(view))), expected.to_string());
    }

    #[test]
    fn revoking_a_phone_ends_only_that_phones_remote_stream() {
        let app = test_app();
        let (me, other) = (Uuid::new_v4(), Uuid::new_v4());
        let map = remote_map(me);
        assert!(map(&app, Some(&Update::Revoked { phone: me })).is_err());
        assert!(matches!(map(&app, Some(&Update::Revoked { phone: other })), Ok(Some(_))));
        assert!(matches!(map(&app, Some(&Update::Job { id: other, percent: 1 })), Ok(None)));
        assert!(matches!(map(&app, Some(&Update::Convert { id: other, percent: 1 })), Ok(None)));
        assert!(matches!(map(&app, Some(&Update::Data)), Ok(Some(_))));
        assert!(matches!(map(&app, None), Ok(Some(_))));
    }

    async fn post_upload(app: &Arc<App>, name: &str, purpose: &str, bytes: Vec<u8>) -> (StatusCode, Value) {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_str(&format!("127.0.0.1:{}", app.port)).unwrap());
        headers.insert("x-rp", HeaderValue::from_static("1"));
        let addr: SocketAddr = "127.0.0.1:50000".parse().unwrap();
        let q = UploadQuery { name: name.into(), purpose: purpose.into() };
        let r = upload(State(app.clone()), ConnectInfo(addr), headers, Query(q), Body::from(bytes)).await;
        let status = r.status();
        (status, serde_json::from_slice(&axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap()).unwrap())
    }

    #[tokio::test]
    async fn http_upload_goes_through_the_shared_import() {
        let app = test_app();
        let mut png = std::io::Cursor::new(vec![]);
        image::RgbaImage::new(2, 2).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let (s, v) = post_upload(&app, "Sunrise.png", "", png.get_ref().clone()).await;
        assert_eq!((s, v["media"]["name"].as_str(), v["media"]["kind"].as_str()), (StatusCode::OK, Some("Sunrise"), Some("image")));
        let (s, v) = post_upload(&app, "logo.png", "asset", png.into_inner()).await;
        assert!(s == StatusCode::OK && v["asset"].as_str().unwrap().ends_with(".png"));
        assert_eq!(app.data.lock().unwrap().media.len(), 1);
        let (s, v) = post_upload(&app, "notes.xyz", "", b"x".to_vec()).await;
        assert_eq!((s, v["error"].as_str()), (StatusCode::BAD_REQUEST, Some("RustyPresenter doesn't know how to show .xyz files.")));
    }

    #[test]
    fn other_updates_keep_their_wire_format() {
        let id = Uuid::nil();
        assert_eq!(update_json(&Update::Data), r#"{"type":"data"}"#);
        assert_eq!(update_json(&Update::Job { id, percent: 5 }), format!(r#"{{"id":"{id}","percent":5,"type":"job"}}"#));
        assert_eq!(update_json(&Update::Revoked { phone: id }), format!(r#"{{"id":"{id}","type":"revoked"}}"#));
    }
}
