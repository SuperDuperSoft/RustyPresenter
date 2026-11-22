//! N31: the stage screen on a phone or tablet, read only, at `/stage-view`: the same boxes as the stage window
//! (`stage::view`), sent as JSON over Server-Sent Events and drawn in HTML by `web/stage-view.js`. Off by default;
//! with a password, `/stage-view/login` sets a sign-in cookie (only its hash is kept, in memory).
use super::{password, slide_pictures::{self, Which}, strings as s};
use crate::{
    app::{App, LiveUpdate, Update},
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
        .route("/stage-view/tally", get(tally_page))
        .route("/stage-view/tally.json", get(tally))
}

async fn tally_page() -> Response { asset(include_str!("../../web/tally.html"), "text/html; charset=utf-8") }

#[derive(Deserialize)]
struct TallyAsk { camera: Option<usize> }

/// A camera's tally light (`stream::tally`): `camera` is 1 for the first.
async fn tally(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<TallyAsk>) -> Response {
    use crate::stream::tally::{Light, tr};
    let camera = q.camera.unwrap_or(1).clamp(1, 99);
    if !allowed(&app, &headers) {
        return Json(json!({ "allowed": false, "light": "off", "text": tr::needs_stage_view() })).into_response();
    }
    let t = *app.tally.lock().unwrap();
    let light = t.light(camera - 1);
    let text = match light { Light::OnAir => tr::on_air(camera), Light::Next => tr::next(camera), Light::Off => tr::off(camera) };
    let sub = if t.live { String::new() } else { tr::not_live() };
    ([(header::CACHE_CONTROL, "no-store")], Json(json!({ "allowed": true, "light": light.code(), "text": text, "sub": sub }))).into_response()
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

/// Whether this request may see the stage: it comes from a phone paired as a remote (`paired`), or the stage view
/// is on and there is no password or the cookie is a current sign-in.
pub fn allowed(app: &App, headers: &HeaderMap) -> bool {
    if paired(app, headers) { return true }
    let data = app.data.lock().unwrap();
    let network = &data.settings.network;
    if !network.stage_view { return false }
    if network.stage_password.is_none() { return true }
    let Some(token) = cookie(headers) else { return false };
    let h = crate::app::hash_token(&token);
    let mut tokens = app.net.stage_tokens.lock().unwrap();
    tokens.retain(|(_, at)| at.elapsed() < SIGN_IN);
    tokens.iter().any(|(t, _)| crate::app::ct_eq(t, &h))
}

/// A phone paired as a remote. It can already change every slide, so it may always see the stage screen (and with
/// it the tally light and the microphone), whether or not the stage screen is switched on for other phones, and
/// without the stage password: that switch and password are for phones that are not paired.
fn paired(app: &App, headers: &HeaderMap) -> bool { crate::server::phone_for(app, headers).is_some() }

async fn status(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let remote = paired(&app, &headers);
    let (on, pw) = { let n = &app.data.lock().unwrap().settings.network; (n.stage_view, n.stage_password.is_some()) };
    Json(json!({ "on": on || remote, "password": pw && !remote, "allowed": allowed(&app, &headers), "off_text": s::stage_view_off() })).into_response()
}

#[derive(Deserialize)]
struct Login { #[serde(default)] password: String }

/// The request comes from a page of this app (or from no page): over http, or the secure pages
/// (`network::secure`), which the phone's microphone needs.
pub(crate) fn same_origin(headers: &HeaderMap) -> bool {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    headers.get(header::ORIGIN).and_then(|h| h.to_str().ok()).is_none_or(|o| o == format!("http://{host}") || o == format!("https://{host}"))
}

fn err(status: StatusCode, msg: String) -> Response { (status, Json(json!({ "error": msg }))).into_response() }

async fn login(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(req): Json<Login>) -> Response {
    if !same_origin(&headers) { return StatusCode::FORBIDDEN.into_response() }
    let (on, hash) = { let n = &app.data.lock().unwrap().settings.network; (n.stage_view, n.stage_password.clone()) };
    if !on { return err(StatusCode::NOT_FOUND, s::stage_view_off()) }
    let Some(hash) = hash else { return Json(json!({ "ok": true })).into_response() };
    if !app.net.may_try(addr.ip()) { return err(StatusCode::TOO_MANY_REQUESTS, s::too_fast()) }
    let checked = tokio::task::spawn_blocking(move || {
        password::verify(&req.password, &hash).then_some(hash)
    }).await.ok().flatten();
    let Some(hash) = checked else { return err(StatusCode::UNAUTHORIZED, s::wrong_password()) };
    stage_sign_in(&app, &hash)
}

fn stage_sign_in(app: &App, verified_hash: &str) -> Response {
    let token = crate::app::random_token();
    {
        let data = app.data.lock().unwrap();
        let network = &data.settings.network;
        if !network.stage_view { return err(StatusCode::NOT_FOUND, s::stage_view_off()) }
        if network.stage_password.as_deref() != Some(verified_hash) {
            return err(StatusCode::UNAUTHORIZED, s::wrong_password());
        }
        let mut tokens = app.net.stage_tokens.lock().unwrap();
        if tokens.len() >= MAX_SIGN_INS { tokens.remove(0); }
        tokens.push((crate::app::hash_token(&token), Instant::now()));
    }
    let c = format!("rp_stage={token}; HttpOnly; SameSite=Strict; Path=/stage-view; Max-Age={}", SIGN_IN.as_secs());
    ([(header::SET_COOKIE, c)], Json(json!({ "ok": true }))).into_response()
}

/// The stage as the stage window would draw it now, as JSON for `stage-view.js`.
pub fn stage_json(app: &App, state: &mut StageState) -> Value { stage_json_of(app, state, &app.live_view()).0 }

/// `stage_json` for a live view already in hand (the one an update carried, so none is built again), and whether
/// the layout shows the time: a clock or a timer, which a view draws again every second.
fn stage_json_of(app: &App, state: &mut StageState, live: &crate::app::LiveView) -> (Value, bool) {
    let timers = app.timers();
    let layout = {
        let d = app.data.lock().unwrap();
        stage::active_layout(&d.stage_layouts, None, None, Some(d.settings.stage.layout)).clone()
    };
    let shows_time = layout.elements.iter().filter_map(|e| e.known()).any(|e| matches!(e.kind, ElementKind::Clock { .. } | ElementKind::Timer { .. }));
    state.track_timers(&timers);
    let v = {
        // The live item's slides are read where they are (the lock is held for the layout, which waits for nothing).
        let held = app.live.lock().unwrap();
        let slides: &[Slide] = held.item.as_ref().map_or(&[], |i| i.slides.as_slice());
        view::build(&StageInputs { live, slides, timers: &timers, state, now: chrono::Local::now().time() }, &layout)
    };
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
            // A chord sheet: `lines` of [chord, words] for this page; `text` (the chords in brackets) for a phone
            // still showing an older page, which draws it as words.
            CellContent::Chords { lines, caption, chord_color } => ("chords", json!({
                "text": stage::chords::plain(lines), "lines": lines, "chord_color": chord_color, "caption": caption,
            })),
        };
        cell["kind"] = kind.into();
        if let (Value::Object(c), Value::Object(e)) = (&mut cell, extra) { c.extend(e) }
        Some(cell)
    }).collect();
    (json!({ "type": "stage", "background": v.background, "cells": cells, "banner": v.message_fallback, "follow": v.follow_line, "version": live.version }), shows_time)
}

