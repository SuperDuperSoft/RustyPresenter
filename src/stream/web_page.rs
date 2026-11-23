//! A web page as a layer of the church's scenes (design `docs/superpowers/specs/2026-11-17-scene-extras-design.md`).
//! RustyPresenter has no browser of its own. The computer's own Microsoft Edge (or Google Chrome or Chromium) is run
//! hidden ("headless"), its window as big as the stream, its background clear; the page is opened in it and its
//! pictures are taken with the browser's DevTools screencast (each picture a PNG, sent when the page changes). Each
//! picture is turned into premultiplied RGBA of the stream's size on a thread of its own; the writer only takes the
//! newest. A page whose browser stops is opened again every `RETRY`.
//!
//! The DevTools connection is a WebSocket on the computer itself (127.0.0.1), spoken here with the little of it that
//! is needed (text messages, sent masked as a client must).
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long after the browser stopped the page is opened again.
const RETRY: Duration = Duration::from_secs(5);
/// How long the browser may take to start and say where it listens. Edge's first start on a fresh profile took 13 s
/// on an idle Linux test machine, and longer on a busy one, so 20 s gave up on browsers that were still coming.
const START_LIMIT: Duration = Duration::from_secs(60);

/// The browsers tried, in order.
pub fn browsers() -> Vec<PathBuf> {
    let mut out = vec![];
    #[cfg(windows)]
    for base in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"].iter().filter_map(std::env::var_os) {
        let base = PathBuf::from(base);
        out.push(base.join(r"Microsoft\Edge\Application\msedge.exe"));
        out.push(base.join(r"Google\Chrome\Application\chrome.exe"));
    }
    #[cfg(target_os = "macos")]
    for app in ["Microsoft Edge.app/Contents/MacOS/Microsoft Edge", "Google Chrome.app/Contents/MacOS/Google Chrome", "Chromium.app/Contents/MacOS/Chromium"] {
        out.push(Path::new("/Applications").join(app));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in ["microsoft-edge", "google-chrome", "google-chrome-stable", "chromium", "chromium-browser"] { out.push(dir.join(name)) }
        }
    }
    out
}

/// The first browser found on this computer.
pub fn find_browser() -> Option<PathBuf> { browsers().into_iter().find(|p| p.is_file()) }

/// The browser's arguments: hidden, `size` big, no sound, its own profile in `profile`.
pub fn browser_args(profile: &Path, (w, h): (u32, u32)) -> Vec<String> {
    vec![
        "--headless=new".into(), "--remote-debugging-port=0".into(), format!("--user-data-dir={}", profile.display()),
        format!("--window-size={w},{h}"), "--hide-scrollbars".into(), "--mute-audio".into(), "--no-first-run".into(),
        "--no-default-browser-check".into(), "--disable-extensions".into(), "--autoplay-policy=no-user-gesture-required".into(),
        // The clear background is set over DevTools: Edge's renderer crashed with `--default-background-color`
        // (tried November 17, 2026).
        "about:blank".into(),
    ]
}

