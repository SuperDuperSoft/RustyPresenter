//! A phone, played by a test, over the app's own https server and certificate, as at church: it scans the QR code
//! shown in Settings, opens the secure page, pairs with the code's secret and sends pictures over a secure
//! WebSocket and sound over posts, which reach the camera feed's pictures and the phone microphone's input.
use super::*;
use crate::{app::App, stream::{phone_camera as cam, phone_mic as mic}};
use axum::serve::ListenerExt;
use rustls::pki_types::{CertificateDer, ServerName};
use serde_json::Value;
use std::net::IpAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// An app on a church network: its remote at `http://192.168.1.20:4318/remote`, as `startup::open` makes it.
pub(crate) fn app_on_network() -> Arc<App> {
    let root = std::env::temp_dir().join(format!("rp-test-{}", uuid::Uuid::new_v4()));
    let store = crate::store::Store::open(root).unwrap();
    let data = store.load().unwrap();
    App::new(store, data, 4318, Some("http://192.168.1.20:4318/remote".into()))
}

/// A phone's browser after "Proceed": it trusts only the app's own certificate.
#[derive(Debug)]
struct OnlyThis(Vec<u8>, Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for OnlyThis {
    fn verify_server_cert(&self, end: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: rustls::pki_types::UnixTime)
        -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if end.as_ref() == self.0.as_slice() { Ok(rustls::client::danger::ServerCertVerified::assertion()) } else { Err(rustls::Error::General("another certificate".into())) }
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.1.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.1.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> { self.1.signature_verification_algorithms.supported_schemes() }
}

type Tls = tokio_rustls::client::TlsStream<tokio::net::TcpStream>;

/// The app's https server on a free port, with its certificate (DER) to trust. Marked running, as `https::serve` does.
async fn secure_server(app: &Arc<App>) -> (SocketAddr, Vec<u8>) {
    let dir = app.store.root.clone();
    let k = https::certificate(&dir, &[IpAddr::from([127, 0, 0, 1])], chrono::Utc::now().timestamp()).unwrap();
    let der = k.cert_der().unwrap();
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = tcp.local_addr().unwrap();
    let listener = https::TlsListener::new(tcp, https::tls_config(&k).unwrap()).unwrap();
    let svc = crate::server::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(listener.tap_io(|_| {}), svc).await });
    *app.net.https.lock().unwrap() = Some(addr.port());
    (addr, der)
}

async fn connect(addr: SocketAddr, der: &[u8]) -> Tls {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let client = rustls::ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().unwrap()
        .dangerous().with_custom_certificate_verifier(Arc::new(OnlyThis(der.to_vec(), provider))).with_no_client_auth();
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    tokio_rustls::TlsConnector::from(Arc::new(client)).connect(ServerName::try_from("localhost").unwrap(), tcp).await.expect("the app's certificate")
}

/// One request over https, as the phone's page makes it (its own origin). The status, the cookie set and the body.
async fn ask(addr: SocketAddr, der: &[u8], method: &str, path: &str, cookie: &str, body: &[u8]) -> (u16, Option<String>, String) {
    let mut s = connect(addr, der).await;
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nOrigin: https://{addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    if !cookie.is_empty() { req += &format!("Cookie: {cookie}\r\n") }
    let mut bytes = (req + "\r\n").into_bytes();
    bytes.extend(body);
    s.write_all(&bytes).await.unwrap();
    let mut all = Vec::new();
    let _ = s.read_to_end(&mut all).await;
    let all = String::from_utf8_lossy(&all).into_owned();
    let (head, body) = all.split_once("\r\n\r\n").unwrap_or((&all, ""));
    let status = head.get(9..12).and_then(|c| c.parse().ok()).unwrap_or(0);
    let cookie = head.lines().find_map(|l| l.strip_prefix("set-cookie: ").or_else(|| l.strip_prefix("Set-Cookie: "))).map(|c| c.split(';').next().unwrap_or("").to_string());
    // A chunked body: the JSON is between the sizes.
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") { body.lines().filter(|l| !l.is_empty() && usize::from_str_radix(l.trim(), 16).is_err()).collect::<String>() } else { body.to_string() };
    (status, cookie, body)
}

fn secret_of(link: &str) -> String { link.split_once("#pair=").map(|(_, s)| s.to_string()).unwrap_or_default() }

fn jpeg(rgb: [u8; 3]) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(64, 36, image::Rgb(rgb));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

