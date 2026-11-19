//! HTTP host for the phone remote, with live updates over Server-Sent Events, behind PIN pairing. `/api/do/*` answers
//! only this computer (loopback): single-instance focus, tests and local automation.
use crate::app::{App, DataPlan, Update, MAX_PHONES, Phone, SESSION_DAYS, ct_eq, hash_token, now_secs, random_token};
use crate::{live::*, model::*, slides::build_item};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, Path as UrlPath, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response, sse::{Event, KeepAlive, Sse}},
    routing::{get, post},
};
use futures_util::{Stream, StreamExt, stream};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
use uuid::Uuid;

// ---------- security helpers ----------

fn local_ok(app: &App, addr: &SocketAddr, headers: &HeaderMap) -> bool {
    let host_ok = headers.get(header::HOST).and_then(|h| h.to_str().ok())
        .is_some_and(|h| h == format!("127.0.0.1:{}", app.port) || h == format!("localhost:{}", app.port));
    let origin_ok = headers.get(header::ORIGIN).and_then(|h| h.to_str().ok())
        .is_none_or(|o| o == format!("http://127.0.0.1:{}", app.port) || o == format!("http://localhost:{}", app.port));
    addr.ip().is_loopback() && host_ok && origin_ok
}

fn forbidden() -> Response { (StatusCode::FORBIDDEN, crate::tr!("This page only opens on the presentation computer.")).into_response() }
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

async fn remote_page() -> Response { asset_response(page!("remote.html"), "text/html; charset=utf-8") }
async fn remote_js() -> Response { asset_response(page!("remote.js"), "text/javascript; charset=utf-8") }
async fn draw_js() -> Response { asset_response(page!("draw.js"), "text/javascript; charset=utf-8") }

// ---------- local API ----------

async fn api(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, UrlPath(action): UrlPath<String>, body: Bytes) -> Response {
    if !local_ok(&app, &addr, &headers) || headers.get("x-rp").is_none() { return forbidden(); }
    match app.do_action(&action, &body).await {
        Ok(v) => ok(v),
        Err(e) => (StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_REQUEST), Json(json!({ "error": e.message }))).into_response(),
    }
}



// ---------- live updates (Server-Sent Events) ----------

/// Decrements the phone count when the stream is dropped (client gone).
struct RemoteGuard { app: Arc<App> }
impl Drop for RemoteGuard {
    fn drop(&mut self) {
        self.app.remote_conns.fetch_sub(1, Ordering::Relaxed);
        self.app.broadcast_data();
    }
}
fn remote_guard(app: &Arc<App>) -> RemoteGuard {
    app.remote_conns.fetch_add(1, Ordering::Relaxed);
    app.broadcast_data();
    RemoteGuard { app: app.clone() }
}

/// A stream of messages: `first`, then every update passed through `map` (None = skip, Err = end).
/// `map` gets `None` when this client fell behind and needs a full snapshot instead of the missed updates.
fn event_stream(app: Arc<App>, first: String, map: impl Fn(&App, Option<&Update>) -> Result<Option<String>, ()> + Send + 'static)
    -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let guard = remote_guard(&app);
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

// ---------- phone remote ----------

pub(crate) fn phone_for(app: &App, headers: &HeaderMap) -> Option<Uuid> {
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
struct PairReq {
    #[serde(default)] pin: String,
    /// The one-time secret from the QR code's `#pair=` (instead of the PIN).
    #[serde(default)] pair: String,
    #[serde(default)] name: String,
    /// N31: the remote's password, when the church set one (Settings → Remote and network).
    #[serde(default)] password: String,
}

fn pair_closed(message: String) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": message, "retry_with_pin": true }))).into_response()
}

async fn remote_pair(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(req): Json<PairReq>) -> Response {
    if !same_origin(&headers) { return forbidden(); }
    {
        let mut att = app.attempts.lock().unwrap();
        let now = Instant::now();
        if att.get(&addr.ip()).is_some_and(|t| now.duration_since(*t) < Duration::from_secs(1)) {
            return bad(crate::tr!("Too fast — wait a second and try again."));
        }
        att.insert(addr.ip(), now);
    }
    let mut pairing = app.pairing.lock().unwrap();
    let Some(p) = pairing.as_mut().filter(|p| p.until > Instant::now()) else {
        *pairing = None;
        return pair_closed(crate::tr!("Pairing isn't open. On the computer, click “Phone” → “Pair a phone or tablet”."));
    };
    let by_qr = !req.pair.trim().is_empty();
    let right = if by_qr { ct_eq(req.pair.trim(), &p.secret) } else { ct_eq(req.pin.trim(), &p.pin) };
    // N31: on top of the code, the remote's password when one is set. A wrong one counts like a wrong code.
    let needed = app.data.lock().unwrap().settings.network.remote_password.clone();
    let password_ok = needed.as_deref().is_none_or(|h| crate::network::password::verify(&req.password, h));
    if !right || !password_ok {
        p.failures += 1;
        if p.failures >= 5 {
            *pairing = None;
            return pair_closed(crate::tr!("Too many wrong codes. Start pairing again on the computer."));
        }
        if !right {
            return if by_qr {
                pair_closed(crate::tr!("That QR code has expired. On the computer, click Pair a phone or tablet again."))
            } else {
                bad(crate::tr!("That code isn't right."))
            };
        }
        return bad(crate::network::strings::wrong_password());
    }
    *pairing = None;
    drop(pairing);
    add_phone(&app, Some(req.name.as_str()))
}

/// A newly paired phone named `name` (or "Phone 3"): kept in the list of paired phones, and its sign-in cookie set
/// on the answer. Also for the phone camera and microphone's QR codes (`network::phone_link`).
pub(crate) fn add_phone(app: &App, name: Option<&str>) -> Response {
    let token = random_token();
    let mut phones = app.phones.lock().unwrap();
    if phones.len() >= MAX_PHONES { phones.sort_by_key(|p| p.last_seen); phones.remove(0); }
    let name = match name.map(str::trim).filter(|n| !n.is_empty()) { Some(n) => n.chars().take(40).collect(), None => crate::tr!("Phone {number}", number = phones.len() + 1) };
    phones.push(Phone { id: Uuid::new_v4(), name, hash: hash_token(&token), last_seen: now_secs(), may_draw: true });
    app.save_phones(&phones);
    drop(phones);
    app.broadcast_data();
    let cookie = format!("rp_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}", SESSION_DAYS * 86400);
    ([(header::SET_COOKIE, cookie)], Json(json!({ "ok": true }))).into_response()
}

/// The phone remote's words in the computer's language, `{ English: shown }`, with the language code in
/// `Content-Language`. Open like `/remote` itself: the pairing screen needs them too, and they are not secret.
async fn remote_i18n() -> Response {
    ([(header::CONTENT_LANGUAGE, crate::i18n::current()), (header::CACHE_CONTROL, "no-store".into())], Json(i18n_map())).into_response()
}

fn i18n_map() -> Value {
    Value::Object(crate::i18n::remote_strings::remote_strings().into_iter().map(|(en, shown)| (en.to_string(), Value::String(shown))).collect())
}

async fn remote_status(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let password = app.data.lock().unwrap().settings.network.remote_password.is_some();
    ok(json!({ "paired": phone_for(&app, &headers).is_some(), "password": password }))
}

fn same_origin(headers: &HeaderMap) -> bool {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    headers.get(header::ORIGIN).and_then(|h| h.to_str().ok()).is_none_or(|o| o == format!("http://{host}") || o == format!("https://{host}"))
}

/// What a phone's remote stream sends for each update: a fresh remote view, nothing for import
/// progress, and the end of the stream when this phone is unpaired.
fn remote_map(phone: Uuid) -> impl Fn(&App, Option<&Update>) -> Result<Option<String>, ()> + Send + 'static {
    // The phone's strokes as last sent: points added to a stroke are not news to the phone that drew them.
    let sent = std::sync::Mutex::new(None::<Vec<u32>>);
    // The phone's view as last sent. Most updates (a video's position twice a second, a job, a screen connecting)
    // change nothing the phone shows, and the view holds every slide of the service, so a repeat is not sent.
    let sent_view = std::sync::Mutex::new(None::<String>);
    move |app, u| match u {
        Some(Update::Revoked { phone: p }) if *p == phone => Err(()),
        Some(Update::Job { .. }) => Ok(None),
        // FEAT-2: a tick sends only the phone's timer lines.
        Some(Update::Timers(v)) => Ok(Some(remote_timers(v))),
        // Drawing: only which of this phone's strokes are still up (undo, clears and slide changes take them away).
        Some(Update::Drawing(d)) => {
            let mine = d.ids_of(phone);
            let mut sent = sent.lock().unwrap();
            if sent.as_ref() == Some(&mine) { return Ok(None) }
            *sent = Some(mine.clone());
            Ok(Some(json!({ "type": "drawing", "mine": mine }).to_string()))
        }
        _ => {
            let view = remote_view(app, Some(phone));
            let mut sent = sent_view.lock().unwrap();
            if sent.as_deref() == Some(view.as_str()) { return Ok(None) }
            *sent = Some(view.clone());
            Ok(Some(view))
        }
    }
}

