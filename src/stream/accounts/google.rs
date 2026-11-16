//! YouTube: Google's sign-in for desktop apps (the web browser opens; Google sends the answer back to this
//! computer, `127.0.0.1`, with PKCE), then the YouTube Live Streaming API.
use super::{Details, Ends, Ids, Live, Service, json, SIGN_IN_LIMIT};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// What the app may do on YouTube: make and manage its broadcasts.
pub const SCOPE: &str = "https://www.googleapis.com/auth/youtube";
/// The name of the one stream (the key) the app keeps on the channel and uses for every broadcast.
pub const STREAM_NAME: &str = "RustyPresenter";

/// The page the browser shows once Google has sent it back.
const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>RustyPresenter</title><body style=\"font:18px sans-serif;margin:3em\">\
<p>Signed in. You can close this tab and go back to RustyPresenter.</p>";

/// Signs in: opens the browser with `open` and waits for Google's answer. Gives what is kept (the refresh token)
/// and the channel's name.
pub async fn sign_in(client: &reqwest::Client, ends: &Ends, ids: &Ids, open: impl FnOnce(&str)) -> Result<(String, String), String> {
    let verifier = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let challenge = super::base64url(&Sha256::digest(verifier.as_bytes()));
    let state = uuid::Uuid::new_v4().simple().to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|e| e.to_string())?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr().map_err(|e| e.to_string())?.port());
    let url = url::Url::parse_with_params(&ends.google_auth, [
        ("client_id", ids.google_id.as_str()), ("redirect_uri", &redirect), ("response_type", "code"), ("scope", SCOPE),
        ("code_challenge", &challenge), ("code_challenge_method", "S256"), ("state", &state), ("access_type", "offline"), ("prompt", "consent"),
    ]).map_err(|e| e.to_string())?;
    open(url.as_str());
    let code = tokio::time::timeout(SIGN_IN_LIMIT, answer(&listener, &state)).await.map_err(|_| "it took too long".to_string())??;
    let tokens = json(client.post(&ends.google_token).form(&[
        ("code", code.as_str()), ("client_id", &ids.google_id), ("client_secret", &ids.google_secret), ("redirect_uri", &redirect),
        ("grant_type", "authorization_code"), ("code_verifier", &verifier),
    ]).send().await).await?;
    let refresh = tokens["refresh_token"].as_str().ok_or("Google gave no lasting sign-in")?.to_string();
    let access = tokens["access_token"].as_str().ok_or("Google gave no sign-in")?;
    let me = json(client.get(format!("{}/channels", ends.youtube)).query(&[("part", "snippet"), ("mine", "true")]).bearer_auth(access).send().await).await?;
    let name = me.pointer("/items/0/snippet/title").and_then(Value::as_str).ok_or("this Google account has no YouTube channel")?;
    Ok((refresh, name.to_string()))
}

/// Waits for the browser to come back with Google's answer: its code, or why there is none. Anything else (the
/// browser asking for an icon, a stray visit) gets the page and is passed over.
async fn answer(listener: &tokio::net::TcpListener, state: &str) -> Result<String, String> {
    loop {
        let (mut s, _) = listener.accept().await.map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 8192];
        let mut n = 0;
        while n < buf.len() {
            let got = s.read(&mut buf[n..]).await.unwrap_or(0);
            if got == 0 { break }
            n += got;
            if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") { break }
        }
        let head = String::from_utf8_lossy(&buf[..n]).into_owned();
        let page = format!("HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{DONE_PAGE}", DONE_PAGE.len());
        let _ = s.write_all(page.as_bytes()).await;
        let _ = s.shutdown().await;
        let Some(target) = head.lines().next().and_then(|l| l.split(' ').nth(1)) else { continue };
        let Ok(url) = url::Url::parse(&format!("http://127.0.0.1{target}")) else { continue };
        let q: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        if q.get("state").map(String::as_str) != Some(state) { continue }
        if let Some(e) = q.get("error") { return Err(if e == "access_denied" { "it was not allowed".into() } else { e.clone() }) }
        if let Some(code) = q.get("code") { return Ok(code.clone()) }
    }
}