#[test]
fn the_codes_open_the_secure_pages_at_this_computers_address() {
    let app = app_on_network();
    // Not yet running: the port it will use, beside the app's port (4318 → 4320), on the network address.
    assert_eq!(address(&app, Kind::Camera).as_deref(), Some("https://192.168.1.20:4320/camera"));
    assert_eq!(address(&app, Kind::Mic).as_deref(), Some("https://192.168.1.20:4320/stage-view/mic"));
    assert_eq!(https::port_for(4318), 4320, "where startup serves the secure pages");
    let camera = link(&app, Kind::Camera).unwrap();
    assert!(camera.starts_with("https://192.168.1.20:4320/camera#pair="), "{camera}");
    assert_eq!(secret_of(&camera).len(), 64);
    assert_eq!(link(&app, Kind::Camera).unwrap(), camera, "the same code until it is used");
    let microphone = link(&app, Kind::Mic).unwrap();
    assert!(microphone.starts_with("https://192.168.1.20:4320/stage-view/mic#pair="));
    assert_ne!(secret_of(&microphone), secret_of(&camera), "a code for each");
    // The QR code is that link, drawn as the remote's.
    assert_eq!(qr_svg(&camera), crate::app::pair_qr_svg(&camera));
    assert!(qr_svg(&camera).is_some_and(|svg| svg.starts_with("<?xml") && svg.contains("<svg")));
    // Near its end: a new code.
    app.net.links.lock().unwrap()[0].as_mut().unwrap().until = Instant::now() + Duration::from_secs(60);
    assert_ne!(link(&app, Kind::Camera).unwrap(), camera);
    // Off the network: no address and no code.
    assert_eq!(address(&crate::app::test_app(), Kind::Camera), None);
    assert_eq!(health(&crate::app::test_app(), Kind::Camera, Firewall::Open), Health::NoNetwork);
}

/// The camera: scan, the secure page, one-scan pairing, then JPEG pictures over a secure WebSocket.
#[tokio::test]
async fn one_scan_pairs_a_phone_camera_and_its_pictures_arrive_over_https() {
    let app = app_on_network();
    let (addr, der) = secure_server(&app).await;
    assert_eq!(health(&app, Kind::Camera, Firewall::Open), Health::Waiting);
    let secret = secret_of(&link(&app, Kind::Camera).unwrap());
    // The page the code opens, over https, reads the secret from the address.
    let (status, _, page) = ask(addr, &der, "GET", Kind::Camera.path(), "", b"").await;
    assert_eq!(status, 200);
    assert!(page.contains("#pair=") || page.contains("pair="), "the page reads the code's secret");
    assert!(page.contains("history.replaceState"), "and wipes it from the address bar");
    // Not paired yet.
    let (_, _, info) = ask(addr, &der, "GET", "/camera/info", "", b"").await;
    assert_eq!(serde_json::from_str::<Value>(&info).unwrap()["paired"], false);
    // Pairing with the secret: no code to type, no pairing window open on the computer.
    assert!(app.pairing.lock().unwrap().is_none());
    let (status, cookie, body) = ask(addr, &der, "POST", "/phone/pair", "", json!({ "pair": secret, "name": "iPhone" }).to_string().as_bytes()).await;
    assert_eq!(status, 200, "{body}");
    let cookie = cookie.expect("the sign-in cookie");
    assert!(cookie.starts_with("rp_session="));
    assert!(app.phones.lock().unwrap().iter().any(|p| p.name == "iPhone"), "listed with the paired phones");
    // Used: the same code no longer works, and Settings shows a new one.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let (status, c, body) = ask(addr, &der, "POST", "/phone/pair", "", json!({ "pair": secret }).to_string().as_bytes()).await;
    assert_eq!((status, c), (400, None));
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["error"], tr::old_code());
    assert_ne!(secret_of(&link(&app, Kind::Camera).unwrap()), secret);
    // The page now knows it is paired; Settings shows the phone connected.
    let (_, _, info) = ask(addr, &der, "GET", "/camera/info", &cookie, b"").await;
    let info: Value = serde_json::from_str(&info).unwrap();
    assert_eq!((info["paired"].as_bool(), info["name"].as_str(), info["secure_port"].as_u64()), (Some(true), Some("iPhone"), Some(u64::from(addr.port()))));
    assert_eq!(health(&app, Kind::Camera, Firewall::Open), Health::Connected(Some("iPhone".into())));
    assert_eq!(health(&app, Kind::Camera, Firewall::Open).text(Kind::Camera, 4320), "Phone connected: iPhone");

    // Pictures over a secure WebSocket (wss), as the page sends them.
    let name = format!("Balcony {}", std::process::id());
    let mut s = connect(addr, &der).await;
    s.write_all(format!("GET /camera/ws?name={} HTTP/1.1\r\nHost: {addr}\r\nOrigin: https://{addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nCookie: {cookie}\r\n\r\n", name.replace(' ', "%20")).as_bytes()).await.unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") { let mut b = [0u8; 1]; s.read_exact(&mut b).await.unwrap(); head.push(b[0]) }
    assert!(String::from_utf8_lossy(&head).starts_with("HTTP/1.1 101"), "{}", String::from_utf8_lossy(&head));
    let picture = jpeg([200, 20, 20]);
    s.write_all(&crate::network::websocket::frame(2, &picture, Some([9, 8, 7, 6]))).await.unwrap();
    let mut h = [0u8; 2];
    s.read_exact(&mut h).await.unwrap();
    let mut answer = vec![0u8; usize::from(h[1] & 0x7F)];
    s.read_exact(&mut answer).await.unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&answer).unwrap()["light"], "off", "answered with the tally light");
    let (got, ..) = cam::newest(&name).expect("the picture reached the camera feed's pictures");
    assert_eq!(*got, picture);
    assert!(matches!(health(&app, Kind::Camera, Firewall::Open), Health::Sending(Some(_))));
    // The decoded picture, as the camera feed reads it: red.
    let rgba = cam::decode(&got, (64, 36)).unwrap();
    assert!(rgba[0] > 160 && rgba[1] < 80);
}