async fn remote_events(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let Some(phone) = phone_for(&app, &headers) else { return (StatusCode::UNAUTHORIZED, "pair first").into_response() };
    let first = remote_view(&app, Some(phone));
    let s = event_stream(app, first, remote_map(phone));
    Sse::new(s).keep_alive(KeepAlive::new().interval(Duration::from_secs(10))).into_response()
}

async fn remote_command(State(app): State<Arc<App>>, headers: HeaderMap, body: Bytes) -> Response {
    if !same_origin(&headers) || !headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json")) { return forbidden(); }
    if phone_for(&app, &headers).is_none() { return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "pair first" }))).into_response(); }
    let Ok(env) = serde_json::from_slice::<Envelope>(&body) else { return bad(crate::tr!("Bad command.")) };
    let phone = phone_for(&app, &headers).unwrap_or_default();
    if let Err(e) = phone_automation(&app, phone, &env) { return ok(serde_json::to_value(Outcome { ok: false, version: 0, error: Some(e), duplicate: false }).unwrap()) }
    let mode = matches!(env.cmd, Command::FollowMode { .. });
    let out = if phone_may(&env) { app.command(env) } else { Outcome { ok: false, version: 0, error: Some(crate::tr!("Not allowed from the phone.")), duplicate: false } };
    // 1.3: a song set to Suggest or Auto from the phone starts listening (FFmpeg: on its own thread).
    if mode && out.ok { let app = app.clone(); let _ = std::thread::Builder::new().name("follow-start".into()).spawn(move || app.start_follow_if_wanted()); }
    ok(serde_json::to_value(out).unwrap())
}

/// A drawing command from a paired phone (`live::drawing`): `{ ok }` or `{ ok: false, error }`. Malformed input is
/// refused; there is no cap on how much or how fast a phone draws.
async fn remote_draw(State(app): State<Arc<App>>, headers: HeaderMap, body: Bytes) -> Response {
    if !same_origin(&headers) || !headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json")) { return forbidden(); }
    let Some(phone) = phone_for(&app, &headers) else { return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "pair first" }))).into_response() };
    let Ok(cmd) = serde_json::from_slice::<crate::live::drawing::DrawCmd>(&body) else { return bad(crate::tr!("Bad command.")) };
    match app.draw(phone, &cmd) {
        Ok(()) => ok(json!({ "ok": true })),
        Err(e) => ok(json!({ "ok": false, "error": e })),
    }
}

#[derive(Deserialize)]
struct SlideSize { #[serde(default)] w: Option<u32> }

/// The live slide as a JPEG for the phone's drawing view, `w` pixels wide, the audience screen's shape. Paired phones and tablets only.
async fn remote_live_slide(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<SlideSize>) -> Response {
    if phone_for(&app, &headers).is_none() { return (StatusCode::UNAUTHORIZED, "pair first").into_response() }
    let width = q.w.unwrap_or(960);
    let Ok(Ok(jpeg)) = tokio::task::spawn_blocking(move || live_slide_jpeg(&app, width)).await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, crate::tr!("Could not draw the slide.")).into_response()
    };
    ([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store")], jpeg).into_response()
}

#[derive(Deserialize)]
struct AnimPic { v: u64, w: u32, i: usize }

/// The live designed slide's click group for the phone to play (`network::live_anim`), `w` pixels wide: `null`
/// when none plays. Its pictures are at `/remote/live-anim/pic?v=…&w=…&i=…`. Paired phones and tablets only.
async fn remote_live_anim(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<SlideSize>) -> Response {
    if phone_for(&app, &headers).is_none() { return (StatusCode::UNAUTHORIZED, "pair first").into_response() }
    let width = q.w.unwrap_or(960);
    let Ok(Ok(made)) = tokio::task::spawn_blocking(move || crate::network::live_anim::phone_anim(&app, width)).await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, crate::tr!("Could not draw the slide.")).into_response()
    };
    let v = made.map(|a| { let mut j = a.json.clone(); j["v"] = json!(a.version); j["w"] = json!(a.width); j });
    ([(header::CACHE_CONTROL, "no-store")], Json(v.unwrap_or(Value::Null))).into_response()
}

/// One picture of the group made for version `v` at width `w` (kept: the same version never changes).
async fn remote_live_anim_pic(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<AnimPic>) -> Response {
    if phone_for(&app, &headers).is_none() { return (StatusCode::UNAUTHORIZED, "pair first").into_response() }
    let Some((px, ctype)) = crate::network::live_anim::cached(q.v, q.w).and_then(|a| a.pics.get(q.i).cloned()) else { return StatusCode::NOT_FOUND.into_response() };
    ([(header::CONTENT_TYPE, ctype), (header::CACHE_CONTROL, "private, max-age=600")], px).into_response()
}

/// The size the phone's picture is drawn at: `width` kept to 160–1920 pixels, the audience screen's shape
/// (`canvas`; 16:9 when it has no size).
pub fn live_slide_size(width: u32, canvas: (u32, u32)) -> (u32, u32) {
    let w = width.clamp(160, 1920);
    let (cw, ch) = if canvas.0 > 0 && canvas.1 > 0 { canvas } else { (16, 9) };
    (w, ((w as u64 * ch as u64 + cw as u64 / 2) / cw as u64).clamp(1, 1920) as u32)
}

