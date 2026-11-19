//! A phone as a camera. The phone opens `/camera` on the app's phone server (`network::phone_camera`), turns on its
//! camera in the browser and sends its pictures as JPEG, the stream's size, over a WebSocket (or one picture a
//! request when the WebSocket cannot connect). Each phone camera has a name the volunteer gives it on the phone
//! ("Balcony"); it is saved in Settings as `phone://Balcony` and goes through the same camera feed as any other
//! camera (`camera_feed`), so the lost-camera light, switching, tally and scenes work the same.
//!
//! The newest picture of each phone is kept here (`put`), by name, while the app runs; the camera feed of a phone
//! camera decodes each new one into the stream's picture (`receive`).
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How a phone camera is saved in Settings: `phone://<name>`.
pub const SCHEME: &str = "phone://";
/// The camera input (`record::args::camera_input`) of a phone camera: this, then its name.
pub const INPUT: &str = "<phone>";
/// The longest name a phone camera can have.
pub const MAX_NAME: usize = 40;
/// The biggest picture a phone may send (a 1080p JPEG is well under 1 MB).
pub const MAX_PICTURE: usize = 8 * 1024 * 1024;
/// A phone that sent nothing for this long is not sending.
const QUIET: Duration = Duration::from_secs(2);

/// The phone camera's name in saved camera `camera`, when it is one.
pub fn source_name(camera: &str) -> Option<&str> { camera.strip_prefix(SCHEME).filter(|n| !n.trim().is_empty()) }

/// The phone camera's name in camera input `input` (`record::args::camera_input`).
pub fn input_source(input: &[String]) -> Option<&str> {
    match input { [kind, name] if kind == INPUT => Some(name.as_str()), _ => None }
}

/// "Phone camera: Balcony".
pub fn label(name: &str) -> String { crate::tr!("Phone camera: {name}", name = name) }

/// A name typed on the phone, made safe to keep: trimmed, no control characters, at most `MAX_NAME` characters.
/// `None` when nothing is left.
pub fn clean_name(raw: &str) -> Option<String> {
    let name: String = raw.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(MAX_NAME).collect();
    let name = name.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// What is kept of one phone camera: its newest picture (JPEG), how many it has sent, and when the last came.
#[derive(Default)]
struct Slot {
    jpeg: Option<Arc<Vec<u8>>>,
    count: u64,
    at: Option<Instant>,
}

fn hub() -> &'static Mutex<HashMap<String, Slot>> {
    static HUB: OnceLock<Mutex<HashMap<String, Slot>>> = OnceLock::new();
    HUB.get_or_init(Mutex::default)
}

fn slots() -> std::sync::MutexGuard<'static, HashMap<String, Slot>> { hub().lock().unwrap_or_else(|e| e.into_inner()) }

/// Phone camera `name` sent picture `jpeg`.
pub fn put(name: &str, jpeg: Vec<u8>) {
    let mut s = slots();
    let slot = s.entry(name.to_string()).or_default();
    slot.jpeg = Some(Arc::new(jpeg));
    slot.count += 1;
    slot.at = Some(Instant::now());
}

/// Phone camera `name`'s newest picture, with its number (counting from 1) and when it came.
pub fn newest(name: &str) -> Option<(Arc<Vec<u8>>, u64, Instant)> {
    let s = slots();
    let slot = s.get(name)?;
    Some((slot.jpeg.clone()?, slot.count, slot.at?))
}

/// The phone cameras that have sent pictures since the app started, by name.
pub fn names() -> Vec<String> {
    let mut n: Vec<String> = slots().keys().cloned().collect();
    n.sort_by_key(|a| a.to_lowercase());
    n
}

/// Whether phone camera `name` is sending now.
pub fn sending(name: &str) -> bool { slots().get(name).and_then(|s| s.at).is_some_and(|at| at.elapsed() < QUIET) }

/// A phone's JPEG as an RGBA picture of `size`, its shape kept with black bars. `None` when it is not a picture.
pub fn decode(jpeg: &[u8], (w, h): (u32, u32)) -> Option<Vec<u8>> {
    let picture = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).ok()?.to_rgba8();
    let mut out = vec![0u8; super::camera_feed::frame_bytes((w, h))];
    let (pw, ph) = picture.dimensions();
    if (pw, ph) == (w, h) {
        out.copy_from_slice(picture.as_raw());
        out.chunks_exact_mut(4).for_each(|p| p[3] = 255);
    } else {
        crate::record::pump::fit(&crate::record::pump::Raw { w: pw, h: ph, bottom_up: false, rgba: picture.as_raw() }, &mut out, w, h);
    }
    Some(out)
}

/// The camera feed of phone camera `name`: each new picture the phone sends goes into `latest` as an RGBA picture
/// of `size`, until `stop`. A phone that stops sending simply leaves `latest` to go stale (the lost-camera light).
pub fn receive(name: &str, size: (u32, u32), latest: &super::camera_feed::Latest, stop: &AtomicBool) {
    let mut seen = 0u64;
    while !stop.load(Ordering::Relaxed) {
        match newest(name) {
            Some((jpeg, count, at)) if count != seen => {
                seen = count;
                // A picture that waited too long (the phone stopped) is not shown as new.
                if at.elapsed() < QUIET && let Some(rgba) = decode(&jpeg, size) {
                    *latest.lock().unwrap_or_else(|e| e.into_inner()) = Some((Arc::new(rgba), at));
                }
            }
            _ => std::thread::sleep(Duration::from_millis(8)),
        }
    }
}

/// The words of the phone's camera page (`web/camera.html`) and of Settings, in the app's language.
pub mod tr {
    use crate::tr;

