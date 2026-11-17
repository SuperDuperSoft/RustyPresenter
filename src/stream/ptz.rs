//! Moving a PTZ camera (pan, tilt, zoom): to one of its saved positions, or by hand from the Cameras box (the
//! arrows and zoom buttons move it while held), and saving the shot it has now as one of its positions.
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
pub fn recall_packet(n: u8, seq: u32, sony: bool) -> Vec<u8> { packet(&[0x81, 0x01, 0x04, 0x3F, 0x02, n.saturating_sub(1).min(0x7F), 0xFF], seq, sony) }

/// VISCA command `visca` as sent: bare, or with Sony's header (01 00: a command; its length; the sequence number).
fn packet(visca: &[u8], seq: u32, sony: bool) -> Vec<u8> {
    if !sony { return visca.to_vec() }
    let mut p = vec![0x01, 0x00];
    p.extend((visca.len() as u16).to_be_bytes());
    p.extend(seq.to_be_bytes());
    p.extend(visca);
    p
}

/// A move by hand: the camera keeps turning or zooming until it is told to stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move { Up, Down, Left, Right, Stop, ZoomIn, ZoomOut, ZoomStop }

impl Move {
    /// The Cameras box's buttons: 1 up, 2 down, 3 left, 4 right (0 stop); zoom 1 in, -1 out (0 stop).
    pub fn of_arrow(n: i32) -> Move { match n { 1 => Move::Up, 2 => Move::Down, 3 => Move::Left, 4 => Move::Right, _ => Move::Stop } }
    pub fn of_zoom(n: i32) -> Move { match n.signum() { 1 => Move::ZoomIn, -1 => Move::ZoomOut, _ => Move::ZoomStop } }
}

/// How fast a camera turns and zooms by hand: slow enough to follow on the stream (VISCA: pan 1-24, tilt 1-20,
/// zoom 0-7; NDI: up to 1).
pub const PAN_SPEED: u8 = 6;
pub const TILT_SPEED: u8 = 5;
pub const ZOOM_SPEED: u8 = 3;
const NDI_TURN: f32 = 0.25;
const NDI_ZOOM: f32 = 0.4;

/// The VISCA command for a move by hand: Pan-tiltDrive `8x 01 06 01 VV WW XX YY FF` (XX 01 left, 02 right, 03 still;
/// YY 01 up, 02 down, 03 still) or Zoom `8x 01 04 07 pp FF` (2p in, 3p out, 00 stop).
pub fn move_command(m: Move) -> Vec<u8> {
    let turn = |x: u8, y: u8| vec![0x81, 0x01, 0x06, 0x01, PAN_SPEED, TILT_SPEED, x, y, 0xFF];
    match m {
        Move::Up => turn(0x03, 0x01),
        Move::Down => turn(0x03, 0x02),
        Move::Left => turn(0x01, 0x03),
        Move::Right => turn(0x02, 0x03),
        Move::Stop => turn(0x03, 0x03),
        Move::ZoomIn => vec![0x81, 0x01, 0x04, 0x07, 0x20 | ZOOM_SPEED, 0xFF],
        Move::ZoomOut => vec![0x81, 0x01, 0x04, 0x07, 0x30 | ZOOM_SPEED, 0xFF],
        Move::ZoomStop => vec![0x81, 0x01, 0x04, 0x07, 0x00, 0xFF],
    }
}

/// The VISCA command that saves the camera's shot now as position `n` (from 1): Memory Set `8x 01 04 3F 01 pp FF`.
pub fn save_command(n: u8) -> Vec<u8> { vec![0x81, 0x01, 0x04, 0x3F, 0x01, n.saturating_sub(1).min(0x7F), 0xFF] }

