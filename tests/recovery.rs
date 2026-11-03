//! The supervisor and the app as real processes (Plan 4 Task 11): a killed app comes back with its frame,
//! a crashing one stops after three tries, and a second launch or a taken port exits calmly. The app runs
//! headless (`RUSTYPRESENTER_HEADLESS=1`): no windows, no player, the same core and server.
use rustypresenter::{live::{AudioLayer, Cursor, Frame, Inst, LiveItem, SlideLayer}, model::{Background, ItemKind, ServiceItem, Slide, SlideContent}, recovery::{exitcode, state::{self, LiveState}, strings}, store::Store};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};
use uuid::Uuid;

const EXE: &str = env!("CARGO_BIN_EXE_rustypresenter-native");

fn free_port() -> u16 { TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port() }

fn wait_for(limit: Duration, done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if done() { return true; }
        std::thread::sleep(Duration::from_millis(25));
    }
    done()
}

/// A supervisor on its own data folder and port. Dropping it stops the supervisor and its app and removes the folder.
struct Run { dir: PathBuf, port: u16, sup: Child }

impl Run {
    fn new() -> (PathBuf, u16) {
        let dir = std::env::temp_dir().join(format!("rp-recovery-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        (dir, free_port())
    }

    fn start(args: &[&str], envs: &[(&str, &str)]) -> Run {
        let (dir, port) = Run::new();
        Run::start_in(dir, port, args, envs)
    }

    fn start_in(dir: PathBuf, port: u16, args: &[&str], envs: &[(&str, &str)]) -> Run {
        let sup = supervisor(&dir, port, args, envs);
        Run { dir, port, sup }
    }

    fn pid(&self) -> Option<u32> { std::fs::read_to_string(self.dir.join("app.pid")).ok()?.trim().parse().ok() }

    /// The app is up and its server answers.
    fn ready(&self) -> u32 {
        assert!(wait_for(Duration::from_secs(10), || self.pid().is_some() && get(self.port, "/api/state").0 == 200), "the app did not start: {}", self.log());
        self.pid().unwrap()
    }

    fn log(&self) -> String { log(&self.dir) }

    fn exit_within(&mut self, limit: Duration) -> Option<ExitStatus> {
        let end = Instant::now() + limit;
        while Instant::now() < end {
            if let Some(st) = self.sup.try_wait().unwrap() { return Some(st); }
            std::thread::sleep(Duration::from_millis(25));
        }
        self.sup.try_wait().unwrap()
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        stop_tree(&mut self.sup);
        // The killed app may hold its files for a moment.
        let _ = wait_for(Duration::from_secs(3), || std::fs::remove_dir_all(&self.dir).is_ok() || !self.dir.exists());
    }
}

/// The supervisor runs in a process group of its own on Unix, so the app it starts can be stopped with it.
fn supervisor(dir: &Path, port: u16, args: &[&str], envs: &[(&str, &str)]) -> Child {
    let mut cmd = Command::new(EXE);
    cmd.args(args).env("RUSTYPRESENTER_DATA", dir).env("RUSTYPRESENTER_PORT", port.to_string()).env("RUSTYPRESENTER_HEADLESS", "1")
        .env_remove("RUSTYPRESENTER_TEST_CRASH").env_remove("RUSTYPRESENTER_TEST_CRASH_AT").envs(envs.iter().copied())
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd.spawn().unwrap()
}

/// Stops the supervisor and everything it started; never by a pid read from a file, which may be reused.
fn stop_tree(sup: &mut Child) {
    #[cfg(windows)]
    let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &sup.id().to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    // SAFETY: `kill` takes no pointers; the group is the supervisor's own (started with `process_group(0)`).
    #[cfg(unix)]
    unsafe { libc::kill(-(sup.id() as i32), libc::SIGKILL); }
    let _ = sup.kill();
    let _ = sup.wait();
}

/// Kills the app as Task Manager's End task or `kill -9` would.
fn kill(pid: u32) {
    #[cfg(windows)]
    assert!(Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).stdout(Stdio::null()).status().unwrap().success());
    // SAFETY: `kill` takes no pointers; `pid` is the app this test's supervisor started a moment ago.
    #[cfg(unix)]
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGKILL) }, 0);
}