/// The microphone: one scan pairs it; it needs no stage screen on phones (still off); its sound reaches the input.
#[tokio::test]
async fn one_scan_pairs_a_phone_microphone_without_the_stage_view() {
    let _turn = mic::TEST_TURN.lock().unwrap_or_else(|e| e.into_inner());
    let app = app_on_network();
    assert!(!app.data.lock().unwrap().settings.network.stage_view, "the stage screen on phones stays off");
    let (addr, der) = secure_server(&app).await;
    let pcm: Vec<u8> = [1000i16, -1000, 500].iter().flat_map(|s| s.to_le_bytes()).collect();
    // Not paired: no.
    let (status, _, _) = ask(addr, &der, "POST", "/stage-view/mic/sound?rate=48000", "", &pcm).await;
    assert_eq!(status, 403);
    let (_, _, st) = ask(addr, &der, "GET", "/stage-view/mic/status", "", b"").await;
    let st: Value = serde_json::from_str(&st).unwrap();
    assert_eq!((st["allowed"].as_bool(), st["paired"].as_bool()), (Some(false), Some(false)));
    assert_eq!(st["words"]["needs_pairing"], mic::tr::needs_pairing());
    // The page the code opens reads the secret, pairs, and wipes it.
    let (status, _, page) = ask(addr, &der, "GET", Kind::Mic.path(), "", b"").await;
    assert!(status == 200 && page.contains("/phone/pair") && page.contains("history.replaceState"));
    let secret = secret_of(&link(&app, Kind::Mic).unwrap());
    let (status, cookie, _) = ask(addr, &der, "POST", "/phone/pair", "", json!({ "pair": secret, "name": "Pastor's phone" }).to_string().as_bytes()).await;
    assert_eq!(status, 200);
    let cookie = cookie.unwrap();
    let (_, _, st) = ask(addr, &der, "GET", "/stage-view/mic/status", &cookie, b"").await;
    let st: Value = serde_json::from_str(&st).unwrap();
    assert_eq!((st["allowed"].as_bool(), st["paired"].as_bool()), (Some(true), Some(true)));
    assert_eq!(st["secure_url"], format!("https://{addr}/stage-view/mic"));
    assert_eq!(health(&app, Kind::Mic, Firewall::Open), Health::Connected(Some("Pastor's phone".into())));
    // Its sound reaches the input reading the phone.
    let rx = mic::listen();
    let (status, _, body) = ask(addr, &der, "POST", "/stage-view/mic/sound?rate=48000", &cookie, &pcm).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["taken"], true);
    let got = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(got.len(), 12, "three samples, made stereo");
    assert_eq!(i16::from_le_bytes([got[0], got[1]]), 1000);
    assert_eq!(health(&app, Kind::Mic, Firewall::Open), Health::Sending(Some("Pastor's phone".into())));
    assert_eq!(health(&app, Kind::Mic, Firewall::Open).text(Kind::Mic, 4320), "Sending sound: Pastor's phone");
    drop(rx);
    // A stage view open to everyone is for looking only (B3): a phone that is not paired may not send.
    app.data.lock().unwrap().settings.network.stage_view = true;
    let (status, _, _) = ask(addr, &der, "POST", "/stage-view/mic/sound?rate=48000", "", &pcm).await;
    assert_eq!(status, 403);
    // The stage view's own access still works for phones signed in there with its password.
    app.data.lock().unwrap().settings.network.stage_password = Some("a stored hash".into());
    app.net.stage_tokens.lock().unwrap().push((crate::app::hash_token("tok"), std::time::Instant::now()));
    let (status, _, _) = ask(addr, &der, "POST", "/stage-view/mic/sound?rate=48000", "rp_stage=tok", &pcm).await;
    assert_eq!(status, 200);
}

