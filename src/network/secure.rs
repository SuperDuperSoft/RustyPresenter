//! The app's pages over `https://` too, on the port after the phone remote's. A phone lets a page use its
//! microphone only over a secure connection (the phone as a microphone, `network::phone_mic`). Nobody can vouch
//! for a computer on the church's own network, so the app makes its own certificate the first time (ECDSA P-256,
//! with `ring`, which rustls already uses) and keeps it in the data folder. Each phone then warns once that the page
//! is "not private", and the person goes on to it (Advanced → Proceed; on an iPhone, Show Details → visit).
//!
//! The certificate names this computer's addresses on the network; a new one is made when they change, and before
//! it is 700 days old (iPhones refuse certificates that last more than 825 days).
use crate::app::App;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};

/// How long a certificate is made to last, and when a new one is made.
const LASTS_DAYS: i64 = 800;
const RENEW_DAYS: i64 = 700;

static PORT: AtomicU16 = AtomicU16::new(0);

/// The port the secure pages are on, once they are.
pub fn port() -> Option<u16> { Some(PORT.load(Ordering::Relaxed)).filter(|p| *p != 0) }

// ---- A small DER writer: just what one certificate needs. ----

fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let n = content.len();
    if n < 0x80 { out.push(n as u8) } else if n < 0x100 { out.extend([0x81, n as u8]) } else { out.extend([0x82, (n >> 8) as u8, n as u8]) }
    out.extend_from_slice(content);
    out
}
fn seq(parts: &[Vec<u8>]) -> Vec<u8> { tlv(0x30, &parts.concat()) }
fn oid(bytes: &[u8]) -> Vec<u8> { tlv(0x06, bytes) }
fn uint(bytes: &[u8]) -> Vec<u8> {
    let mut b: Vec<u8> = bytes.iter().copied().skip_while(|x| *x == 0).collect();
    if b.first().is_none_or(|x| x & 0x80 != 0) { b.insert(0, 0) }
    tlv(0x02, &b)
}
fn bits(bytes: &[u8]) -> Vec<u8> { tlv(0x03, &[&[0u8][..], bytes].concat()) }
fn utc(t: chrono::DateTime<chrono::Utc>) -> Vec<u8> { tlv(0x17, t.format("%y%m%d%H%M%SZ").to_string().as_bytes()) }
fn name(cn: &str) -> Vec<u8> { seq(&[tlv(0x31, &seq(&[oid(&[0x55, 0x04, 0x03]), tlv(0x0C, cn.as_bytes())]))]) }

const ECDSA_SHA256: [u8; 8] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02];
const EC_PUBLIC_KEY: [u8; 7] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
const P256: [u8; 8] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];

/// A certificate for `ips` (and the name `localhost`), signed by its own key: `(certificate, key)`, both DER (the
/// key as PKCS#8).
pub fn make(ips: &[IpAddr], now: chrono::DateTime<chrono::Utc>) -> Result<(Vec<u8>, Vec<u8>), String> {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).map_err(|_| "no key could be made")?;
    let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng).map_err(|_| "the key could not be read")?;
    let mut serial = [0u8; 16];
    ring::rand::SecureRandom::fill(&rng, &mut serial).map_err(|_| "no serial number")?;
    serial[0] &= 0x7F;
    let algorithm = seq(&[oid(&ECDSA_SHA256)]);
    let who = name("RustyPresenter on this computer");
    let validity = seq(&[utc(now - chrono::Duration::days(1)), utc(now + chrono::Duration::days(LASTS_DAYS))]);
    let public = seq(&[seq(&[oid(&EC_PUBLIC_KEY), oid(&P256)]), bits(key.public_key().as_ref())]);
    // Where it may be used: these addresses, and as a web server.
    let mut alt: Vec<Vec<u8>> = ips.iter().map(|ip| match ip { IpAddr::V4(a) => tlv(0x87, &a.octets()), IpAddr::V6(a) => tlv(0x87, &a.octets()) }).collect();
    alt.push(tlv(0x82, b"localhost"));
    let san = seq(&[oid(&[0x55, 0x1D, 0x11]), tlv(0x04, &seq(&alt))]);
    let eku = seq(&[oid(&[0x55, 0x1D, 0x25]), tlv(0x04, &seq(&[oid(&[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01])]))]);
    let extensions = tlv(0xA3, &seq(&[san, eku]));
    let tbs = seq(&[tlv(0xA0, &uint(&[2])), uint(&serial), algorithm.clone(), who.clone(), validity, who, public, extensions]);
    let signature = key.sign(&rng, &tbs).map_err(|_| "the certificate could not be signed")?;
    Ok((seq(&[tbs, algorithm, bits(signature.as_ref())]), pkcs8.as_ref().to_vec()))
}

