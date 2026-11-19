//! The phone pages over https, for the phone camera. A browser lets a page use the camera only when the page is
//! private (https) or comes from the phone itself, and the app's phone server is plain http on the church's
//! network. So the same pages are also served over https on the next port but one (4320 beside 4318), with a
//! certificate the app makes itself (self-signed: no one on the internet can vouch for a computer on a church's
//! network). The phone warns once that the connection is not private; the volunteer goes on, and the phone
//! remembers. The certificate is kept in the data folder so that the phone's "go on" lasts: it is made again only
//! when this computer's network address changes or it is about to run out (Apple phones refuse certificates made
//! for more than 825 days).
use crate::app::App;
use axum::serve::ListenerExt;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::{net::{IpAddr, SocketAddr}, path::Path, sync::Arc, time::Duration};

/// The https port beside the phone server's `port`.
pub fn port_for(port: u16) -> u16 { port.wrapping_add(2) }

/// How long a certificate is made for, and how long before its end a new one is made.
const LASTS_DAYS: i64 = 800;
const RENEW_DAYS: i64 = 30;

/// The certificate and its key, as kept in the data folder (`https.json`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Kept {
    /// The certificate (DER) and its private key (PKCS#8 DER), in hex.
    cert: String,
    key: String,
    /// The addresses it was made for, and when it runs out (seconds since 1970).
    ips: Vec<IpAddr>,
    until: i64,
}

impl Kept {
    pub(crate) fn cert_der(&self) -> Option<Vec<u8>> { unhex(&self.cert) }
    fn key_der(&self) -> Option<Vec<u8>> { unhex(&self.key) }
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) { return None }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

// ---- The certificate, written out by hand in DER (the certificate crates are not in the build) ----

fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let n = content.len();
    if n < 0x80 { out.push(n as u8) } else {
        let bytes: Vec<u8> = n.to_be_bytes().into_iter().skip_while(|b| *b == 0).collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend(bytes);
    }
    out.extend(content);
    out
}

fn seq(parts: &[Vec<u8>]) -> Vec<u8> { der(0x30, &parts.concat()) }

const ECDSA_SHA256: &[u8] = &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02];
const EC_PUBLIC_KEY: &[u8] = &[0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
const P256: &[u8] = &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
const COMMON_NAME: &[u8] = &[0x06, 0x03, 0x55, 0x04, 0x03];
const ALT_NAMES: &[u8] = &[0x06, 0x03, 0x55, 0x1D, 0x11];
const KEY_PURPOSE: &[u8] = &[0x06, 0x03, 0x55, 0x1D, 0x25];
const SERVER_AUTH: &[u8] = &[0x06, 0x08, 0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01];
const BASIC: &[u8] = &[0x06, 0x03, 0x55, 0x1D, 0x13];

/// `UTCTime` for `secs` since 1970.
fn utc_time(secs: i64) -> Vec<u8> {
    let t = chrono::DateTime::from_timestamp(secs, 0).unwrap_or_default();
    der(0x17, t.format("%y%m%d%H%M%SZ").to_string().as_bytes())
}

/// A new certificate for this computer at `ips` (and `localhost`), from `now` (seconds since 1970).
pub fn make(ips: &[IpAddr], now: i64) -> Result<Kept, String> {
    use ring::{rand::SecureRandom, signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair}};
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).map_err(|e| e.to_string())?;
    let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng).map_err(|e| e.to_string())?;
    let mut serial = [0u8; 16];
    rng.fill(&mut serial).map_err(|e| e.to_string())?;
    serial[0] = (serial[0] & 0x7F) | 0x10;
    let name = seq(&[der(0x31, &seq(&[COMMON_NAME.to_vec(), der(0x0C, b"RustyPresenter on this computer")]))]);
    let algorithm = seq(&[ECDSA_SHA256.to_vec()]);
    let until = now + LASTS_DAYS * 86400;
    let mut alt: Vec<Vec<u8>> = ips.iter().map(|ip| match ip { IpAddr::V4(a) => der(0x87, &a.octets()), IpAddr::V6(a) => der(0x87, &a.octets()) }).collect();
    alt.push(der(0x82, b"localhost"));
    let extensions = der(0xA3, &seq(&[
        seq(&[BASIC.to_vec(), der(0x04, &seq(&[]))]),
        seq(&[KEY_PURPOSE.to_vec(), der(0x04, &seq(&[SERVER_AUTH.to_vec()]))]),
        seq(&[ALT_NAMES.to_vec(), der(0x04, &seq(&alt))]),
    ]));
    let public = seq(&[seq(&[EC_PUBLIC_KEY.to_vec(), P256.to_vec()]), der(0x03, &[&[0u8][..], pair.public_key().as_ref()].concat())]);
    let tbs = seq(&[
        der(0xA0, &der(0x02, &[2])),
        der(0x02, &serial),
        algorithm.clone(),
        name.clone(),
        seq(&[utc_time(now - 86400), utc_time(until)]),
        name,
        public,
        extensions,
    ]);
    let signature = pair.sign(&rng, &tbs).map_err(|e| e.to_string())?;
    let cert = seq(&[tbs, algorithm, der(0x03, &[&[0u8][..], signature.as_ref()].concat())]);
    Ok(Kept { cert: crate::sha256::hex(&cert), key: crate::sha256::hex(pkcs8.as_ref()), ips: ips.to_vec(), until })
}