/// The live slide drawn like an operator thumbnail (the shared slide renderer), as a JPEG. Black when nothing
/// is on screen; the drawing itself is not in it (the phone draws its own lines over it).
pub fn live_slide_jpeg(app: &App, width: u32) -> Result<Vec<u8>, String> {
    use crate::ui::thumbs;
    static RENDER: thumbs::TextRenderer = thumbs::TextRenderer::new();
    let size = live_slide_size(width, app.room_canvas());
    let rgba = thumbs::live_slide(app, size, &RENDER);
    let rgb: Vec<u8> = rgba.as_bytes().chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .encode(&rgb, size.0, size.1, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
    Ok(out)
}

/// What a phone may send. "Show this slide" and a section jump must say which screen state they were based on.
fn phone_may(env: &Envelope) -> bool {
    let allowed = matches!(env.cmd, Command::Next | Command::Prev | Command::Show { .. } | Command::ClearAll | Command::ClearSlide
        | Command::ClearMedia | Command::ClearAudio | Command::Logo | Command::MediaPause { .. } | Command::Jump { .. }
        // Overlays on and off (on needs `seen_clear`, checked by the live engine) and F6 (FEAT-1 spec §5.6).
        | Command::Overlay { .. } | Command::ClearOverlays
        // FEAT-2 §5.11: macros and messages marked for the phone (`phone_automation` checks the marks).
        | Command::RunMacro { .. } | Command::ShowMessage { .. } | Command::HideMessage
        // N29: a slide for the stage screen only.
        | Command::StageOnly { .. }
        // 1.3: the live song's auto-follow chip and Hold.
        | Command::FollowMode { .. } | Command::FollowHold { .. });
    let has_base = !matches!(env.cmd, Command::Show { .. } | Command::Jump { .. }) || env.base.is_some();
    allowed && has_base
}

/// When each phone last showed a message: one a second (spec §5.11).
static LAST_MESSAGE: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<Uuid, Instant>>> = std::sync::LazyLock::new(Default::default);

/// FEAT-2: only macros and messages marked "on the phone", and messages at most once a second per phone.
fn phone_automation(app: &App, phone: Uuid, env: &Envelope) -> Result<(), String> {
    use crate::automation::strings;
    let data = app.data.lock().unwrap();
    let a = &data.automation;
    match &env.cmd {
        Command::RunMacro { macro_id, .. } if !a.macros.iter().any(|m| m.id == *macro_id && m.remote) => Err(strings::not_allowed_from_phone()),
        Command::ShowMessage { template, .. } => {
            if !a.messages.iter().any(|t| t.id == *template && t.remote) { return Err(strings::not_allowed_from_phone()) }
            let mut last = LAST_MESSAGE.lock().unwrap();
            let now = Instant::now();
            if last.get(&phone).is_some_and(|t| now.duration_since(*t) < Duration::from_secs(1)) { return Err(strings::too_fast()) }
            last.insert(phone, now);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The phone's timer lines: only timers marked "Show on phone".
fn remote_timers(v: &crate::timers::TimersView) -> String {
    json!({ "type": "timers", "timers": timer_lines(v), "music": v.music }).to_string()
}

/// N13: a walk-in music control from a paired phone: `{ op: "toggle" | "next" | "stop" | "volume", … }`.
async fn remote_music(State(app): State<Arc<App>>, headers: HeaderMap, body: Bytes) -> Response {
    if !same_origin(&headers) || !headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json")) { return forbidden(); }
    if phone_for(&app, &headers).is_none() { return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "pair first" }))).into_response() }
    let Ok(v) = serde_json::from_slice::<Value>(&body) else { return bad(crate::tr!("Bad command.")) };
    let Some(op) = crate::automation::music::MusicOp::parse(v["op"].as_str().unwrap_or_default(), &v) else { return bad(crate::tr!("Bad command.")) };
    match app.music(op) {
        Ok(()) => ok(json!({ "ok": true })),
        Err(e) => ok(json!({ "ok": false, "error": e })),
    }
}

fn timer_lines(v: &crate::timers::TimersView) -> Vec<Value> {
    v.timers.iter().filter(|t| t.on_remote).map(|t| json!({ "name": t.name, "text": crate::timers::text(t.value_ms), "over": t.overrun })).collect()
}

/// A picture or designed slide's own transition, for the phone to play as the room does (`null`: the usual
/// change). A build step on a designed slide has none: only its click plays.
fn slide_transition(s: &crate::model::Slide, frame: &Frame) -> Option<crate::design::Transition> {
    match (&s.content, &frame.slide) {
        (SlideContent::Image { transition, .. }, _) => *transition,
        (SlideContent::Design { design, .. }, Some(SlideLayer::Design { build: false, .. })) => Some(design.transition),
        _ => None,
    }
}

/// The phone shows the slide itself for a picture (PowerPoint, PDF) or designed slide.
fn shows_picture(s: &crate::model::Slide) -> bool { matches!(s.content, SlideContent::Image { .. } | SlideContent::Design { .. }) }

/// A designed slide's click group plays now (the phone asks `/remote/live-anim` for it).
fn plays_anim(frame: &Frame) -> bool { matches!(&frame.slide, Some(SlideLayer::Design { play: Some(p), .. }) if p.duration_ms() > 0) }

fn remote_view(app: &App, phone: Option<Uuid>) -> String {
    let may_draw = phone.is_some_and(|id| app.phones.lock().unwrap().iter().any(|p| p.id == id && p.may_draw));
    let timers = app.timers();
    let data = app.data.lock().unwrap();
    let live = app.live.lock().unwrap();
    let plan = DataPlan(&data);
    let items: Vec<Value> = data.service().map(|s| s.items.iter().map(|i| {
        let (title, kind, slides) = build_item(&data, i);
        json!({ "id": i.id, "title": title, "kind": kind, "count": crate::trn!("{n} slide", "{n} slides", slides.len()), "slides": slides.iter().map(|s| json!({ "label": s.label, "text": slide_text(s) })).collect::<Vec<_>>() })
    }).collect()).unwrap_or_default();
    let sections = live.item.as_ref().map(|i| crate::sections::occurrence_views(&i.slides, live.cursor.as_ref().map(|c| c.slide))).unwrap_or_default();
    let current = live.cursor.as_ref().and_then(|c| live.item.as_ref().and_then(|i| i.slides.get(c.slide))).map(|s| json!({ "label": s.label, "text": slide_text(s), "notes": s.notes, "picture": shows_picture(s), "transition": slide_transition(s, &live.frame), "anim": plays_anim(&live.frame) }));
    let next = live.next_preview(&plan).map(|(t, s)| json!({ "title": t, "label": s.label, "text": slide_text(&s) }));
    json!({
        "type": "remote",
        "version": live.version,
        "service": data.service().map(|s| s.name.clone()),
        "items": items,
        "cursor": live.cursor,
        "sections": sections,
        "current": current,
        "next": next,
        "on_air": { "slide": live.frame.slide.is_some(), "media": live.frame.media.is_some(), "audio": live.frame.audio.is_some(), "logo": live.frame.logo, "background": live.frame.background.is_some() },
        "media": live.frame.media.as_ref().map(|m| json!({ "inst": m.inst, "paused": matches!(m.v, MediaLayer::Video { paused: true, .. }) })),
        "notice": live.notice,
        // FEAT-1: the saved overlays with whether each is on, and the newest clear for `seen_clear`.
        "overlays": crate::theme::overlay::slots::remote_list(&data.overlays, &live.frame.overlays),
        "clear_epoch": live.clear_epoch,
        // FEAT-2: the macros and messages marked for the phone, whether a message is up, and the timer lines.
        "macros": data.automation.macros.iter().filter(|m| m.remote).map(|m| json!({ "id": m.id, "name": m.name, "color": m.color, "relative": m.is_relative() })).collect::<Vec<_>>(),
        "messages": data.automation.messages.iter().filter(|t| t.remote).map(|t| json!({ "id": t.id, "name": t.name, "fields": crate::automation::messages::parse(&t.text).fields })).collect::<Vec<_>>(),
        "message_shown": crate::automation::slots::shown_message(&live.frame.overlays).is_some(),
        "timers": timer_lines(&timers),
        // N13: the walk-in music.
        "music": timers.music,
        // 1.3: the live song's auto-follow (mode, light, line, Hold), for the chip and Hold.
        "follow": app.follow_view(&data, &live).map(|f| json!({ "mode": f.mode, "light": f.light, "line": f.line, "holding": f.holding })),
        // N29: the slide on the stage screen only, if any.
        "stage_only": app.live_view_stage_only().map(|o| json!({ "item_id": o.item_id, "slide": o.index, "label": o.slide.label, "text": slide_text(&o.slide) })),
        // I18N: the page reads `/i18n.json` again when this changes.
        "lang": crate::i18n::current(),
        // Drawing: whether this phone may draw (its Draw button), and its strokes still up.
        "may_draw": may_draw,
        "fade_secs": live.drawing.fade_after.as_secs(),
        // The audience screen's shape: the phone draws on a slide of the same shape, so lines land where drawn.
        "aspect": app.room_canvas(),
        "my_strokes": phone.map(|id| live.drawing.ids_of(id)).unwrap_or_default(),
        // ...and when a translation is edited in the app.
        "words": crate::i18n::revision(),
    }).to_string()
}

