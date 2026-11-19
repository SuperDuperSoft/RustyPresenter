//! Finding RustyPresenter on the Wi-Fi without typing: every address of the phone's own network (the last
//! number 1–254) is tried on the phone server's port, many at once, and each that answers `/remote/status` like
//! RustyPresenter is listed. Nothing needs turning on at the computer, and older versions are found too (newer ones
//! also say the computer's name). Home and church networks are almost always this size; on a bigger one the
//! address is typed or the QR code scanned.
use crate::{http::Client, protocol::RemoteStatus};
use std::{
    net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket},
    sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}},
    time::Duration,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub host: String,
    /// The computer's name, when it says it.
    pub name: String,
}

/// This phone's address on the Wi-Fi: the one it would use to reach the internet (nothing is sent).
pub fn own_ipv4() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:80").ok()?;
    match s.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// This phone's addresses on its networks (Wi-Fi first, then other private ones), so the computer is found on
/// the Wi-Fi even when the phone sends its internet over mobile data (a church Wi-Fi without internet).
pub fn own_ipv4s() -> Vec<Ipv4Addr> {
    let mut found: Vec<(bool, Ipv4Addr)> = interfaces().into_iter().filter(|(_, ip)| ip.is_private()).map(|(name, ip)| (name.starts_with("wlan"), ip)).collect();
    found.sort_by_key(|(wifi, _)| !*wifi);
    let mut out: Vec<Ipv4Addr> = found.into_iter().map(|(_, ip)| ip).collect();
    if let Some(ip) = own_ipv4().filter(|ip| ip.is_private() && !out.contains(ip)) { out.push(ip) }
    out
}

/// Each network interface's name and IPv4 address.
#[cfg(unix)]
fn interfaces() -> Vec<(String, Ipv4Addr)> {
    let mut out = vec![];
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) != 0 { return out }
        let mut at = list;
        while !at.is_null() {
            let a = &*at;
            if !a.ifa_addr.is_null() && i32::from((*a.ifa_addr).sa_family) == libc::AF_INET {
                let sin = &*(a.ifa_addr as *const libc::sockaddr_in);
                let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                let name = std::ffi::CStr::from_ptr(a.ifa_name).to_string_lossy().into_owned();
                if !ip.is_loopback() { out.push((name, ip)) }
            }
            at = a.ifa_next;
        }
        libc::freeifaddrs(list);
    }
    out
}

#[cfg(not(unix))]
fn interfaces() -> Vec<(String, Ipv4Addr)> { vec![] }

/// The other addresses of `own`'s network (the last number 1–254), nearest first.
pub fn neighbours(own: Ipv4Addr) -> Vec<Ipv4Addr> {
    let [a, b, c, me] = own.octets();
    let mut v: Vec<u8> = (1..=254).filter(|n| *n != me).collect();
    v.sort_by_key(|n| (i16::from(*n) - i16::from(me)).abs());
    v.into_iter().map(|d| Ipv4Addr::new(a, b, c, d)).collect()
}

/// Whether an answer to `/remote/status` is RustyPresenter's, and what it says about itself.
pub fn recognise(host: &str, status: u16, body: &[u8]) -> Option<Found> {
    if status != 200 { return None }
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    // Every version answers `paired` and `password`; newer ones also say their name.
    if !v["paired"].is_boolean() { return None }
    let s: RemoteStatus = serde_json::from_value(v).ok()?;
    if s.app.as_deref().is_some_and(|a| a != "RustyPresenter") { return None }
    Some(Found { host: host.to_string(), name: s.computer.unwrap_or_default() })
}

/// Tries every neighbour of each of `own` on `port`: those whose port is open are asked `/remote/status`. `found` is called
/// as each one is found; the whole list comes back at the end (in a few seconds).
pub fn scan(own: &[Ipv4Addr], port: u16, found: impl Fn(&Found) + Send + Sync + 'static) -> Vec<Found> {
    let mut list = vec![];
    for ip in own {
        for n in neighbours(*ip) { if !list.contains(&n) && !own.contains(&n) { list.push(n) } }
    }
    scan_list(list, port, Duration::from_millis(400), found)
}

