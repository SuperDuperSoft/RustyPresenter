//! Moving a PTZ camera (pan, tilt, zoom) to one of its saved positions. Positions are saved with the camera's own
//! remote or web page; the app only calls them back.
//!
//! Two ways (`Control`):
//! - VISCA over IP, at the camera's control address in Settings: what most church PTZ cameras understand (Sony,
//!   PTZOptics, Avonic, Lumens, and many more). One UDP packet, sent and not waited for: a camera that is off
//!   changes nothing in the stream. The address has its port: 52381 (the default) is Sony's VISCA over IP, with
//!   its 8-byte header; any other port (PTZOptics: 1259) takes the bare VISCA command.
//! - NDI: a camera that is an NDI source and has no control address is moved through its NDI connection
//!   (`super::ndi::recall_position`), when it says it can be.
use std::net::UdpSocket;
use std::sync::atomic::{AtomicU32, Ordering};

/// Sony's port for VISCA over IP, and the default.
pub const SONY_PORT: u16 = 52381;
/// Positions offered (VISCA has up to 128; cameras' remotes save the first few).
pub const POSITIONS: u8 = 6;

/// How a camera is moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    /// VISCA over IP: where the camera listens, and whether it wants Sony's header.
    Visca { host: String, port: u16 },
    /// Through the NDI source of this name.
    Ndi(String),
}

impl Control {
    /// From what Settings keep: "192.168.1.60" or "192.168.1.60:1259". `None` when empty or not an address.
    pub fn parse(address: &str) -> Option<Control> {
        let a = address.trim();
        if a.is_empty() || a.contains("://") || a.contains(char::is_whitespace) { return None }
        let (host, port) = match a.rsplit_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().ok().filter(|p| *p > 0)?),
            None => (a, SONY_PORT),
        };
        if host.is_empty() { return None }
        Some(Control::Visca { host: host.to_string(), port })
    }

    /// Saved camera `camera` with its control address `address`: the address when there is one, else NDI when the
    /// camera is an NDI source.
    pub fn of(camera: &str, address: &str) -> Option<Control> {
        Control::parse(address).or_else(|| super::ndi::source_name(camera).map(|s| Control::Ndi(s.to_string())))
    }

    /// Sony's header is wanted.
    pub fn sony(&self) -> bool { matches!(self, Control::Visca { port: SONY_PORT, .. }) }

    /// Whether the camera can be moved now: VISCA always (it is not asked); an NDI camera while it is received and
    /// says it can.
    pub fn ready(&self) -> bool {
        match self {
            Control::Visca { .. } => true,
            Control::Ndi(source) => super::ndi::remote(source).is_some_and(|r| r.can_move()),
        }
    }
}

/// The packet that calls back position `n` (from 1), the `seq`th sent.
pub fn recall_packet(n: u8, seq: u32, sony: bool) -> Vec<u8> {
    let visca = [0x81, 0x01, 0x04, 0x3F, 0x02, n.saturating_sub(1).min(0x7F), 0xFF];
    if !sony { return visca.to_vec() }
    let mut p = vec![0x01, 0x00];
    p.extend((visca.len() as u16).to_be_bytes());
    p.extend(seq.to_be_bytes());
    p.extend(visca);
    p
}

/// The cameras that can be moved (1: the first camera, up to 4), each with its saved positions. An NDI camera is
/// listed: whether it can move is only known once it is connected.
pub fn moves(prefs: &crate::record::RecordPrefs) -> Vec<(u8, u8)> {
    prefs.cameras().into_iter().enumerate().filter(|(_, (cam, ctl))| Control::of(cam, ctl).is_some())
        .flat_map(|(i, _)| (1..=POSITIONS).map(move |p| (i as u8 + 1, p))).collect()
}

/// A camera's control (1 to 4) from Settings.
pub fn control_of(prefs: &crate::record::RecordPrefs, camera: u8) -> Option<Control> {
    let cameras = prefs.cameras();
    let (cam, _) = cameras.get(usize::from(camera).checked_sub(1)?)?;
    Control::of(cam, prefs.camera_control_of(camera))
}

/// "Camera 2: position 3".
pub fn label(camera: u8, position: u8) -> String { crate::tr!("Camera {camera}: position {position}", camera = camera, position = position) }

