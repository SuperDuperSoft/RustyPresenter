//! Startup shared by the Tauri and native binaries: the port (single instance), the data folder,
//! `App`, the phone/web server and the daily backup.
use crate::{app::App, server, store::Store};
use std::{io::Write, net::{SocketAddr, TcpListener, TcpStream, UdpSocket}, path::PathBuf, sync::Arc, time::Duration};

/// The core of a running RustyPresenter, before any window.
pub struct Started { pub app: Arc<App>, pub listener: TcpListener, pub root: PathBuf }

/// RustyPresenter is already running on `port`: ask it to bring its operator window forward.
fn show_running(port: u16) -> Result<(), String> {
    let body = r#"{"role":"operator","monitor":null}"#;
    let req = format!(
        "POST /api/do/display/open HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nx-rp: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_secs(3))
        .and_then(|mut s| s.write_all(req.as_bytes()))
        .map_err(|_| format!("Port {port} is in use by another program, so RustyPresenter cannot start."))
}

/// Binds the port and opens the data folder. `Ok(None)`: RustyPresenter is already running
/// (asked to show its operator window when `focus`).
pub fn start(focus: bool) -> Result<Option<Started>, String> {
    let port: u16 = std::env::var("RUSTYPRESENTER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(4318);
    let root = server::data_dir().ok_or("Cannot find a folder to keep your data in. Set RUSTYPRESENTER_DATA.")?;
    let Ok(listener) = TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)) else {
        if focus { show_running(port)? }
        return Ok(None);
    };
    let _ = listener.set_nonblocking(true);

    let store = Store::open(root.clone()).map_err(|e| format!("Cannot use the data folder {}: {e}", root.display()))?;
    store.clean_tmp();
    let data = store.load()?;
    if !store.data_path().exists() { let _ = store.save(&data); }

    let lan_ip = UdpSocket::bind("0.0.0.0:0").ok().and_then(|s| { s.connect("192.0.2.1:80").ok()?; s.local_addr().ok().map(|a| a.ip()) });
    let lan_url = lan_ip.map(|ip| format!("http://{ip}:{port}/remote"));
    Ok(Some(Started { app: App::new(store, data, port, lan_url), listener, root }))
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

/// A complete backup at every start, then daily.
pub async fn backups(app: Arc<App>) {
    loop {
        let data = app.data.lock().unwrap().clone();
        if !data.songs.is_empty() || data.services.iter().any(|s| !s.items.is_empty()) {
            if let Err(e) = app.store.backup(&data) { eprintln!("Backup failed: {e}"); }
        }
        tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
    }
}