/// NDI's speeds for a move by hand: `(pan, tilt)` (pan: + left, - right; tilt: + up, - down) or zoom (+ in, - out).
pub fn ndi_speeds(m: Move) -> (Option<(f32, f32)>, Option<f32>) {
    match m {
        Move::Up => (Some((0.0, NDI_TURN)), None),
        Move::Down => (Some((0.0, -NDI_TURN)), None),
        Move::Left => (Some((NDI_TURN, 0.0)), None),
        Move::Right => (Some((-NDI_TURN, 0.0)), None),
        Move::Stop => (Some((0.0, 0.0)), None),
        Move::ZoomIn => (None, Some(NDI_ZOOM)),
        Move::ZoomOut => (None, Some(-NDI_ZOOM)),
        Move::ZoomStop => (None, Some(0.0)),
    }
}

/// The cameras that can be moved (1: the first camera, up to 4), each with its saved positions. An NDI camera is
/// listed: whether it can move is only known once it is connected.
pub fn moves(prefs: &crate::record::RecordPrefs) -> Vec<(u8, u8)> {
    prefs.cameras().into_iter().enumerate().filter(|(_, (cam, ctl))| Control::of(cam, ctl).is_some())
        .flat_map(|(i, _)| (1..=POSITIONS).map(move |p| (i as u8 + 1, p))).collect()
}

/// A camera's control (1 to 4) from Settings.
pub fn control_of(prefs: &crate::record::RecordPrefs, camera: u8) -> Option<Control> {
    // A control address alone is enough (a camera moved by VISCA whose picture comes some other way).
    let name = match camera { 1 => &prefs.camera, 2 => &prefs.camera2, 3 => &prefs.camera3, 4 => &prefs.camera4, _ => return None };
    Control::of(name, prefs.camera_control_of(camera))
}

/// "Camera 2: position 3".
pub fn label(camera: u8, position: u8) -> String { crate::tr!("Camera {camera}: position {position}", camera = camera, position = position) }

static SEQ: AtomicU32 = AtomicU32::new(1);

fn send_visca(host: &str, port: u16, visca: &[u8]) -> Result<(), String> {
    let p = packet(visca, SEQ.fetch_add(1, Ordering::Relaxed), port == SONY_PORT);
    let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| e.to_string())?;
    socket.send_to(&p, (host, port)).map(|_| ()).map_err(|e| e.to_string())
}

/// Moves the camera at `control` by hand (`Move::Stop` / `ZoomStop` end the move).
pub fn drive(control: &Control, m: Move) -> Result<(), String> {
    match control {
        Control::Visca { host, port } => send_visca(host, *port, &move_command(m)),
        Control::Ndi(source) => super::ndi::drive(source, ndi_speeds(m)),
    }
}

/// Saves the shot the camera at `control` has now as its position `n` (from 1).
pub fn save(control: &Control, n: u8) -> Result<(), String> {
    match control {
        Control::Visca { host, port } => send_visca(host, *port, &save_command(n)),
        Control::Ndi(source) => super::ndi::store_position(source, n),
    }
}

/// What the camera is asked to do, for `send`.
#[derive(Clone, Debug, PartialEq)]
pub enum Order { Recall(u8), Move(Move), Save(u8) }

/// Carries out `order` now (waits: not on the UI thread).
pub fn carry_out(control: &Control, order: &Order) -> Result<(), String> {
    match order { Order::Recall(n) => recall(control, *n), Order::Move(m) => drive(control, *m), Order::Save(n) => save(control, *n) }
}

/// Carries out `order` on a thread of its own that takes the orders one after the other, so a quick press and
/// release reach the camera in that order. Problems are written to the log only: a camera that does not answer
/// changes nothing in the stream.
pub fn send(control: Control, order: Order) {
    use std::sync::{Mutex, OnceLock, mpsc};
    static WORKER: OnceLock<Mutex<mpsc::Sender<(Control, Order)>>> = OnceLock::new();
    let tx = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<(Control, Order)>();
        let _ = std::thread::Builder::new().name("camera-control".into()).spawn(move || {
            for (c, o) in rx { if let Err(e) = carry_out(&c, &o) { eprintln!("camera control: {e}") } }
        });
        Mutex::new(tx)
    });
    let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send((control, order));
}

