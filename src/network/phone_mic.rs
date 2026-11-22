//! The phone as a microphone for the stream (`stream::phone_mic`): its page at `/stage-view/mic`, and where the
//! page sends its sound. A paired phone may send (scanning the QR code in Settings pairs it, `phone_link`), and so may
//! a phone that signed in to the stage view with its password. A stage view left open to everyone (no password) is
//! for looking only: it never lets a device send sound into the stream.
use super::stage_view::allowed;
use crate::{app::App, stream::phone_mic::{self, tr}};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

/// The most one post may carry: two seconds of 48 kHz mono (a phone sends a tenth of a second at a time).
const MOST: usize = 2 * 48_000 * 2;

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/stage-view/mic", get(page))
        .route("/stage-view/mic/status", get(status))
        .route("/stage-view/mic/sound", post(sound).layer(DefaultBodyLimit::max(MOST)))
}

async fn page() -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-store")], include_str!("../../web/phone-mic.html")).into_response()
}

/// Whether this phone may send sound: it is paired, or it signed in to the stage view with the stage password.
pub fn may_send(app: &App, headers: &HeaderMap) -> bool {
    if crate::server::phone_for(app, headers).is_some() { return true }
    let password_set = app.data.lock().unwrap().settings.network.stage_password.is_some();
    password_set && allowed(app, headers)
}

/// The page's words, whether this phone may send (and whether pairing asks for the remote's password), and the
/// secure address of the page (for the microphone). The page asks every few seconds: Settings shows it connected.
async fn status(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let secure = secure_url(host, super::phone_link::secure_port(&app));
    let may = may_send(&app, &headers);
    if may { super::phone_link::saw(&app, super::phone_link::Kind::Mic, super::phone_link::phone_name(&app, &headers)) }
    let password = app.data.lock().unwrap().settings.network.remote_password.is_some();
    ([(header::CACHE_CONTROL, "no-store")], Json(json!({
        "allowed": may, "paired": crate::server::phone_for(&app, &headers).is_some(), "password": password,
        "taken": phone_mic::taken(), "secure_url": secure,
        "words": { "title": tr::title(), "start": tr::start(), "stop": tr::stop(), "sending": tr::sending(), "not_taken": tr::not_taken(),
            "needs_secure": tr::needs_secure(), "secure_how": crate::stream::phone_camera::tr::secure_how(), "no_proceed": crate::stream::phone_camera::tr::no_proceed(), "no_microphone": tr::no_microphone(),
            "needs_stage_view": tr::needs_pairing(), "needs_pairing": tr::needs_pairing(), "code": crate::stream::phone_camera::tr::code(),
            "password": crate::stream::phone_camera::tr::password(), "connect": crate::stream::phone_camera::tr::connect(), "lost": tr::lost(),
            "tap_start": tr::tap_start() },
    }))).into_response()
}

/// `https://<host>:<port>/stage-view/mic` for the host the phone typed (its port left out).
pub fn secure_url(host: &str, port: u16) -> String {
    let name = if host.starts_with('[') { &host[..host.find(']').map_or(host.len(), |i| i + 1)] } else { host.split(':').next().unwrap_or(host) };
    format!("https://{name}:{port}/stage-view/mic")
}

#[derive(Deserialize)]
struct Ask { rate: Option<u32> }