static SEQ: AtomicU32 = AtomicU32::new(1);

/// Sends the camera at `control` to position `n` (from 1). An NDI camera not being received may be waited for a
/// few seconds: not on the UI thread.
pub fn recall(control: &Control, n: u8) -> Result<(), String> {
    match control {
        Control::Visca { host, port } => {
            let packet = recall_packet(n, SEQ.fetch_add(1, Ordering::Relaxed), control.sony());
            let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| e.to_string())?;
            socket.send_to(&packet, (host.as_str(), *port)).map(|_| ()).map_err(|e| e.to_string())
        }
        Control::Ndi(source) => super::ndi::recall_position(source, n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visca(host: &str, port: u16) -> Control { Control::Visca { host: host.into(), port } }

    #[test]
    fn a_camera_control_address_is_read_with_its_port() {
        assert_eq!(Control::parse(" 192.168.1.60 "), Some(visca("192.168.1.60", SONY_PORT)));
        assert_eq!(Control::parse("192.168.1.60:1259"), Some(visca("192.168.1.60", 1259)));
        assert_eq!(Control::parse("ptz-front.local:52381"), Some(visca("ptz-front.local", SONY_PORT)));
        assert!(Control::parse("192.168.1.60:1259").is_some_and(|c| !c.sony()));
        for bad in ["", "   ", "192.168.1.60:", "192.168.1.60:99999", "rtsp://192.168.1.60/stream", "two words"] {
            assert_eq!(Control::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn an_ndi_camera_is_moved_over_ndi_unless_it_has_a_control_address() {
        assert_eq!(Control::of("ndi://STUDIO (PTZ 1)", ""), Some(Control::Ndi("STUDIO (PTZ 1)".into())));
        assert_eq!(Control::of("ndi://STUDIO (PTZ 1)", "192.168.1.60"), Some(visca("192.168.1.60", SONY_PORT)), "the address set by hand wins");
        assert_eq!(Control::of("Integrated Camera", ""), None);
        assert_eq!(Control::of("Integrated Camera", "192.168.1.60:1259"), Some(visca("192.168.1.60", 1259)));
        assert!(!Control::Ndi("STUDIO (PTZ 1)".into()).sony());
        // Not being received: it cannot be moved from the operator's list now.
        assert!(!Control::Ndi("Nobody here (PTZ)".into()).ready());
        assert!(visca("192.168.1.60", SONY_PORT).ready());
        let prefs = crate::record::RecordPrefs { camera: "ndi://STUDIO (PTZ 1)".into(), camera2: "USB Cam".into(), ..Default::default() };
        assert_eq!(moves(&prefs), (1..=POSITIONS).map(|p| (1, p)).collect::<Vec<_>>(), "the NDI camera's positions only");
        assert_eq!(control_of(&prefs, 1), Some(Control::Ndi("STUDIO (PTZ 1)".into())));
        assert_eq!(control_of(&prefs, 2), None);
    }

    #[test]
    fn a_position_is_called_back_with_visca() {
        // Bare: 8x 01 04 3F 02 pp FF, pp from 0.
        assert_eq!(recall_packet(1, 7, false), [0x81, 0x01, 0x04, 0x3F, 0x02, 0x00, 0xFF]);
        assert_eq!(recall_packet(6, 7, false), [0x81, 0x01, 0x04, 0x3F, 0x02, 0x05, 0xFF]);
        // Sony: 01 00 (a command), the length (7), the sequence number, then the same.
        assert_eq!(recall_packet(2, 0x0102_0304, true), [0x01, 0x00, 0x00, 0x07, 0x01, 0x02, 0x03, 0x04, 0x81, 0x01, 0x04, 0x3F, 0x02, 0x01, 0xFF]);
    }

    #[test]
    fn the_camera_gets_the_packet() {
        let camera = UdpSocket::bind("127.0.0.1:0").unwrap();
        camera.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let port = camera.local_addr().unwrap().port();
        recall(&visca("127.0.0.1", port), 3).unwrap();
        let mut buf = [0u8; 64];
        let n = camera.recv(&mut buf).unwrap();
        assert_eq!(&buf[..n], [0x81, 0x01, 0x04, 0x3F, 0x02, 0x02, 0xFF]);
    }
}