/// Sends the camera at `control` to position `n` (from 1). An NDI camera not being received may be waited for a
/// few seconds: not on the UI thread.
pub fn recall(control: &Control, n: u8) -> Result<(), String> {
    match control {
        Control::Visca { host, port } => send_visca(host, *port, &[0x81, 0x01, 0x04, 0x3F, 0x02, n.saturating_sub(1).min(0x7F), 0xFF]),
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
    fn a_camera_is_moved_by_hand_and_its_shot_saved_with_visca() {
        assert_eq!(move_command(Move::Up), [0x81, 0x01, 0x06, 0x01, PAN_SPEED, TILT_SPEED, 0x03, 0x01, 0xFF]);
        assert_eq!(move_command(Move::Down)[6..8], [0x03, 0x02]);
        assert_eq!(move_command(Move::Left)[6..8], [0x01, 0x03]);
        assert_eq!(move_command(Move::Right)[6..8], [0x02, 0x03]);
        assert_eq!(move_command(Move::Stop)[6..8], [0x03, 0x03]);
        assert_eq!(move_command(Move::ZoomIn), [0x81, 0x01, 0x04, 0x07, 0x23, 0xFF]);
        assert_eq!(move_command(Move::ZoomOut), [0x81, 0x01, 0x04, 0x07, 0x33, 0xFF]);
        assert_eq!(move_command(Move::ZoomStop), [0x81, 0x01, 0x04, 0x07, 0x00, 0xFF]);
        assert_eq!(save_command(1), [0x81, 0x01, 0x04, 0x3F, 0x01, 0x00, 0xFF]);
        assert_eq!(save_command(6), [0x81, 0x01, 0x04, 0x3F, 0x01, 0x05, 0xFF]);
        assert!((1..=24).contains(&PAN_SPEED) && (1..=20).contains(&TILT_SPEED) && ZOOM_SPEED <= 7);
        assert_eq!((Move::of_arrow(1), Move::of_arrow(4), Move::of_arrow(0)), (Move::Up, Move::Right, Move::Stop));
        assert_eq!((Move::of_zoom(1), Move::of_zoom(-1), Move::of_zoom(0)), (Move::ZoomIn, Move::ZoomOut, Move::ZoomStop));
        // NDI: left is a positive pan, up a positive tilt, in a positive zoom.
        assert_eq!(ndi_speeds(Move::Left), (Some((NDI_TURN, 0.0)), None));
        assert_eq!(ndi_speeds(Move::Down), (Some((0.0, -NDI_TURN)), None));
        assert_eq!(ndi_speeds(Move::ZoomOut), (None, Some(-NDI_ZOOM)));
        // Sony's header: a command, nine bytes, the sequence number.
        assert_eq!(packet(&move_command(Move::Stop), 5, true)[..8], [0x01, 0x00, 0x00, 0x09, 0, 0, 0, 5]);
    }

    #[test]
    fn the_camera_gets_the_moves_in_order() {
        let camera = UdpSocket::bind("127.0.0.1:0").unwrap();
        camera.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let c = visca("127.0.0.1", camera.local_addr().unwrap().port());
        send(c.clone(), Order::Move(Move::Left));
        send(c.clone(), Order::Move(Move::Stop));
        send(c.clone(), Order::Move(Move::ZoomIn));
        send(c.clone(), Order::Save(4));
        send(c.clone(), Order::Recall(2));
        let mut buf = [0u8; 64];
        let mut got = vec![];
        for _ in 0..5 { let n = camera.recv(&mut buf).unwrap(); got.push(buf[..n].to_vec()) }
        assert_eq!(got, [move_command(Move::Left), move_command(Move::Stop), move_command(Move::ZoomIn), save_command(4), recall_packet(2, 0, false)]);
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