/// What the kept certificate was made for: its addresses and the day it was made (`2026-11-17 192.168.1.5 …`).
fn note(ips: &[IpAddr], today: chrono::NaiveDate) -> String {
    let mut s = today.format("%Y-%m-%d").to_string();
    for ip in ips { s.push(' '); s.push_str(&ip.to_string()) }
    s
}

/// Whether the kept certificate (`note` as written) still does for `ips` today.
pub fn still_good(kept: &str, ips: &[IpAddr], today: chrono::NaiveDate) -> bool {
    let mut words = kept.split_whitespace();
    let Some(made) = words.next().and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) else { return false };
    let had: Vec<&str> = words.collect();
    (today - made).num_days() < RENEW_DAYS && ips.iter().all(|ip| had.contains(&ip.to_string().as_str()))
}

/// The certificate and key in `dir` (`secure/` in the data folder), made again when they no longer do.
pub fn certificate(dir: &Path, ips: &[IpAddr]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let (cert, key, about) = (dir.join("certificate.der"), dir.join("key.der"), dir.join("certificate.txt"));
    let today = chrono::Local::now().date_naive();
    if let (Ok(c), Ok(k), Ok(n)) = (std::fs::read(&cert), std::fs::read(&key), std::fs::read_to_string(&about)) && still_good(&n, ips, today) {
        return Ok((c, k));
    }
    let (c, k) = make(ips, chrono::Utc::now())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(&cert, &c).map_err(|e| e.to_string())?;
    std::fs::write(&key, &k).map_err(|e| e.to_string())?;
    std::fs::write(&about, note(ips, today)).map_err(|e| e.to_string())?;
    Ok((c, k))
}

/// rustls' settings for serving with `cert` and `key`.
pub fn tls(cert: Vec<u8>, key: Vec<u8>) -> Result<Arc<rustls::ServerConfig>, String> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider).with_safe_default_protocol_versions().map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(cert)], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key))).map_err(|e| e.to_string())?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// Serves `router` over TLS on `listener` until the program ends: each connection on a task of its own.
pub async fn serve_on(listener: tokio::net::TcpListener, config: Arc<rustls::ServerConfig>, router: axum::Router) {
    let acceptor = tokio_rustls::TlsAcceptor::from(config);
    loop {
        let Ok((tcp, addr)) = listener.accept().await else { continue };
        let (acceptor, router) = (acceptor.clone(), router.clone());
        tokio::spawn(async move {
            // A phone that does not trust the certificate yet hangs up here: that is the warning it shows.
            let Ok(tls) = tokio::time::timeout(std::time::Duration::from_secs(10), acceptor.accept(tcp)).await else { return };
            let Ok(tls) = tls else { return };
            // The pages read who is asking (`ConnectInfo`), as on the plain port.
            let svc = hyper_util::service::TowerToHyperService::new(router.layer(axum::Extension(axum::extract::ConnectInfo(addr))));
            let _ = hyper::server::conn::http1::Builder::new().serve_connection(hyper_util::rt::TokioIo::new(tls), svc).with_upgrades().await;
        });
    }
}

