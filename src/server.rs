//! HTTP host for the phone remote, with live updates over Server-Sent Events, behind PIN pairing. `/api/do/*` answers
//! only this computer (loopback): single-instance focus, tests and local automation.
use crate::app::{App, DataPlan, Update, MAX_PHONES, Phone, SESSION_DAYS, ct_eq, hash_token, now_secs, random_token};
use crate::{live::*, model::*, slides::build_item};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, Path as UrlPath, State},
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

async fn remote_page() -> Response { asset_response(page!("remote.html"), "text/html; charset=utf-8") }
async fn remote_js() -> Response { asset_response(page!("remote.js"), "text/javascript; charset=utf-8") }

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
struct PairReq {
    #[serde(default)] pin: String,
    /// The one-time secret from the QR code's `#pair=` (instead of the PIN).
    #[serde(default)] pair: String,
    #[serde(default)] name: String,
}

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
    let by_qr = !req.pair.trim().is_empty();
    let right = if by_qr { ct_eq(req.pair.trim(), &p.secret) } else { ct_eq(req.pin.trim(), &p.pin) };
    if !right {
        p.failures += 1;
        if p.failures >= 5 { *pairing = None; return bad("Too many wrong codes. Start pairing again on the computer."); }
        return bad(if by_qr { "That QR code has expired. On the computer, click Pair a phone again." } else { "That code isn't right." });
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
        Some(Update::Job { .. }) => Ok(None),
        _ => Ok(Some(remote_view(app))),
    }
}

async fn remote_events(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let Some(phone) = phone_for(&app, &headers) else { return (StatusCode::UNAUTHORIZED, "pair first").into_response() };
    let first = remote_view(&app);
    let s = event_stream(app, first, remote_map(phone));
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
        SlideContent::Text { text, parallel, .. } => crate::model::all_bibles_text(text, parallel.as_ref()),
        SlideContent::Image { .. } => "(slide picture)".into(),
        SlideContent::Video { .. } => "(video)".into(),
        SlideContent::Still { .. } => "(picture)".into(),
        SlideContent::Audio { .. } => "(audio)".into(),
    }
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/static/remote.js", get(remote_js))
        .route("/api/do/{*action}", post(api).layer(DefaultBodyLimit::max(64 * 1024 * 1024)))
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
        assert!(matches!(map(&app, Some(&Update::Data)), Ok(Some(_))));
        assert!(matches!(map(&app, None), Ok(Some(_))));
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

    #[tokio::test]
    async fn remote_page_still_served() {
        let addr = served(test_app()).await;
        for path in ["/remote", "/static/remote.js", "/remote/status"] {
            assert_eq!(status_of(addr, "GET", path).await, 200, "{path}");
        }
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
        let (s, c, _) = pair(&app, 2, json!({ "pair": secret })).await;
        assert_eq!((s, c), (StatusCode::BAD_REQUEST, None));
        assert!(app.phones.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_wrong_secret_counts_as_a_failure() {
        let app = lan_app();
        let (_, _, secret) = start(&app).await;
        let (s, _, v) = pair(&app, 2, json!({ "pair": "0".repeat(64) })).await;
        assert_eq!((s, v["error"].as_str()), (StatusCode::BAD_REQUEST, Some("That QR code has expired. On the computer, click Pair a phone again.")));
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

}
