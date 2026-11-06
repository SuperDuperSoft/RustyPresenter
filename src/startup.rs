//! Startup shared by the Tauri and native binaries: the port (single instance), the data folder,
//! `App`, the phone/web server and the daily backup.
use crate::{app::App, platform::instance, server, store::Store};
use std::{net::{SocketAddr, TcpListener, UdpSocket}, path::PathBuf, sync::Arc, time::Duration};

/// The core of a running RustyPresenter, before any window.
pub struct Started { pub app: Arc<App>, pub listener: TcpListener, pub root: PathBuf }

/// The port from `RUSTYPRESENTER_PORT`, or 4318.
pub fn port() -> u16 { std::env::var("RUSTYPRESENTER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(4318) }

/// Binds the port and opens the data folder. `Ok(None)`: RustyPresenter is already running
/// (asked to show its operator window when `focus`).
pub fn start(focus: bool) -> Result<Option<Started>, String> {
    let port = port();
    server::data_dir().ok_or(NO_DATA_FOLDER)?;
    let Ok(listener) = TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)) else {
        if focus { instance::focus_running(port)? }
        return Ok(None);
    };
    open(listener, port).map(Some)
}

const NO_DATA_FOLDER: &str = "Cannot find a folder to keep your data in. Set RUSTYPRESENTER_DATA.";

/// Opens the data folder and builds `App` on a port already bound (`platform::instance::bind`). Leftovers of
/// unfinished imports in `tmp/` go on every start, a restore after a crash included.
pub fn open(listener: TcpListener, port: u16) -> Result<Started, String> {
    let root = server::data_dir().ok_or(NO_DATA_FOLDER)?;
    let _ = listener.set_nonblocking(true);

    let store = Store::open(root.clone()).map_err(|e| format!("Cannot use the data folder {}: {e}", root.display()))?;
    store.clean_tmp();
    let data = store.load()?;
    if !store.data_path().exists() { let _ = store.save(&data); }

    let lan_ip = UdpSocket::bind("0.0.0.0:0").ok().and_then(|s| { s.connect("192.0.2.1:80").ok()?; s.local_addr().ok().map(|a| a.ip()) });
    let lan_url = lan_ip.map(|ip| format!("http://{ip}:{port}/remote"));
    Ok(Started { app: App::new(store, data, port, lan_url), listener, root })
}

/// The console banner; `quit` says how to quit. `web_operator`: list the browser operator page (the Tauri app);
/// the native binary leaves it out, because its operator is the native window, never a browser.
pub fn banner(s: &Started, quit: &str, web_operator: bool) { print!("{}", banner_text(s, quit, web_operator)) }

fn banner_text(s: &Started, quit: &str, web_operator: bool) -> String {
    let mut t = format!("\n  RustyPresenter {}\n\n", env!("CARGO_PKG_VERSION"));
    if web_operator { t += &format!("  Operator:      http://127.0.0.1:{}/\n", s.app.port); }
    if let Some(u) = &s.app.lan_url { t += &format!("  Phone remote:  {u}\n"); }
    t += &format!("  Your data:     {}\n", s.root.display());
    t += &format!("\n  {quit}\n\n");
    t
}

/// The operator page, phone remote and room/stage pages. Runs on a tokio runtime.
pub async fn serve(app: Arc<App>, listener: TcpListener) {
    let svc = server::router(app).into_make_service_with_connect_info::<SocketAddr>();
    let listener = match tokio::net::TcpListener::from_std(listener) { Ok(l) => l, Err(e) => { eprintln!("Server could not start: {e}"); return; } };
    if let Err(e) = axum::serve(listener, svc).await { eprintln!("Server stopped: {e}"); }
}

/// When the first backup is made: at once, except after a crash restart. Each restart would otherwise copy
/// the same data again and push older good backups out of the ten that are kept.
fn first_backup_after(restored: bool) -> Duration {
    if restored { BACKUP_EVERY } else { Duration::ZERO }
}

const BACKUP_EVERY: Duration = Duration::from_secs(24 * 3600);

/// A complete backup at every start (not a restored one, see `first_backup_after`), then daily.
pub async fn backups(app: Arc<App>, restored: bool) {
    tokio::time::sleep(first_backup_after(restored)).await;
    loop {
        let data = app.data.lock().unwrap().clone();
        if !data.songs.is_empty() || data.services.iter().any(|s| !s.items.is_empty()) {
            if let Err(e) = app.store.backup(&data) { eprintln!("Backup failed: {e}"); }
        }
        tokio::time::sleep(BACKUP_EVERY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restored_start_waits_a_day_before_its_first_backup() {
        assert_eq!(first_backup_after(false), Duration::ZERO);
        assert_eq!(first_backup_after(true), Duration::from_secs(24 * 3600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::App, store::Store};

    fn started(lan_url: Option<&str>) -> Started {
        let root = std::env::temp_dir().join(format!("rp-test-{}", uuid::Uuid::new_v4()));
        let store = Store::open(root.clone()).unwrap();
        let data = store.load().unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        Started { app: App::new(store, data, 4399, lan_url.map(String::from)), listener, root }
    }

    #[test]
    fn native_banner_lists_the_phone_remote_and_data_but_no_operator_page() {
        let s = started(Some("http://192.168.1.20:4399/remote"));
        let native = banner_text(&s, "Close the operator window (or press Ctrl+C here) to quit.", false);
        assert!(!native.contains("Operator:"), "{native}");
        assert!(!native.contains("127.0.0.1"), "{native}");
        assert!(native.contains("  Phone remote:  http://192.168.1.20:4399/remote\n"), "{native}");
        assert!(native.contains(&format!("  Your data:     {}\n", s.root.display())), "{native}");
        let tauri = banner_text(&s, "Press Ctrl+C to quit.", true);
        assert!(tauri.contains("  Operator:      http://127.0.0.1:4399/\n"), "the Tauri app still lists its page");
    }
}