/// A fresh access token from what sign-in kept.
async fn access(client: &reqwest::Client, ends: &Ends, ids: &Ids, refresh: &str) -> Result<String, String> {
    let t = json(client.post(&ends.google_token).form(&[
        ("refresh_token", refresh), ("client_id", &ids.google_id), ("client_secret", &ids.google_secret), ("grant_type", "refresh_token"),
    ]).send().await).await?;
    t["access_token"].as_str().map(str::to_string).ok_or_else(|| "Google gave no sign-in".into())
}

/// Makes a broadcast that starts when the stream arrives and ends when it stops, tied to the app's own stream.
pub async fn prepare(client: &reqwest::Client, ends: &Ends, ids: &Ids, refresh: &str, details: &Details) -> Result<Live, String> {
    let token = access(client, ends, ids, refresh).await?;
    let api = &ends.youtube;
    // The app's stream (its key), made once and used again every week.
    let streams = json(client.get(format!("{api}/liveStreams")).query(&[("part", "id,snippet,cdn"), ("mine", "true"), ("maxResults", "50")]).bearer_auth(&token).send().await).await?;
    let found = streams["items"].as_array().and_then(|all| all.iter().find(|s| s.pointer("/snippet/title").and_then(Value::as_str) == Some(STREAM_NAME)).cloned());
    let stream = match found {
        Some(s) => s,
        None => json(client.post(format!("{api}/liveStreams")).query(&[("part", "id,snippet,cdn,contentDetails")]).bearer_auth(&token).json(&json!({
            "snippet": { "title": STREAM_NAME },
            "cdn": { "ingestionType": "rtmp", "resolution": "variable", "frameRate": "variable" },
            "contentDetails": { "isReusable": true },
        })).send().await).await?,
    };
    let stream_id = stream["id"].as_str().ok_or("YouTube gave no stream")?;
    let base = stream.pointer("/cdn/ingestionInfo/ingestionAddress").and_then(Value::as_str).unwrap_or(Service::YouTube.ingest().unwrap_or_default());
    let key = stream.pointer("/cdn/ingestionInfo/streamName").and_then(Value::as_str).ok_or("YouTube gave no stream key")?;
    let start = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let broadcast = json(client.post(format!("{api}/liveBroadcasts")).query(&[("part", "id,snippet,status,contentDetails")]).bearer_auth(&token).json(&json!({
        "snippet": { "title": details.title, "description": details.description, "scheduledStartTime": start },
        "status": { "privacyStatus": details.privacy.youtube(), "selfDeclaredMadeForKids": false },
        "contentDetails": { "enableAutoStart": true, "enableAutoStop": true },
    })).send().await).await?;
    let id = broadcast["id"].as_str().ok_or("YouTube gave no broadcast")?.to_string();
    json(client.post(format!("{api}/liveBroadcasts/bind")).query(&[("id", id.as_str()), ("streamId", stream_id), ("part", "id")]).bearer_auth(&token).send().await).await?;
    Ok(Live { service: Service::YouTube, watch: format!("https://youtu.be/{id}"), address: format!("{}/{key}", base.trim_end_matches('/')), id, token })
}

pub async fn viewers(client: &reqwest::Client, ends: &Ends, live: &Live) -> Option<u64> {
    let v = json(client.get(format!("{}/videos", ends.youtube)).query(&[("part", "liveStreamingDetails"), ("id", live.id.as_str())]).bearer_auth(&live.token).send().await).await.ok()?;
    let n = v.pointer("/items/0/liveStreamingDetails/concurrentViewers")?;
    n.as_str().and_then(|s| s.parse().ok()).or_else(|| n.as_u64())
}
