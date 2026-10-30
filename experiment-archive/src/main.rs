use rustypresenter::{model::Service, server::{Host, router}};
use std::{net::{SocketAddr, UdpSocket}, path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::var("RUSTYPRESENTER_PORT").unwrap_or_else(|_| "4318".into()).parse()?;
    let data = std::env::var_os("RUSTYPRESENTER_DATA").map(PathBuf::from)
        .or_else(|| dirs::data_local_dir().map(|p| p.join("RustyPresenter")))
        .ok_or("Cannot find your data folder. Set RUSTYPRESENTER_DATA to a writable folder.")?;
    let path = data.join("service.json");
    // Never overwrite a damaged or newer service with an empty default.
    let service = Service::load(&path).map_err(std::io::Error::other)?;
    let lan_ip = UdpSocket::bind("0.0.0.0:0").ok().and_then(|socket| {
        socket.connect("192.0.2.1:80").ok()?;
        socket.local_addr().ok().map(|a| a.ip())
    });
    let remote_url = lan_ip.map(|ip| format!("http://{ip}:{port}/remote"));
    let host = Arc::new(Host::new(service, path, remote_url));
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).await?;
    let operator_url = format!("http://127.0.0.1:{port}/#{}", host.operator_token);
    println!("\nRustyPresenter — live church presentation prototype\n");
    println!("Operator: {operator_url}");
    if let Some(url) = &host.remote_url { println!("Phone remote: {url}"); }
    println!("Pairing code: {}", host.pairing_code);
    println!("Service file: {}", host.path.display());
    println!("\nKeep this window open during the service. Press Ctrl+C to stop.\n");
    if !std::env::args().any(|arg| arg == "--no-open") { open_browser(&operator_url); }
    axum::serve(listener, router(host).into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; }).await?;
    Ok(())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let result = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", url]).creation_flags(0x08000000).spawn()
    };
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(error) = result { eprintln!("Open the operator link above in your browser: {error}"); }
}