/// The certificate kept in `dir`, or a new one (kept there) when there is none, it does not cover `ips`, or it
/// runs out within `RENEW_DAYS`.
pub fn certificate(dir: &Path, ips: &[IpAddr], now: i64) -> Result<Kept, String> {
    let file = dir.join("https.json");
    let kept: Option<Kept> = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok());
    if let Some(k) = kept.filter(|k| ips.iter().all(|ip| k.ips.contains(ip)) && k.until - now > RENEW_DAYS * 86400 && k.cert_der().is_some() && k.key_der().is_some()) {
        return Ok(k);
    }
    let k = make(ips, now)?;
    if let Err(e) = crate::store::write_atomic(&file, &serde_json::to_vec(&k).map_err(|e| e.to_string())?) { eprintln!("https: the certificate could not be kept: {e}") }
    Ok(k)
}

/// The server's TLS settings for certificate `k`.
pub fn tls_config(k: &Kept) -> Result<Arc<rustls::ServerConfig>, String> {
    let (cert, key) = (k.cert_der().ok_or("bad certificate")?, k.key_der().ok_or("bad key")?);
    let mut c = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions().map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(cert)], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)))
        .map_err(|e| e.to_string())?;
    c.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(c))
}

/// Connections that finished their TLS handshake, for `axum::serve`. Handshakes run side by side, so a slow or
/// silent phone holds up no one.
pub struct TlsListener {
    rx: tokio::sync::mpsc::Receiver<(tokio_rustls::server::TlsStream<tokio::net::TcpStream>, SocketAddr)>,
    local: SocketAddr,
}

impl TlsListener {
    pub fn new(tcp: tokio::net::TcpListener, config: Arc<rustls::ServerConfig>) -> std::io::Result<TlsListener> {
        let local = tcp.local_addr()?;
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let acceptor = tokio_rustls::TlsAcceptor::from(config);
        tokio::spawn(async move {
            loop {
                let Ok((stream, addr)) = tcp.accept().await else { tokio::time::sleep(Duration::from_millis(100)).await; continue };
                let (acceptor, to) = (acceptor.clone(), tx.clone());
                tokio::spawn(async move {
                    if let Ok(Ok(tls)) = tokio::time::timeout(Duration::from_secs(10), acceptor.accept(stream)).await { let _ = to.send((tls, addr)).await; }
                });
                if tx.is_closed() { return }
            }
        });
        Ok(TlsListener { rx, local })
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.rx.recv().await {
            Some(c) => c,
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> { Ok(self.local) }
}

/// This computer's address on the church's network (the one phones reach it at), if it has one.
pub fn lan_ip() -> Option<IpAddr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:80").ok()?;
    s.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_unspecified())
}

/// How often the port is tried again while another program holds it.
const RETRY: Duration = Duration::from_secs(5);

/// Serves the phone pages over https on `port` until the app ends. When it cannot, it says why where Settings shows
/// it (`phone_link::Health`) and, while the port is taken, tries again every few seconds. Runs on a tokio runtime.
pub async fn serve(app: Arc<App>, port: u16) { serve_trying(app, port, RETRY).await }

