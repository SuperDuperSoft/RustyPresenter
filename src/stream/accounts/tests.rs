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
    /// Google: how often an access was got from the refresh token.
    pub refreshes: u32,
    /// Broadcasts made ahead (their bodies), and the one asked about: its life ("ready", "complete").
    pub scheduled: Vec<Value>,
    pub scheduled_life: String,
    /// The thumbnail YouTube was sent: the video, its type and size.
    pub thumbnail: (String, String, usize),
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
                ("POST", "/token") if form.get("refresh_token").map(String::as_str) == Some("rt") => { s.refreshes += 1; ok(json!({"access_token": "at2", "expires_in": 3599})) }
                ("POST", "/token") => no(json!({"error": "invalid_grant", "error_description": "Token has been expired or revoked."})),
                ("GET", "/youtube/v3/channels") if bearer == "Bearer at" => ok(json!({"items": [{"snippet": {"title": "Grace Church"}}]})),
                ("GET", "/youtube/v3/channels") if bearer == "Bearer at2" => ok(json!({"items": [{"id": "UC_x5XG1OV2P6uZZ5FSM9Ttw"}]})),
                (_, p) if (p.starts_with("/youtube/") || p.starts_with("/upload/")) && bearer != "Bearer at2" => (axum::http::StatusCode::UNAUTHORIZED, axum::Json(json!({"error": {"message": "Invalid Credentials"}}))),
                ("GET", "/youtube/v3/liveStreams") => {
                    let items = if s.has_stream { json!([{"id": "old", "snippet": {"title": "Something else"}}, {"id": "s0", "snippet": {"title": "RustyPresenter"},
                        "cdn": {"ingestionInfo": {"ingestionAddress": "rtmp://a.rtmp.youtube.com/live2", "streamName": "kept-key"}}}]) } else { json!([]) };
                    ok(json!({"items": items}))
                }
                ("POST", "/youtube/v3/liveStreams") => {
                    let at = if s.ingest.is_empty() { "rtmp://a.rtmp.youtube.com/live2/".to_string() } else { s.ingest.clone() };
                    ok(json!({"id": "s1", "cdn": {"ingestionInfo": {"ingestionAddress": at, "streamName": "new-key"}}}))
                }
                ("POST", "/youtube/v3/liveBroadcasts") => {
                    let b: Value = serde_json::from_str(&text).unwrap();
                    if b.pointer("/snippet/scheduledStartTime").and_then(Value::as_str).is_some_and(|t| t.starts_with("2026-11-22")) {
                        s.scheduled.push(b);
                        return ok(json!({"id": "sched1", "snippet": {"liveChatId": "lc9"}}));
                    }
                    s.broadcast = b;
                    ok(json!({"id": "b1", "snippet": {"liveChatId": "lc1"}}))
                }
                ("GET", "/youtube/v3/liveBroadcasts") if form.get("id").map(String::as_str) == Some("sched1") => {
                    if s.scheduled_life.is_empty() { return ok(json!({"items": []})) }
                    ok(json!({"items": [{"id": "sched1", "status": {"lifeCycleStatus": s.scheduled_life}, "snippet": {"liveChatId": "lc9"}}]}))
                }
                ("GET", "/youtube/v3/liveChat/messages") if form.get("liveChatId").map(String::as_str) == Some("lc1") => {
                    if form.get("pageToken").map(String::as_str) == Some("p2") { return ok(json!({"items": [], "nextPageToken": "p2"})) }
                    ok(json!({"nextPageToken": "p2", "items": [{"snippet": {"displayMessage": "Hello from Ohio"}, "authorDetails": {"displayName": "Ruth"}}]}))
                }
                ("GET", "/graph/v1/comments") => ok(json!({"data": [
                    {"id": "c2", "from": {"name": "Mark"}, "message": "Amen"},
                    {"id": "c1", "from": {"name": "Lydia"}, "message": "Good morning"}]})),
                ("POST", "/youtube/v3/liveBroadcasts/bind") => ok(json!({"id": "b1"})),
                ("POST", "/upload/youtube/v3/thumbnails/set") => {
                    let kind = headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                    s.thumbnail = (form.get("videoId").cloned().unwrap_or_default(), kind, body.len());
                    ok(json!({"items": [{}]}))
                }
                ("GET", "/youtube/v3/videos") => ok(json!({"items": [{"liveStreamingDetails": {"concurrentViewers": "12"}}]})),
                ("POST", "/twitch-id/device") => ok(json!({"device_code": "tdc", "user_code": "TWCODE", "verification_uri": "https://www.twitch.tv/activate", "expires_in": 60, "interval": 0})),
                ("POST", "/twitch-id/token") if form.get("grant_type").map(String::as_str) == Some("urn:ietf:params:oauth:grant-type:device_code") => {
                    s.twitch_polls += 1;
                    if s.twitch_polls < 2 { return no(json!({"status": 400, "message": "authorization_pending"})) }
                    ok(json!({"access_token": "tat", "refresh_token": "trt"}))
                }
                // A public client's refresh token works once: a new one comes with each.
                ("POST", "/twitch-id/token") if form.get("refresh_token").map(String::as_str) == Some("trt") => ok(json!({"access_token": "tat2", "refresh_token": "trt2", "expires_in": 14000})),
                ("POST", "/twitch-id/token") if form.get("refresh_token").map(String::as_str) == Some("trt2") => ok(json!({"access_token": "tat3", "refresh_token": "trt3", "expires_in": 14000})),
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