pub fn scan_list(list: Vec<Ipv4Addr>, port: u16, wait: Duration, found: impl Fn(&Found) + Send + Sync + 'static) -> Vec<Found> {
    let list = Arc::new(list);
    let at = Arc::new(AtomicUsize::new(0));
    let out = Arc::new(Mutex::new(Vec::new()));
    let found = Arc::new(found);
    let workers: Vec<_> = (0..48).map(|_| {
        let (list, at, out, found) = (list.clone(), at.clone(), out.clone(), found.clone());
        std::thread::spawn(move || loop {
            let i = at.fetch_add(1, Ordering::Relaxed);
            let Some(ip) = list.get(i) else { break };
            if let Some(f) = ask(SocketAddr::from((*ip, port)), wait) {
                found(&f);
                out.lock().unwrap().push(f);
            }
        })
    }).collect();
    for w in workers { let _ = w.join(); }
    let mut v = out.lock().unwrap().clone();
    v.sort_by(|a, b| a.host.cmp(&b.host));
    v
}

/// One address: is RustyPresenter there?
pub fn ask(addr: SocketAddr, wait: Duration) -> Option<Found> {
    TcpStream::connect_timeout(&addr, wait).ok()?;
    let host = addr.to_string();
    let mut c = Client::new(&host);
    c.timeout = Duration::from_secs(2);
    let r = c.get("/remote/status").ok()?;
    recognise(&host, r.status, &r.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    #[test]
    fn the_neighbours_are_the_rest_of_the_network_nearest_first() {
        let n = neighbours(Ipv4Addr::new(192, 168, 1, 20));
        assert_eq!(n.len(), 253);
        assert!(!n.contains(&Ipv4Addr::new(192, 168, 1, 20)));
        assert_eq!(&n[..2], &[Ipv4Addr::new(192, 168, 1, 19), Ipv4Addr::new(192, 168, 1, 21)]);
        assert!(!n.contains(&Ipv4Addr::new(192, 168, 1, 0)) && !n.contains(&Ipv4Addr::new(192, 168, 1, 255)));
    }

    #[test]
    fn only_rustypresenter_is_recognised() {
        assert_eq!(recognise("h:4318", 200, br#"{"paired":false,"password":true}"#), Some(Found { host: "h:4318".into(), name: String::new() }), "an older version");
        assert_eq!(recognise("h:4318", 200, br#"{"paired":true,"password":false,"app":"RustyPresenter","computer":"CHURCH-PC","version":"1.4.0"}"#).unwrap().name, "CHURCH-PC");
        assert_eq!(recognise("h:4318", 200, br#"{"paired":true,"app":"Other"}"#), None);
        assert_eq!(recognise("h:4318", 404, b""), None);
        assert_eq!(recognise("h:4318", 200, b"<html>"), None);
        assert_eq!(recognise("h:4318", 200, br#"{"ok":true}"#), None);
    }

    /// A RustyPresenter on this computer is found among addresses where nothing listens.
    #[test]
    fn a_scan_finds_the_computer() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { break };
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                while r.read_line(&mut line).is_ok_and(|n| n > 2) { line.clear() }
                let body = br#"{"paired":false,"password":false,"app":"RustyPresenter","computer":"Sanctuary"}"#;
                let _ = s.write_all(format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n", body.len()).as_bytes());
                let _ = s.write_all(body);
            }
        });
        let seen = Arc::new(Mutex::new(0));
        let s2 = seen.clone();
        // 127.0.0.2–4 have nothing on that port.
        let list = vec![Ipv4Addr::new(127, 0, 0, 2), Ipv4Addr::new(127, 0, 0, 1), Ipv4Addr::new(127, 0, 0, 3)];
        let got = scan_list(list, port, Duration::from_millis(300), move |_| *s2.lock().unwrap() += 1);
        assert_eq!(got, vec![Found { host: format!("127.0.0.1:{port}"), name: "Sanctuary".into() }]);
        assert_eq!(*seen.lock().unwrap(), 1);
    }
}
