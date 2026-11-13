//! `network/get` and `network/save` (local only, like every `/api/do` action): what the Network dialog shows and
//! saves. Passwords come in as typed and are stored only as hashes; `get` says only whether each one is set.
use super::{NetworkSettings, password, strings as s, tcp};
use crate::app::{ActionError, App};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub fn handles(action: &str) -> bool { matches!(action, "network/get" | "network/save") }

pub async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        "network/get" => Ok(view(app)),
        _ => {
            let req: SaveReq = serde_json::from_slice(body).map_err(|e| ActionError::bad(e.to_string()))?;
            save(app, req).map_err(ActionError::bad)?;
            Ok(view(app))
        }
    }
}

/// What to do with a password.
#[derive(Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum PasswordChange {
    #[default]
    Keep,
    Off,
    Set { password: String },
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct SaveReq {
    pub remote_password: PasswordChange,
    pub stage_view: bool,
    pub stage_password: PasswordChange,
    pub tcp: bool,
    pub tcp_port: Option<u16>,
    pub tcp_password: PasswordChange,
}

/// The new hash for one password, or an error to show.
fn apply(old: &Option<String>, change: &PasswordChange) -> Result<Option<String>, String> {
    match change {
        PasswordChange::Keep => Ok(old.clone()),
        PasswordChange::Off => Ok(None),
        PasswordChange::Set { password: p } if p.is_empty() => if old.is_some() { Ok(old.clone()) } else { Err(s::password_missing()) },
        PasswordChange::Set { password: p } if p.chars().count() < 4 => Err(s::password_short()),
        PasswordChange::Set { password: p } => Ok(Some(password::hash(p))),
    }
}

/// The settings `req` asks for, checked: passwords hashed, the port in range and not the app's own.
pub fn plan(old: &NetworkSettings, req: &SaveReq, http_port: u16) -> Result<NetworkSettings, String> {
    let port = req.tcp_port.unwrap_or(old.tcp_port);
    if port < 1024 { return Err(s::port_range()) }
    if port == http_port { return Err(s::port_taken(port)) }
    Ok(NetworkSettings {
        remote_password: apply(&old.remote_password, &req.remote_password)?,
        stage_view: req.stage_view,
        stage_password: apply(&old.stage_password, &req.stage_password)?,
        tcp: req.tcp,
        tcp_port: port,
        tcp_password: apply(&old.tcp_password, &req.tcp_password)?,
    })
}

fn save(app: &Arc<App>, req: SaveReq) -> Result<(), String> {
    let old = app.data.lock().unwrap().settings.network.clone();
    let new = plan(&old, &req, app.port)?;
    app.edit_untracked(|d| {
        // Serialize sign-outs with logins and the change to the password they check.
        if new.stage_password != d.settings.network.stage_password || !new.stage_view {
            app.net.stage_tokens.lock().unwrap().clear();
        }
        d.settings.network = new;
        Ok(())
    })?;
    tcp::sync(app)
}

/// What the dialog shows: the switches, whether each password is set, and the addresses to type on other devices.
pub fn view(app: &App) -> Value {
    let n = app.data.lock().unwrap().settings.network.clone();
    let host = app.lan_url.as_deref().and_then(|u| u.strip_prefix("http://")).and_then(|u| u.split('/').next()).unwrap_or("");
    let ip = host.rsplit_once(':').map_or(host, |(ip, _)| ip);
    json!({
        "remote_password": n.remote_password.is_some(),
        "stage_view": n.stage_view,
        "stage_password": n.stage_password.is_some(),
        "stage_url": if host.is_empty() { String::new() } else { format!("http://{host}/stage-view") },
        "tcp": n.tcp,
        "tcp_port": n.tcp_port,
        "tcp_password": n.tcp_password.is_some(),
        "tcp_address": if ip.is_empty() { String::new() } else { format!("{ip}:{}", n.tcp_port) },
        "tcp_running": tcp::running_port(app),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;

    fn set(p: &str) -> PasswordChange { PasswordChange::Set { password: p.into() } }

    #[test]
    fn plan_hashes_new_passwords_keeps_or_drops_old_ones_and_checks_the_port() {
        let old = NetworkSettings::default();
        let req = SaveReq { remote_password: set("praise"), stage_view: true, stage_password: PasswordChange::Keep, tcp: true, tcp_port: Some(5000), tcp_password: PasswordChange::Off };
        let n = plan(&old, &req, 4318).unwrap();
        assert!(password::verify("praise", n.remote_password.as_deref().unwrap()));
        assert_eq!((n.stage_view, n.stage_password.clone(), n.tcp, n.tcp_port, n.tcp_password.clone()), (true, None, true, 5000, None));
        // Keep, and an empty field, keep the old hash; Off drops it.
        let again = plan(&n, &SaveReq { remote_password: set(""), stage_view: true, tcp_port: None, ..Default::default() }, 4318).unwrap();
        assert_eq!((again.remote_password.clone(), again.tcp_port), (n.remote_password.clone(), 5000));
        assert_eq!(plan(&n, &SaveReq { remote_password: PasswordChange::Off, ..Default::default() }, 4318).unwrap().remote_password, None);
        // Errors.
        assert_eq!(plan(&old, &SaveReq { stage_password: set(""), ..Default::default() }, 4318).unwrap_err(), "Type a password, or turn the password off.");
        assert_eq!(plan(&old, &SaveReq { tcp_password: set("abc"), ..Default::default() }, 4318).unwrap_err(), "Use at least 4 characters for a password.");
        assert_eq!(plan(&old, &SaveReq { tcp_port: Some(80), ..Default::default() }, 4318).unwrap_err(), "Use a port from 1024 to 65535.");
        assert_eq!(plan(&old, &SaveReq { tcp_port: Some(4318), ..Default::default() }, 4318).unwrap_err(), "Port 4318 is the app's own. Pick another one.");
    }

    #[tokio::test]
    async fn save_stores_hashes_never_the_password_and_get_says_only_whether_one_is_set() {
        let app = test_app();
        let v = run(&app, "network/get", b"").await.unwrap();
        assert_eq!((v["stage_view"].as_bool(), v["tcp"].as_bool(), v["remote_password"].as_bool(), v["tcp_running"].clone()), (Some(false), Some(false), Some(false), Value::Null));
        let body = json!({ "remote_password": { "do": "set", "password": "hallelujah" }, "stage_view": true, "stage_password": { "do": "set", "password": "singers" } });
        let v = run(&app, "network/save", body.to_string().as_bytes()).await.unwrap();
        assert_eq!((v["remote_password"].as_bool(), v["stage_password"].as_bool(), v["stage_view"].as_bool()), (Some(true), Some(true), Some(true)));
        assert!(!v.to_string().contains("pbkdf2"), "no hash goes out");
        let stored = serde_json::to_string(&app.data.lock().unwrap().settings).unwrap();
        assert!(!stored.contains("hallelujah") && !stored.contains("singers") && stored.contains("pbkdf2-sha256$"));
        // A new stage password signs the stage views out.
        app.net.stage_tokens.lock().unwrap().push(("x".into(), std::time::Instant::now()));
        let body = json!({ "stage_view": true, "stage_password": { "do": "set", "password": "altos" } });
        run(&app, "network/save", body.to_string().as_bytes()).await.unwrap();
        assert!(app.net.stage_tokens.lock().unwrap().is_empty());
        assert!(run(&app, "network/save", br#"{"tcp_port": 10}"#).await.is_err());
    }
}
