//! N31: the stage screen on a phone or tablet, read only, at `/stage-view`: the same boxes as the stage window
//! (`stage::view`), sent as JSON over Server-Sent Events and drawn in HTML by `web/stage-view.js`. Off by default;
//! with a password, `/stage-view/login` sets a sign-in cookie (only its hash is kept, in memory).
use super::{password, strings as s};
use crate::{
    app::App,
    model::{Slide, SlideContent},
    stage::{self, Align, ElementKind, VAlign, StageState, view::{self, CellContent, StageInputs}},
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response, sse::{Event, KeepAlive, Sse}},
    routing::{get, post},
};
use futures_util::stream;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{net::SocketAddr, sync::Arc, time::{Duration, Instant}};

/// How long a stage view stays signed in, and how many sign-ins are kept.
const SIGN_IN: Duration = Duration::from_secs(7 * 86400);
const MAX_SIGN_INS: usize = 20;

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/stage-view", get(page))
        .route("/static/stage-view.js", get(script))
        .route("/stage-view/status", get(status))
        .route("/stage-view/login", post(login))
        .route("/stage-view/events", get(events))
        .route("/stage-view/next.jpg", get(next_picture))
        .route("/stage-view/live.jpg", get(live_picture))
}

fn asset(body: &'static str, ctype: &'static str) -> Response {
    ([(header::CONTENT_TYPE, ctype), (header::CACHE_CONTROL, "no-store")], body).into_response()
}
async fn page() -> Response { asset(include_str!("../../web/stage-view.html"), "text/html; charset=utf-8") }
async fn script() -> Response { asset(include_str!("../../web/stage-view.js"), "text/javascript; charset=utf-8") }

fn cookie(headers: &HeaderMap) -> Option<String> {
    headers.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix("rp_stage=").map(str::to_string))
}

/// Whether this request may see the stage: the stage view is on, and there is no password or the cookie is a
/// current sign-in.
pub fn allowed(app: &App, headers: &HeaderMap) -> bool {
    let (on, pw) = { let n = &app.data.lock().unwrap().settings.network; (n.stage_view, n.stage_password.is_some()) };
    if !on { return false }
    if !pw { return true }
    let Some(token) = cookie(headers) else { return false };
    let h = crate::app::hash_token(&token);
    let mut tokens = app.net.stage_tokens.lock().unwrap();
    tokens.retain(|(_, at)| at.elapsed() < SIGN_IN);
    tokens.iter().any(|(t, _)| crate::app::ct_eq(t, &h))
}

async fn status(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let (on, pw) = { let n = &app.data.lock().unwrap().settings.network; (n.stage_view, n.stage_password.is_some()) };
    Json(json!({ "on": on, "password": pw, "allowed": allowed(&app, &headers), "off_text": s::stage_view_off() })).into_response()
}