/// Every log in the data folder, oldest first (a test may run across midnight).
fn log(dir: &Path) -> String {
    let mut files: Vec<_> = std::fs::read_dir(dir.join("logs")).into_iter().flatten().flatten().map(|e| e.path()).collect();
    files.sort();
    files.iter().map(|f| String::from_utf8_lossy(&std::fs::read(f).unwrap_or_default()).into_owned()).collect()
}

/// One loopback HTTP request: (status, body).
fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_secs(1)) else { return (0, String::new()) };
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nx-rp: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    if s.write_all(req.as_bytes()).is_err() { return (0, String::new()); }
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).into_owned();
    let status = text.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, text.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default())
}

fn get(port: u16, path: &str) -> (u16, String) { http(port, "GET", path, "") }

fn post(port: u16, action: &str, body: Value) -> Value {
    let (status, text) = http(port, "POST", &format!("/api/do/{action}"), &body.to_string());
    assert_eq!(status, 200, "{action}: {text}");
    serde_json::from_str(&text).unwrap()
}

/// The live view, as the first message of the operator's event stream.
fn live(port: u16) -> Value {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(s, "GET /events?role=operator HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").unwrap();
    let (mut seen, mut buf) = (String::new(), [0u8; 4096]);
    loop {
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0, "the event stream ended: {seen}");
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
        if let Some(line) = seen.split_once("data:").and_then(|(_, rest)| rest.split_once('\n')).map(|(l, _)| l.trim().to_string()) {
            return serde_json::from_str(&line).unwrap();
        }
    }
}

/// Adds a text item and takes it live; returns once the headless room committed it and the state is on disk.
fn show_text(run: &Run, body: &str) {
    let id = post(run.port, "service/add", json!({ "item": { "kind": "text", "title": "Welcome", "body": body } }))["id"].clone();
    let base = live(run.port)["version"].as_u64().unwrap();
    let out = post(run.port, "command", json!({ "id": Uuid::new_v4().to_string(), "base": base, "cmd": "show", "item_id": id, "slide": 0 }));
    assert_eq!(out["ok"], true, "{out}");
    let version = out["version"].as_u64().unwrap();
    assert!(wait_for(Duration::from_secs(5), || live(run.port)["committed"].as_u64() == Some(version)), "not committed");
    assert!(wait_for(Duration::from_secs(5), || state::load(&run.dir).is_some_and(|s| s.frame.slide.is_some())), "the state was not saved");
}

/// Waits for the supervisor to start a new app after `old` was killed.
fn new_pid(run: &Run, old: u32, limit: Duration) -> Option<u32> {
    wait_for(limit, || run.pid().is_some_and(|p| p != old)).then(|| run.pid().unwrap())
}