/// A piece of the phone's sound: 16-bit mono numbers at `rate`.
async fn sound(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<Ask>, body: Bytes) -> Response {
    if !super::stage_view::same_origin(&headers) { return StatusCode::FORBIDDEN.into_response() }
    if !may_send(&app, &headers) { return (StatusCode::FORBIDDEN, Json(json!({ "allowed": false, "text": tr::needs_pairing() }))).into_response() }
    super::phone_link::sent(&app, super::phone_link::Kind::Mic, super::phone_link::phone_name(&app, &headers));
    let taken = phone_mic::hand_in(q.rate.unwrap_or(48_000), &body);
    Json(json!({ "allowed": true, "taken": taken, "text": if taken { tr::sending() } else { tr::not_taken() } })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;

    async fn post(app: &Arc<App>, origin: Option<&str>, body: Vec<u8>) -> (StatusCode, serde_json::Value) { post_with(app, origin, None, body).await }

    async fn post_with(app: &Arc<App>, origin: Option<&str>, cookie: Option<String>, body: Vec<u8>) -> (StatusCode, serde_json::Value) {
        let mut h = HeaderMap::new();
        if let Some(c) = cookie { h.insert(header::COOKIE, c.parse().unwrap()); }
        // The phone typed the address the page came from.
        let host = origin.and_then(|o| o.split_once("://")).map_or("192.168.1.5:4318", |(_, h)| if h.starts_with("evil") { "192.168.1.5:4318" } else { h });
        h.insert(header::HOST, host.parse().unwrap());
        if let Some(o) = origin { h.insert(header::ORIGIN, o.parse().unwrap()); }
        let r = sound(State(app.clone()), h, Query(Ask { rate: Some(48_000) }), Bytes::from(body)).await;
        let status = r.status();
        let b = axum::body::to_bytes(r.into_body(), 1 << 16).await.unwrap();
        (status, serde_json::from_slice(&b).unwrap_or_default())
    }

    /// B3: a stage view open to everyone is for looking; sound needs a paired phone or the stage password.
    #[tokio::test]
    async fn only_a_paired_phone_or_one_signed_in_to_the_stage_view_may_send_sound() {
        let _turn = phone_mic::TEST_TURN.lock().unwrap_or_else(|e| e.into_inner());
        let app = test_app();
        let pcm: Vec<u8> = [100i16, -100].iter().flat_map(|s| s.to_le_bytes()).collect();
        // The stage view is off: nothing is taken.
        let (status, v) = post(&app, None, pcm.clone()).await;
        assert_eq!((status, v["allowed"].as_bool()), (StatusCode::FORBIDDEN, Some(false)));
        // On with no password: every device may look, none may send.
        app.data.lock().unwrap().settings.network.stage_view = true;
        let (status, v) = post(&app, Some("http://192.168.1.5:4318"), pcm.clone()).await;
        assert_eq!((status, v["allowed"].as_bool()), (StatusCode::FORBIDDEN, Some(false)));
        // With a password: only a device that signed in.
        app.data.lock().unwrap().settings.network.stage_password = Some("a stored hash".into());
        assert_eq!(post(&app, Some("http://192.168.1.5:4318"), pcm.clone()).await.0, StatusCode::FORBIDDEN);
        let token = crate::app::random_token();
        app.net.stage_tokens.lock().unwrap().push((crate::app::hash_token(&token), std::time::Instant::now()));
        let signed_in = || Some(format!("rp_stage={token}"));
        // Another site's page may not send.
        assert_eq!(post_with(&app, Some("http://evil.example"), signed_in(), pcm.clone()).await.0, StatusCode::FORBIDDEN);
        // From its own page (over http or https): taken while the stream reads the phone.
        let rx = phone_mic::listen();
        let (status, v) = post_with(&app, Some("https://192.168.1.5:4319"), signed_in(), pcm.clone()).await;
        assert_eq!((status, v["taken"].as_bool()), (StatusCode::OK, Some(true)));
        assert_eq!(rx.recv().unwrap().len(), 8, "two samples, made stereo");
        drop(rx);
        let (_, v) = post_with(&app, Some("http://192.168.1.5:4318"), signed_in(), pcm.clone()).await;
        assert_eq!((v["taken"].as_bool(), v["text"].as_str()), (Some(false), Some(tr::not_taken().as_str())));
        // A paired phone may send whatever the stage view is set to.
        {
            let mut d = app.data.lock().unwrap();
            d.settings.network.stage_view = false;
            d.settings.network.stage_password = None;
        }
        app.phones.lock().unwrap().push(crate::app::Phone { id: uuid::Uuid::new_v4(), name: "Pixel".into(), hash: crate::app::hash_token("tok"), last_seen: crate::app::now_secs(), may_draw: true });
        let (status, v) = post_with(&app, Some("http://192.168.1.5:4318"), Some("rp_session=tok".into()), pcm).await;
        assert_eq!((status, v["allowed"].as_bool()), (StatusCode::OK, Some(true)));
    }

    #[test]
    fn the_secure_page_is_on_the_same_computer() {
        assert_eq!(secure_url("192.168.1.5:4318", 4319), "https://192.168.1.5:4319/stage-view/mic");
        assert_eq!(secure_url("[fe80::1]:4318", 4319), "https://[fe80::1]:4319/stage-view/mic");
        assert_eq!(secure_url("church-pc", 4319), "https://church-pc:4319/stage-view/mic");
    }
}
