//! One scan for a phone camera or a phone microphone (Jake, November 18: "I can't get the phone as a microphone or
//! phone as a camera to work. Can you add a QR code to that as well?"). Settings → Remote and network → Phone camera
//! and microphone shows a QR code beside each address: the secure page (`https`, which a phone needs before it lets a
//! page use its camera or microphone) with a one-time secret in the fragment (`#pair=…`, never sent in a request
//! line or kept in a log), like the remote's. The page reads it, wipes it from the address bar and posts it to
//! `/phone/pair`, which pairs the phone as the remote's pairing does (the same sign-in cookie, the same list of
//! paired phones, the remote's password when the church set one). A used code is gone at once and Settings shows a
//! new one, so each phone gets its own.
//!
//! Also here: what each page last heard from a phone, for the live line under each code in Settings ("Waiting for
//! a phone", "Phone connected: …", "Sending pictures: …", or what is wrong).
use super::https;
use crate::{app::{App, ct_eq, random_token}, platform::firewall::Firewall};
use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;
use serde_json::json;
use std::{net::SocketAddr, sync::Arc, time::{Duration, Instant}};

/// A code lasts this long while nobody scans it (Settings makes a new one before it runs out).
const LASTS: Duration = Duration::from_secs(30 * 60);
/// Settings makes a new code when the one shown has less than this left, so a code is never scanned as it dies.
const RENEW: Duration = Duration::from_secs(5 * 60);
/// A phone counts as connected while its page asked within this long (the pages ask every few seconds).
const CONNECTED: Duration = Duration::from_secs(15);
/// Wrong passwords with one code before that code is gone.
const MAX_FAILURES: u32 = 5;

/// Which page a code opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Camera, Mic }

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Camera, Kind::Mic];
    fn i(self) -> usize { self as usize }
    /// The page on the phone server (the Android app uses the same ones).
    pub fn path(self) -> &'static str { match self { Kind::Camera => "/camera", Kind::Mic => "/stage-view/mic" } }
}

/// A code on show in Settings: its secret, until when it works, and wrong passwords tried with it.
pub(crate) struct Link { secret: String, until: Instant, failures: u32 }

/// What a phone's page last asked, for the line in Settings: when, from which phone, and when it last sent pictures
/// or sound (with the camera's or phone's name).
#[derive(Clone)]
pub(crate) struct Seen { phone: Option<String>, at: Instant, sent: Option<(Instant, Option<String>)> }

/// Pictures or sound count as coming in while they came within this long.
const SENDING: Duration = Duration::from_secs(2);

/// The phone server's host as phones reach it (this computer's network address, never a name a phone cannot look
/// up), from the remote's address.
pub fn host(app: &App) -> Option<String> {
    let host = app.lan_url.as_deref()?.strip_prefix("http://")?.split('/').next()?;
    let ip = host.rsplit_once(':').map_or(host, |(ip, _)| ip);
    (!ip.is_empty()).then(|| ip.to_string())
}

/// The secure port: the one the https server listens on, or the one it will.
pub fn secure_port(app: &App) -> u16 { app.net.https.lock().unwrap().unwrap_or_else(|| https::port_for(app.port)) }

/// The page's secure address, as Settings shows it: `https://192.168.1.20:4320/camera`. None off the network.
pub fn address(app: &App, kind: Kind) -> Option<String> {
    host(app).map(|h| format!("https://{h}:{}{}", secure_port(app), kind.path()))
}

/// What the QR code opens: the secure page with a one-time secret (`#pair=…`). The same code until it is used or
/// near its end; then a new one.
pub fn link(app: &App, kind: Kind) -> Option<String> {
    let address = address(app, kind)?;
    let mut links = app.net.links.lock().unwrap();
    let now = Instant::now();
    let slot = &mut links[kind.i()];
    if slot.as_ref().is_none_or(|l| l.until.saturating_duration_since(now) < RENEW) {
        *slot = Some(Link { secret: random_token(), until: now + LASTS, failures: 0 });
    }
    Some(format!("{address}#pair={}", slot.as_ref().map_or("", |l| l.secret.as_str())))
}

/// The QR code of `link` (an SVG), as the remote's pairing draws it.
pub fn qr_svg(link: &str) -> Option<String> { crate::app::pair_qr_svg(link) }

pub fn routes() -> Router<Arc<App>> { Router::new().route("/phone/pair", post(pair)) }

