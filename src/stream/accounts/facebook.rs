//! Facebook: "Login for Devices" (the operator types a short code at facebook.com/device on any phone or
//! computer; no web page comes back to the app), then the church's Page's Live Video API. A stream can only go to a
//! Page; the first Page the account manages is used.
use super::{Details, Ends, Ids, Live, Service, json};
use serde_json::Value;
use std::time::{Duration, Instant};

/// The Graph API version the app was written against.
pub const VERSION: &str = "v21.0";
/// What the app may do: find the Pages the account manages and go live on them.
pub const SCOPE: &str = "pages_show_list,pages_read_engagement,pages_manage_posts,publish_video";

/// A sign-in under way: what the operator types, and where.
#[derive(Clone, Debug)]
pub struct Device {
    pub user_code: String,
    pub at: String,
    code: String,
    every: Duration,
    until: Instant,
}

fn app_token(ids: &Ids) -> String { format!("{}|{}", ids.facebook_app, ids.facebook_token) }

/// Starts signing in: the code to show the operator.
pub async fn start(client: &reqwest::Client, ends: &Ends, ids: &Ids) -> Result<Device, String> {
    let v = json(client.post(format!("{}/device/login", ends.graph)).form(&[("access_token", app_token(ids).as_str()), ("scope", SCOPE)]).send().await).await?;
    let text = |k: &str| v[k].as_str().map(str::to_string).ok_or_else(|| format!("Facebook gave no {k}"));
    Ok(Device {
        user_code: text("user_code")?,
        at: v["verification_uri"].as_str().unwrap_or("https://www.facebook.com/device").to_string(),
        code: text("code")?,
        every: Duration::from_secs(v["interval"].as_u64().unwrap_or(5)),
        until: Instant::now() + Duration::from_secs(v["expires_in"].as_u64().unwrap_or(420)),
    })
}

/// Waits for the operator to type the code and allow it. Gives what is kept (the Page and its token) and the
/// Page's name.
pub async fn wait(client: &reqwest::Client, ends: &Ends, ids: &Ids, device: &Device) -> Result<(String, String), String> {
    let mut every = device.every;
    let user = loop {
        if Instant::now() >= device.until { return Err("the code ran out of time".into()) }
        tokio::time::sleep(every).await;
        let r = client.post(format!("{}/device/login_status", ends.graph)).form(&[("access_token", app_token(ids).as_str()), ("code", device.code.as_str())]).send().await.map_err(|e| e.to_string())?;
        let v: Value = r.json().await.map_err(|e| e.to_string())?;
        if let Some(t) = v["access_token"].as_str() { break t.to_string() }
        match v.pointer("/error/error_subcode").and_then(Value::as_u64) {
            // Not typed yet; asked too often.
            Some(1_349_174) => {}
            Some(1_349_172) => every += Duration::from_secs(5),
            Some(1_349_152) => return Err("the code ran out of time".into()),
            _ => return Err(v.pointer("/error/message").and_then(Value::as_str).unwrap_or("Facebook said no").to_string()),
        }
    };
    // A Page's token got with a lasting sign-in does not run out.
    let pages = json(client.get(format!("{}/me/accounts", ends.graph)).query(&[("fields", "id,name,access_token"), ("access_token", user.as_str())]).send().await).await?;
    let page = pages.pointer("/data/0").ok_or_else(super::tr::no_page)?;
    let (Some(id), Some(name), Some(token)) = (page["id"].as_str(), page["name"].as_str(), page["access_token"].as_str()) else { return Err(super::tr::no_page()) };
    Ok((format!("{id}|{token}"), name.to_string()))
}

/// Makes a live video on the Page, live as soon as the stream arrives.
pub async fn prepare(client: &reqwest::Client, ends: &Ends, saved: &str, details: &Details) -> Result<Live, String> {
    let (page, token) = saved.split_once('|').ok_or("the sign-in is not complete")?;
    let made = json(client.post(format!("{}/{page}/live_videos", ends.graph)).form(&[
        ("status", "LIVE_NOW"), ("title", details.title.as_str()), ("description", details.description.as_str()), ("access_token", token),
    ]).send().await).await?;
    let id = made["id"].as_str().ok_or("Facebook gave no live video")?.to_string();
    let more = json(client.get(format!("{}/{id}", ends.graph)).query(&[("fields", "secure_stream_url,permalink_url"), ("access_token", token)]).send().await).await?;
    let address = made["secure_stream_url"].as_str().or_else(|| more["secure_stream_url"].as_str()).ok_or("Facebook gave no stream address")?.to_string();
    let link = more["permalink_url"].as_str().unwrap_or_default();
    let watch = if link.starts_with("http") { link.to_string() } else if link.is_empty() { format!("https://www.facebook.com/{page}/live") } else { format!("https://www.facebook.com{link}") };
    Ok(Live { service: Service::Facebook, id, address, watch, token: token.to_string(), renewed: None, client: String::new() })
}

pub async fn viewers(client: &reqwest::Client, ends: &Ends, live: &Live) -> Option<u64> {
    let v = json(client.get(format!("{}/{}", ends.graph, live.id)).query(&[("fields", "live_views"), ("access_token", live.token.as_str())]).send().await).await.ok()?;
    v["live_views"].as_u64()
}

/// Ends the live video, so the Page shows it as a replay at once.
pub async fn finish(client: &reqwest::Client, ends: &Ends, live: &Live) {
    let r = client.post(format!("{}/{}", ends.graph, live.id)).form(&[("end_live_video", "true"), ("access_token", live.token.as_str())]).send().await;
    if let Err(e) = json(r).await { eprintln!("facebook: ending the live video: {e}") }
}
