//! Signing in and making broadcasts against a stand-in for Google and Facebook on this computer: the app's own
//! requests are checked, and the answers are the services' own shapes.
use super::*;
use axum::{body::Bytes, http::{HeaderMap, Method, Uri}};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct Seen {
    /// Every request: method, path, and its form or query.
    pub calls: Vec<(String, String, HashMap<String, String>)>,
    /// PKCE's challenge, from the address the browser was sent to.
    pub challenge: String,
    /// The broadcast YouTube was asked to make.
    pub broadcast: Value,
    /// Facebook: how often the app asked whether the code was typed.
    pub polls: u32,
    /// A stream named for the app is on the channel already.
    pub has_stream: bool,
    /// Where YouTube says to send the stream (empty: its real address).
    pub ingest: String,
    /// Twitch: how often the app asked whether the code was typed, and the channel title it set.
    pub twitch_polls: u32,
    pub twitch_title: String,
}

fn pairs(s: &str) -> HashMap<String, String> { url::form_urlencoded::parse(s.as_bytes()).into_owned().collect() }

pub async fn serve(seen: Arc<Mutex<Seen>>) -> String {
    let app = axum::Router::new().fallback(move |method: Method, uri: Uri, headers: HeaderMap, body: Bytes| {
        let seen = seen.clone();
        async move {
            let path = uri.path().to_string();
            let text = String::from_utf8_lossy(&body).into_owned();
            let mut form = pairs(uri.query().unwrap_or(""));
            if !text.starts_with('{') { form.extend(pairs(&text)) }
            let bearer = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            let mut s = seen.lock().unwrap();
            s.calls.push((method.to_string(), path.clone(), form.clone()));
            let ok = |v: Value| (axum::http::StatusCode::OK, axum::Json(v));
            let no = |v: Value| (axum::http::StatusCode::BAD_REQUEST, axum::Json(v));
            match (method.as_str(), path.as_str()) {
                ("POST", "/token") if form.get("grant_type").map(String::as_str) == Some("authorization_code") => {
                    let made = base64url(&Sha256::digest(form["code_verifier"].as_bytes()));
                    if form["code"] != "the-code" || made != s.challenge || form["client_secret"] != "secret" { return no(json!({"error": "invalid_grant"})) }
                    ok(json!({"access_token": "at", "refresh_token": "rt"}))
                }
                ("POST", "/token") if form.get("refresh_token").map(String::as_str) == Some("rt") => ok(json!({"access_token": "at2"})),
                ("POST", "/token") => no(json!({"error": "invalid_grant", "error_description": "Token has been expired or revoked."})),
                ("GET", "/youtube/v3/channels") if bearer == "Bearer at" => ok(json!({"items": [{"snippet": {"title": "Grace Church"}}]})),
                (_, p) if p.starts_with("/youtube/") && bearer != "Bearer at2" => (axum::http::StatusCode::UNAUTHORIZED, axum::Json(json!({"error": {"message": "Invalid Credentials"}}))),
                ("GET", "/youtube/v3/liveStreams") => {
                    let items = if s.has_stream { json!([{"id": "old", "snippet": {"title": "Something else"}}, {"id": "s0", "snippet": {"title": "RustyPresenter"},
                        "cdn": {"ingestionInfo": {"ingestionAddress": "rtmp://a.rtmp.youtube.com/live2", "streamName": "kept-key"}}}]) } else { json!([]) };
                    ok(json!({"items": items}))
                }
                ("POST", "/youtube/v3/liveStreams") => {
                    let at = if s.ingest.is_empty() { "rtmp://a.rtmp.youtube.com/live2/".to_string() } else { s.ingest.clone() };
                    ok(json!({"id": "s1", "cdn": {"ingestionInfo": {"ingestionAddress": at, "streamName": "new-key"}}}))
                }
                ("POST", "/youtube/v3/liveBroadcasts") => { s.broadcast = serde_json::from_str(&text).unwrap(); ok(json!({"id": "b1"})) }
                ("POST", "/youtube/v3/liveBroadcasts/bind") => ok(json!({"id": "b1"})),
                ("GET", "/youtube/v3/videos") => ok(json!({"items": [{"liveStreamingDetails": {"concurrentViewers": "12"}}]})),
                ("POST", "/twitch-id/device") => ok(json!({"device_code": "tdc", "user_code": "TWCODE", "verification_uri": "https://www.twitch.tv/activate", "expires_in": 60, "interval": 0})),
                ("POST", "/twitch-id/token") if form.get("grant_type").map(String::as_str) == Some("urn:ietf:params:oauth:grant-type:device_code") => {
                    s.twitch_polls += 1;
                    if s.twitch_polls < 2 { return no(json!({"status": 400, "message": "authorization_pending"})) }
                    ok(json!({"access_token": "tat", "refresh_token": "trt"}))
                }
                // A public client's refresh token works once: a new one comes with each.
                ("POST", "/twitch-id/token") if form.get("refresh_token").map(String::as_str) == Some("trt") => ok(json!({"access_token": "tat2", "refresh_token": "trt2"})),
                ("POST", "/twitch-id/token") => no(json!({"status": 400, "message": "Invalid refresh token"})),
                (_, p) if p.starts_with("/helix/") && (headers.get("client-id").and_then(|v| v.to_str().ok()) != Some("tw-app") || !bearer.starts_with("Bearer tat")) => {
                    (axum::http::StatusCode::UNAUTHORIZED, axum::Json(json!({"message": "Invalid OAuth token"})))
                }
                ("GET", "/helix/users") => ok(json!({"data": [{"id": "u9", "login": "gracechurch", "display_name": "GraceChurch"}]})),
                ("PATCH", "/helix/channels") if form.get("broadcaster_id").map(String::as_str) == Some("u9") => {
                    s.twitch_title = serde_json::from_str::<Value>(&text).unwrap()["title"].as_str().unwrap_or("").to_string();
                    (axum::http::StatusCode::NO_CONTENT, axum::Json(json!(null)))
                }
                ("GET", "/helix/streams/key") if form.get("broadcaster_id").map(String::as_str) == Some("u9") => ok(json!({"data": [{"stream_key": "live_123_abc"}]})),
                ("GET", "/helix/streams") => ok(json!({"data": [{"viewer_count": 5}]})),
                ("POST", "/graph/device/login") => ok(json!({"code": "dc", "user_code": "ABC123", "verification_uri": "https://www.facebook.com/device", "expires_in": 60, "interval": 0})),
                ("POST", "/graph/device/login_status") => {
                    s.polls += 1;
                    if s.polls < 2 { return no(json!({"error": {"message": "pending", "code": 31, "error_subcode": 1_349_174}})) }
                    ok(json!({"access_token": "ut", "expires_in": 5_184_000}))
                }
                ("GET", "/graph/me/accounts") if form["access_token"] == "ut" => ok(json!({"data": [{"id": "p1", "name": "Grace Page", "access_token": "pt"}]})),
                ("POST", "/graph/p1/live_videos") if form["access_token"] == "pt" => ok(json!({"id": "v1", "secure_stream_url": "rtmps://live-api-s.facebook.com:443/rtmp/FB-1-0-xyz"})),
                ("GET", "/graph/v1") if form["fields"].contains("live_views") => ok(json!({"live_views": 7})),
                ("GET", "/graph/v1") => ok(json!({"permalink_url": "/gracepage/videos/v1/"})),
                ("POST", "/graph/v1") => ok(json!({"success": true})),
                _ => no(json!({"error": {"message": format!("unexpected {method} {path}")}})),
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await });
    base
}

fn ids() -> Ids { Ids { google_id: "app-id".into(), google_secret: "secret".into(), facebook_app: "fb-app".into(), facebook_token: "fb-token".into(), twitch_client: "tw-app".into() } }

fn client() -> &'static reqwest::Client { crate::bible::sources::client().unwrap() }

fn details() -> Details { Details { title: "Sunday Service — November 16, 2026".into(), description: "Welcome".into(), privacy: Privacy::Unlisted } }

#[tokio::test]
async fn youtube_sign_in_opens_the_browser_and_takes_googles_answer() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let base = serve(seen.clone()).await;
    let ends = Ends::at(&base);
    let seen2 = seen.clone();
    let (refresh, name) = google::sign_in(client(), &ends, &ids(), move |url| {
        // The browser: Google is told where to send the answer, and does (a stray icon request first).
        let u = url::Url::parse(url).unwrap();
        let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
        assert_eq!(q["scope"], google::SCOPE);
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["access_type"], "offline");
        assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"), "{}", q["redirect_uri"]);
        seen2.lock().unwrap().challenge = q["code_challenge"].clone();
        let back = q["redirect_uri"].clone();
        let state = q["state"].clone();
        tokio::spawn(async move {
            let _ = client().get(format!("{back}/favicon.ico")).send().await;
            let _ = client().get(format!("{back}/?state=wrong&code=stolen")).send().await;
            let page = client().get(format!("{back}/?state={state}&code=the-code&scope=x")).send().await.unwrap().text().await.unwrap();
            assert!(page.contains("close this tab"), "{page}");
        });
    }).await.unwrap();
    assert_eq!((refresh.as_str(), name.as_str()), ("rt", "Grace Church"));
}

