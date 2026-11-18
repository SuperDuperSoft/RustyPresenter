//! Remote guests (livestreaming design §8, "Later"): a guest speaker or a missionary joins the stream from a
//! browser at home. Design: `docs/superpowers/specs/2026-11-17-livestream-extras-design.md`.
//!
//! The app has no WebRTC of its own (no WebRTC, DTLS, SRTP or ICE crate is in the offline registry, and FFmpeg 9
//! only *sends* WebRTC, with its WHIP muxer), so a web browser on the presentation computer does the WebRTC part:
//! - `/guest/host` (this computer only) makes the invitation, takes the guest's reply, receives the guest's picture
//!   and sound directly from the guest's browser (a free public STUN server helps the two find each other; there
//!   is no relay server), plays the sound (so the room and the stream hear the guest through this computer's
//!   sound), and hands the picture back to the app here as WebM, a piece every 200 ms (`/guest/video`);
//! - `/guest/video.webm` (this computer only) is that picture as one endless WebM file, which the stream reads as a
//!   network camera (FFmpeg, like any camera at an address);
//! - `/guest/join` is the page the guest uses. A browser lets a page use the camera only from a secure address
//!   (https, or a file opened on the guest's own computer), so the church sends it as a file
//!   (`/guest/join?save=1`), with the invitation code to paste.
//!
//! Invitation and reply are short texts the two people send each other (text message, email): nothing else is
//! needed to connect. When the two internets will not let a direct connection through (some company and phone
//! networks), both pages say so plainly.
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{ConnectInfo, Query},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::{net::SocketAddr, sync::{Arc, Mutex, OnceLock}};
use tokio::sync::broadcast;

/// The most a piece of picture may be (a 200 ms piece at a few Mbit/s is far smaller).
const MAX_PIECE: usize = 4 * 1024 * 1024;

/// The guest's picture as it comes from the host page: the WebM start (its header, the first piece of a session)
/// and every piece after it, handed to whoever is reading `/guest/video.webm`.
pub struct Relay {
    /// The recording session in the host page (a new one starts a new WebM file).
    session: u64,
    header: Option<Bytes>,
    pieces: broadcast::Sender<(u64, Bytes)>,
    /// What the host page says about the connection, for the operator ("waiting", "connected", "failed").
    pub state: String,
}

impl Default for Relay {
    fn default() -> Self { Relay { session: 0, header: None, pieces: broadcast::channel(256).0, state: String::new() } }
}

impl Relay {
    /// A piece from the host page: piece 0 of a session starts it (and is its WebM header).
    pub fn piece(&mut self, session: u64, n: u64, data: Bytes) {
        if n == 0 { self.session = session; self.header = Some(data.clone()) }
        if session != self.session || self.header.is_none() { return }
        let _ = self.pieces.send((session, data));
    }

    /// For a new reader: the header of the session now (with its session), and the pieces to come.
    pub fn reader(&self) -> Option<(u64, Bytes, broadcast::Receiver<(u64, Bytes)>)> {
        self.header.clone().map(|h| (self.session, h, self.pieces.subscribe()))
    }

    /// The guest left: readers end, and a new session must start again.
    pub fn end(&mut self) { self.header = None; self.session = 0; let _ = self.pieces.send((u64::MAX, Bytes::new())); }
}

/// The app's one relay.
pub fn relay() -> &'static Mutex<Relay> {
    static RELAY: OnceLock<Mutex<Relay>> = OnceLock::new();
    RELAY.get_or_init(Mutex::default)
}

/// The address the stream reads the guest's picture from, as a network camera (Settings' camera rows).
pub fn camera_address(port: u16) -> String { format!("http://127.0.0.1:{port}/guest/video.webm") }

/// Whether camera `name` is the remote guest.
pub fn is_guest_camera(name: &str) -> bool { name.trim().starts_with("http://127.0.0.1:") && name.trim().ends_with("/guest/video.webm") }

/// The page where the operator invites the guest, opened in this computer's browser (`127.0.0.1` is a secure
/// address to a browser, so the page may use WebRTC).
pub fn host_address(port: u16) -> String { format!("http://127.0.0.1:{port}/guest/host") }

pub fn routes() -> Router<Arc<crate::app::App>> {
    Router::new()
        .route("/guest/host", get(host_page))
        .route("/guest/join", get(join_page))
        .route("/guest/video", post(video_piece))
        .route("/guest/video.webm", get(video))
        .route("/guest/state", post(state))
}

/// Only this computer, asked by its own address (a page elsewhere cannot reach it through the browser: the
/// header `x-rp-guest` makes the browser ask first, and nothing answers that).
fn local(addr: &SocketAddr, headers: &HeaderMap, from_page: bool) -> bool {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let host_ok = host.starts_with("127.0.0.1:") || host.starts_with("localhost:") || host == "127.0.0.1" || host == "localhost";
    addr.ip().is_loopback() && host_ok && (!from_page || headers.contains_key("x-rp-guest"))
}

fn html(body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-store")], body).into_response()
}

async fn host_page(ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    if !local(&addr, &headers, false) { return StatusCode::FORBIDDEN.into_response() }
    html(include_str!("../../web/guest-host.html"))
}

#[derive(Deserialize)]
struct Save { save: Option<u8> }

/// The guest's page. `?save=1` gives it as a file to send the guest.
async fn join_page(Query(q): Query<Save>) -> Response {
    let body = include_str!("../../web/guest.html");
    if q.save.is_some() {
        return ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CONTENT_DISPOSITION, "attachment; filename=\"Join the livestream.html\"")], body).into_response();
    }
    html(body)
}

#[derive(Deserialize)]
struct Piece { session: u64, n: u64 }

async fn video_piece(ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Query(p): Query<Piece>, body: Bytes) -> Response {
    if !local(&addr, &headers, true) { return StatusCode::FORBIDDEN.into_response() }
    if body.len() > MAX_PIECE { return StatusCode::PAYLOAD_TOO_LARGE.into_response() }
    relay().lock().unwrap_or_else(|e| e.into_inner()).piece(p.session, p.n, body);
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
struct State { state: String }

async fn state(ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Query(s): Query<State>) -> Response {
    if !local(&addr, &headers, true) { return StatusCode::FORBIDDEN.into_response() }
    let mut r = relay().lock().unwrap_or_else(|e| e.into_inner());
    if s.state != "connected" { r.end() }
    r.state = s.state.chars().take(32).collect();
    StatusCode::NO_CONTENT.into_response()
}

/// The guest's picture as one WebM file that goes on while the guest is there. No guest: 503, and the camera
/// feed tries again by itself.
async fn video(ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    if !local(&addr, &headers, false) { return StatusCode::FORBIDDEN.into_response() }
    let Some((session, head, rx)) = relay().lock().unwrap_or_else(|e| e.into_inner()).reader() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    use futures_util::StreamExt;
    // The header, then each piece of the same session, until it ends (the guest left, or a new session began).
    let rest = futures_util::stream::unfold(rx, move |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok((s, b)) if s == session => return Some((Ok::<Bytes, std::io::Error>(b), rx)),
                Ok(_) => return None,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return None,
            }
        }
    });
    let stream = futures_util::stream::once(async move { Ok::<Bytes, std::io::Error>(head) }).chain(rest);
    ([(header::CONTENT_TYPE, "video/webm"), (header::CACHE_CONTROL, "no-store")], Body::from_stream(stream)).into_response()
}

pub mod tr {
    use crate::tr;
    /// The guest's camera on the scene button and in Settings.
    pub fn camera() -> String { tr!("Remote guest") }
}

#[cfg(test)]
#[path = "guest_tests.rs"]
mod tests;