    /// Every word the camera page shows, by the name the page looks it up with.
    pub fn page_words() -> serde_json::Value {
        serde_json::json!({
            "title": tr!("Phone camera"),
            "secure_title": tr!("One step the first time"),
            "secure_body": tr!("A phone only lets a page use its camera when the page is private. Tap the button below. The phone then warns that the connection is not private: that is expected, because the page comes from your church's own computer, not from the internet."),
            "secure_how": secure_how(),
            "no_proceed": no_proceed(),
            "secure_go": tr!("Open the camera page"),
            "secure_off": tr!("The private camera page is not running on the computer. Close the app and open it again; if it still does not start, another program is using its port."),
            "pair_title": tr!("Connect this phone"),
            "pair_body": tr!("Scan the camera's QR code in RustyPresenter (Settings → Remote and network → Phone camera and microphone). Or, on the presentation computer, click Phone, then Pair a phone or tablet, and type the 6-digit code here."),
            "code": code(),
            "password": password(),
            "connect": connect(),
            "pairing": tr!("Connecting this phone…"),
            "name": tr!("Camera name"),
            "name_hint": tr!("Like Balcony or Side. The computer lists this phone by this name."),
            "start": tr!("Start the camera"),
            "stop": tr!("Stop"),
            "front": tr!("Front camera"),
            "back": tr!("Back camera"),
            "sideways": tr!("Turn the phone sideways for a full picture."),
            "sending": tr!("Sending to the computer"),
            "waiting": tr!("Connecting to the computer…"),
            "lost": tr!("The computer cannot be reached. Trying again…"),
            "no_camera": tr!("The camera could not be opened. Allow the camera for this page in the phone's settings, and close other apps using it."),
            "on_air": tr!("On the stream now"),
            "next": tr!("Next on the stream"),
            "off_air": tr!("Not on the stream"),
            "choose": tr!("On the computer, choose this phone in Settings → Livestream → Cameras."),
        })
    }

    /// When the computer has no network address for phones.
    pub fn no_network() -> String { tr!("This computer is not on a network, so no phone can reach it.") }
    /// The one-time "not private" step, for Android and iPhone (the phone pages and Settings).
    pub fn secure_how() -> String { tr!("Android (Chrome): tap Advanced, then Proceed. iPhone (Safari): tap Show Details, then visit this website, then Visit Website. The phone remembers it, so this is only needed once.") }
    /// When an Android phone shows no way to go on: Chrome's own setting that treats this computer's plain page as
    /// secure. `{address}` is filled in by the page with this computer's plain address.
    pub fn no_proceed() -> String { tr!("No Proceed button? In Chrome on Android, open chrome://flags, search for \"insecure origins\", turn on \"Insecure origins treated as secure\", type {address} in its box, tap Relaunch, then open this page again.") }
    pub fn code() -> String { tr!("6-digit code") }
    pub fn password() -> String { tr!("Password") }
    pub fn connect() -> String { tr!("Connect") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb(rgb));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_phone_camera_is_saved_and_named_by_its_name() {
        assert_eq!(source_name("phone://Balcony"), Some("Balcony"));
        assert_eq!(source_name("phone://  "), None);
        assert_eq!(source_name("Integrated Camera"), None);
        assert_eq!(input_source(&[INPUT.into(), "Balcony".into()]), Some("Balcony"));
        assert_eq!(input_source(&["-i".into(), "x".into()]), None);
        assert_eq!(clean_name("  Side\u{7}  door "), Some("Side  door".into()));
        assert_eq!(clean_name(" \n "), None);
        assert_eq!(clean_name(&"x".repeat(100)).map(|n| n.chars().count()), Some(MAX_NAME));
        assert_eq!(crate::record::args::camera_input(crate::platform::tools::OS, "phone://Balcony"), [INPUT.to_string(), "Balcony".into()]);
        assert_eq!(crate::record::args::camera_label("phone://Balcony"), "Phone camera: Balcony");
    }

    #[test]
    fn a_phone_picture_fills_the_stream_keeping_its_shape() {
        // The stream's size already: as sent.
        let p = decode(&jpeg(32, 18, [200, 30, 30]), (32, 18)).unwrap();
        assert_eq!(p.len(), 32 * 18 * 4);
        assert!(p[0] > 170 && p[1] < 70 && p[3] == 255, "{:?}", &p[..4]);
        // Held upright: black bars at the sides.
        let p = decode(&jpeg(18, 32, [30, 30, 200]), (64, 36)).unwrap();
        assert_eq!(&p[..4], &[0, 0, 0, 255], "a black bar on the left");
        let mid = (18 * 64 + 32) * 4;
        assert!(p[mid + 2] > 170, "blue in the middle: {:?}", &p[mid..mid + 4]);
        assert!(decode(b"not a picture", (32, 18)).is_none());
    }

    #[test]
    fn the_feed_shows_each_new_picture_from_the_phone() {
        let name = format!("Feed test {}", std::process::id());
        let feed = crate::stream::camera_feed::start("ffmpeg".into(), vec![INPUT.into(), name.clone()], (32, 18), 30).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(feed.picture().is_none(), "nothing sent yet");
        put(&name, jpeg(32, 18, [20, 200, 20]));
        let began = Instant::now();
        while !feed.seeing.load(Ordering::Relaxed) && began.elapsed() < Duration::from_secs(5) { std::thread::sleep(Duration::from_millis(20)) }
        let p = feed.picture().expect("the phone's picture");
        assert!(p[1] > 170, "green: {:?}", &p[..4]);
        assert!(names().contains(&name) && sending(&name));
    }
}