/// This computer's address on the network, as the phone remote's address has it.
fn lan_ips(app: &App) -> Vec<IpAddr> {
    let host = app.lan_url.as_deref().and_then(|u| u.strip_prefix("http://")).and_then(|u| u.split('/').next()).unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(h, _)| h).trim_start_matches('[').trim_end_matches(']');
    let mut ips: Vec<IpAddr> = host.parse().ok().into_iter().collect();
    ips.push(IpAddr::from([127, 0, 0, 1]));
    ips
}

/// The secure pages, on the port after the app's own; nothing (and the phone page says so) when that port is taken.
pub async fn serve(app: Arc<App>) {
    let Some(port) = app.port.checked_add(1) else { return };
    let dir = app.store.root.join("secure");
    let ips = lan_ips(&app);
    let made = tokio::task::spawn_blocking(move || certificate(&dir, &ips).and_then(|(c, k)| tls(c, k))).await.map_err(|e| e.to_string()).and_then(|r| r);
    let config = match made {
        Ok(c) => c,
        Err(e) => { eprintln!("secure pages: {e}"); return }
    };
    let listener = match tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await {
        Ok(l) => l,
        Err(e) => { eprintln!("secure pages: port {port}: {e}"); return }
    };
    PORT.store(port, Ordering::Relaxed);
    serve_on(listener, config, crate::server::router(app)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_certificate_is_made_again_when_the_address_changes_or_it_gets_old() {
        let ip: IpAddr = "192.168.1.5".parse().unwrap();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 17).unwrap();
        let kept = note(&[ip, IpAddr::from([127, 0, 0, 1])], day);
        assert_eq!(kept, "2026-11-17 192.168.1.5 127.0.0.1");
        assert!(still_good(&kept, &[ip], day + chrono::Duration::days(RENEW_DAYS - 1)));
        assert!(!still_good(&kept, &[ip], day + chrono::Duration::days(RENEW_DAYS)), "old");
        assert!(!still_good(&kept, &["192.168.1.9".parse().unwrap()], day), "moved");
        assert!(!still_good("", &[ip], day));
    }

    #[test]
    fn the_certificate_is_kept_and_used_again() {
        let dir = std::env::temp_dir().join(format!("rp-secure-{}", uuid::Uuid::new_v4()));
        let ips = [IpAddr::from([127, 0, 0, 1])];
        let first = certificate(&dir, &ips).unwrap();
        assert_eq!(certificate(&dir, &ips).unwrap(), first, "the same one again");
        assert_ne!(certificate(&dir, &[IpAddr::from([10, 0, 0, 2])]).unwrap(), first, "a new one for a new address");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A real TLS client that trusts only this certificate, as a phone does once it has gone on past the warning:
    /// the certificate is read and checked whole (signature, address, dates), and the page comes over it.
    #[tokio::test]
    async fn a_page_comes_over_the_secure_connection() {
        let ip = IpAddr::from([127, 0, 0, 1]);
        let (cert, key) = make(&[ip], chrono::Utc::now()).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = axum::Router::new().route("/stage-view/mic", axum::routing::get(|| async { "ok" }));
        tokio::spawn(serve_on(listener, tls(cert.clone(), key).unwrap(), router));
        let mut roots = rustls::RootCertStore::empty();
        roots.add(rustls::pki_types::CertificateDer::from(cert)).unwrap();
        let client = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions().unwrap().with_root_certificates(roots).with_no_client_auth();
        let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut tls = tokio_rustls::TlsConnector::from(Arc::new(client)).connect(rustls::pki_types::ServerName::IpAddress(ip.into()), tcp).await.expect("the certificate is good for 127.0.0.1");
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tls.write_all(b"GET /stage-view/mic HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await.unwrap();
        let mut got = vec![];
        let _ = tls.read_to_end(&mut got).await;
        let got = String::from_utf8_lossy(&got);
        assert!(got.starts_with("HTTP/1.1 200") && got.ends_with("ok"), "{got}");
    }
}