#[derive(Deserialize)]
struct PairReq {
    #[serde(default)] pair: String,
    #[serde(default)] name: String,
    #[serde(default)] password: String,
}

fn answer(status: StatusCode, error: String, password: bool) -> Response {
    (status, Json(json!({ "ok": false, "error": error, "password": password }))).into_response()
}

/// A phone posts the code it scanned: paired (the sign-in cookie set), and the code is gone. Wrong or old codes
/// answer `400`; a missing or wrong remote password answers `401` with `password: true` (the page then asks for it,
/// the code still good, up to `MAX_FAILURES` tries).
async fn pair(State(app): State<Arc<App>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(req): Json<PairReq>) -> Response {
    if !super::stage_view::same_origin(&headers) { return StatusCode::FORBIDDEN.into_response() }
    if !app.net.may_try(addr.ip()) { return answer(StatusCode::TOO_MANY_REQUESTS, super::strings::too_fast(), false) }
    let secret = req.pair.trim().to_string();
    let found = {
        let links = app.net.links.lock().unwrap();
        let now = Instant::now();
        links.iter().position(|l| l.as_ref().is_some_and(|l| l.until > now && !secret.is_empty() && ct_eq(&secret, &l.secret)))
    };
    let Some(i) = found else { return answer(StatusCode::BAD_REQUEST, tr::old_code(), false) };
    let needed = app.data().settings.network.remote_password.clone();
    if let Some(hash) = needed {
        let typed = req.password.clone();
        let right = tokio::task::spawn_blocking(move || super::password::verify(&typed, &hash)).await.unwrap_or(false);
        if !right {
            let mut links = app.net.links.lock().unwrap();
            let gone = links[i].as_mut().is_none_or(|l| { l.failures += 1; l.failures >= MAX_FAILURES });
            if gone { links[i] = None; return answer(StatusCode::BAD_REQUEST, tr::old_code(), false) }
            let text = if req.password.is_empty() { tr::password_needed() } else { super::strings::wrong_password() };
            return answer(StatusCode::UNAUTHORIZED, text, true);
        }
    }
    // Used: gone, unless it changed meanwhile (another phone was quicker).
    {
        let mut links = app.net.links.lock().unwrap();
        if !links[i].as_ref().is_some_and(|l| ct_eq(&secret, &l.secret)) { return answer(StatusCode::BAD_REQUEST, tr::old_code(), false) }
        links[i] = None;
    }
    let name = if req.name.trim().is_empty() { None } else { Some(req.name.as_str()) };
    crate::server::add_phone(&app, name)
}

/// A phone's page asked something just now (`phone`: its paired name, when it is paired).
pub fn saw(app: &App, kind: Kind, phone: Option<String>) {
    let mut seen = app.net.seen.lock().unwrap();
    let sent = seen[kind.i()].as_ref().and_then(|s| s.sent.clone());
    seen[kind.i()] = Some(Seen { phone, at: Instant::now(), sent });
}

/// A phone sent pictures or sound just now (`name`: the camera's name, or the phone's).
pub fn sent(app: &App, kind: Kind, name: Option<String>) {
    let mut seen = app.net.seen.lock().unwrap();
    let now = Instant::now();
    let s = seen[kind.i()].get_or_insert(Seen { phone: name.clone(), at: now, sent: None });
    s.at = now;
    s.sent = Some((now, name));
}

/// The paired name of the phone asking, if it is paired.
pub fn phone_name(app: &App, headers: &HeaderMap) -> Option<String> {
    let id = crate::server::phone_for(app, headers)?;
    app.phones.lock().unwrap().iter().find(|p| p.id == id).map(|p| p.name.clone())
}

/// Why the secure server is not running, when it tried and could not.
pub fn set_secure_problem(app: &App, problem: Option<String>) { *app.net.https_problem.lock().unwrap() = problem }

/// The line under each code in Settings, most important first.
#[derive(Clone, Debug, PartialEq)]
pub enum Health {
    /// This computer is on no network.
    NoNetwork,
    /// The https server is not running: why (empty while it starts).
    SecureOff(String),
    /// Pictures or sound are coming in, from this camera or phone.
    Sending(Option<String>),
    /// A phone has the page open.
    Connected(Option<String>),
    /// Windows Firewall keeps phones out (`true`: on a network Windows calls public).
    Firewall { public: bool },
    Waiting,
}

