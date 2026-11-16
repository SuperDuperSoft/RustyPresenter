//! Twitch: the Device Code sign-in (the operator types a short code at twitch.tv/activate on any phone or
//! computer), then the Helix API: the channel's title is set to the broadcast's, and its stream key is got. A
//! Twitch stream needs no broadcast made: the channel goes live when the stream arrives.
//!
//! The app is a public client (no secret in it), so each refresh token works once: a new one comes with each use,
//! and it is kept again (`Live::renewed`).
use super::{Details, Ends, Ids, Live, Service};
use serde_json::Value;
use std::time::{Duration, Instant};

/// What the app may do: set the channel's title, and read its stream key.
pub const SCOPES: &str = "channel:manage:broadcast channel:read:stream_key";
/// Where Twitch takes streams.
pub const INGEST: &str = "rtmp://live.twitch.tv/app/";

/// A sign-in under way: what the operator types, and where.
#[derive(Clone, Debug)]
pub struct Device {
    pub user_code: String,
    pub at: String,
    code: String,
    every: Duration,
    until: Instant,
}

/// Twitch's answer: its JSON, and its own words when it said no (`message`).
async fn answer(r: Result<reqwest::Response, reqwest::Error>) -> Result<Value, String> {
    let r = r.map_err(|e| e.to_string())?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or_default();
    if status.is_success() { return Ok(v) }
    Err(v["message"].as_str().map_or_else(|| status.to_string(), str::to_string))
}

/// Starts signing in: the code to show the operator.
pub async fn start(client: &reqwest::Client, ends: &Ends, ids: &Ids) -> Result<Device, String> {
    let v = answer(client.post(format!("{}/device", ends.twitch_id)).form(&[("client_id", ids.twitch_client.as_str()), ("scopes", SCOPES)]).send().await).await?;
    let text = |k: &str| v[k].as_str().map(str::to_string).ok_or_else(|| format!("Twitch gave no {k}"));
    Ok(Device {
        user_code: text("user_code")?,
        at: v["verification_uri"].as_str().unwrap_or("https://www.twitch.tv/activate").to_string(),
        code: text("device_code")?,
        every: Duration::from_secs(v["interval"].as_u64().unwrap_or(5)),
        until: Instant::now() + Duration::from_secs(v["expires_in"].as_u64().unwrap_or(1800)),
    })
}

/// The channel signed in with `access`: its id, login and display name.
async fn user(client: &reqwest::Client, ends: &Ends, ids: &Ids, access: &str) -> Result<(String, String, String), String> {
    let v = answer(client.get(format!("{}/users", ends.helix)).bearer_auth(access).header("Client-Id", &ids.twitch_client).send().await).await?;
    let u = v.pointer("/data/0").ok_or("Twitch gave no channel")?;
    let text = |k: &str| u[k].as_str().map(str::to_string).ok_or_else(|| format!("Twitch gave no {k}"));
    Ok((text("id")?, text("login")?, text("display_name")?))
}

/// Waits for the operator to type the code and allow it. Gives what is kept ("id|login|refresh token") and the
/// channel's name.
pub async fn wait(client: &reqwest::Client, ends: &Ends, ids: &Ids, device: &Device) -> Result<(String, String), String> {
    let mut every = device.every;
    let (access, refresh) = loop {
        if Instant::now() >= device.until { return Err("the code ran out of time".into()) }
        tokio::time::sleep(every).await;
        let r = client.post(format!("{}/token", ends.twitch_id)).form(&[
            ("client_id", ids.twitch_client.as_str()), ("scopes", SCOPES), ("device_code", device.code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ]).send().await;
        match answer(r).await {
            Ok(v) => break (v["access_token"].as_str().unwrap_or_default().to_string(), v["refresh_token"].as_str().unwrap_or_default().to_string()),
            Err(m) if m == "authorization_pending" => {}
            Err(m) if m == "slow_down" => every += Duration::from_secs(5),
            Err(m) => return Err(m),
        }
    };
    let (id, login, name) = user(client, ends, ids, &access).await?;
    Ok((format!("{id}|{login}|{refresh}"), name))
}

/// Sets the channel's title and gets its stream key. What is kept is renewed (a refresh token works once).
pub async fn prepare(client: &reqwest::Client, ends: &Ends, ids: &Ids, saved: &str, details: &Details) -> Result<Live, String> {
    let mut parts = saved.splitn(3, '|');
    let (Some(id), Some(login), Some(refresh)) = (parts.next(), parts.next(), parts.next()) else { return Err("the sign-in is not complete".into()) };
    let v = answer(client.post(format!("{}/token", ends.twitch_id)).form(&[
        ("client_id", ids.twitch_client.as_str()), ("grant_type", "refresh_token"), ("refresh_token", refresh),
    ]).send().await).await?;
    let access = v["access_token"].as_str().ok_or("Twitch gave no access")?.to_string();
    let renewed = v["refresh_token"].as_str().map(|r| format!("{id}|{login}|{r}"));
    let auth = |b: reqwest::RequestBuilder| b.bearer_auth(&access).header("Client-Id", &ids.twitch_client);
    let title: String = details.title.chars().take(140).collect();
    answer(auth(client.patch(format!("{}/channels", ends.helix))).query(&[("broadcaster_id", id)]).json(&serde_json::json!({ "title": title })).send().await).await?;
    let key = answer(auth(client.get(format!("{}/streams/key", ends.helix))).query(&[("broadcaster_id", id)]).send().await).await?;
    let key = key.pointer("/data/0/stream_key").and_then(Value::as_str).ok_or("Twitch gave no stream key")?;
    Ok(Live { service: Service::Twitch, id: id.to_string(), address: format!("{INGEST}{key}"), watch: format!("https://www.twitch.tv/{login}"), token: access, renewed, client: ids.twitch_client.clone() })
}

pub async fn viewers(client: &reqwest::Client, ends: &Ends, ids: &Ids, live: &Live) -> Option<u64> {
    let v = answer(client.get(format!("{}/streams", ends.helix)).bearer_auth(&live.token).header("Client-Id", &ids.twitch_client).query(&[("user_id", live.id.as_str())]).send().await).await.ok()?;
    v.pointer("/data/0/viewer_count").and_then(Value::as_u64)
}