fn details() -> Details { Details { title: "Sunday Service — November 16, 2026".into(), description: "Welcome".into(), privacy: Privacy::Unlisted, thumbnail: String::new(), scheduled: String::new() } }

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
    assert_eq!(viewers(client(), &ends, &ids(), &live).await, Some(12));
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
    assert_eq!(viewers(client(), &ends, &ids(), &live).await, Some(7));
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
    assert_eq!(viewers(client(), &ends, &ids(), &live).await, Some(5));
    assert_eq!(seen.lock().unwrap().twitch_title, "Sunday Service — November 16, 2026");
    // The old refresh token no longer works: said, so the operator signs in again.
    assert!(prepare(client(), &ends, &ids(), &d, "u9|gracechurch|used", &details()).await.is_err());
}

/// A service longer than an hour: YouTube's access runs out after one, so it is renewed before it does.
#[tokio::test]
async fn a_youtube_broadcast_keeps_its_sign_in_fresh_through_a_long_service() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let base = serve(seen.clone()).await;
    let ends = Ends::at(&base);
    let d = Destination { service: Service::YouTube, on: true, ..Default::default() };
    let live = prepare(client(), &ends, &ids(), &d, "rt", &details()).await.unwrap();
    assert_eq!(viewers(client(), &ends, &ids(), &live).await, Some(12));
    assert_eq!(seen.lock().unwrap().refreshes, 1, "fresh: not renewed");
    // An hour later.
    live.age_by(Duration::from_secs(3600));
    assert_eq!(viewers(client(), &ends, &ids(), &live).await, Some(12));
    assert_eq!(seen.lock().unwrap().refreshes, 2, "renewed before it ran out");
    // The copies kept elsewhere share it.
    let copy = live.clone();
    assert_eq!(viewers(client(), &ends, &ids(), &copy).await, Some(12));
    assert_eq!(seen.lock().unwrap().refreshes, 2);
}

/// Twitch's refresh token works once: a renewal while live is kept, so next week's sign-in still works.
#[tokio::test]
async fn a_twitch_renewal_while_live_is_kept() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let base = serve(seen.clone()).await;
    let ends = Ends::at(&base);
    let store = crate::stream::keys::Memory::default();
    let d = Destination { id: Uuid::from_u128(super::super::BUILT_IN[2].1), service: Service::Twitch, on: true, ..Default::default() };
    keep(&store, d.id, "GraceChurch", "u9|gracechurch|trt").unwrap();
    let live = prepare(client(), &ends, &ids(), &d, "u9|gracechurch|trt", &details()).await.unwrap();
    live.age_by(Duration::from_secs(14_000));
    assert_eq!(viewers_kept(client(), &ends, &ids(), &live, &store).await, Some(5));
    assert_eq!(kept(&store, d.id), Some(("GraceChurch".to_string(), "u9|gracechurch|trt3".to_string())));
}

#[tokio::test]
async fn a_broadcast_made_ahead_is_the_one_go_live_uses() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let base = serve(seen.clone()).await;
    let ends = Ends::at(&base);
    let at = chrono::DateTime::parse_from_rfc3339("2026-11-22T15:30:00Z").unwrap().with_timezone(&chrono::Utc);
    let (id, link) = google::schedule(client(), &ends, &ids(), "rt", &details(), at).await.unwrap();
    assert_eq!((id.as_str(), link.as_str()), ("sched1", "https://youtu.be/sched1"));
    let made = seen.lock().unwrap().scheduled[0].clone();
    assert_eq!(made.pointer("/snippet/scheduledStartTime").and_then(Value::as_str), Some("2026-11-22T15:30:00Z"));
    assert_eq!(made.pointer("/contentDetails/enableAutoStart"), Some(&json!(true)));
    // Sunday: Go Live binds the stream to it instead of making another.
    seen.lock().unwrap().scheduled_life = "ready".into();
    let d = Destination { service: Service::YouTube, on: true, ..Default::default() };
    let ahead = Details { scheduled: "sched1".into(), ..details() };
    let live = prepare(client(), &ends, &ids(), &d, "rt", &ahead).await.unwrap();
    assert_eq!((live.id.as_str(), live.watch.as_str(), live.chat.as_str()), ("sched1", "https://youtu.be/sched1", "lc9"));
    let s = seen.lock().unwrap();
    assert!(s.broadcast.is_null(), "no new broadcast");
    assert!(s.calls.iter().any(|c| c.1 == "/youtube/v3/liveBroadcasts/bind" && c.2["id"] == "sched1"));
    drop(s);
    // Already over (or deleted on YouTube): a new one is made.
    seen.lock().unwrap().scheduled_life = "complete".into();
    let live = prepare(client(), &ends, &ids(), &d, "rt", &ahead).await.unwrap();
    assert_eq!(live.id, "b1");
}

