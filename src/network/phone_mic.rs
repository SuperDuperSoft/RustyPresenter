//! The phone as a microphone for the stream (`stream::phone_mic`): its page at `/stage-view/mic`, and where the
//! page sends its sound. Same access and password as the stage view (and the camera tally page).
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

/// The page's words, whether this phone may send, and the secure address of the page (for the microphone).
async fn status(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let secure = super::secure::port().map(|p| secure_url(host, p)).unwrap_or_default();
    Json(json!({
        "allowed": allowed(&app, &headers), "taken": phone_mic::taken(), "secure_url": secure,
        "words": { "title": tr::title(), "start": tr::start(), "stop": tr::stop(), "sending": tr::sending(), "not_taken": tr::not_taken(),
            "needs_secure": tr::needs_secure(), "no_microphone": tr::no_microphone(), "needs_stage_view": tr::needs_stage_view() },
    })).into_response()
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
    if !allowed(&app, &headers) { return (StatusCode::FORBIDDEN, Json(json!({ "allowed": false, "text": tr::needs_stage_view() }))).into_response() }
    let taken = phone_mic::hand_in(q.rate.unwrap_or(48_000), &body);
    Json(json!({ "allowed": true, "taken": taken, "text": if taken { tr::sending() } else { tr::not_taken() } })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;

    async fn post(app: &Arc<App>, origin: Option<&str>, body: Vec<u8>) -> (StatusCode, serde_json::Value) {
        let mut h = HeaderMap::new();
        // The phone typed the address the page came from.
        let host = origin.and_then(|o| o.split_once("://")).map_or("192.168.1.5:4318", |(_, h)| if h.starts_with("evil") { "192.168.1.5:4318" } else { h });
        h.insert(header::HOST, host.parse().unwrap());
        if let Some(o) = origin { h.insert(header::ORIGIN, o.parse().unwrap()); }
        let r = sound(State(app.clone()), h, Query(Ask { rate: Some(48_000) }), Bytes::from(body)).await;
        let status = r.status();
        let b = axum::body::to_bytes(r.into_body(), 1 << 16).await.unwrap();
        (status, serde_json::from_slice(&b).unwrap_or_default())
    }

    #[tokio::test]
    async fn only_a_phone_allowed_to_see_the_stage_view_may_send_sound() {
        let _turn = phone_mic::TEST_TURN.lock().unwrap_or_else(|e| e.into_inner());
        let app = test_app();
        let pcm: Vec<u8> = [100i16, -100].iter().flat_map(|s| s.to_le_bytes()).collect();
        // The stage view is off: nothing is taken.
        let (status, v) = post(&app, None, pcm.clone()).await;
        assert_eq!((status, v["allowed"].as_bool()), (StatusCode::FORBIDDEN, Some(false)));
        app.data.lock().unwrap().settings.network.stage_view = true;
        // Another site's page may not send.
        assert_eq!(post(&app, Some("http://evil.example"), pcm.clone()).await.0, StatusCode::FORBIDDEN);
        // From its own page (over http or https): taken while the stream reads the phone.
        let rx = phone_mic::listen();
        let (status, v) = post(&app, Some("https://192.168.1.5:4319"), pcm.clone()).await;
        assert_eq!((status, v["taken"].as_bool()), (StatusCode::OK, Some(true)));
        assert_eq!(rx.recv().unwrap().len(), 8, "two samples, made stereo");
        drop(rx);
        let (_, v) = post(&app, Some("http://192.168.1.5:4318"), pcm).await;
        assert_eq!((v["taken"].as_bool(), v["text"].as_str()), (Some(false), Some(tr::not_taken().as_str())));
    }

    #[test]
    fn the_secure_page_is_on_the_same_computer() {
        assert_eq!(secure_url("192.168.1.5:4318", 4319), "https://192.168.1.5:4319/stage-view/mic");
        assert_eq!(secure_url("[fe80::1]:4318", 4319), "https://[fe80::1]:4319/stage-view/mic");
        assert_eq!(secure_url("church-pc", 4319), "https://church-pc:4319/stage-view/mic");
    }
}
