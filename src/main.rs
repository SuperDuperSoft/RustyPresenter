// Release builds are a windowed app with no console window behind them.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rustypresenter::{displays, server::{self, App}, store::Store};
use tauri::Manager;
use std::{io::Write, net::{SocketAddr, TcpStream, UdpSocket}, time::Duration};

fn fail(msg: &str) -> ! {
    displays::error_box(msg);
    std::process::exit(1);
}

/// RustyPresenter is already running on `port`: ask it to bring its operator window forward.
fn show_running(port: u16) {
    let body = r#"{"role":"operator","monitor":null}"#;
    let req = format!(
        "POST /api/do/display/open HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nx-rp: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let sent = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_secs(3))
        .and_then(|mut s| s.write_all(req.as_bytes()));
    if sent.is_err() { fail(&format!("Port {port} is in use by another program, so RustyPresenter cannot start.")); }
}

fn main() {
    let port: u16 = std::env::var("RUSTYPRESENTER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(4318);
    let headless = std::env::args().any(|a| a == "--no-open");
    let Some(root) = server::data_dir() else { fail("Cannot find a folder to keep your data in. Set RUSTYPRESENTER_DATA.") };

    // Already running? Just bring up its operator window.
    let listener = match std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)) {
        Ok(l) => l,
        Err(_) => {
            if !headless { show_running(port); }
            println!("RustyPresenter is already running.");
            return;
        }
    };
    let _ = listener.set_nonblocking(true);

    let store = match Store::open(root.clone()) { Ok(s) => s, Err(e) => fail(&format!("Cannot use the data folder {}: {e}", root.display())) };
    store.clean_tmp();
    let data = match store.load() { Ok(d) => d, Err(e) => fail(&e) };
    let first_run = !store.data_path().exists();
    if first_run { let _ = store.save(&data); }

    let lan_ip = UdpSocket::bind("0.0.0.0:0").ok().and_then(|s| { s.connect("192.0.2.1:80").ok()?; s.local_addr().ok().map(|a| a.ip()) });
    let lan_url = lan_ip.map(|ip| format!("http://{ip}:{port}/remote"));
    let app = App::new(store, data, port, lan_url.clone());

    println!("\n  RustyPresenter {}\n", env!("CARGO_PKG_VERSION"));
    println!("  Operator:      http://127.0.0.1:{port}/");
    if let Some(u) = &lan_url { println!("  Phone remote:  {u}"); }
    println!("  Your data:     {}", root.display());
    println!("\n  Close the operator window to quit.\n");

    let setup_app = app.clone();
    let result = tauri::Builder::default()
        .setup(move |t| {
            let app = setup_app;
            let _ = app.tauri.set(t.handle().clone());

            // A complete backup at every start, then daily.
            {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        let data = app.data.lock().unwrap().clone();
                        if !data.songs.is_empty() || data.services.iter().any(|s| !s.items.is_empty()) {
                            if let Err(e) = app.store.backup(&data) { eprintln!("Backup failed: {e}"); }
                        }
                        tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
                    }
                });
            }
            tauri::async_runtime::spawn(app.clone().watchdog());

            let svc = server::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
            tauri::async_runtime::spawn(async move {
                let listener = match tokio::net::TcpListener::from_std(listener) { Ok(l) => l, Err(e) => { eprintln!("Server could not start: {e}"); return; } };
                if let Err(e) = axum::serve(listener, svc).await { eprintln!("Server stopped: {e}"); }
            });

            if !headless {
                app.open_window("operator", None)?;
                // Reopen the room/stage screens where they were last time, if those screens are plugged in.
                let roles = app.data.lock().unwrap().settings.displays.clone();
                let mons = displays::monitors();
                for (role, id) in [("room", roles.room), ("stage", roles.stage)] {
                    if let Some(id) = id { if mons.iter().any(|m| m.id == id && !m.primary) { let _ = app.open_window(role, Some(&id)); } }
                }
            }
            Ok(())
        })
        .on_window_event(|w, ev| {
            // Closing the operator window quits, taking the room and stage screens with it.
            if matches!(ev, tauri::WindowEvent::CloseRequested { .. }) && w.label().starts_with("operator-") {
                w.app_handle().exit(0);
            }
        })
        .run(tauri::generate_context!());
    if let Err(e) = result { fail(&format!("RustyPresenter could not start its windows: {e}")); }
}