#[tokio::test]
async fn youtube_sign_in_says_when_it_was_not_allowed() {
    let base = serve(Arc::new(Mutex::new(Seen::default()))).await;
    let r = google::sign_in(client(), &Ends::at(&base), &ids(), |url| {
        let q: HashMap<String, String> = url::Url::parse(url).unwrap().query_pairs().into_owned().collect();
        let back = format!("{}/?state={}&error=access_denied", q["redirect_uri"], q["state"]);
        tokio::spawn(async move { let _ = client().get(back).send().await; });
    }).await;
    assert_eq!(r, Err("it was not allowed".to_string()));
}

#[tokio::test]
async fn going_live_on_youtube_makes_the_broadcast_and_uses_the_apps_stream() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let base = serve(seen.clone()).await;
    let ends = Ends::at(&base);
    let d = Destination { service: Service::YouTube, on: true, ..Default::default() };
    let live = prepare(client(), &ends, &ids(), &d, "rt", &details()).await.unwrap();
    assert_eq!(live.address, "rtmp://a.rtmp.youtube.com/live2/new-key", "a stream for the app is made the first time");
    assert_eq!(live.watch, "https://youtu.be/b1");
    {
        let s = seen.lock().unwrap();
        assert_eq!(s.broadcast["snippet"]["title"], "Sunday Service — November 16, 2026");
        assert_eq!(s.broadcast["snippet"]["description"], "Welcome");
        assert_eq!(s.broadcast["status"]["privacyStatus"], "unlisted");
        assert_eq!(s.broadcast["contentDetails"]["enableAutoStart"], true, "it goes live when the stream arrives");
        assert_eq!(s.broadcast["contentDetails"]["enableAutoStop"], true);
        let bind = s.calls.iter().find(|c| c.1.ends_with("/bind")).expect("the broadcast is tied to the stream");
        assert_eq!((bind.2["id"].as_str(), bind.2["streamId"].as_str()), ("b1", "s1"));
    }
    assert_eq!(viewers(client(), &ends, &live).await, Some(12));
    // The next week, the same stream (and key) is used again.
    seen.lock().unwrap().has_stream = true;
    let again = prepare(client(), &ends, &ids(), &d, "rt", &details()).await.unwrap();
    assert_eq!(again.address, "rtmp://a.rtmp.youtube.com/live2/kept-key");
    let made = seen.lock().unwrap().calls.iter().filter(|c| c.0 == "POST" && c.1.ends_with("/liveStreams")).count();
    assert_eq!(made, 1, "the app's stream is made once");
}

