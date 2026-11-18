//! What the computer sends and what the app asks, exactly as the browser pages do (`web/remote.js`,
//! `web/stage-view.js`, `web/tally.html`, `web/phone-mic.html`, `web/camera.html`; the server side is the desktop
//! app's `server.rs` and `network/*`). Every field is optional: an older or newer computer still works.
use serde::Deserialize;
use serde_json::{Value, json};

// ---------- the remote ----------

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct RemoteView {
    pub version: u64,
    pub service: Option<String>,
    pub items: Vec<Item>,
    pub cursor: Option<Cursor>,
    pub sections: Vec<Section>,
    pub current: Option<Current>,
    pub next: Option<Next>,
    pub on_air: OnAir,
    pub media: Option<Media>,
    pub lang: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Item {
    pub id: String,
    pub title: String,
    /// "3 slides", in the computer's language.
    pub count: String,
    pub slides: Vec<SlideText>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct SlideText { pub label: String, pub text: String }

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Cursor { pub item_id: String, pub slide: usize }

/// A song section of the live song: a tap jumps to it.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Section {
    pub occurrence: Value,
    pub kind: Value,
    pub label: String,
    pub short: String,
    pub color: String,
    pub live: bool,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Current {
    pub label: String,
    pub text: String,
    pub notes: String,
    /// A PowerPoint, PDF or designed slide: it shows as its picture (`/remote/live-slide`).
    pub picture: bool,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Next { pub title: String, pub label: String, pub text: String }

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct OnAir { pub slide: bool, pub media: bool, pub audio: bool, pub logo: bool, pub background: bool }

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Media { pub inst: Value, pub paused: bool }

/// One message on `/remote/events`.
#[derive(Debug, PartialEq)]
pub enum RemoteMsg { View(Box<RemoteView>), Other }

pub fn remote_msg(data: &str) -> RemoteMsg {
    let Ok(v) = serde_json::from_str::<Value>(data) else { return RemoteMsg::Other };
    if v["type"] != "remote" { return RemoteMsg::Other }
    serde_json::from_value(v).map(|r| RemoteMsg::View(Box::new(r))).unwrap_or(RemoteMsg::Other)
}

/// What the "On screen" box shows, as `remote.js`'s `render`.
#[derive(Debug, PartialEq)]
pub enum NowShowing { Logo, Clear, Music, Slide { label: String, text: String, picture: bool, notes: String } }

impl RemoteView {
    pub fn now_showing(&self) -> NowShowing {
        let air = &self.on_air;
        if air.logo { return NowShowing::Logo }
        match &self.current {
            Some(c) if air.slide || air.media => NowShowing::Slide { label: c.label.clone(), text: c.text.clone(), picture: c.picture, notes: c.notes.clone() },
            _ if air.audio => NowShowing::Music,
            _ => NowShowing::Clear,
        }
    }

    pub fn live_item(&self) -> Option<&str> { self.cursor.as_ref().map(|c| c.item_id.as_str()) }
}

/// A fresh command id (the computer drops a repeat of the same id).
pub fn command_id() -> String {
    use std::{hash::{BuildHasher, Hasher}, sync::atomic::{AtomicU64, Ordering}};
    static N: AtomicU64 = AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(N.fetch_add(1, Ordering::Relaxed));
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    format!("app{:016x}{:x}", h.finish(), N.load(Ordering::Relaxed))
}

/// A command for `/remote/command`: `{ id, cmd, base?, …extra }`. Next, Back, showing a slide and jumping to a
/// section say which screen they were based on (`base`), so a tap on an old view is refused rather than wrong.
pub fn command(cmd: &str, extra: Value, base: Option<u64>) -> Value {
    let mut env = json!({ "id": command_id(), "cmd": cmd });
    if let Some(b) = base { env["base"] = json!(b) }
    if let (Value::Object(e), Value::Object(x)) = (&mut env, extra) { e.extend(x) }
    env
}

/// The clear buttons, as the browser remote's row: the command each sends.
pub const CLEARS: [&str; 5] = ["clear_all", "clear_slide", "clear_media", "clear_audio", "logo"];

/// What a command's answer means for the person.
#[derive(Debug, PartialEq)]
pub enum CommandResult { Done, PairAgain, Stale, Error(String) }

pub fn command_result(status: u16, body: &Value) -> CommandResult {
    if status == 401 { return CommandResult::PairAgain }
    if body["ok"] == true { return CommandResult::Done }
    match body["error"].as_str() {
        Some(e) if e.starts_with("stale") => CommandResult::Stale,
        Some(e) => CommandResult::Error(e.to_string()),
        None => CommandResult::Error(String::new()),
    }
}

// ---------- pairing ----------

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct RemoteStatus {
    pub paired: bool,
    /// A remote password is asked on top of the code.
    pub password: bool,
    /// Newer computers: "RustyPresenter", the computer's name and the app's version (for finding it on the Wi-Fi).
    pub app: Option<String>,
    pub computer: Option<String>,
    pub version: Option<String>,
}

/// The pairing request: the 6-digit code, or the one-time secret from the QR code, with the password when needed.
pub fn pair_request(code: &PairCode, password: &str, name: &str) -> Value {
    let mut v = json!({ "password": password, "name": name });
    match code {
        PairCode::Pin(p) => v["pin"] = json!(p.trim()),
        PairCode::Secret(s) => v["pair"] = json!(s),
    }
    v
}

#[derive(Debug, Clone, PartialEq)]
pub enum PairCode { Pin(String), Secret(String) }

/// The answer to a pairing request.
#[derive(Debug, PartialEq)]
pub enum PairResult { Paired(String), Error { text: String, retry_with_pin: bool } }

pub fn pair_result(r: &crate::http::Response) -> PairResult {
    let token = r.set_cookies().into_iter().find(|(n, _)| n == "rp_session").map(|(_, v)| v);
    let body = r.json();
    match token {
        Some(t) if r.ok() && body["ok"] == true => PairResult::Paired(t),
        _ => PairResult::Error { text: body["error"].as_str().unwrap_or_default().to_string(), retry_with_pin: body["retry_with_pin"] == true },
    }
}

// ---------- the stage screen ----------

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct StageStatus {
    /// The stage screen on phones is turned on (Settings → Remote and network).
    pub on: bool,
    pub password: bool,
    /// This phone may see it now.
    pub allowed: bool,
    pub off_text: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct StageView {
    pub background: String,
    pub cells: Vec<Cell>,
    pub banner: Option<String>,
    pub follow: Option<String>,
    pub version: u64,
}

/// A box of the stage layout, in % of the screen.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Cell {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// text, chords, message, picture or shape.
    pub kind: String,
    pub color: String,
    pub fill: Option<String>,
    /// Biggest and smallest words, % of the screen's height.
    pub max: f32,
    pub min: f32,
    pub bold: bool,
    pub align: String,
    pub valign: String,
    pub text: String,
    pub dim: bool,
    pub red: bool,
    pub caption: Option<String>,
    /// A picture: "live" or "next".
    pub src: String,
    pub shape: String,
    /// A chord sheet: lines of [chord, words].
    pub lines: Option<Vec<Vec<(String, String)>>>,
    pub chord_color: String,
}

pub fn stage_view(data: &str) -> Option<StageView> {
    let v: Value = serde_json::from_str(data).ok()?;
    if v["type"] != "stage" { return None }
    serde_json::from_value(v).ok()
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa` → (r, g, b, a). Anything else (`transparent`, a name) is None.
pub fn parse_color(s: &str) -> Option<[u8; 4]> {
    let h = s.trim().strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        3 | 4 => {
            let d: Vec<u8> = h.chars().map(|c| c.to_digit(16).map(|v| v as u8 * 17)).collect::<Option<_>>()?;
            Some([d[0], d[1], d[2], *d.get(3).unwrap_or(&255)])
        }
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

// ---------- tally, microphone, camera ----------

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Tally {
    pub allowed: bool,
    /// on_air, next or off.
    pub light: String,
    pub text: String,
    pub sub: String,
}

/// The computer's answer to a piece of the phone's sound.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct MicAnswer { pub allowed: bool, pub taken: bool, pub text: String }

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct CameraInfo {
    pub paired: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// The phone's name on the computer: a first camera name.
    pub name: Option<String>,
    /// The page's words in the computer's language (on_air, next, off_air, …).
    pub words: Value,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct CameraLight { pub light: String, pub text: String }

/// The camera's address: `/camera/ws?name=…`.
pub fn camera_path(name: &str) -> String { format!("/camera/ws?name={}", crate::http::encode(name)) }

/// The camera name kept to 40 letters, as the page does; empty → `fallback`.
pub fn camera_name(typed: &str, fallback: &str) -> String {
    let t: String = typed.trim().chars().take(40).collect();
    if t.is_empty() { fallback.to_string() } else { t }
}

/// 16-bit mono samples as the little-endian bytes the computer reads.
pub fn pcm_bytes(samples: &[i16]) -> Vec<u8> { samples.iter().flat_map(|s| s.to_le_bytes()).collect() }

/// The loudest sample, 0–1, for the level bar.
pub fn peak(samples: &[i16]) -> f32 {
    samples.iter().map(|s| (i32::from(*s)).unsigned_abs()).max().unwrap_or(0) as f32 / 32768.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A view as the desktop's `remote_view` makes it (trimmed).
    const VIEW: &str = r##"{"type":"remote","version":42,"service":"Sunday","items":[{"id":"a1","title":"Amazing Grace","kind":"song","count":"2 slides","slides":[{"label":"Verse 1","text":"Amazing grace"},{"label":"Chorus","text":"My chains are gone"}]}],
        "cursor":{"item_id":"a1","item_index":0,"slide":1},"sections":[{"occurrence":1,"label":"Chorus","short":"C","kind":"chorus","color":"#ff8f93","first_slide":1,"slides":1,"live":true,"key":"C"}],
        "current":{"label":"Chorus","text":"My chains are gone","notes":"","picture":false,"transition":null,"anim":false},"next":{"title":"Amazing Grace","label":"Verse 2","text":"The Lord has promised"},
        "on_air":{"slide":true,"media":false,"audio":false,"logo":false,"background":true},"media":null,"notice":null,"overlays":[],"lang":"en","words":3}"##;

    #[test]
    fn the_remote_view_reads_as_the_browser_reads_it() {
        let RemoteMsg::View(v) = remote_msg(VIEW) else { panic!() };
        assert_eq!((v.version, v.service.as_deref()), (42, Some("Sunday")));
        assert_eq!(v.items[0].slides[1].text, "My chains are gone");
        assert_eq!(v.live_item(), Some("a1"));
        assert_eq!(v.sections[0].kind, json!("chorus"));
        assert_eq!(v.now_showing(), NowShowing::Slide { label: "Chorus".into(), text: "My chains are gone".into(), picture: false, notes: String::new() });
        assert_eq!(remote_msg(r#"{"type":"timers","timers":[]}"#), RemoteMsg::Other);
        assert_eq!(remote_msg("not json"), RemoteMsg::Other);
    }

    #[test]
    fn on_screen_follows_the_clears() {
        let RemoteMsg::View(mut v) = remote_msg(VIEW) else { panic!() };
        v.on_air.logo = true;
        assert_eq!(v.now_showing(), NowShowing::Logo);
        v.on_air = OnAir { audio: true, ..Default::default() };
        assert_eq!(v.now_showing(), NowShowing::Music);
        v.on_air.audio = false;
        assert_eq!(v.now_showing(), NowShowing::Clear);
    }

    #[test]
    fn commands_carry_an_id_and_the_screen_they_saw() {
        let c = command("show", json!({ "item_id": "a1", "slide": 1 }), Some(42));
        assert_eq!((c["cmd"].as_str(), c["base"].as_u64(), c["slide"].as_u64()), (Some("show"), Some(42), Some(1)));
        assert!(c["id"].as_str().unwrap().len() > 8);
        assert_ne!(command("next", json!({}), None)["id"], command("next", json!({}), None)["id"]);
        assert!(command("clear_all", json!({}), None).get("base").is_none());
        assert_eq!(command_result(200, &json!({ "ok": true })), CommandResult::Done);
        assert_eq!(command_result(401, &json!({ "error": "pair first" })), CommandResult::PairAgain);
        assert_eq!(command_result(200, &json!({ "ok": false, "error": "stale: version 41" })), CommandResult::Stale);
        assert_eq!(command_result(200, &json!({ "ok": false, "error": "Not allowed from the phone." })), CommandResult::Error("Not allowed from the phone.".into()));
    }

    #[test]
    fn pairing_sends_the_code_or_the_secret() {
        let pin = pair_request(&PairCode::Pin(" 123456 ".into()), "worship", "Pixel 8");
        assert_eq!(pin, json!({ "pin": "123456", "password": "worship", "name": "Pixel 8" }));
        let qr = pair_request(&PairCode::Secret("abc".into()), "", "Pixel 8");
        assert_eq!((qr["pair"].as_str(), qr.get("pin")), (Some("abc"), None));
        let ok = crate::http::Response { status: 200, headers: vec![("set-cookie".into(), "rp_session=t0k; HttpOnly; Path=/".into())], body: br#"{"ok":true}"#.to_vec() };
        assert_eq!(pair_result(&ok), PairResult::Paired("t0k".into()));
        let no = crate::http::Response { status: 400, headers: vec![], body: r#"{"error":"That QR code has expired.","retry_with_pin":true}"#.as_bytes().to_vec() };
        assert_eq!(pair_result(&no), PairResult::Error { text: "That QR code has expired.".into(), retry_with_pin: true });
    }

    #[test]
    fn the_stage_view_reads_its_boxes_and_chord_lines() {
        let s = r##"{"type":"stage","background":"#000000","banner":null,"follow":"Auto: following — Chorus 2","version":7,"cells":[
            {"x":2.0,"y":3.0,"w":60.0,"h":50.0,"color":"#ffffff","fill":null,"max":9.0,"min":3.0,"bold":true,"align":"left","valign":"top","kind":"chords","text":"[G]Amazing","lines":[[["G","Amazing "],["D","grace"]],[]],"chord_color":"#f5b94a","caption":"Now"},
            {"x":0,"y":80,"w":100,"h":20,"color":"#fff","fill":"#202020","max":6,"min":2,"bold":false,"align":"center","valign":"middle","kind":"picture","caption":null,"src":"next"},
            {"x":0,"y":0,"w":10,"h":10,"color":"#ff0000","fill":"#ff0000","max":1,"min":1,"bold":false,"align":"center","valign":"middle","kind":"shape","shape":"ellipse"}]}"##;
        let v = stage_view(s).unwrap();
        assert_eq!((v.cells.len(), v.version, v.follow.as_deref()), (3, 7, Some("Auto: following — Chorus 2")));
        let c = &v.cells[0];
        assert_eq!(c.lines.as_ref().unwrap()[0], vec![("G".to_string(), "Amazing ".to_string()), ("D".into(), "grace".into())]);
        assert_eq!((c.caption.as_deref(), c.chord_color.as_str(), c.bold), (Some("Now"), "#f5b94a", true));
        assert_eq!((v.cells[1].src.as_str(), v.cells[1].fill.as_deref()), ("next", Some("#202020")));
        assert_eq!(v.cells[2].shape, "ellipse");
        assert!(stage_view(r#"{"type":"remote"}"#).is_none());
    }

    #[test]
    fn colours_as_the_layouts_write_them() {
        assert_eq!(parse_color("#f5b94a"), Some([0xf5, 0xb9, 0x4a, 255]));
        assert_eq!(parse_color("#fff"), Some([255, 255, 255, 255]));
        assert_eq!(parse_color("#ff4f5566"), Some([0xff, 0x4f, 0x55, 0x66]));
        assert_eq!(parse_color("transparent"), None);
        assert_eq!(parse_color("#12"), None);
    }

    #[test]
    fn tally_microphone_and_camera_answers() {
        let t: Tally = serde_json::from_str(r#"{"allowed":true,"light":"on_air","text":"Camera 2 — on air","sub":""}"#).unwrap();
        assert_eq!((t.light.as_str(), t.text.as_str()), ("on_air", "Camera 2 — on air"));
        let m: MicAnswer = serde_json::from_str(r#"{"allowed":true,"taken":false,"text":"Not used"}"#).unwrap();
        assert!(m.allowed && !m.taken);
        let i: CameraInfo = serde_json::from_str(r#"{"words":{"on_air":"On air"},"paired":true,"password":false,"secure_port":4320,"width":1920,"height":1080,"fps":30,"name":"Pastor's phone"}"#).unwrap();
        assert_eq!((i.width, i.fps, i.name.as_deref()), (1920, 30, Some("Pastor's phone")));
        assert_eq!(camera_path("Pastor's phone"), "/camera/ws?name=Pastor%27s%20phone");
        assert_eq!(camera_name("  ", "Phone camera"), "Phone camera");
        assert_eq!(camera_name(&"x".repeat(50), "").len(), 40);
        assert_eq!(pcm_bytes(&[1, -2]), vec![1, 0, 0xFE, 0xFF]);
        assert!((peak(&[0, -16384, 100]) - 0.5).abs() < 1e-3);
    }
}