#[test]
fn killed_app_is_restored_with_its_frame() {
    let run = Run::start(&[], &[]);
    run.ready();
    show_text(&run, "Hello again");
    let old = run.pid().unwrap();
    kill(old);
    assert!(new_pid(&run, old, Duration::from_secs(2)).is_some(), "no new app within 2 s: {}", run.log());
    assert!(wait_for(Duration::from_secs(5), || run.log().contains("restored version=")), "{}", run.log());
    let log = run.log();
    assert!(log.contains("--restore"), "{log}");
    assert!(log.contains(r#"slide="Hello again" media=false"#), "{log}");
    assert_eq!(live(run.port)["notice"], strings::RESTARTED);
}

#[test]
fn three_crashes_in_a_minute_stop_the_supervisor() {
    let mut run = Run::start(&[], &[("RUSTYPRESENTER_TEST_CRASH", "panic"), ("RUSTYPRESENTER_TEST_CRASH_AT", "startup")]);
    let st = run.exit_within(Duration::from_secs(5)).unwrap_or_else(|| panic!("the supervisor is still running: {}", run.log()));
    assert_eq!(st.code(), Some(exitcode::GAVE_UP));
    assert!(run.log().contains("gave up"), "{}", run.log());
}

/// The saved state: a text slide, with an audio layer that crashes each restore that plays it.
fn saved_with_audio() -> LiveState {
    let item_id = Uuid::new_v4();
    let slide = Slide { label: "Verse 1".into(), content: SlideContent::Text { text: "Amazing grace".into(), footer: None, reference: None }, notes: String::new() };
    let frame = Frame {
        slide: Some(SlideLayer::Text { text: "Amazing grace".into(), footer: None, reference: None, scripture: false }),
        audio: Some(Inst { inst: 7, v: AudioLayer { asset: "a.mp3".into(), title: "Hymn".into(), looped: false, paused: false } }),
        ..Frame::default()
    };
    LiveState {
        v: 1, seq: 3, saved_at: "now".into(), frame,
        cursor: Some(Cursor { item_id, item_index: 0, slide: 0 }),
        item: Some(LiveItem { item_id, title: "Amazing Grace".into(), kind: "song".into(), slides: vec![slide], background: Background::Color { color: "#000".into() } }),
        positions: vec![(7, 12.0)], windows: vec![],
    }
}

#[test]
fn second_attempt_restores_without_media() {
    let (dir, port) = Run::new();
    state::write_with_retry(&state::path(&dir), &serde_json::to_vec(&saved_with_audio()).unwrap()).unwrap();
    let run = Run::start_in(dir, port, &["--restore"], &[("RUSTYPRESENTER_TEST_CRASH", "panic"), ("RUSTYPRESENTER_TEST_CRASH_AT", "restore-media")]);
    assert!(wait_for(Duration::from_secs(10), || run.log().contains("restored version=")), "{}", run.log());
    let log = run.log();
    assert!(log.contains(r#"slide="Amazing grace" media=false"#), "{log}");
    assert!(log.contains("test crash at restore-media"), "{log}");
    assert_eq!(log.matches("PANIC thread=").count(), 2, "attempts 0 and 1 crashed: {log}");
    assert!(run.dir.join("live-state.crashed.json").exists());
    run.ready();
    assert_eq!(live(run.port)["notice"], strings::RESTARTED_NO_MEDIA);
}

/// Complete backups in the data folder.
fn backups(dir: &Path) -> usize {
    std::fs::read_dir(dir.join("backups")).into_iter().flatten().flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("backup-") && e.path().join("COMPLETE").exists()).count()
}

#[test]
fn restore_skips_startup_backup() {
    // Something worth backing up, so the first start makes its backup.
    let (dir, port) = Run::new();
    let store = Store::open(dir.clone()).unwrap();
    let mut data = store.load().unwrap();
    data.services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Hello".into() }, background: None, notes: String::new() });
    store.save(&data).unwrap();
    drop(store);
    let run = Run::start_in(dir, port, &[], &[]);
    let old = run.ready();
    assert!(wait_for(Duration::from_secs(5), || backups(&run.dir) == 1), "no start-up backup: {}", run.log());
    kill(old);
    assert!(new_pid(&run, old, Duration::from_secs(5)).is_some(), "{}", run.log());
    run.ready();
    // Backups are named by the second: give a restored start time to make a second one.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(backups(&run.dir), 1, "the restore made another backup");
}

#[test]
fn second_launch_exits_already_running() {
    let first = Run::start(&[], &[]);
    let pid = first.ready();
    let began = Instant::now();
    let mut second = supervisor(&first.dir, first.port, &[], &[]);
    let st = loop {
        if let Some(st) = second.try_wait().unwrap() { break st; }
        if began.elapsed() > Duration::from_secs(3) { stop_tree(&mut second); panic!("the second launch is still running: {}", first.log()); }
        std::thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(st.code(), Some(exitcode::OK));
    assert_eq!(first.pid(), Some(pid), "the first app is untouched");
    assert_eq!(get(first.port, "/api/state").0, 200);
}

#[test]
fn port_taken_by_other_program_does_not_restart() {
    let (dir, port) = Run::new();
    // A program that accepts connections and never answers.
    let _other = TcpListener::bind(("0.0.0.0", port)).unwrap();
    let mut run = Run::start_in(dir, port, &[], &[]);
    let st = run.exit_within(Duration::from_secs(10)).unwrap_or_else(|| panic!("the supervisor is still running: {}", run.log()));
    assert_eq!(st.code(), Some(exitcode::PORT_TAKEN));
    assert_eq!(run.log().matches("app exited").count(), 1, "started once: {}", run.log());
    assert!(run.log().contains(&strings::port_taken(port)), "{}", run.log());
}

#[test]
fn normal_start_clears_old_state() {
    let (dir, port) = Run::new();
    state::write_with_retry(&state::path(&dir), &serde_json::to_vec(&saved_with_audio()).unwrap()).unwrap();
    let run = Run::start_in(dir, port, &[], &[]);
    run.ready();
    assert!(wait_for(Duration::from_secs(3), || !state::path(&run.dir).exists()), "the old state is still there");
    assert!(!run.log().contains("restored"), "{}", run.log());
    assert_eq!(live(run.port)["notice"], Value::Null);
}