#[test]
fn a_broadcast_made_ahead_is_for_the_services_day_and_time() {
    use chrono::TimeZone;
    let tz = chrono::FixedOffset::west_opt(5 * 3600).unwrap();
    let now = tz.with_ymd_and_hms(2026, 11, 18, 9, 0, 0).unwrap();
    let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 22).unwrap();
    let at = |d, t| google::schedule_time(d, t, now);
    assert_eq!(at(Some(day), Some((10, 30))), tz.with_ymd_and_hms(2026, 11, 22, 10, 30, 0).unwrap());
    assert_eq!(at(None, None), tz.with_ymd_and_hms(2026, 11, 22, 10, 0, 0).unwrap(), "next Sunday at 10");
    assert_eq!(at(Some(chrono::NaiveDate::from_ymd_opt(2026, 11, 15).unwrap()), Some((10, 30))), tz.with_ymd_and_hms(2026, 11, 22, 10, 30, 0).unwrap(), "a past day: next Sunday");
}

#[tokio::test]
async fn a_youtube_broadcast_gets_the_churchs_thumbnail() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let base = serve(seen.clone()).await;
    let ends = Ends::at(&base);
    let dir = std::env::temp_dir().join(format!("rp-test-thumb-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let pic = dir.join("Sunday.png");
    image::RgbImage::from_pixel(64, 36, image::Rgb([10, 80, 160])).save(&pic).unwrap();
    let d = Destination { service: Service::YouTube, on: true, ..Default::default() };
    let with = Details { thumbnail: pic.display().to_string(), ..details() };
    prepare(client(), &ends, &ids(), &d, "rt", &with).await.unwrap();
    let (video, kind, size) = seen.lock().unwrap().thumbnail.clone();
    assert_eq!((video.as_str(), kind.as_str()), ("b1", "image/png"));
    assert_eq!(size, std::fs::metadata(&pic).unwrap().len() as usize);
    // A picture that is gone: the broadcast is made all the same.
    let gone = Details { thumbnail: dir.join("gone.jpg").display().to_string(), ..details() };
    assert!(prepare(client(), &ends, &ids(), &d, "rt", &gone).await.is_ok());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn chat_is_read_from_youtube_and_facebook_once_each() {
    use crate::stream::chat::{Cursor, Message, next};
    let seen = Arc::new(Mutex::new(Seen::default()));
    let base = serve(seen.clone()).await;
    let ends = Ends::at(&base);
    let yt = prepare(client(), &ends, &ids(), &Destination { service: Service::YouTube, on: true, ..Default::default() }, "rt", &details()).await.unwrap();
    assert_eq!(yt.chat, "lc1");
    let mut c = Cursor::default();
    assert_eq!(next(client(), &ends, &ids(), &yt, &mut c).await.unwrap(), [Message { from: Service::YouTube, name: "Ruth".into(), text: "Hello from Ohio".into() }]);
    assert_eq!(next(client(), &ends, &ids(), &yt, &mut c).await.unwrap(), [], "the next page: nothing new");
    let fb = prepare(client(), &ends, &ids(), &Destination { service: Service::Facebook, on: true, ..Default::default() }, "p1|pt", &details()).await.unwrap();
    let mut c = Cursor::default();
    let got: Vec<String> = next(client(), &ends, &ids(), &fb, &mut c).await.unwrap().into_iter().map(|m| format!("{}: {}", m.name, m.text)).collect();
    assert_eq!(got, ["Lydia: Good morning", "Mark: Amen"], "oldest first");
    assert_eq!(next(client(), &ends, &ids(), &fb, &mut c).await.unwrap(), [], "each once");
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

#[tokio::test]
async fn the_signed_in_channels_id_is_found_for_the_website_player() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let ends = Ends::at(&serve(seen.clone()).await);
    assert_eq!(google::channel_id(client(), &ends, &ids(), "rt").await.as_deref(), Ok("UC_x5XG1OV2P6uZZ5FSM9Ttw"));
    assert!(google::channel_id(client(), &ends, &ids(), "revoked").await.is_err());
}
