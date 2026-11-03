//! Startup shared by the Tauri and native binaries: the port (single instance), the data folder,
//! `App`, the phone/web server and the daily backup.
use crate::{app::App, platform::instance, server, store::Store};
use std::{net::{SocketAddr, TcpListener, UdpSocket}, path::PathBuf, sync::Arc, time::Duration};

/// The core of a running RustyPresenter, before any window.
pub struct Started { pub app: Arc<App>, pub listener: TcpListener, pub root: PathBuf }

/// The port the server listens on.
pub fn port() -> u16 { std::env::var("RUSTYPRESENTER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(4318) }

/// The data folder, without creating it.
pub fn data_root() -> Result<PathBuf, String> {
    server::data_dir().ok_or("Cannot find a folder to keep your data in. Set RUSTYPRESENTER_DATA.".to_string())
}

/// Binds the port and opens the data folder. `Ok(None)`: RustyPresenter is already running
/// (asked to show its operator window when `focus`).
pub fn start(focus: bool) -> Result<Option<Started>, String> {
    let port = port();
    let root = data_root()?;
    let Ok(listener) = TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)) else {
        if focus { instance::focus_running(port)? }
        return Ok(None);
    };
    let _ = listener.set_nonblocking(true);
    open(listener, root, port).map(Some)
}

/// Opens the data folder behind a port already held. Left-over work folders go on every start, so a killed
/// import's go too.
pub fn open(listener: TcpListener, root: PathBuf, port: u16) -> Result<Started, String> {
    let store = Store::open(root.clone()).map_err(|e| format!("Cannot use the data folder {}: {e}", root.display()))?;
    store.clean_tmp();
    let data = store.load()?;
    if !store.data_path().exists() { let _ = store.save(&data); }

    let lan_ip = UdpSocket::bind("0.0.0.0:0").ok().and_then(|s| { s.connect("192.0.2.1:80").ok()?; s.local_addr().ok().map(|a| a.ip()) });
    let lan_url = lan_ip.map(|ip| format!("http://{ip}:{port}/remote"));
    Ok(Started { app: App::new(store, data, port, lan_url), listener, root })
}

/// The console banner; `quit` says how to quit.
pub fn banner(s: &Started, quit: &str) {
    println!("\n  RustyPresenter {}\n", env!("CARGO_PKG_VERSION"));
    println!("  Operator:      http://127.0.0.1:{}/", s.app.port);
    if let Some(u) = &s.app.lan_url { println!("  Phone remote:  {u}"); }
    println!("  Your data:     {}", s.root.display());
    println!("\n  {quit}\n");
}

/// The operator page, phone remote and room/stage pages. Runs on a tokio runtime.
pub async fn serve(app: Arc<App>, listener: TcpListener) {
    let svc = server::router(app).into_make_service_with_connect_info::<SocketAddr>();
    let listener = match tokio::net::TcpListener::from_std(listener) { Ok(l) => l, Err(e) => { eprintln!("Server could not start: {e}"); return; } };
    if let Err(e) = axum::serve(listener, svc).await { eprintln!("Server stopped: {e}"); }
}

const BACKUP_EVERY: Duration = Duration::from_secs(24 * 3600);
/// While something plays, its position is saved this often: the room's reports alone don't save it.
const SAVE_PLAYING_EVERY: Duration = Duration::from_secs(5);

/// A complete backup at every start, then daily. A restart after a crash (`restored`) waits the full day:
/// another copy of the same data each time would push older good backups out of the ten kept.
pub async fn backups(app: Arc<App>, restored: bool) {
    if restored { tokio::time::sleep(BACKUP_EVERY).await; }
    loop {
        let data = app.data.lock().unwrap().clone();
        if !data.songs.is_empty() || data.services.iter().any(|s| !s.items.is_empty()) {
            if let Err(e) = app.store.backup(&data) { eprintln!("Backup failed: {e}"); }
        }
        tokio::time::sleep(BACKUP_EVERY).await;
    }
}

/// Keeps the saved frame's playback positions fresh while video or audio is on air.
pub async fn save_playing(app: Arc<App>) {
    let mut every = tokio::time::interval(SAVE_PLAYING_EVERY);
    loop {
        every.tick().await;
        if app.playing() { app.save_live_state(); }
    }
}