/// How `kind` stands now. `firewall`: what Windows Firewall does with phones (asked now and then, not here: it is
/// slow).
pub fn health(app: &App, kind: Kind, firewall: Firewall) -> Health {
    if host(app).is_none() { return Health::NoNetwork }
    if app.net.https.lock().unwrap().is_none() { return Health::SecureOff(app.net.https_problem.lock().unwrap().clone().unwrap_or_default()) }
    let seen = app.net.seen.lock().unwrap()[kind.i()].clone().filter(|s| s.at.elapsed() < CONNECTED);
    if let Some((_, name)) = seen.as_ref().and_then(|s| s.sent.clone()).filter(|(at, _)| at.elapsed() < SENDING) { return Health::Sending(name) }
    if let Some(s) = seen { return Health::Connected(s.phone) }
    if let Firewall::Blocked { public } = firewall { return Health::Firewall { public } }
    Health::Waiting
}

impl Health {
    /// Something to fix: shown in red.
    pub fn problem(&self) -> bool { matches!(self, Health::NoNetwork | Health::SecureOff(_) | Health::Firewall { .. }) }

    pub fn text(&self, kind: Kind, port: u16) -> String {
        match (self, kind) {
            (Health::NoNetwork, _) => crate::stream::phone_camera::tr::no_network(),
            (Health::SecureOff(why), _) if why.is_empty() => tr::secure_starting(),
            (Health::SecureOff(why), _) => tr::secure_off(why),
            (Health::Sending(Some(n)), Kind::Camera) => tr::sending_pictures(n),
            (Health::Sending(None), Kind::Camera) => tr::sending_pictures_any(),
            (Health::Sending(Some(n)), Kind::Mic) => tr::sending_sound(n),
            (Health::Sending(None), Kind::Mic) => tr::sending_sound_any(),
            (Health::Connected(Some(n)), _) => tr::connected(n),
            (Health::Connected(None), _) => tr::connected_any(),
            (Health::Firewall { public }, _) => tr::firewall(*public, port),
            (Health::Waiting, _) => tr::waiting(),
        }
    }
}

/// Why binding the secure port failed, in plain words.
pub fn port_problem(port: u16, e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::AddrInUse => tr::port_in_use(port),
        std::io::ErrorKind::PermissionDenied => tr::port_kept(port),
        _ => tr::port_other(port, &e.to_string()),
    }
}

pub mod tr {
    use crate::tr;
    pub fn waiting() -> String { tr!("Waiting for a phone") }
    pub fn connected(name: &str) -> String { tr!("Phone connected: {name}", name = name) }
    pub fn connected_any() -> String { tr!("A phone is connected") }
    pub fn sending_pictures(name: &str) -> String { tr!("Sending pictures: {name}", name = name) }
    pub fn sending_pictures_any() -> String { tr!("A phone is sending pictures") }
    pub fn sending_sound(name: &str) -> String { tr!("Sending sound: {name}", name = name) }
    pub fn sending_sound_any() -> String { tr!("A phone is sending sound") }
    pub fn secure_starting() -> String { tr!("The secure server is starting…") }
    pub fn secure_off(why: &str) -> String { tr!("The secure server isn't running: {why}", why = why) }
    pub fn port_in_use(port: u16) -> String { tr!("port {port} is in use by another program. Close that program, or restart this computer.", port = port) }
    pub fn port_kept(port: u16) -> String { tr!("Windows keeps port {port} for itself. Restart this computer and try again.", port = port) }
    pub fn port_other(port: u16, error: &str) -> String { tr!("port {port} cannot be used ({error}).", port = port, error = error) }
    pub fn no_certificate(error: &str) -> String { tr!("it could not make its certificate ({error}).", error = error) }
    pub fn firewall(public: bool, port: u16) -> String {
        if public {
            tr!("This computer's firewall may be blocking phones: Windows Firewall keeps them out on this network (Windows calls it a public network). Click “Let phones in”, or in Windows Security → Firewall, allow RustyPresenter on public networks (port {port}).", port = port)
        } else {
            tr!("This computer's firewall may be blocking phones: Windows Firewall keeps them out. Click “Let phones in”, or in Windows Security → Firewall, allow RustyPresenter (port {port}).", port = port)
        }
    }
    pub fn old_code() -> String { tr!("This QR code was already used or is too old. Scan the code in RustyPresenter again (Settings → Remote and network → Phone camera and microphone).") }
    pub fn password_needed() -> String { tr!("Type the remote's password (set in RustyPresenter's Settings → Remote and network).") }
}

#[cfg(test)]
#[path = "phone_link_tests.rs"]
pub(crate) mod tests;