#[tokio::test]
async fn a_youtube_sign_in_that_was_taken_back_says_so() {
    let base = serve(Arc::new(Mutex::new(Seen::default()))).await;
    let d = Destination { service: Service::YouTube, on: true, ..Default::default() };
    let e = prepare(client(), &Ends::at(&base), &ids(), &d, "revoked", &details()).await.unwrap_err();
    assert!(e.contains("expired or revoked"), "{e}");
}

#[tokio::test]
async fn facebook_sign_in_shows_a_code_and_keeps_the_pages_token() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let base = serve(seen.clone()).await;
    let ends = Ends::at(&base);
    let device = facebook::start(client(), &ends, &ids()).await.unwrap();
    assert_eq!((device.user_code.as_str(), device.at.as_str()), ("ABC123", "https://www.facebook.com/device"));
    let (saved, name) = facebook::wait(client(), &ends, &ids(), &device).await.unwrap();
    assert_eq!((saved.as_str(), name.as_str()), ("p1|pt", "Grace Page"));
    let s = seen.lock().unwrap();
    assert_eq!(s.polls, 2, "asked again while the code was not typed yet");
    let login = s.calls.iter().find(|c| c.1 == "/graph/device/login").unwrap();
    assert_eq!(login.2["access_token"], "fb-app|fb-token");
    assert!(login.2["scope"].contains("publish_video"));
}

#[tokio::test]
async fn going_live_on_facebook_makes_a_live_video_on_the_page_and_ends_it() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let base = serve(seen.clone()).await;
    let ends = Ends::at(&base);
    let d = Destination { service: Service::Facebook, on: true, ..Default::default() };
    let live = prepare(client(), &ends, &ids(), &d, "p1|pt", &details()).await.unwrap();
    assert_eq!(live.address, "rtmps://live-api-s.facebook.com:443/rtmp/FB-1-0-xyz");
    assert_eq!(live.watch, "https://www.facebook.com/gracepage/videos/v1/");
    assert_eq!(viewers(client(), &ends, &live).await, Some(7));
    finish(client(), &ends, &live).await;
    let s = seen.lock().unwrap();
    let made = s.calls.iter().find(|c| c.1 == "/graph/p1/live_videos").unwrap();
    assert_eq!((made.2["status"].as_str(), made.2["title"].as_str()), ("LIVE_NOW", "Sunday Service — November 16, 2026"));
    let ended = s.calls.iter().find(|c| c.0 == "POST" && c.1 == "/graph/v1").expect("the live video is ended");
    assert_eq!(ended.2["end_live_video"], "true");
}

