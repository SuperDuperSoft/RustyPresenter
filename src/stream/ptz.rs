//! Moving a PTZ camera (pan, tilt, zoom) to one of its saved positions, with VISCA over IP: what most church PTZ
//! cameras understand (Sony, PTZOptics, Avonic, Lumens, and many more). Positions are saved with the camera's own
//! remote or web page; the app only calls them back. One UDP packet, sent and not waited for: a camera that is
//! off changes nothing in the stream.
//!
//! The address is the camera's, with its port: 52381 (the default) is Sony's VISCA over IP, with its 8-byte
//! header; any other port (PTZOptics: 1259) takes the bare VISCA command.
use std::net::UdpSocket;
use std::sync::atomic::{AtomicU32, Ordering};

/// Sony's port for VISCA over IP, and the default.
pub const SONY_PORT: u16 = 52381;
/// Positions offered (VISCA has up to 128; cameras' remotes save the first few).
pub const POSITIONS: u8 = 6;

/// Where a camera listens, and whether it wants Sony's header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub host: String,
    pub port: u16,
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
        Some(Control { host: host.to_string(), port })
    }

    /// Sony's header is wanted.
    pub fn sony(&self) -> bool { self.port == SONY_PORT }
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

/// The cameras that can be moved (1: the first camera, 2: the second, up to 4), each with its saved positions.
pub fn moves(prefs: &crate::record::RecordPrefs) -> Vec<(u8, u8)> {
    prefs.cameras().into_iter().enumerate().filter(|(_, (_, ctl))| Control::parse(ctl).is_some())
        .flat_map(|(i, _)| (1..=POSITIONS).map(move |p| (i as u8 + 1, p))).collect()
}

/// A camera's control (1 to 4) from Settings.
pub fn control_of(prefs: &crate::record::RecordPrefs, camera: u8) -> Option<Control> {
    Control::parse(prefs.camera_control_of(camera))
}

/// "Camera 2: position 3".
pub fn label(camera: u8, position: u8) -> String { crate::tr!("Camera {camera}: position {position}", camera = camera, position = position) }

static SEQ: AtomicU32 = AtomicU32::new(1);

/// Sends the camera at `control` to position `n` (from 1).
pub fn recall(control: &Control, n: u8) -> Result<(), String> {
    let packet = recall_packet(n, SEQ.fetch_add(1, Ordering::Relaxed), control.sony());
    let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| e.to_string())?;
    socket.send_to(&packet, (control.host.as_str(), control.port)).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_camera_control_address_is_read_with_its_port() {
        assert_eq!(Control::parse(" 192.168.1.60 "), Some(Control { host: "192.168.1.60".into(), port: SONY_PORT }));
        assert_eq!(Control::parse("192.168.1.60:1259"), Some(Control { host: "192.168.1.60".into(), port: 1259 }));
        assert_eq!(Control::parse("ptz-front.local:52381"), Some(Control { host: "ptz-front.local".into(), port: SONY_PORT }));
        assert!(Control::parse("192.168.1.60:1259").is_some_and(|c| !c.sony()));
        for bad in ["", "   ", "192.168.1.60:", "192.168.1.60:99999", "rtsp://192.168.1.60/stream", "two words"] {
            assert_eq!(Control::parse(bad), None, "{bad:?}");
        }
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
        recall(&Control { host: "127.0.0.1".into(), port }, 3).unwrap();
        let mut buf = [0u8; 64];
        let n = camera.recv(&mut buf).unwrap();
        assert_eq!(&buf[..n], [0x81, 0x01, 0x04, 0x3F, 0x02, 0x02, 0xFF]);
    }
}
