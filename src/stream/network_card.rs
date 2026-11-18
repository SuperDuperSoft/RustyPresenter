//! The computer's network cards, for Livestream → More settings → "Send the stream from": a church with a wired
//! line for the stream and Wi-Fi for everything else can send the stream down the line. Each card that is up, with
//! its IPv4 address (what FFmpeg's RTMP sender binds to: `record::advanced::bind_args`).
use std::net::Ipv4Addr;

/// A network card: its name as the computer shows it ("Ethernet", "Wi-Fi") and its address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub name: String,
    pub address: Ipv4Addr,
}

impl Card {
    /// "Ethernet (192.168.1.20)".
    pub fn label(&self) -> String { format!("{} ({})", self.name, self.address) }
}

/// The cards that are up and have an address other than this computer's own (127.0.0.1) or a made-up one
/// (169.254.x.x, given when no network answered), each address once.
pub fn list() -> Vec<Card> {
    let mut out: Vec<Card> = vec![];
    for c in raw() {
        if c.address.is_loopback() || c.address.is_link_local() || c.address.is_unspecified() || out.iter().any(|o| o.address == c.address) { continue }
        out.push(c);
    }
    out
}

/// The choices for Settings: the computer chooses (empty), then each card; a saved address that is not on this
/// computer now stays, marked so.
pub fn choices(cards: &[Card], saved: &str) -> (Vec<String>, Vec<String>, usize) {
    let mut labels = vec![crate::tr!("Let the computer choose")];
    let mut keys = vec![String::new()];
    for c in cards { labels.push(c.label()); keys.push(c.address.to_string()) }
    let saved = saved.trim();
    if !saved.is_empty() && !keys.iter().any(|k| k == saved) {
        labels.push(crate::tr!("{address} (not on this computer now)", address = saved));
        keys.push(saved.to_string());
    }
    let index = keys.iter().position(|k| k == saved).unwrap_or(0);
    (labels, keys, index)
}

#[cfg(windows)]
fn raw() -> Vec<Card> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH};
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};
    let mut size: u32 = 16 * 1024;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    for _ in 0..3 {
        // u64s, so the list is aligned as Windows writes it.
        let mut buf: Vec<u64> = vec![0; (size as usize).div_ceil(8)];
        let first = buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: `buf` holds `size` bytes, as GetAdaptersAddresses is told.
        let r = unsafe { GetAdaptersAddresses(u32::from(AF_INET), flags, std::ptr::null(), first, &mut size) };
        if r == 111 { continue } // ERROR_BUFFER_OVERFLOW: `size` is now what it needs.
        if r != 0 { return vec![] }
        let mut out = vec![];
        let mut a = first;
        // SAFETY: Windows wrote a list of adapters into `buf`, each `Next` pointing within it or null; the
        // addresses are read only while `buf` lives.
        unsafe {
            while !a.is_null() {
                let ad = &*a;
                if ad.OperStatus == IfOperStatusUp {
                    let name = wide(ad.FriendlyName);
                    let mut u = ad.FirstUnicastAddress;
                    while !u.is_null() {
                        let sa = (*u).Address.lpSockaddr;
                        if !sa.is_null() && (*sa).sa_family == AF_INET {
                            let sin = &*sa.cast::<SOCKADDR_IN>();
                            let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.S_un.S_addr));
                            out.push(Card { name: name.clone(), address: ip });
                        }
                        u = (*u).Next;
                    }
                }
                a = ad.Next;
            }
        }
        return out;
    }
    vec![]
}

#[cfg(windows)]
/// SAFETY: `p` is null or a string of UTF-16 ending with 0.
unsafe fn wide(p: *const u16) -> String {
    if p.is_null() { return String::new() }
    let mut n = 0;
    // SAFETY: as the caller promises, it ends with 0.
    while unsafe { *p.add(n) } != 0 { n += 1 }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, n) })
}

#[cfg(unix)]
fn raw() -> Vec<Card> {
    let mut out = vec![];
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills `list`, freed below with freeifaddrs; each entry is read while it lives.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 { return out }
        let mut i = list;
        while !i.is_null() {
            let e = &*i;
            let up = e.ifa_flags & (libc::IFF_UP as libc::c_uint) != 0;
            if up && !e.ifa_addr.is_null() && i32::from((*e.ifa_addr).sa_family) == libc::AF_INET {
                let sin = &*e.ifa_addr.cast::<libc::sockaddr_in>();
                let name = std::ffi::CStr::from_ptr(e.ifa_name).to_string_lossy().into_owned();
                out.push(Card { name, address: Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)) });
            }
            i = e.ifa_next;
        }
        libc::freeifaddrs(list);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_computers_cards_are_listed_without_its_own_address() {
        let cards = list();
        // A computer running the tests is on some network, or none; never its own address alone.
        assert!(cards.iter().all(|c| !c.address.is_loopback() && !c.name.is_empty()), "{cards:?}");
        eprintln!("network cards: {:?}", cards.iter().map(Card::label).collect::<Vec<_>>());
    }

    #[test]
    fn the_choices_keep_a_saved_card_that_is_gone() {
        let cards = [Card { name: "Ethernet".into(), address: Ipv4Addr::new(192, 168, 1, 20) }];
        let (labels, keys, i) = choices(&cards, "");
        assert_eq!((labels.len(), keys, i), (2, vec![String::new(), "192.168.1.20".into()], 0));
        assert_eq!(labels[1], "Ethernet (192.168.1.20)");
        assert_eq!(choices(&cards, "192.168.1.20").2, 1);
        let (labels, keys, i) = choices(&cards, "10.0.0.9");
        assert_eq!((keys.len(), i), (3, 2));
        assert!(labels[2].starts_with("10.0.0.9"), "{labels:?}");
    }
}