pub fn slide_text(s: &Slide) -> String {
    match &s.content {
        SlideContent::Text { text, parallel, .. } => crate::model::all_bibles_text(text, parallel.as_ref()),
        SlideContent::Image { .. } => crate::tr!("(slide picture)"),
        SlideContent::Video { .. } => crate::tr!("(video)"),
        SlideContent::Still { .. } => crate::tr!("(picture)"),
        SlideContent::Audio { .. } => crate::tr!("(audio)"),
        SlideContent::Design { design, .. } => design.words(),
    }
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/static/remote.js", get(remote_js))
        .route("/static/draw.js", get(draw_js))
        .route("/api/do/{*action}", post(api).layer(DefaultBodyLimit::max(64 * 1024 * 1024)))
        .route("/remote", get(remote_page))
        .route("/remote/pair", post(remote_pair))
        .route("/remote/status", get(remote_status))
        .route("/i18n.json", get(remote_i18n))
        .route("/remote/events", get(remote_events))
        .route("/remote/command", post(remote_command))
        .route("/remote/music", post(remote_music))
        .route("/remote/draw", post(remote_draw))
        .route("/remote/live-slide", get(remote_live_slide))
        .route("/remote/live-anim", get(remote_live_anim))
        .route("/remote/live-anim/pic", get(remote_live_anim_pic))
        // N31: the stage screen on a phone or tablet.
        .merge(crate::network::stage_view::routes())
        // The phone as a microphone for the livestream.
        .merge(crate::network::phone_mic::routes())
        // Remote guests: the invitation page and the guest's picture, this computer only (`network::guest`).
        .merge(crate::network::guest::routes())
        // A phone as a livestream camera.
        .merge(crate::network::phone_camera::routes())
        // One scan pairs a phone camera or microphone (`network::phone_link`).
        .merge(crate::network::phone_link::routes())
        .with_state(app)
}