/// With the remote's password set, the code alone is not enough: the page asks for the password, and the code
/// still works for it, up to five wrong tries.
#[tokio::test]
async fn the_remotes_password_is_asked_with_the_code() {
    let app = app_on_network();
    app.data.lock().unwrap().settings.network.remote_password = Some(crate::network::password::hash("worship"));
    let (addr, der) = secure_server(&app).await;
    let secret = secret_of(&link(&app, Kind::Mic).unwrap());
    let (status, c, body) = ask(addr, &der, "POST", "/phone/pair", "", json!({ "pair": secret }).to_string().as_bytes()).await;
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!((status, c, v["password"].as_bool()), (401, None, Some(true)));
    assert_eq!(v["error"], tr::password_needed());
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let (status, c, _) = ask(addr, &der, "POST", "/phone/pair", "", json!({ "pair": secret, "password": "praise" }).to_string().as_bytes()).await;
    assert_eq!((status, c), (401, None));
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let (status, c, _) = ask(addr, &der, "POST", "/phone/pair", "", json!({ "pair": secret, "password": "worship" }).to_string().as_bytes()).await;
    assert_eq!(status, 200);
    assert!(c.is_some());
    // Another site's page may not pair.
    let mut s = connect(addr, &der).await;
    let body = json!({ "pair": "x" }).to_string();
    s.write_all(format!("POST /phone/pair HTTP/1.1\r\nHost: {addr}\r\nOrigin: https://evil.example\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    let mut all = String::new();
    let _ = s.read_to_string(&mut all).await;
    assert!(all.starts_with("HTTP/1.1 403"), "{all}");
}

#[test]
fn the_health_lines_say_what_is_wrong_first() {
    let app = app_on_network();
    // The secure server has not started, or could not.
    assert_eq!(health(&app, Kind::Camera, Firewall::Open).text(Kind::Camera, 4320), "The secure server is starting…");
    set_secure_problem(&app, Some(port_problem(4320, &std::io::Error::from(std::io::ErrorKind::AddrInUse))));
    let h = health(&app, Kind::Mic, Firewall::Open);
    assert!(h.problem());
    assert_eq!(h.text(Kind::Mic, 4320), "The secure server isn't running: port 4320 is in use by another program. Close that program, or restart this computer.");
    assert!(port_problem(4320, &std::io::Error::from(std::io::ErrorKind::PermissionDenied)).starts_with("Windows keeps port 4320"));
    // Running: the firewall, when nothing came from a phone yet.
    *app.net.https.lock().unwrap() = Some(4320);
    let h = health(&app, Kind::Mic, Firewall::Blocked { public: true });
    assert_eq!(h, Health::Firewall { public: true });
    assert!(h.problem() && h.text(Kind::Mic, 4320).starts_with("This computer's firewall may be blocking phones:") && h.text(Kind::Mic, 4320).contains("public"));
    assert_eq!(health(&app, Kind::Mic, Firewall::Unknown).text(Kind::Mic, 4320), "Waiting for a phone");
    assert!(!health(&app, Kind::Mic, Firewall::Unknown).problem());
    // A phone that reached the page shows the firewall lets it in, whatever Windows said.
    saw(&app, Kind::Mic, None);
    assert_eq!(health(&app, Kind::Mic, Firewall::Blocked { public: false }), Health::Connected(None));
    assert_eq!(Health::Connected(None).text(Kind::Mic, 4320), "A phone is connected");
    app.net.seen.lock().unwrap()[1].as_mut().unwrap().at = Instant::now() - Duration::from_secs(60);
    assert_eq!(health(&app, Kind::Mic, Firewall::Open), Health::Waiting, "a while ago: waiting again");
    assert_eq!(Health::Sending(Some("Side".into())).text(Kind::Camera, 4320), "Sending pictures: Side");
}

/// By hand, to look at the real handshake with other tools (openssl s_client, curl): serves the phone pages over
/// https on `RP_HOLD_PORT` (default 4399) for `RP_HOLD_SECS` (default 60), its certificate written to `RP_HOLD_CERT` when set.
#[tokio::test]
#[ignore]
async fn hold_a_secure_server_open_for_a_minute() {
    let app = app_on_network();
    let port: u16 = std::env::var("RP_HOLD_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(4399);
    let k = https::make(&[IpAddr::from([127, 0, 0, 1])], chrono::Utc::now().timestamp()).unwrap();
    if let Ok(path) = std::env::var("RP_HOLD_CERT") { std::fs::write(path, k.cert_der().unwrap()).unwrap() }
    let tcp = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    let listener = https::TlsListener::new(tcp, https::tls_config(&k).unwrap()).unwrap();
    let svc = crate::server::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let secs = std::env::var("RP_HOLD_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    let _ = tokio::time::timeout(Duration::from_secs(secs), axum::serve(listener.tap_io(|_| {}), svc)).await;
}