async fn serve_trying(app: Arc<App>, port: u16, retry: Duration) {
    use super::phone_link::{port_problem, set_secure_problem, tr};
    let ips: Vec<IpAddr> = std::iter::once(IpAddr::from([127, 0, 0, 1])).chain(lan_ip()).collect();
    let dir = app.store.root.clone();
    let made = tokio::task::spawn_blocking(move || certificate(&dir, &ips, chrono::Utc::now().timestamp()).and_then(|k| tls_config(&k))).await;
    let config = match made.map_err(|e| e.to_string()).and_then(|r| r) {
        Ok(c) => c,
        Err(e) => { eprintln!("https: no certificate: {e}"); return set_secure_problem(&app, Some(tr::no_certificate(&e))) }
    };
    let tcp = loop {
        match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
            Ok(t) => break t,
            Err(e) => {
                eprintln!("https: port {port} cannot be used: {e}");
                set_secure_problem(&app, Some(port_problem(port, &e)));
                tokio::time::sleep(retry).await;
            }
        }
    };
    let listener = match TlsListener::new(tcp, config) { Ok(l) => l, Err(e) => { eprintln!("https: {e}"); return set_secure_problem(&app, Some(port_problem(port, &e))) } };
    set_secure_problem(&app, None);
    *app.net.https.lock().unwrap() = Some(port);
    let svc = crate::server::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    if let Err(e) = axum::serve(listener.tap_io(|_| {}), svc).await { eprintln!("https stopped: {e}") }
    *app.net.https.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A phone's browser, trusting only this certificate.
    #[derive(Debug)]
    struct OnlyThis(Vec<u8>, Arc<rustls::crypto::CryptoProvider>);

    impl rustls::client::danger::ServerCertVerifier for OnlyThis {
        fn verify_server_cert(&self, end: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &rustls::pki_types::ServerName<'_>, _: &[u8], _: rustls::pki_types::UnixTime)
            -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            if end.as_ref() == self.0.as_slice() { Ok(rustls::client::danger::ServerCertVerified::assertion()) } else { Err(rustls::Error::General("another certificate".into())) }
        }
        fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(m, c, d, &self.1.signature_verification_algorithms)
        }
        fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            // Checks the signature with the public key read from the certificate: the certificate must be readable.
            rustls::crypto::verify_tls13_signature(m, c, d, &self.1.signature_verification_algorithms)
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> { self.1.signature_verification_algorithms.supported_schemes() }
    }

    #[test]
    fn the_certificate_is_kept_until_the_address_changes_or_it_runs_out() {
        let dir = crate::store::test_dir();
        let now = 1_790_000_000;
        let a: IpAddr = "192.168.1.20".parse().unwrap();
        let first = certificate(&dir, &[a], now).unwrap();
        let cert = first.cert_der().unwrap();
        assert!(cert.windows(6).any(|w| w == [0x87, 4, 192, 168, 1, 20]), "made for this computer's address");
        assert!(cert.windows(9).any(|w| w == *b"localhost"));
        assert_eq!(certificate(&dir, &[a], now + 86400).unwrap(), first, "kept");
        assert_ne!(certificate(&dir, &["192.168.1.21".parse().unwrap()], now).unwrap(), first, "a new address: a new certificate");
        let again = certificate(&dir, &[a], now).unwrap();
        assert_ne!(certificate(&dir, &[a], now + (LASTS_DAYS - RENEW_DAYS + 1) * 86400).unwrap(), again, "about to run out");
        assert!(LASTS_DAYS <= 825, "Apple phones refuse longer");
    }

    /// Another program holds the secure port: Settings says so in plain words, and the server starts by itself once
    /// the port is free.
    #[tokio::test]
    async fn a_taken_port_is_reported_and_tried_again() {
        use crate::network::phone_link::{Kind, health};
        use crate::platform::firewall::Firewall;
        let app = crate::network::phone_link::tests::app_on_network();
        let blocker = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let port = blocker.local_addr().unwrap().port();
        tokio::spawn(serve_trying(app.clone(), port, Duration::from_millis(200)));
        let began = std::time::Instant::now();
        let mut seen = String::new();
        while began.elapsed() < Duration::from_secs(20) {
            seen = health(&app, Kind::Camera, Firewall::Open).text(Kind::Camera, port);
            if seen.contains("in use") { break }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(seen, format!("The secure server isn't running: port {port} is in use by another program. Close that program, or restart this computer."));
        assert!(health(&app, Kind::Mic, Firewall::Open).problem());
        drop(blocker);
        while app.net.https.lock().unwrap().is_none() && began.elapsed() < Duration::from_secs(20) { tokio::time::sleep(Duration::from_millis(50)).await }
        assert_eq!(*app.net.https.lock().unwrap(), Some(port), "running once the port is free");
        assert!(!health(&app, Kind::Camera, Firewall::Open).problem());
    }

    #[tokio::test]
    async fn the_phone_pages_answer_over_https() {
        let app = crate::app::test_app();
        let k = make(&[IpAddr::from([127, 0, 0, 1])], chrono::Utc::now().timestamp()).unwrap();
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let listener = TlsListener::new(tcp, tls_config(&k).unwrap()).unwrap();
        let svc = crate::server::router(app).into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(listener.tap_io(|_| {}), svc).await });

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let client = rustls::ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().unwrap()
            .dangerous().with_custom_certificate_verifier(Arc::new(OnlyThis(k.cert_der().unwrap(), provider))).with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(client));
        let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut tls = connector.connect(rustls::pki_types::ServerName::try_from("localhost").unwrap(), tcp).await.expect("the handshake with the app's own certificate");
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tls.write_all(format!("GET /camera HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", addr.port()).as_bytes()).await.unwrap();
        let mut answer = String::new();
        let _ = tls.read_to_string(&mut answer).await;
        assert!(answer.starts_with("HTTP/1.1 200"), "{}", &answer[..answer.len().min(200)]);
        assert!(answer.contains("getUserMedia"), "the camera page");
    }
}