/// What an update means for a stage view that holds `live`: whether to draw the stage again. A live view the
/// update carried replaces `live`; import progress and lines drawn on the slide are not on the stage view.
fn takes(update: Result<Update, tokio::sync::broadcast::error::RecvError>, live: &mut LiveUpdate, app: &App) -> Option<bool> {
    use tokio::sync::broadcast::error::RecvError;
    Some(match update {
        Ok(Update::Live(v) | Update::Playback(v)) => { *live = v; true }
        Ok(Update::Job { .. } | Update::Drawing(_)) => false,
        // The data (a new stage layout), a timer readout, a phone taken off the list.
        Ok(_) => true,
        // Fell behind: the view as it is now.
        Err(RecvError::Lagged(_)) => { *live = LiveUpdate::new(app.live_view()); true }
        Err(RecvError::Closed) => return None,
    })
}

/// Live stage updates: a new view when something it shows changes, and once a second while the layout shows a
/// clock or a timer; sent only when it differs. The live view comes from the update itself (none is built per
/// phone). The stream ends when the stage view is turned off or this sign-in is gone.
async fn events(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !allowed(&app, &headers) { return StatusCode::UNAUTHORIZED.into_response() }
    // Subscribed before the first view is read, so no update between the two is missed.
    let rx = app.subscribe();
    let live = LiveUpdate::new(app.live_view());
    let tick = tokio::time::interval(Duration::from_secs(1));
    // The first tick comes at once and draws the first view; after that a tick draws only while time shows.
    let start = (app, headers, rx, tick, StageState::default(), String::new(), live, true);
    let s = stream::unfold(start, |(app, headers, mut rx, mut tick, mut state, mut last, mut live, mut shows_time)| async move {
        loop {
            let draw = tokio::select! {
                _ = tick.tick() => shows_time,
                r = rx.recv() => takes(r, &mut live, &app)?,
            };
            if !draw { continue }
            if !allowed(&app, &headers) { return None }
            let (view, time) = stage_json_of(&app, &mut state, live.view());
            shows_time = time;
            let out = view.to_string();
            if out != last {
                last.clone_from(&out);
                return Some((Ok::<_, std::convert::Infallible>(Event::default().data(out)), (app, headers, rx, tick, state, last, live, shows_time)));
            }
        }
    });
    Sse::new(s).keep_alive(KeepAlive::new().interval(Duration::from_secs(10))).into_response()
}

