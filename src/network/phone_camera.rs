//! A phone as a camera, on the phone server: `/camera` (`web/camera.html`) opens the phone's camera and sends its
//! pictures to the app (`stream::phone_camera`). Same rules as the phone remote: only a phone paired with the
//! 6-digit code (and the remote's password, when the church set one) may send pictures. Pictures come as JPEG over
//! a WebSocket (`/camera/ws`), each answered with the camera's tally light, which is also what paces the phone;
//! when a WebSocket cannot connect, the page sends one picture a request instead (`/camera/frame`).
//!
//! A browser opens the camera only on a private (https) page, so on the church's network the page is used over
//! https (`super::https`); the plain page explains the one-time step and links there. Settings shows the https page
//! with a QR code that also pairs the phone (`super::phone_link`).
use super::websocket::{self, Message, Socket};
use crate::{app::App, stream::phone_camera as cam};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/camera", get(page))
        .route("/camera/info", get(info))
        .route("/camera/ws", get(socket))
        .route("/camera/frame", post(frame).layer(DefaultBodyLimit::max(cam::MAX_PICTURE)))
}

async fn page() -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-store")], include_str!("../../web/camera.html")).into_response()
}

/// The phone camera page on this computer, for Settings: the secure one, `https://192.168.1.20:4320/camera` (the
/// plain one, on the remote's port, can only point there).
pub fn page_address(app: &App) -> Option<String> { super::phone_link::address(app, super::phone_link::Kind::Camera) }

/// Settings → Remote and network → Phone camera and microphone: the address to open on the phone.
pub fn settings_help(app: &App) -> String {
    page_address(app).unwrap_or_else(cam::tr::no_network)
}

/// What the page needs: its words, whether this phone is paired (and a password is asked when pairing), the private
/// page's port, the picture size and rate, and the phone's name as a first camera name.
async fn info(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let phone = crate::server::phone_for(&app, &headers);
    let name = phone.and_then(|id| app.phones.lock().unwrap().iter().find(|p| p.id == id).map(|p| p.name.clone()));
    // The page asks every few seconds while it is open: Settings shows the phone connected.
    if phone.is_some() { super::phone_link::saw(&app, super::phone_link::Kind::Camera, name.clone()) }
    let (password, size) = { let d = app.data(); (d.settings.network.remote_password.is_some(), d.settings.record.size.size()) };
    let secure = *app.net.https.lock().unwrap();
    ([(header::CACHE_CONTROL, "no-store")], Json(json!({
        "words": cam::tr::page_words(),
        "paired": phone.is_some(),
        "password": password,
        "secure_port": secure,
        "width": size.0, "height": size.1, "fps": crate::record::FPS,
        "name": name,
    }))).into_response()
}

#[derive(Deserialize)]
struct Named { #[serde(default)] name: String }

use super::same_origin;

/// Whether phone `id` is still paired (it may be unpaired on the computer while it sends).
fn still_paired(app: &App, id: Uuid) -> bool { app.phones.lock().unwrap().iter().any(|p| p.id == id) }

/// Phone camera `name`'s tally light for its page: `on_air`, `next` or `off`, with its words.
pub fn light(app: &App, name: &str) -> Value {
    use crate::stream::tally::Light;
    let key = format!("{}{name}", cam::SCHEME);
    let cameras = app.data().settings.record.cameras();
    let t = *app.tally.lock().unwrap();
    let light = cameras.iter().position(|(c, _)| *c == key).filter(|_| t.live).map_or(Light::Off, |i| t.light(i));
    let words = cam::tr::page_words();
    let text = match light { Light::OnAir => &words["on_air"], Light::Next => &words["next"], Light::Off => &words["off_air"] };
    json!({ "light": light.code(), "text": text })
}

/// A JPEG starts with FF D8.
fn is_jpeg(b: &[u8]) -> bool { b.len() > 4 && b[0] == 0xFF && b[1] == 0xD8 }

/// One picture a request, when the WebSocket cannot connect.
async fn frame(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<Named>, body: Bytes) -> Response {
    if !same_origin(&headers) { return StatusCode::FORBIDDEN.into_response() }
    if crate::server::phone_for(&app, &headers).is_none() { return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "pair first" }))).into_response() }
    let Some(name) = cam::clean_name(&q.name) else { return (StatusCode::BAD_REQUEST, Json(json!({ "error": "no name" }))).into_response() };
    if !is_jpeg(&body) { return (StatusCode::BAD_REQUEST, Json(json!({ "error": "not a picture" }))).into_response() }
    super::phone_link::sent(&app, super::phone_link::Kind::Camera, Some(name.clone()));
    cam::put(&name, body.to_vec());
    ([(header::CACHE_CONTROL, "no-store")], Json(light(&app, &name))).into_response()
}