#[derive(Deserialize)]
struct Login { #[serde(default)] password: String }

fn same_origin(headers: &HeaderMap) -> bool {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    headers.get(header::ORIGIN).and_then(|h| h.to_str().ok()).is_none_or(|o| o == format!("http://{host}"))
}

fn err(status: StatusCode, msg: String) -> Response { (status, Json(json!({ "error": msg }))).into_response() }

async fn login(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(req): Json<Login>) -> Response {
    if !same_origin(&headers) { return StatusCode::FORBIDDEN.into_response() }
    let (on, hash) = { let n = &app.data.lock().unwrap().settings.network; (n.stage_view, n.stage_password.clone()) };
    if !on { return err(StatusCode::NOT_FOUND, s::stage_view_off()) }
    let Some(hash) = hash else { return Json(json!({ "ok": true })).into_response() };
    if !app.net.may_try(addr.ip()) { return err(StatusCode::TOO_MANY_REQUESTS, s::too_fast()) }
    let right = tokio::task::spawn_blocking(move || password::verify(&req.password, &hash)).await.unwrap_or(false);
    if !right { return err(StatusCode::UNAUTHORIZED, s::wrong_password()) }
    let token = crate::app::random_token();
    {
        let mut tokens = app.net.stage_tokens.lock().unwrap();
        if tokens.len() >= MAX_SIGN_INS { tokens.remove(0); }
        tokens.push((crate::app::hash_token(&token), Instant::now()));
    }
    let c = format!("rp_stage={token}; HttpOnly; SameSite=Strict; Path=/stage-view; Max-Age={}", SIGN_IN.as_secs());
    ([(header::SET_COOKIE, c)], Json(json!({ "ok": true }))).into_response()
}

/// The stage as the stage window would draw it now, as JSON for `stage-view.js`.
pub fn stage_json(app: &App, state: &mut StageState) -> Value {
    let live = app.live_view();
    let timers = app.timers();
    let layout = {
        let d = app.data.lock().unwrap();
        stage::active_layout(&d.stage_layouts, None, None, Some(d.settings.stage.layout)).clone()
    };
    let slides = app.live.lock().unwrap().item.as_ref().map(|i| i.slides.clone()).unwrap_or_default();
    state.track_timers(&timers);
    let v = view::build(&StageInputs { live: &live, slides: &slides, timers: &timers, state, now: chrono::Local::now().time() }, &layout);
    // A picture slide (PowerPoint, PDF) or a designed slide shows as its picture: the live one or the next one.
    let drawn = |s: &Slide, content: &CellContent| matches!(content, CellContent::Picture { .. }) || matches!(s.content, SlideContent::Design { .. });
    let on_air = live.frame.slide.is_some() || live.frame.media.is_some();
    let picture_of = |c: &view::Cell| -> Option<&'static str> {
        let kind = layout.elements.iter().filter_map(|e| e.known()).find(|e| e.id == c.element).map(|e| &e.kind)?;
        match kind {
            ElementKind::CurrentSlide { .. } if live.stage_only.is_none() && on_air => live.slide.as_ref().filter(|s| drawn(s, &c.content)).map(|_| "live"),
            ElementKind::NextSlide { .. } => live.next.as_ref().filter(|n| drawn(&n.slide, &c.content)).map(|_| "next"),
            _ => None,
        }
    };
    let cells: Vec<Value> = v.cells.iter().filter_map(|c| {
        let st = &c.style;
        let mut cell = json!({
            "x": c.rect.x, "y": c.rect.y, "w": c.rect.w, "h": c.rect.h, "color": st.color, "fill": st.fill,
            "max": st.max_size, "min": st.min_size, "bold": st.bold,
            "align": match st.align { Align::Left => "left", Align::Center => "center", Align::Right => "right" },
            "valign": match st.valign { VAlign::Top => "top", VAlign::Middle => "middle", VAlign::Bottom => "bottom" },
        });
        let (kind, extra) = match &c.content {
            CellContent::Empty => return None,
            CellContent::Picture { caption, .. } | CellContent::Text { caption, .. } | CellContent::Designed { caption, .. } if let Some(src) = picture_of(c) => ("picture", json!({ "caption": caption, "src": src })),
            CellContent::Text { text, dim, caption, red } => ("text", json!({ "text": text, "dim": dim, "caption": caption, "red": red })),
            CellContent::Picture { caption, .. } => ("text", json!({ "text": stage::strings::slide_picture(), "caption": caption })),
            CellContent::Designed { words, caption, .. } => ("text", json!({ "text": words, "caption": caption })),
            CellContent::Message { text } => ("message", json!({ "text": text })),
            CellContent::NextPicture { caption } => ("picture", json!({ "caption": caption, "src": "next" })),
            CellContent::Shape { shape } => ("shape", json!({ "shape": shape })),
        };
        cell["kind"] = kind.into();
        if let (Value::Object(c), Value::Object(e)) = (&mut cell, extra) { c.extend(e) }
        Some(cell)
    }).collect();
    json!({ "type": "stage", "background": v.background, "cells": cells, "banner": v.message_fallback, "version": live.version })
}

/// Live stage updates: a new view when anything changes and once a second for the clock and timers, sent only
/// when it differs. The stream ends when the stage view is turned off or this sign-in is gone.
async fn events(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !allowed(&app, &headers) { return StatusCode::UNAUTHORIZED.into_response() }
    let rx = app.subscribe();
    let tick = tokio::time::interval(Duration::from_secs(1));
    let s = stream::unfold((app, headers, rx, tick, StageState::default(), String::new()), |(app, headers, mut rx, mut tick, mut state, mut last)| async move {
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                r = rx.recv() => if matches!(r, Err(tokio::sync::broadcast::error::RecvError::Closed)) { return None },
            }
            if !allowed(&app, &headers) { return None }
            let out = stage_json(&app, &mut state).to_string();
            if out != last {
                last.clone_from(&out);
                return Some((Ok::<_, std::convert::Infallible>(Event::default().data(out)), (app, headers, rx, tick, state, last)));
            }
        }
    });
    Sse::new(s).keep_alive(KeepAlive::new().interval(Duration::from_secs(10))).into_response()
}