#[derive(Deserialize)]
struct Size { #[serde(default)] w: Option<u32> }

/// The next slide's picture as a JPEG, `w` pixels wide (16:9). Drawn two at a time at most, whoever asks
/// (`slide_pictures`): with no password set, every device on the network may ask.
async fn next_picture(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<Size>) -> Response {
    if !allowed(&app, &headers) { return StatusCode::UNAUTHORIZED.into_response() }
    slide_pictures::respond(app, Which::Next, q.w.unwrap_or(640)).await
}

/// The slide on the audience screen now as a JPEG, `w` pixels wide, the room's shape (the phone remote's picture).
async fn live_picture(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<Size>) -> Response {
    if !allowed(&app, &headers) { return StatusCode::UNAUTHORIZED.into_response() }
    slide_pictures::respond(app, Which::Live, q.w.unwrap_or(640)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;

    /// Q7: a stage view takes the live view an update carries, and draws nothing for import progress or drawing.
    #[test]
    fn a_stage_view_draws_again_only_for_what_it_shows() {
        use tokio::sync::broadcast::error::RecvError;
        let app = test_app();
        let mut live = LiveUpdate::new(app.live_view());
        assert_eq!(takes(Ok(Update::Job { id: uuid::Uuid::new_v4(), percent: 40 }), &mut live, &app), Some(false));
        assert_eq!(takes(Ok(Update::Drawing(Default::default())), &mut live, &app), Some(false));
        assert_eq!(takes(Ok(Update::Data), &mut live, &app), Some(true), "the layout may be another one");
        assert_eq!(takes(Ok(Update::Timers(app.timers())), &mut live, &app), Some(true));
        // A live view, or a playback report, is the view from then on.
        let mut newer = app.live_view();
        newer.version = 41;
        assert_eq!(takes(Ok(Update::Playback(LiveUpdate::new(newer.clone()))), &mut live, &app), Some(true));
        assert_eq!(live.view().version, 41);
        newer.version = 42;
        assert_eq!(takes(Ok(Update::Live(LiveUpdate::new(newer))), &mut live, &app), Some(true));
        assert_eq!(live.view().version, 42);
        assert_eq!(takes(Err(RecvError::Lagged(9)), &mut live, &app), Some(true));
        assert_eq!(live.view().version, app.live_view().version, "after falling behind: the view as it is now");
        assert_eq!(takes(Err(RecvError::Closed), &mut live, &app), None);
    }
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

    #[test]
    fn a_login_checked_against_a_replaced_password_cannot_issue_a_cookie() {
        let app = test_app();
        turn_on(&app, Some("altos"));
        let old = app.data.lock().unwrap().settings.network.stage_password.clone().unwrap();
        turn_on(&app, Some("sopranos"));
        let response = stage_sign_in(&app, &old);
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(!response.headers().contains_key(header::SET_COOKIE));
    }

    fn with_cookie(c: &str) -> HeaderMap { let mut h = HeaderMap::new(); h.insert(header::COOKIE, c.parse().unwrap()); h }

    async fn tally_of(app: &Arc<App>, camera: usize, headers: HeaderMap) -> Value {
        let r = tally(State(app.clone()), headers, Query(TallyAsk { camera: Some(camera) })).await;
        let body = axum::body::to_bytes(r.into_body(), 1 << 16).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn a_cameras_tally_light_is_shown_with_the_stage_view() {
        let app = test_app();
        // Off with the stage view.
        let v = tally_of(&app, 1, HeaderMap::new()).await;
        assert_eq!((v["allowed"].as_bool(), v["light"].as_str()), (Some(false), Some("off")));
        turn_on(&app, None);
        let v = tally_of(&app, 1, HeaderMap::new()).await;
        assert_eq!((v["allowed"].as_bool(), v["light"].as_str()), (Some(true), Some("off")));
        assert_eq!(v["sub"].as_str(), Some(crate::stream::tally::tr::not_live().as_str()));
        *app.tally.lock().unwrap() = crate::stream::tally::Tally::of(true, Some(1), None, true);
        let (one, two) = (tally_of(&app, 1, HeaderMap::new()).await, tally_of(&app, 2, HeaderMap::new()).await);
        assert_eq!((one["light"].as_str(), two["light"].as_str()), (Some("off"), Some("on_air")));
        assert_eq!(two["text"].as_str(), Some(crate::stream::tally::tr::on_air(2).as_str()));
        // With a password, only a phone that signed in.
        turn_on(&app, Some("altos"));
        assert_eq!(tally_of(&app, 2, HeaderMap::new()).await["allowed"].as_bool(), Some(false));
        let (_, cookie) = sign_in(&app, 31, "altos").await;
        assert_eq!(tally_of(&app, 2, with_cookie(&cookie.unwrap())).await["light"].as_str(), Some("on_air"));
    }

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

    #[tokio::test]
    async fn a_phone_paired_as_a_remote_always_sees_the_stage() {
        use crate::app::{Phone, hash_token, now_secs};
        let app = test_app();
        app.phones.lock().unwrap().push(Phone { id: uuid::Uuid::new_v4(), name: "Pixel".into(), hash: hash_token("tok"), last_seen: now_secs(), may_draw: false });
        let remote = with_cookie("rp_session=tok");
        // The stage screen for phones is off, and a stranger sees nothing.
        assert!(!allowed(&app, &HeaderMap::new()) && !allowed(&app, &with_cookie("rp_session=guess")));
        assert!(allowed(&app, &remote), "the remote needs no switch");
        let r = status(State(app.clone()), remote.clone()).await;
        let v: Value = serde_json::from_slice(&axum::body::to_bytes(r.into_body(), 1 << 16).await.unwrap()).unwrap();
        assert_eq!((v["on"].as_bool(), v["allowed"].as_bool(), v["password"].as_bool()), (Some(true), Some(true), Some(false)));
        assert_eq!(events(State(app.clone()), remote.clone()).await.status(), StatusCode::OK);
        // Nor the stage password: that is for phones that are not paired.
        turn_on(&app, Some("altos"));
        assert!(allowed(&app, &remote) && !allowed(&app, &HeaderMap::new()));
        // Unpaired on the computer: it is a stranger again.
        app.phones.lock().unwrap().clear();
        app.data.lock().unwrap().settings.network.stage_view = false;
        assert!(!allowed(&app, &remote));
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

    #[test]
    fn a_song_with_chords_is_sent_to_the_phone_as_a_chord_sheet() {
        let app = test_app();
        let song = crate::model::Song { title: "T".into(), key: "D".into(), lyrics: "Verse 1\n[D]  Sometimes the [A/D]night is beautiful\nYou could [A7]touch it".into(), ..Default::default() };
        let song_id = song.id;
        app.data.lock().unwrap().songs.push(song);
        show(&app, crate::model::ItemKind::Song { song_id, arrangement: None });
        app.data.lock().unwrap().settings.stage.layout = stage::presets::BAND;
        let v = stage_json(&app, &mut StageState::default());
        let cell = v["cells"].as_array().unwrap().iter().find(|c| c["kind"] == "chords" && c["y"].as_f64() == Some(13.0)).expect("the current slide as a chord sheet");
        assert_eq!(cell["lines"], json!([[["D", "  Sometimes the "], ["A/D", "night is beautiful"]], [["", "You could "], ["A7", "touch it"]]]));
        assert_eq!(cell["chord_color"], "#f5c66b");
        // An older phone page draws `text` as words: the chords as written, as before.
        assert_eq!(cell["text"], "[D]  Sometimes the [A/D]night is beautiful\nYou could [A7]touch it");
        assert!(cell["max"].as_f64().is_some() && cell["align"].is_string());
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