/// A web page being shown. Dropping it closes the browser.
pub struct Page {
    latest: Arc<Mutex<Option<Arc<Vec<u8>>>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Page {
    /// Opens `address` at `size`. An error when there is no browser on this computer.
    pub fn start(address: &str, size: (u32, u32)) -> Result<Page, String> {
        let browser = find_browser().ok_or_else(|| "no Microsoft Edge, Google Chrome or Chromium on this computer".to_string())?;
        let (latest, stop): (Arc<Mutex<Option<Arc<Vec<u8>>>>>, Arc<AtomicBool>) = (Arc::default(), Arc::default());
        let (l, s, address) = (latest.clone(), stop.clone(), address.to_string());
        let thread = std::thread::Builder::new().name("stream-web-page".into()).spawn(move || {
            while !s.load(Ordering::Relaxed) {
                if let Err(e) = show(&browser, &address, size, &l, &s) { eprintln!("stream web page {address}: {e}") }
                let until = Instant::now() + RETRY;
                while !s.load(Ordering::Relaxed) && Instant::now() < until { std::thread::sleep(Duration::from_millis(100)) }
            }
        }).map_err(|e| e.to_string())?;
        Ok(Page { latest, stop, thread: Some(thread) })
    }

    /// The page's newest picture (premultiplied RGBA of the stream's size), `None` before the first.
    pub fn picture(&self) -> Option<Arc<Vec<u8>>> { self.latest.lock().unwrap_or_else(|e| e.into_inner()).clone() }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}

impl Drop for Page {
    fn drop(&mut self) { self.stop() }
}

/// Runs the browser and takes the page's pictures until `stop` or until something goes wrong.
fn show(browser: &Path, address: &str, size: (u32, u32), latest: &Mutex<Option<Arc<Vec<u8>>>>, stop: &AtomicBool) -> Result<(), String> {
    let profile = std::env::temp_dir().join(format!("RustyPresenter-page-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    let result = rt.block_on(async {
        let mut cmd = tokio::process::Command::new(browser);
        cmd.args(browser_args(&profile, size)).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        // The browser and every helper it starts die together.
        let mut tree = crate::platform::proc::spawn_tree(cmd).map_err(|e| format!("the browser could not start: {e}"))?;
        let r = screencast(&profile, address, size, latest, stop);
        tree.kill();
        r
    });
    // The browser lets go of its profile a moment after it ends.
    for _ in 0..20 {
        if std::fs::remove_dir_all(&profile).is_ok() || !profile.exists() { break }
        std::thread::sleep(Duration::from_millis(100));
    }
    result
}

/// Waits for the browser to say its DevTools port (the `DevToolsActivePort` file in its profile).
fn devtools_port(profile: &Path, stop: &AtomicBool) -> Result<u16, String> {
    let began = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(profile.join("DevToolsActivePort")) && let Some(port) = text.lines().next().and_then(|l| l.trim().parse().ok()) { return Ok(port) }
        if stop.load(Ordering::Relaxed) { return Err("stopped".into()) }
        if began.elapsed() > START_LIMIT { return Err("the browser did not start".into()) }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A plain HTTP GET on the computer itself: the body.
fn http_get(port: u16, path: &str) -> Result<String, String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").map_err(|e| e.to_string())?;
    // The browser may keep the connection open: the body is read up to its Content-Length.
    let mut r = BufReader::new(s);
    let mut length = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).map_err(|e| e.to_string())? == 0 || line == "\r\n" { break }
        if let Some((k, v)) = line.split_once(':') && k.trim().eq_ignore_ascii_case("content-length") { length = v.trim().parse::<usize>().ok() }
    }
    let mut body = vec![];
    match length {
        Some(n) => { body.resize(n, 0); r.read_exact(&mut body).map_err(|e| format!("DevTools list: {e}"))? }
        None => { r.read_to_end(&mut body).map_err(|e| e.to_string())?; }
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// The page tab's DevTools address (`/devtools/page/…`).
fn page_target(port: u16, stop: &AtomicBool) -> Result<String, String> {
    let began = Instant::now();
    loop {
        if let Ok(body) = http_get(port, "/json/list") && let Ok(serde_json::Value::Array(targets)) = serde_json::from_str::<serde_json::Value>(&body) {
            let page = targets.iter().find(|t| t["type"] == "page").and_then(|t| t["webSocketDebuggerUrl"].as_str());
            if let Some(url) = page && let Some(path) = url.split_once("://").and_then(|(_, rest)| rest.find('/').map(|i| rest[i..].to_string())) { return Ok(path) }
        }
        if stop.load(Ordering::Relaxed) { return Err("stopped".into()) }
        if began.elapsed() > START_LIMIT { return Err("the browser has no page".into()) }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn screencast(profile: &Path, address: &str, size: (u32, u32), latest: &Mutex<Option<Arc<Vec<u8>>>>, stop: &AtomicBool) -> Result<(), String> {
    let port = devtools_port(profile, stop)?;
    let target = page_target(port, stop)?;
    let mut ws = Socket::connect(port, &target)?;
    let (w, h) = size;
    let mut id = 0u64;
    let mut call = |ws: &mut Socket, method: &str, params: serde_json::Value| -> Result<(), String> {
        id += 1;
        ws.send(&serde_json::json!({ "id": id, "method": method, "params": params }).to_string())
    };
    call(&mut ws, "Emulation.setDeviceMetricsOverride", serde_json::json!({ "width": w, "height": h, "deviceScaleFactor": 1, "mobile": false }))?;
    call(&mut ws, "Emulation.setDefaultBackgroundColorOverride", serde_json::json!({ "color": { "r": 0, "g": 0, "b": 0, "a": 0 } }))?;
    call(&mut ws, "Page.enable", serde_json::json!({}))?;
    call(&mut ws, "Page.navigate", serde_json::json!({ "url": address }))?;
    call(&mut ws, "Page.startScreencast", serde_json::json!({ "format": "png", "maxWidth": w, "maxHeight": h, "everyNthFrame": 1 }))?;
    while !stop.load(Ordering::Relaxed) {
        let Some(text) = ws.receive()? else { continue };
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        if msg["method"] != "Page.screencastFrame" { continue }
        let params = &msg["params"];
        // Asked for first, so the browser makes the next picture while this one is turned into RGBA.
        if let Some(session) = params["sessionId"].as_i64() { call(&mut ws, "Page.screencastFrameAck", serde_json::json!({ "sessionId": session }))? }
        let Some(data) = params["data"].as_str() else { continue };
        if let Some(px) = picture(data, size) { *latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(px)) }
    }
    Ok(())
}

/// A screencast picture (base64 PNG) as premultiplied RGBA of `size`, fitted inside it, clear around it.
pub fn picture(base64: &str, (w, h): (u32, u32)) -> Option<Vec<u8>> {
    let png = decode_base64(base64)?;
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).ok()?.to_rgba8();
    let mut px = if (img.width(), img.height()) == (w, h) { img.into_raw() } else {
        let (x0, y0, fw, fh) = crate::record::pump::fit_rect(img.width(), img.height(), w, h);
        let small = image::imageops::resize(&img, fw, fh, image::imageops::FilterType::Triangle);
        let mut out = vec![0u8; w as usize * h as usize * 4];
        for (y, row) in small.rows().enumerate() {
            let d0 = ((y0 as usize + y) * w as usize + x0 as usize) * 4;
            for (d, p) in out[d0..d0 + fw as usize * 4].chunks_exact_mut(4).zip(row) { d.copy_from_slice(&p.0) }
        }
        out
    };
    super::scene_media::premultiply(&mut px);
    Some(px)
}

/// Standard base64 (the browser's pictures); `None` for anything else.
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c { b'A'..=b'Z' => c - b'A', b'a'..=b'z' => c - b'a' + 26, b'0'..=b'9' => c - b'0' + 52, b'+' => 62, b'/' => 63, _ => return None }.into())
    };
    let bytes: Vec<u8> = text.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    let body = bytes.strip_suffix(b"==").or_else(|| bytes.strip_suffix(b"=")).unwrap_or(&bytes);
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    for chunk in body.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() { n |= value(*c)? << (18 - 6 * i) }
        let take = match chunk.len() { 4 => 3, 3 => 2, 2 => 1, _ => return None };
        out.extend_from_slice(&n.to_be_bytes()[1..1 + take]);
    }
    Some(out)
}