/// The data folder: `RUSTYPRESENTER_DATA`, a portable copy's folder beside the app, or the person's own folder
/// (`platform::data_folder`).
pub fn data_dir() -> Option<PathBuf> { crate::platform::data_folder::data_dir() }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Update, test_app};
    use axum::http::HeaderValue;

    #[test]
    fn revoking_a_phone_ends_only_that_phones_remote_stream() {
        let app = test_app();
        let (me, other) = (Uuid::new_v4(), Uuid::new_v4());
        let map = remote_map(me);
        assert!(map(&app, Some(&Update::Revoked { phone: me })).is_err());
        assert!(matches!(map(&app, Some(&Update::Revoked { phone: other })), Ok(Some(_))));
        assert!(matches!(map(&app, Some(&Update::Job { id: other, percent: 1 })), Ok(None)));
        // Nothing the phone shows changed since: no repeat, whether an update or a catch-up after falling behind.
        assert!(matches!(map(&app, Some(&Update::Data)), Ok(None)));
        assert!(matches!(map(&app, None), Ok(None)));
    }

    #[test]
    fn a_phone_gets_its_view_again_only_when_it_changed() {
        let app = test_app();
        let map = remote_map(Uuid::new_v4());
        assert!(matches!(map(&app, Some(&Update::Data)), Ok(Some(_))), "the first view goes out");
        assert_eq!(map(&app, Some(&Update::Data)), Ok(None), "the same view is not sent twice");
        app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::Logo });
        assert!(matches!(map(&app, Some(&Update::Data)), Ok(Some(_))), "a change goes out");
        assert_eq!(map(&app, None), Ok(None));
    }

    /// `GET`/`POST` on a real listener, as a phone or a local tool would. The status line's code.
    async fn status_of(addr: SocketAddr, method: &str, path: &str) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", addr.port());
        s.write_all(req.as_bytes()).await.unwrap();
        let mut head = [0u8; 12];
        s.read_exact(&mut head).await.unwrap();
        String::from_utf8_lossy(&head[9..12]).parse().unwrap()
    }

    async fn served(app: Arc<App>) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let svc = router(app).into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(listener, svc).await });
        addr
    }

    #[tokio::test]
    async fn operator_page_is_gone_404() {
        let addr = served(test_app()).await;
        for path in ["/", "/output", "/stage", "/assets/a.png", "/api/state", "/events?role=room", "/static/operator.js", "/static/render.js", "/static/output.js"] {
            assert_eq!(status_of(addr, "GET", path).await, 404, "GET {path}");
        }
        for path in ["/api/upload?name=a.png", "/api/room-event"] {
            assert_eq!(status_of(addr, "POST", path).await, 404, "POST {path}");
        }
    }

    #[test]
    fn phone_jump_needs_a_base_and_refresh_is_not_allowed() {
        let env = |cmd: &str, base: Option<u64>| -> Envelope {
            let mut v = json!({ "id": "x", "cmd": cmd, "item_id": Uuid::nil(), "kind": "chorus", "occurrence": 1 });
            if let Some(b) = base { v["base"] = json!(b) }
            serde_json::from_value(v).unwrap()
        };
        assert!(phone_may(&env("jump", Some(3))));
        assert!(!phone_may(&env("jump", None)));
        assert!(!phone_may(&env("refresh", Some(3))));
    }

    #[test]
    fn the_phone_may_put_a_slide_on_the_stage_only_and_sees_it() {
        let app = test_app();
        let id = Uuid::new_v4();
        app.data.lock().unwrap().services[0].items.push(crate::model::ServiceItem { id, kind: crate::model::ItemKind::Text { title: "Band".into(), body: "Watch me".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None });
        let env: Envelope = serde_json::from_value(json!({ "id": "x", "cmd": "stage_only", "item_id": id, "slide": 0 })).unwrap();
        assert!(phone_may(&env), "no base needed: the room does not change");
        assert!(app.command(env).ok);
        let v: Value = serde_json::from_str(&remote_view(&app, None)).unwrap();
        assert_eq!(v["stage_only"]["text"], "Watch me");
        assert!(phone_may(&serde_json::from_value(json!({ "id": "y", "cmd": "stage_only" })).unwrap()));
    }

    #[test]
    fn the_phone_gets_a_picture_slides_transition() {
        use crate::design::{Dir, Transition, TransitionKind};
        let app = test_app();
        let push = Transition { kind: TransitionKind::Push, ms: 800, dir: Dir::Up };
        let deck = crate::model::Deck { id: Uuid::new_v4(), title: "Talk".into(), slides: vec!["1.png".into(), "2.png".into()], notes: vec![], transitions: vec![None, Some(push)], advance_ms: vec![], show: None };
        let id = Uuid::new_v4();
        {
            let mut d = app.data.lock().unwrap();
            d.decks.push(deck.clone());
            d.services[0].items.push(crate::model::ServiceItem { id, kind: crate::model::ItemKind::Deck { deck_id: deck.id }, background: None, notes: String::new(), theme: None });
        }
        let show = |slide| { let base = app.live.lock().unwrap().version; assert!(app.command(Envelope { id: "s".into(), base: Some(base), cmd: Command::Show { item_id: id, slide } }).ok) };
        show(1);
        let v: Value = serde_json::from_str(&remote_view(&app, None)).unwrap();
        assert_eq!(v["current"]["transition"], json!({ "kind": "push", "ms": 800, "dir": "up" }));
        let first = crate::model::Slide::new("Slide 1", SlideContent::Image { asset: "1.png".into(), transition: None });
        assert_eq!(serde_json::to_value(slide_transition(&first, &Frame::default())).unwrap(), Value::Null, "the usual change");
    }

    #[test]
    fn the_phone_gets_a_designed_slides_picture_and_its_click_group() {
        use crate::design::{Anim, AnimKind, Design, Effect, Obj, ObjKind, PctRect, Shape, Start};
        let app = test_app();
        let shape = |x| { let mut o = Obj::new(ObjKind::Shape { shape: Shape::Rect }, PctRect::new(x, 10.0, 20.0, 20.0)); o.look.fill = Some("#ff0000".into()); o };
        let (a, b) = (shape(10.0), shape(60.0));
        let mut fly = Anim::new(b.id, AnimKind::Entrance, Effect::Fly, Start::OnClick);
        fly.dur_ms = 600;
        let id = Uuid::new_v4();
        let kind = crate::model::ItemKind::Design { title: "Talk".into(), slides: vec![Design { objects: vec![a, b], anims: vec![fly], ..Default::default() }] };
        app.data.lock().unwrap().services[0].items.push(crate::model::ServiceItem { id, kind, background: None, notes: String::new(), theme: None });
        let base = app.live.lock().unwrap().version;
        assert!(app.command(Envelope { id: "s".into(), base: Some(base), cmd: Command::Show { item_id: id, slide: 0 } }).ok);
        let v: Value = serde_json::from_str(&remote_view(&app, None)).unwrap();
        assert_eq!((v["current"]["picture"].clone(), v["current"]["anim"].clone()), (json!(true), json!(false)), "a designed slide shows itself; nothing plays as it comes up");
        assert_eq!(v["current"]["transition"]["kind"], "fade");
        assert!(crate::network::live_anim::phone_anim(&app, 640).unwrap().is_none());
        let base = app.live.lock().unwrap().version;
        assert!(app.command(Envelope { id: "n".into(), base: Some(base), cmd: Command::Next }).ok);
        let v: Value = serde_json::from_str(&remote_view(&app, None)).unwrap();
        assert_eq!(v["current"]["anim"], json!(true));
        assert_eq!(v["current"]["transition"], Value::Null, "a click has no transition of its own");
        let made = crate::network::live_anim::phone_anim(&app, 640).unwrap().unwrap();
        let layers = made.json["layers"].as_array().unwrap();
        assert_eq!(layers.len(), 2, "the still shape and the flying one");
        assert_eq!(made.pics.len(), 3, "the picture under them and one per layer");
        assert_eq!(made.json["lead"], 0);
        assert!(made.json.to_string().len() < 1500, "small: {}", made.json.to_string().len());
        let again = crate::network::live_anim::cached(made.version, 640).unwrap();
        assert!(Arc::ptr_eq(&made, &again), "kept for the next phone");
        assert_eq!(made.pics[1].1, "image/png");
    }

    #[tokio::test]
    async fn remote_page_still_served() {
        let addr = served(test_app()).await;
        for path in ["/remote", "/static/remote.js", "/static/draw.js", "/remote/status"] {
            assert_eq!(status_of(addr, "GET", path).await, 200, "{path}");
        }
    }

    const XX_REMOTE: &str = r#"
msgid ""
msgstr ""
"Language: xx\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "Connect"
msgstr "Verbinden"

msgid "On the presentation computer, click {phone} → {pair}, then type the 6-digit code here."
msgstr "Am Computer: {phone} → {pair}, dann den Code tippen."

msgid "{n} slide"
msgid_plural "{n} slides"
msgstr[0] "{n} Folie"
msgstr[1] "{n} Folien"
"#;

    #[test]
    fn i18n_json_maps_the_remote_words_to_the_current_language() {
        let en = i18n_map();
        assert!(en.as_object().unwrap().iter().all(|(k, v)| v == k), "in English every word is itself (catches a typo in a pair)");
        crate::i18n::testing::with_catalog(XX_REMOTE, || {
            let m = i18n_map();
            assert_eq!(m["Connect"], "Verbinden");
            assert_eq!(m["On the presentation computer, click {phone} → {pair}, then type the 6-digit code here."], "Am Computer: {phone} → {pair}, dann den Code tippen.");
            assert_eq!(m["Cancel"], "Cancel", "an untranslated word stays English");
            let app = test_app();
            let v: Value = serde_json::from_str(&remote_view(&app, None)).unwrap();
            assert_eq!(v["lang"], "xx", "the phone is told the language");
        });
    }

    #[test]
    fn i18n_json_follows_translations_edited_in_the_app() {
        crate::i18n::testing::reset();
        let app = test_app();
        let dir = app.store.root.join("translations");
        crate::i18n::load_user_folder(&dir);
        crate::i18n::set_language("es").unwrap();
        assert_eq!(i18n_map()["Connect"], "Conectar");
        let words = |app: &App| serde_json::from_str::<Value>(&remote_view(app, None)).unwrap()["words"].as_u64().unwrap();
        let before = words(&app);
        let mut d = crate::i18n::user::Draft::open("es");
        d.set("", "Connect", &["Enlazar".into()]);
        d.save(&dir).unwrap();
        assert_eq!(i18n_map()["Connect"], "Enlazar");
        assert!(words(&app) > before, "the phone is told to read the words again");
        crate::i18n::set_language("en").unwrap();
    }

    #[test]
    fn phone_has_auto_and_hold() {
        use crate::model::{ItemKind, ServiceItem, Song};
        let app = test_app();
        app.start_follower();
        let song = Song { id: Uuid::new_v4(), title: "Grace".into(), lyrics: "Verse 1\nAmazing grace\n\nChorus\nMy chains are gone".into(), ..Default::default() };
        let (id, text) = (Uuid::new_v4(), Uuid::new_v4());
        {
            let mut d = app.data.lock().unwrap();
            d.songs.push(song.clone());
            d.services[0].items.push(ServiceItem { id, kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None });
            d.services[0].items.push(ServiceItem { id: text, kind: ItemKind::Text { title: "Welcome".into(), body: "Hi".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None });
        }
        let cmd = |v: Value| -> Envelope { serde_json::from_value(v).unwrap() };
        assert!(app.command(cmd(json!({ "id": "a", "cmd": "show", "item_id": id, "slide": 0 }))).ok);
        let follow = |app: &App| serde_json::from_str::<Value>(&remote_view(app, None)).unwrap()["follow"].clone();
        assert_eq!((follow(&app)["mode"].as_str(), follow(&app)["holding"].as_bool()), (Some("manual"), Some(false)));
        // The chip: no base needed, the room doesn't change.
        let auto = cmd(json!({ "id": "b", "cmd": "follow_mode", "item_id": id, "mode": "auto" }));
        assert!(phone_may(&auto));
        assert!(app.command(auto).ok);
        assert_eq!(follow(&app)["mode"], "auto");
        let hold = cmd(json!({ "id": "c", "cmd": "follow_hold", "on": true }));
        assert!(phone_may(&hold));
        assert!(app.command(hold).ok);
        assert_eq!((follow(&app)["holding"].as_bool(), follow(&app)["light"].as_str()), (Some(true), Some("holding")));
        // Not a song: no chip.
        assert!(app.command(cmd(json!({ "id": "d", "cmd": "show", "item_id": text, "slide": 0 }))).ok);
        assert!(follow(&app).is_null());
        // The page sends both and has its words.
        let js = include_str!("../web/remote.js");
        assert!(js.contains("send('follow_mode'") && js.contains("send('follow_hold'"));
        let m = i18n_map();
        for w in ["Manual", "Suggest", "Auto", "Hold", "Holding", "Not listening", "Listening…", "Following the singers",
            "Not sure — staying on this slide", "Holding — the slides stay put", "Nobody singing — words off"] {
            assert!(m.get(w).is_some(), "“{w}” is not in src/i18n/remote_strings.rs");
            assert!(js.contains(&format!("'{w}'")), "“{w}” is not in remote.js");
        }
    }

    /// Every `t('…')` in `remote.js` and `data-t` text in `remote.html` is in `/i18n.json`.
    #[test]
    fn every_remote_word_is_listed() {
        let m = i18n_map();
        let js = concat!(include_str!("../web/remote.js"), include_str!("../web/draw.js"), include_str!("../web/stage-view.js"));
        let mut asked = Vec::new();
        for (at, _) in js.match_indices("t('") {
            if js[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '.') { continue }
            asked.push(js[at + 3..].split('\'').next().unwrap().to_string());
        }
        let html = include_str!("../web/remote.html");
        for part in html.split(" data-t>").skip(1) {
            asked.push(part.split('<').next().unwrap().trim().to_string());
        }
        assert!(asked.len() > 20, "found {}", asked.len());
        for w in &asked { assert!(m.get(w).is_some(), "“{w}” is not in src/i18n/remote_strings.rs"); }
    }

    #[tokio::test]
    async fn i18n_json_is_served_without_pairing() {
        let addr = served(test_app()).await;
        assert_eq!(status_of(addr, "GET", "/i18n.json").await, 200);
    }

    #[tokio::test]
    async fn api_do_rejects_non_loopback() {
        let app = test_app();
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, format!("127.0.0.1:{}", app.port).parse().unwrap());
        headers.insert("x-rp", "1".parse().unwrap());
        let call = |ip: &str, headers: HeaderMap| {
            let addr: SocketAddr = format!("{ip}:50000").parse().unwrap();
            api(State(app.clone()), ConnectInfo(addr), headers, UrlPath("live".to_string()), Bytes::new())
        };
        assert_eq!(call("192.168.1.20", headers.clone()).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(call("127.0.0.1", headers.clone()).await.status(), StatusCode::OK);
        headers.remove("x-rp");
        assert_eq!(call("127.0.0.1", headers).await.status(), StatusCode::FORBIDDEN, "no x-rp");
    }

    fn lan_app() -> Arc<App> {
        let root = std::env::temp_dir().join(format!("rp-test-{}", Uuid::new_v4()));
        let store = crate::store::Store::open(root).unwrap();
        let data = store.load().unwrap();
        App::new(store, data, 0, Some("http://192.168.1.20:7878/remote".into()))
    }

    /// Posts to /remote/pair from its own IP (the per-IP limit allows one try a second), with a right Origin.
    async fn pair(app: &Arc<App>, ip: u8, body: Value) -> (StatusCode, Option<String>, Value) {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("192.168.1.20:7878"));
        headers.insert(header::ORIGIN, HeaderValue::from_static("http://192.168.1.20:7878"));
        let addr: SocketAddr = format!("192.168.1.{ip}:50000").parse().unwrap();
        let req: PairReq = serde_json::from_value(body).unwrap();
        let r = remote_pair(State(app.clone()), ConnectInfo(addr), headers, Json(req)).await;
        let cookie = r.headers().get(header::SET_COOKIE).map(|v| v.to_str().unwrap().to_owned());
        let status = r.status();
        (status, cookie, serde_json::from_slice(&axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap()).unwrap())
    }

    async fn start(app: &Arc<App>) -> (Value, String, String) {
        let v = app.do_action("pair/start", b"{}").await.ok().unwrap();
        let p = app.pairing.lock().unwrap();
        let p = p.as_ref().unwrap();
        (v, p.pin.clone(), p.secret.clone())
    }

    #[tokio::test]
    async fn the_qr_code_carries_a_one_time_secret_in_the_fragment() {
        let app = lan_app();
        let (v, pin, secret) = start(&app).await;
        assert!(secret.len() >= 32 && secret.chars().all(|c| c.is_ascii_alphanumeric()), "128+ bits, URL-safe: {secret}");
        assert_ne!(secret, pin);
        let link = format!("http://192.168.1.20:7878/remote#pair={secret}");
        assert_eq!(crate::app::pair_link(app.lan_url.as_deref().unwrap(), &secret), link);
        assert_eq!(v["qr"].as_str(), crate::app::pair_qr_svg(&link).as_deref(), "the QR code is that link");
        assert_eq!(v["url"], "http://192.168.1.20:7878/remote", "the address shown for typing has no secret");
        assert_eq!(v["pin"], pin.as_str());
        // A fresh pairing makes a fresh secret.
        let (_, _, again) = start(&app).await;
        assert_ne!(again, secret);
    }

    #[tokio::test]
    async fn pairing_with_the_secret_sets_the_cookie_and_closes_pairing() {
        let app = lan_app();
        let (_, pin, secret) = start(&app).await;
        let (s, cookie, v) = pair(&app, 2, json!({ "pair": secret, "name": "iPhone" })).await;
        assert_eq!((s, v["ok"].as_bool()), (StatusCode::OK, Some(true)));
        let cookie = cookie.unwrap();
        assert!(cookie.starts_with("rp_session=") && cookie.contains("HttpOnly"), "{cookie}");
        let token = cookie.trim_start_matches("rp_session=").split(';').next().unwrap();
        let phones = app.phones.lock().unwrap().clone();
        assert_eq!(phones.len(), 1);
        assert_eq!((phones[0].name.as_str(), phones[0].hash.clone()), ("iPhone", hash_token(token)));
        assert!(app.pairing.lock().unwrap().is_none(), "closed");
        // Single use: neither the secret nor the PIN works again.
        let (s, c, _) = pair(&app, 3, json!({ "pair": secret })).await;
        assert_eq!((s, c), (StatusCode::BAD_REQUEST, None));
        let (s, c, _) = pair(&app, 4, json!({ "pin": pin })).await;
        assert_eq!((s, c), (StatusCode::BAD_REQUEST, None));
        assert_eq!(app.phones.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_secret_expires_with_the_window() {
        let app = lan_app();
        let (_, _, secret) = start(&app).await;
        app.pairing.lock().unwrap().as_mut().unwrap().until = Instant::now() - Duration::from_secs(1);
        let (s, c, v) = pair(&app, 2, json!({ "pair": secret })).await;
        assert_eq!((s, c), (StatusCode::BAD_REQUEST, None));
        assert_eq!(v["retry_with_pin"], true);
        assert!(app.phones.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_wrong_secret_counts_as_a_failure() {
        let app = lan_app();
        let (_, _, secret) = start(&app).await;
        let (s, _, v) = pair(&app, 2, json!({ "pair": "0".repeat(64) })).await;
        assert_eq!((s, v["error"].as_str()), (StatusCode::BAD_REQUEST, Some("That QR code has expired. On the computer, click Pair a phone or tablet again.")));
        assert_eq!(app.pairing.lock().unwrap().as_ref().unwrap().failures, 1);
        for ip in 3..6 { pair(&app, ip, json!({ "pair": "x" })).await; }
        let (_, _, v) = pair(&app, 6, json!({ "pin": "nope" })).await;
        assert_eq!(v["error"], "Too many wrong codes. Start pairing again on the computer.");
        assert!(app.pairing.lock().unwrap().is_none(), "five failures close pairing");
        let (s, c, _) = pair(&app, 7, json!({ "pair": secret })).await;
        assert_eq!((s, c), (StatusCode::BAD_REQUEST, None), "even the right secret is dead now");
    }

    #[tokio::test]
    async fn the_pin_still_works_and_kills_the_secret() {
        let app = lan_app();
        let (_, pin, secret) = start(&app).await;
        let (s, cookie, _) = pair(&app, 2, json!({ "pin": format!(" {pin} ") })).await;
        assert_eq!(s, StatusCode::OK);
        assert!(cookie.is_some());
        let (s, c, _) = pair(&app, 3, json!({ "pair": secret })).await;
        assert_eq!((s, c), (StatusCode::BAD_REQUEST, None));
        assert_eq!(app.phones.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn pairing_keeps_the_same_origin_and_rate_limit_checks() {
        let app = lan_app();
        let (_, _, secret) = start(&app).await;
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("192.168.1.20:7878"));
        headers.insert(header::ORIGIN, HeaderValue::from_static("http://evil.example"));
        let addr: SocketAddr = "192.168.1.9:1".parse().unwrap();
        let req: PairReq = serde_json::from_value(json!({ "pair": secret })).unwrap();
        assert_eq!(remote_pair(State(app.clone()), ConnectInfo(addr), headers, Json(req)).await.status(), StatusCode::FORBIDDEN);
        pair(&app, 2, json!({ "pair": "x" })).await;
        let (_, _, v) = pair(&app, 2, json!({ "pair": secret })).await;
        assert_eq!(v["error"], "Too fast — wait a second and try again.");
        assert!(app.pairing.lock().unwrap().is_some());
    }

    #[tokio::test]
    async fn a_remote_password_is_needed_on_top_of_the_code_when_set() {
        let app = lan_app();
        app.data.lock().unwrap().settings.network.remote_password = Some(crate::network::password::hash("worship"));
        let st: Value = serde_json::from_slice(&axum::body::to_bytes(remote_status(State(app.clone()), HeaderMap::new()).await.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(st["password"], true, "the page asks for it");
        let (_, pin, secret) = start(&app).await;
        let (s, c, v) = pair(&app, 2, json!({ "pin": pin })).await;
        assert_eq!((s, c, v["error"].as_str()), (StatusCode::BAD_REQUEST, None, Some("That password isn't right.")));
        let (s, c, _) = pair(&app, 3, json!({ "pair": secret, "password": "praise" })).await;
        assert_eq!((s, c), (StatusCode::BAD_REQUEST, None));
        assert_eq!(app.pairing.lock().unwrap().as_ref().unwrap().failures, 2, "wrong passwords count like wrong codes");
        let (s, c, _) = pair(&app, 4, json!({ "pair": secret, "password": "worship" })).await;
        assert_eq!(s, StatusCode::OK);
        assert!(c.is_some());
        // Without a password set, nothing changes.
        let app = lan_app();
        let (_, pin, _) = start(&app).await;
        assert_eq!(pair(&app, 2, json!({ "pin": pin })).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_stage_view_is_served_but_off_by_default() {
        let addr = served(test_app()).await;
        assert_eq!(status_of(addr, "GET", "/stage-view").await, 200);
        assert_eq!(status_of(addr, "GET", "/static/stage-view.js").await, 200);
        assert_eq!(status_of(addr, "GET", "/stage-view/events").await, 401);
        assert_eq!(status_of(addr, "GET", "/stage-view/next.jpg").await, 401);
    }

    #[test]
    fn secrets_are_compared_in_constant_time() {
        use crate::app::ct_eq;
        assert!(ct_eq("abc123", "abc123"));
        assert!(!ct_eq("abc123", "abc124"));
        assert!(!ct_eq("abc", "abc123"));
        assert!(!ct_eq("", "123456"));
        assert!(ct_eq("", ""));
        // remote_pair goes through ct_eq for both the PIN and the secret, never `==`.
        let src = include_str!("server.rs");
        let body = &src[src.find("async fn remote_pair").unwrap()..src.find("async fn remote_status").unwrap()];
        assert!(body.contains("ct_eq(req.pair.trim(), &p.secret)") && body.contains("ct_eq(req.pin.trim(), &p.pin)"));
        assert!(!body.contains("!= p.pin") && !body.contains("== p.secret"));
    }


    /// A paired phone with session cookie `rp_session=<token>`, and the headers it sends.
    fn paired(app: &App, token: &str, may_draw: bool) -> (Uuid, HeaderMap) {
        let id = Uuid::new_v4();
        app.phones.lock().unwrap().push(Phone { id, name: "iPad".into(), hash: hash_token(token), last_seen: now_secs(), may_draw });
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, format!("rp_session={token}").parse().unwrap());
        h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
        (id, h)
    }

    async fn body_json(r: Response) -> (StatusCode, Value) {
        let status = r.status();
        let b = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&b).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn the_live_slide_picture_is_for_paired_phones_only() {
        let app = test_app();
        let get = |h: HeaderMap, w| remote_live_slide(State(app.clone()), h, Query(SlideSize { w }));
        assert_eq!(get(HeaderMap::new(), None).await.status(), StatusCode::UNAUTHORIZED);
        let mut wrong = HeaderMap::new();
        wrong.insert(header::COOKIE, "rp_session=guess".parse().unwrap());
        assert_eq!(get(wrong, None).await.status(), StatusCode::UNAUTHORIZED);
        let (_, h) = paired(&app, "secret-token", false);
        let r = get(h, Some(320)).await;
        assert_eq!(r.status(), StatusCode::OK, "seeing the slide needs no drawing permission");
        assert_eq!(r.headers()[header::CONTENT_TYPE], "image/jpeg");
        let b = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
        let img = image::load_from_memory_with_format(&b, image::ImageFormat::Jpeg).unwrap();
        assert_eq!((img.width(), img.height()), (320, 180), "the size the phone asked for, 16:9");
        assert_eq!(live_slide_size(10, (1920, 1080)), (160, 90));
        assert_eq!(live_slide_size(9999, (1920, 1080)), (1920, 1080));
        // Over the network too: no cookie, no picture.
        let addr = served(test_app()).await;
        assert_eq!(status_of(addr, "GET", "/remote/live-slide?w=320").await, 401);
    }

    #[tokio::test]
    async fn the_live_slide_picture_shows_the_live_text_slide() {
        let app = test_app();
        let id = Uuid::new_v4();
        app.data.lock().unwrap().services[0].items.push(ServiceItem { id, kind: ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses: vec![VerseText::new("John 3:16", "For God so loved the world")], per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() }, background: Some(Background::Color { color: "#204080".into() }), notes: String::new(), theme: None });
        let base = app.live.lock().unwrap().version;
        assert!(app.command(Envelope { id: "s".into(), base: Some(base), cmd: Command::Show { item_id: id, slide: 0 } }).ok);
        let jpeg = live_slide_jpeg(&app, 320).unwrap();
        let img = image::load_from_memory(&jpeg).unwrap().into_rgb8();
        let p = img.get_pixel(4, 4);
        assert!(p[2] > 100 && p[0] < 70, "the slide's blue background, not black: {p:?}");
        assert!(img.pixels().any(|p| p[0] > 200 && p[1] > 200 && p[2] > 200), "the white words");
    }

    #[tokio::test]
    async fn the_phone_picture_and_view_follow_the_rooms_shape() {
        let app = test_app();
        assert_eq!(app.room_canvas(), (1920, 1080), "16:9 until a audience screen says otherwise");
        app.set_room_canvas((1024, 768));
        assert_eq!(app.room_canvas(), (1024, 768));
        app.set_room_canvas((0, 768));
        assert_eq!(app.room_canvas(), (1024, 768), "an empty size is ignored");
        assert_eq!(live_slide_size(320, (1024, 768)), (320, 240));
        assert_eq!(live_slide_size(400, (1080, 1920)), (400, 711), "a portrait screen");
        assert_eq!(live_slide_size(1920, (3000, 100)), (1920, 64));
        assert_eq!(live_slide_size(320, (0, 0)), (320, 180));
        let (me, h) = paired(&app, "tok", true);
        let r = remote_live_slide(State(app.clone()), h, Query(SlideSize { w: Some(320) })).await;
        let b = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
        let img = image::load_from_memory_with_format(&b, image::ImageFormat::Jpeg).unwrap();
        assert_eq!((img.width(), img.height()), (320, 240), "4:3, the room's shape, so lines land where they are drawn");
        let v: Value = serde_json::from_str(&remote_view(&app, Some(me))).unwrap();
        assert_eq!(v["aspect"], json!([1024, 768]));
    }

    #[tokio::test]
    async fn drawing_from_the_phone_checks_pairing_permission_and_input() {
        let app = test_app();
        let draw = |h: HeaderMap, body: Value| remote_draw(State(app.clone()), h, Bytes::from(body.to_string()));
        let start = json!({ "op": "start", "stroke": 1, "color": "#ffd60a", "width": 12, "pen": "pen", "points": [[0.1, 0.2]] });
        let (s, _) = body_json(draw(HeaderMap::new(), start.clone()).await).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "not JSON from this page");
        let mut unpaired = HeaderMap::new();
        unpaired.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
        assert_eq!(body_json(draw(unpaired, start.clone()).await).await.0, StatusCode::UNAUTHORIZED);
        let (_, no) = paired(&app, "no-token", false);
        let (s, v) = body_json(draw(no, start.clone()).await).await;
        assert_eq!((s, v["ok"].as_bool()), (StatusCode::OK, Some(false)));
        assert_eq!(v["error"], crate::live::drawing::actions::not_allowed());
        let (me, yes) = paired(&app, "yes-token", true);
        assert_eq!(body_json(draw(yes.clone(), start).await).await.1["ok"], true);
        let more = json!({ "op": "more", "stroke": 1, "points": [[0.3, 0.2], [0.5, 0.25]] });
        assert_eq!(body_json(draw(yes.clone(), more).await).await.1["ok"], true);
        let off = json!({ "op": "more", "stroke": 1, "points": [[1.5, 0.2]] });
        assert_eq!(body_json(draw(yes.clone(), off).await).await.1["ok"], false);
        let (s, _) = body_json(remote_draw(State(app.clone()), yes.clone(), Bytes::from_static(b"{nope")).await).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let colour = json!({ "op": "start", "stroke": 2, "color": "#123456", "width": 12, "pen": "pen", "points": [[0.1, 0.2]] });
        assert_eq!(body_json(draw(yes.clone(), colour).await).await.1["ok"], false);
        let d = app.live.lock().unwrap().drawing.clone();
        assert_eq!((d.strokes.len(), d.points()), (1, 3));
        assert_eq!(d.strokes[0].phone, me);
        let v: Value = serde_json::from_str(&remote_view(&app, Some(me))).unwrap();
        assert_eq!((v["may_draw"].as_bool(), v["my_strokes"].clone()), (Some(true), json!([1])));
        let v: Value = serde_json::from_str(&remote_view(&app, None)).unwrap();
        assert_eq!(v["may_draw"], false);
    }

    #[test]
    fn a_phone_hears_only_when_its_own_strokes_change() {
        use crate::live::drawing::{DrawCmd, Drawing, Pen};
        let (me, you) = (Uuid::new_v4(), Uuid::new_v4());
        let app = test_app();
        let map = remote_map(me);
        let mut d = Drawing::default();
        let send = |d: &Drawing| map(&app, Some(&Update::Drawing(Arc::new(d.clone()))));
        d.apply(me, &DrawCmd::Start { stroke: 4, color: "#ffd60a".into(), width: 12.0, pen: Pen::Pen, points: vec![[0.1, 0.1]], ink: Default::default() }).unwrap();
        let first: Value = serde_json::from_str(&send(&d).unwrap().unwrap()).unwrap();
        assert_eq!(first, json!({ "type": "drawing", "mine": [4] }));
        d.apply(me, &DrawCmd::More { stroke: 4, points: vec![[0.2, 0.1]] }).unwrap();
        assert_eq!(send(&d), Ok(None), "more points: nothing to say");
        d.apply(you, &DrawCmd::Start { stroke: 1, color: "#ffd60a".into(), width: 12.0, pen: Pen::Pen, points: vec![[0.1, 0.1]], ink: Default::default() }).unwrap();
        assert_eq!(send(&d), Ok(None), "someone else's stroke");
        d.clear();
        assert_eq!(serde_json::from_str::<Value>(&send(&d).unwrap().unwrap()).unwrap()["mine"], json!([]));
    }

}