/// The WebSocket: each JPEG the phone sends is kept and answered with the tally light.
async fn socket(State(app): State<Arc<App>>, Query(q): Query<Named>, mut req: Request) -> Response {
    let headers = req.headers().clone();
    if !same_origin(&headers) { return StatusCode::FORBIDDEN.into_response() }
    let Some(phone) = crate::server::phone_for(&app, &headers) else { return (StatusCode::UNAUTHORIZED, "pair first").into_response() };
    let Some(name) = cam::clean_name(&q.name) else { return (StatusCode::BAD_REQUEST, "no name").into_response() };
    let Some(key) = websocket::wants_websocket(&headers) else { return (StatusCode::BAD_REQUEST, "a WebSocket is needed").into_response() };
    let upgrade = hyper::upgrade::on(&mut req);
    let phone_name = super::phone_link::phone_name(&app, &headers);
    tokio::spawn(async move {
        let Ok(io) = upgrade.await else { return };
        let mut s = Socket::new(hyper_util::rt::TokioIo::new(io), cam::MAX_PICTURE);
        let mut count = 0u32;
        super::phone_link::saw(&app, super::phone_link::Kind::Camera, phone_name);
        while let Some(m) = s.recv().await {
            if let Message::Binary(jpeg) = m && is_jpeg(&jpeg) {
                cam::put(&name, jpeg);
                super::phone_link::sent(&app, super::phone_link::Kind::Camera, Some(name.clone()));
            }
            count = count.wrapping_add(1);
            if count.is_multiple_of(30) && !still_paired(&app, phone) { let _ = s.close().await; return }
            if s.send(&light(&app, &name).to_string()).await.is_err() { return }
        }
    });
    websocket::switching(&key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Phone, hash_token, now_secs, test_app};
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn jpeg(rgb: [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(64, 36, image::Rgb(rgb));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    /// A paired phone: its sign-in cookie.
    fn paired(app: &App) -> String {
        let token = crate::app::random_token();
        app.phones.lock().unwrap().push(Phone { id: Uuid::new_v4(), name: "Pastor's phone".into(), hash: hash_token(&token), last_seen: now_secs(), may_draw: true });
        format!("rp_session={token}")
    }

    async fn served(app: Arc<App>) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let svc = crate::server::router(app).into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(listener, svc).await });
        addr
    }

    /// The head of an HTTP answer.
    async fn head(s: &mut tokio::net::TcpStream) -> String {
        let mut h = Vec::new();
        while !h.ends_with(b"\r\n\r\n") { let mut b = [0u8; 1]; if s.read_exact(&mut b).await.is_err() { break } h.push(b[0]) }
        String::from_utf8_lossy(&h).into_owned()
    }

    /// One text message from the server (unmasked, short).
    async fn text(s: &mut tokio::net::TcpStream) -> String {
        let mut h = [0u8; 2];
        s.read_exact(&mut h).await.unwrap();
        assert_eq!(h[0], 0x81);
        let mut b = vec![0u8; usize::from(h[1] & 0x7F)];
        s.read_exact(&mut b).await.unwrap();
        String::from_utf8(b).unwrap()
    }

    /// A phone, played by a test: it pairs, opens the WebSocket and sends JPEG pictures; the app keeps them, shows
    /// them through a camera feed and answers each with the tally light.
    #[tokio::test]
    async fn a_phone_sends_its_camera_over_a_websocket() {
        let app = test_app();
        let name = format!("Balcony {}", std::process::id());
        app.data().settings.record.camera = format!("{}{name}", cam::SCHEME);
        let addr = served(app.clone()).await;
        let cookie = paired(&app);
        let ask = |cookie: &str| format!("GET /camera/ws?name={} HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://{addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nCookie: {cookie}\r\n\r\n", name.replace(' ', "%20"));

        // Not paired: no.
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(ask("rp_session=nope").as_bytes()).await.unwrap();
        assert!(head(&mut s).await.starts_with("HTTP/1.1 401"));

        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(ask(&cookie).as_bytes()).await.unwrap();
        let h = head(&mut s).await;
        assert!(h.starts_with("HTTP/1.1 101") && h.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="), "{h}");
        let feed = crate::stream::camera_feed::start("ffmpeg".into(), crate::record::args::camera_input(crate::platform::tools::OS, &format!("{}{name}", cam::SCHEME)), (64, 36), 30).unwrap();
        // Not live: its light is off.
        s.write_all(&websocket::frame(2, &jpeg([200, 20, 20]), Some([9, 8, 7, 6]))).await.unwrap();
        let answer: Value = serde_json::from_str(&text(&mut s).await).unwrap();
        assert_eq!(answer["light"], "off");
        let began = std::time::Instant::now();
        while !feed.seeing.load(std::sync::atomic::Ordering::Relaxed) && began.elapsed().as_secs() < 5 { tokio::time::sleep(std::time::Duration::from_millis(20)).await }
        let p = feed.picture().expect("the phone's picture through the camera feed");
        assert!(p[0] > 160 && p[1] < 80, "red: {:?}", &p[..4]);
        // Live, on air: red on the phone.
        *app.tally.lock().unwrap() = crate::stream::tally::Tally::of(true, Some(0), None, true);
        s.write_all(&websocket::frame(2, &jpeg([20, 20, 200]), Some([1, 2, 3, 4]))).await.unwrap();
        let answer: Value = serde_json::from_str(&text(&mut s).await).unwrap();
        assert_eq!(answer["light"], "on_air");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let p = feed.picture().unwrap();
        assert!(p[2] > 160, "blue now: {:?}", &p[..4]);
        // Pictures from another site are refused.
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(ask(&cookie).replace(&format!("Origin: http://{addr}"), "Origin: http://evil.example").as_bytes()).await.unwrap();
        assert!(head(&mut s).await.starts_with("HTTP/1.1 403"));
    }

    #[tokio::test]
    async fn without_a_websocket_a_picture_a_request_works_too() {
        let app = test_app();
        let addr = served(app.clone()).await;
        let cookie = paired(&app);
        let name = format!("Side {}", std::process::id());
        let post = |cookie: &str, body: &[u8]| {
            let mut r = format!("POST /camera/frame?name={} HTTP/1.1\r\nHost: {addr}\r\nCookie: {cookie}\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", name.replace(' ', "%20"), body.len()).into_bytes();
            r.extend(body);
            r
        };
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(&post("rp_session=nope", &jpeg([1, 2, 3]))).await.unwrap();
        assert!(head(&mut s).await.starts_with("HTTP/1.1 401"));
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(&post(&cookie, &jpeg([1, 2, 3]))).await.unwrap();
        assert!(head(&mut s).await.starts_with("HTTP/1.1 200"));
        assert!(cam::newest(&name).is_some() && cam::names().contains(&name));
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(&post(&cookie, b"not a picture")).await.unwrap();
        assert!(head(&mut s).await.starts_with("HTTP/1.1 400"));
    }

    #[tokio::test]
    async fn the_page_says_what_it_needs() {
        let app = test_app();
        let addr = served(app.clone()).await;
        let cookie = paired(&app);
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(format!("GET /camera/info HTTP/1.1\r\nHost: {addr}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut all = String::new();
        s.read_to_string(&mut all).await.unwrap();
        let body: Value = serde_json::from_str(all.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["paired"], true);
        assert_eq!(body["name"], "Pastor's phone");
        assert_eq!((body["width"].as_u64(), body["fps"].as_u64()), (Some(1920), Some(30)));
        assert!(body["words"]["secure_body"].as_str().is_some_and(|t| t.contains("not private")));
    }
}