/// The DevTools WebSocket: text messages only.
struct Socket { stream: BufReader<TcpStream>, partial: Vec<u8> }

impl Socket {
    fn connect(port: u16, path: &str) -> Result<Socket, String> {
        let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
        // The key only has to be 16 bytes in base64; DevTools does not check it further.
        write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n")
            .map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
        let mut stream = BufReader::new(s);
        let mut status = String::new();
        stream.read_line(&mut status).map_err(|e| e.to_string())?;
        if !status.contains(" 101") { return Err(format!("DevTools said: {}", status.trim())) }
        loop {
            let mut line = String::new();
            if stream.read_line(&mut line).map_err(|e| e.to_string())? == 0 || line == "\r\n" { break }
        }
        // Short waits from now on, so `stop` is looked at often.
        stream.get_ref().set_read_timeout(Some(Duration::from_millis(500))).map_err(|e| e.to_string())?;
        Ok(Socket { stream, partial: vec![] })
    }

    fn send(&mut self, text: &str) -> Result<(), String> { self.frame(0x1, text.as_bytes()) }

    fn frame(&mut self, opcode: u8, payload: &[u8]) -> Result<(), String> {
        let mut f = vec![0x80 | opcode];
        let n = payload.len();
        if n < 126 { f.push(0x80 | n as u8) } else if n < 65536 { f.push(0x80 | 126); f.extend_from_slice(&(n as u16).to_be_bytes()) } else { f.push(0x80 | 127); f.extend_from_slice(&(n as u64).to_be_bytes()) }
        let mask: [u8; 4] = rand_mask();
        f.extend_from_slice(&mask);
        f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.stream.get_mut().write_all(&f).map_err(|e| e.to_string())
    }

    /// The next whole text message; `None` when none came for a moment.
    fn receive(&mut self) -> Result<Option<String>, String> {
        let mut head = [0u8; 2];
        match self.stream.read_exact(&mut head) {
            Ok(()) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Ok(None),
            Err(e) => return Err(format!("DevTools connection: {e}")),
        }
        // Once a frame has begun it is read to its end, however long that takes.
        self.stream.get_ref().set_read_timeout(Some(Duration::from_secs(10))).map_err(|e| e.to_string())?;
        let r = self.rest(head);
        self.stream.get_ref().set_read_timeout(Some(Duration::from_millis(500))).map_err(|e| e.to_string())?;
        r
    }