#[derive(Deserialize)]
struct Size { #[serde(default)] w: Option<u32> }

/// The next slide's picture as a JPEG, `w` pixels wide (16:9).
async fn next_picture(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<Size>) -> Response {
    if !allowed(&app, &headers) { return StatusCode::UNAUTHORIZED.into_response() }
    let size = crate::server::live_slide_size(q.w.unwrap_or(640), (16, 9));
    let jpeg = tokio::task::spawn_blocking(move || {
        static RENDER: crate::ui::thumbs::TextRenderer = crate::ui::thumbs::TextRenderer::new();
        let px = crate::ui::thumbs::next_slide(&app, size, &RENDER);
        let rgb: Vec<u8> = px.as_bytes().chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode(&rgb, size.0, size.1, image::ExtendedColorType::Rgb8).map(|_| out)
    }).await;
    match jpeg {
        Ok(Ok(b)) => ([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store")], b).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The slide on the room screen now as a JPEG, `w` pixels wide, the room's shape (the phone remote's picture).
async fn live_picture(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<Size>) -> Response {
    if !allowed(&app, &headers) { return StatusCode::UNAUTHORIZED.into_response() }
    match tokio::task::spawn_blocking(move || crate::server::live_slide_jpeg(&app, q.w.unwrap_or(640))).await {
        Ok(Ok(b)) => ([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store")], b).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;
    use axum::http::HeaderValue;

    fn turn_on(app: &App, pw: Option<&str>) {
        let mut d = app.data.lock().unwrap();
        d.settings.network.stage_view = true;
        d.settings.network.stage_password = pw.map(password::hash);
    }

    async fn sign_in(app: &Arc<App>, ip: u8, pw: &str) -> (StatusCode, Option<String>) {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_static("192.168.1.20:4318"));
        let addr: SocketAddr = format!("192.168.1.{ip}:5000").parse().unwrap();
        let r = login(State(app.clone()), ConnectInfo(addr), h, Json(Login { password: pw.into() })).await;
        let c = r.headers().get(header::SET_COOKIE).map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
        (r.status(), c)
    }

    fn with_cookie(c: &str) -> HeaderMap { let mut h = HeaderMap::new(); h.insert(header::COOKIE, c.parse().unwrap()); h }

    #[tokio::test]
    async fn off_by_default_then_open_or_behind_a_password() {
        let app = test_app();
        assert!(!allowed(&app, &HeaderMap::new()), "off by default");
        assert_eq!(events(State(app.clone()), HeaderMap::new()).await.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(sign_in(&app, 2, "x").await.0, StatusCode::NOT_FOUND);
        turn_on(&app, None);
        assert!(allowed(&app, &HeaderMap::new()), "no password: open to the church network");
        turn_on(&app, Some("altos"));
        assert!(!allowed(&app, &HeaderMap::new()));
        assert_eq!(sign_in(&app, 2, "sopranos").await, (StatusCode::UNAUTHORIZED, None));
        assert_eq!(sign_in(&app, 2, "altos").await.0, StatusCode::TOO_MANY_REQUESTS, "one try a second");
        let (s, c) = sign_in(&app, 3, "altos").await;
        assert_eq!(s, StatusCode::OK);
        let c = c.unwrap();
        assert!(allowed(&app, &with_cookie(&c)));
        assert!(!allowed(&app, &with_cookie("rp_stage=guess")));
        let stored = app.net.stage_tokens.lock().unwrap().clone();
        assert!(!stored.iter().any(|(t, _)| c.contains(t.as_str())), "only the hash is kept");
        // Turned off: even a signed-in tablet sees nothing.
        app.data.lock().unwrap().settings.network.stage_view = false;
        assert!(!allowed(&app, &with_cookie(&c)));
    }

    #[test]
    fn the_json_has_the_stage_layouts_boxes() {
        let app = test_app();
        app.data.lock().unwrap().settings.stage.layout = stage::presets::CLOCK_COUNTDOWN;
        let v = stage_json(&app, &mut StageState::default());
        assert_eq!(v["type"], "stage");
        let cells = v["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 1, "only the clock has something to show");
        assert_eq!((cells[0]["kind"].as_str(), cells[0]["x"].as_f64(), cells[0]["max"].as_f64()), (Some("text"), Some(5.0), Some(30.0)));
        assert!(cells[0]["text"].as_str().unwrap().contains(':'));
    }

    /// Shows slide 0 of a new item with `slides`.
    fn show(app: &Arc<App>, kind: crate::model::ItemKind) {
        use crate::live::{Command, Envelope};
        let id = uuid::Uuid::new_v4();
        app.data.lock().unwrap().services[0].items.push(crate::model::ServiceItem { id, kind, background: None, notes: String::new(), theme: None });
        let base = app.live.lock().unwrap().version;
        assert!(app.command(Envelope { id: "s".into(), base: Some(base), cmd: Command::Show { item_id: id, slide: 0 } }).ok);
    }

    /// The slide boxes of the Classic layout: (kind, src or text).
    fn slide_boxes(app: &App) -> Vec<(String, String)> {
        app.data.lock().unwrap().settings.stage.layout = stage::presets::CLASSIC;
        let v = stage_json(app, &mut StageState::default());
        v["cells"].as_array().unwrap().iter().filter(|c| c["y"].as_f64() == Some(10.0) || c["y"].as_f64() == Some(76.0))
            .map(|c| (c["kind"].as_str().unwrap().to_string(), c["src"].as_str().or(c["text"].as_str()).unwrap_or_default().to_string())).collect()
    }

    #[tokio::test]
    async fn picture_and_designed_slides_show_their_pictures() {
        let app = test_app();
        let deck = crate::model::Deck { id: uuid::Uuid::new_v4(), title: "Talk".into(), slides: vec!["a.png".into(), "b.png".into()], notes: vec![], transitions: vec![], advance_ms: vec![], show: None };
        app.data.lock().unwrap().decks.push(deck.clone());
        show(&app, crate::model::ItemKind::Deck { deck_id: deck.id });
        assert_eq!(slide_boxes(&app), [("picture".into(), "live".into()), ("picture".into(), "next".into())], "not \"(slide picture)\"");
        let app = test_app();
        show(&app, crate::model::ItemKind::Design { title: "Welcome".into(), slides: vec![crate::design::Design::default(); 2] });
        assert_eq!(slide_boxes(&app), [("picture".into(), "live".into()), ("picture".into(), "next".into())]);
        // Words stay words.
        let app = test_app();
        show(&app, crate::model::ItemKind::Text { title: "T".into(), body: "Hello".into(), layout: Default::default() });
        assert_eq!(slide_boxes(&app)[0], ("text".into(), "Hello".into()));
    }

    #[tokio::test]
    async fn the_pages_are_served_and_the_picture_needs_permission() {
        let app = test_app();
        assert_eq!(page().await.status(), StatusCode::OK);
        assert_eq!(script().await.status(), StatusCode::OK);
        assert_eq!(next_picture(State(app.clone()), HeaderMap::new(), Query(Size { w: Some(320) })).await.status(), StatusCode::UNAUTHORIZED);
        turn_on(&app, None);
        let r = next_picture(State(app.clone()), HeaderMap::new(), Query(Size { w: Some(320) })).await;
        assert_eq!((r.status(), r.headers()[header::CONTENT_TYPE].to_str().unwrap()), (StatusCode::OK, "image/jpeg"));
        let r = live_picture(State(app.clone()), HeaderMap::new(), Query(Size { w: Some(320) })).await;
        assert_eq!((r.status(), r.headers()[header::CONTENT_TYPE].to_str().unwrap()), (StatusCode::OK, "image/jpeg"));
        let b: Value = serde_json::from_slice(&axum::body::to_bytes(status(State(app.clone()), HeaderMap::new()).await.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!((b["on"].as_bool(), b["password"].as_bool(), b["allowed"].as_bool()), (Some(true), Some(false), Some(true)));
        app.data.lock().unwrap().settings.network.stage_view = false;
        assert_eq!(live_picture(State(app.clone()), HeaderMap::new(), Query(Size { w: Some(320) })).await.status(), StatusCode::UNAUTHORIZED);
    }
}