#[tokio::test]
async fn twitch_sign_in_shows_a_code_and_keeps_the_channel() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let base = serve(seen.clone()).await;
    let ends = Ends::at(&base);
    let device = twitch::start(client(), &ends, &ids()).await.unwrap();
    assert_eq!((device.user_code.as_str(), device.at.as_str()), ("TWCODE", "https://www.twitch.tv/activate"));
    let (saved, name) = twitch::wait(client(), &ends, &ids(), &device).await.unwrap();
    assert_eq!((saved.as_str(), name.as_str()), ("u9|gracechurch|trt", "GraceChurch"));
    let s = seen.lock().unwrap();
    assert_eq!(s.twitch_polls, 2, "asked again while the code was not typed yet");
    let asked = s.calls.iter().find(|c| c.1 == "/twitch-id/device").unwrap();
    assert_eq!(asked.2["client_id"], "tw-app");
    assert!(asked.2["scopes"].contains("channel:read:stream_key") && asked.2["scopes"].contains("channel:manage:broadcast"));
}

#[tokio::test]
async fn going_live_on_twitch_sets_the_title_and_gets_the_key() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let base = serve(seen.clone()).await;
    let ends = Ends::at(&base);
    let d = Destination { service: Service::Twitch, on: true, ..Default::default() };
    let live = prepare(client(), &ends, &ids(), &d, "u9|gracechurch|trt", &details()).await.unwrap();
    assert_eq!(live.address, "rtmp://live.twitch.tv/app/live_123_abc");
    assert_eq!(live.watch, "https://www.twitch.tv/gracechurch");
    assert_eq!(live.renewed.as_deref(), Some("u9|gracechurch|trt2"), "the new refresh token is kept for next time");
    assert_eq!(viewers(client(), &ends, &live).await, Some(5));
    assert_eq!(seen.lock().unwrap().twitch_title, "Sunday Service — November 16, 2026");
    // The old refresh token no longer works: said, so the operator signs in again.
    assert!(prepare(client(), &ends, &ids(), &d, "u9|gracechurch|used", &details()).await.is_err());
}

#[test]
fn the_title_gets_the_date() {
    let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
    assert_eq!(title_for("", day, Service::YouTube), "Sunday Service — November 22, 2026");
    assert_eq!(title_for("Evening worship {date} on {service}", day, Service::Facebook), "Evening worship November 22, 2026 on Facebook");
    assert_eq!(title_for("Easter", day, Service::YouTube), "Easter");
}

#[test]
fn pkce_is_written_the_way_google_reads_it() {
    // The same as Python's hashlib and base64.urlsafe_b64encode, with the padding taken off.
    let c = base64url(&Sha256::digest(b"dBjftJeZ4CVP-mJ92K27uhbUJU1p1r_wW1gFWFOEjXk"));
    assert_eq!(c, "ngF5GsXcbwljx6u133FFr3Xht9xooA_DuaX_3QwODtc");
    assert_eq!(base64url(b"f"), "Zg");
    assert_eq!(base64url(b"fo"), "Zm8");
}

#[test]
fn a_sign_in_is_kept_with_its_name_and_forgotten() {
    let store = crate::stream::keys::Memory::default();
    let id = Uuid::from_u128(super::super::BUILT_IN[1].1);
    assert_eq!(kept(&store, id), None);
    keep(&store, id, "Grace\nPage", "p1|pt").unwrap();
    assert_eq!(kept(&store, id), Some(("Grace Page".to_string(), "p1|pt".to_string())));
    use crate::stream::keys::KeyStore;
    assert_eq!(store.get(id), None, "the stream key is not touched");
    forget(&store, id);
    assert_eq!(kept(&store, id), None);
}

#[test]
fn a_sign_in_is_kept_apart_from_the_stream_key() {
    let id = Uuid::from_u128(super::super::BUILT_IN[0].1);
    assert_ne!(token_id(id), id);
    assert_eq!(token_id(token_id(id)), id);
    assert!(!Ids::default().can_sign_in(Service::YouTube));
    assert!(ids().can_sign_in(Service::Facebook));
    assert!(ids().can_sign_in(Service::Twitch));
    assert!(!Ids::default().can_sign_in(Service::Twitch));
}