    fn rest(&mut self, head: [u8; 2]) -> Result<Option<String>, String> {
        let (fin, opcode) = (head[0] & 0x80 != 0, head[0] & 0x0f);
        let masked = head[1] & 0x80 != 0;
        let mut n = u64::from(head[1] & 0x7f);
        if n == 126 { let mut b = [0u8; 2]; self.stream.read_exact(&mut b).map_err(|e| e.to_string())?; n = u64::from(u16::from_be_bytes(b)) }
        else if n == 127 { let mut b = [0u8; 8]; self.stream.read_exact(&mut b).map_err(|e| e.to_string())?; n = u64::from_be_bytes(b) }
        if n > 256 * 1024 * 1024 { return Err("a message too big".into()) }
        let mut mask = [0u8; 4];
        if masked { self.stream.read_exact(&mut mask).map_err(|e| e.to_string())? }
        let mut payload = vec![0u8; n as usize];
        self.stream.read_exact(&mut payload).map_err(|e| e.to_string())?;
        if masked { payload.iter_mut().enumerate().for_each(|(i, b)| *b ^= mask[i % 4]) }
        match opcode {
            0x8 => Err("the browser closed the connection".into()),
            0x9 => { self.frame(0xA, &payload)?; Ok(None) }
            0x0..=0x2 => {
                self.partial.extend_from_slice(&payload);
                if !fin { return Ok(None) }
                let whole = std::mem::take(&mut self.partial);
                Ok(Some(String::from_utf8_lossy(&whole).into_owned()))
            }
            _ => Ok(None),
        }
    }
}

/// A client's frame mask: it only has to differ from frame to frame.
fn rand_mask() -> [u8; 4] {
    let n = uuid::Uuid::new_v4().as_u128();
    (n as u32).to_be_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_is_read() {
        assert_eq!(decode_base64("aGVsbG8="), Some(b"hello".to_vec()));
        assert_eq!(decode_base64("aGVsbG8hIQ=="), Some(b"hello!!".to_vec()));
        assert_eq!(decode_base64("aGk/"), Some(b"hi?".to_vec()));
        assert_eq!(decode_base64("a*=="), None);
    }

    #[test]
    fn the_browser_runs_hidden_with_a_clear_background() {
        let a = browser_args(Path::new("C:/p"), (1280, 720)).join(" ");
        assert!(a.contains("--headless=new") && a.contains("--window-size=1280,720") && !a.contains("--default-background-color") && a.contains("--user-data-dir=C:/p"), "{a}");
    }

    #[test]
    fn a_screencast_picture_is_fitted_and_premultiplied() {
        let img = image::RgbaImage::from_fn(4, 2, |x, _| if x < 2 { image::Rgba([200, 0, 0, 255]) } else { image::Rgba([0, 0, 200, 128]) });
        let mut png = std::io::Cursor::new(vec![]);
        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let text = base64_of(png.get_ref());
        let px = picture(&text, (4, 4)).unwrap();
        assert_eq!(px.len(), 64);
        assert_eq!(&px[..4], &[0, 0, 0, 0], "clear above the fitted picture");
        assert_eq!(&px[16..20], &[200, 0, 0, 255]);
        assert_eq!(&px[28..32], &[0, 0, 100, 128], "half see-through blue, premultiplied");
    }

    fn base64_of(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in bytes.chunks(3) {
            let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
            for i in 0..4 { if i <= c.len() { s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char) } else { s.push('=') } }
        }
        s
    }

    /// The computer's own browser shows a page: red, with a white box top left. Skipped without a browser.
    #[test]
    fn a_web_page_is_shown_from_the_computers_browser() {
        if find_browser().is_none() { eprintln!("no browser here: skipped"); return }
        let page = "data:text/html,<html><body style='margin:0;background:red'><div style='width:50%;height:50%;background:white'></div></body></html>";
        let p = Page::start(page, (160, 90)).unwrap();
        let began = Instant::now();
        let mut got = None;
        while began.elapsed() < START_LIMIT + Duration::from_secs(20) {
            // The first picture may be the blank page before the address opens. That page is white, and white has
            // as much red as red has: it is the page only once there is no green there.
            if let Some(px) = p.picture() && px[(60 * 160 + 120) * 4] > 200 && px[(60 * 160 + 120) * 4 + 1] < 60 { got = Some(px); break }
            std::thread::sleep(Duration::from_millis(200));
        }
        drop(p);
        let px = got.expect("a picture of the page");
        assert_eq!(px.len(), 160 * 90 * 4);
        let at = |x: usize, y: usize| px[(y * 160 + x) * 4..(y * 160 + x) * 4 + 4].to_vec();
        assert!(at(120, 60)[0] > 200 && at(120, 60)[1] < 60 && at(120, 60)[3] == 255, "red: {:?}", at(120, 60));
        assert!(at(20, 20).iter().all(|c| *c > 200), "white: {:?}", at(20, 20));
    }
}
