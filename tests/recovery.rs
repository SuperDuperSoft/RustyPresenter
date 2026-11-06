//! Crash recovery end to end (Plan 4 Task 11): the real binary, supervised, with the headless room.
use rustypresenter::{live::{Command as LiveCommand, Envelope, Inst, AudioLayer}, model::{ItemKind, ServiceItem}, recovery::{exitcode, state}, store::Store};
use std::{fs, io::{Read, Write}, net::{Ipv4Addr, TcpListener, TcpStream}, path::{Path, PathBuf}, process::{Child, Command, ExitStatus, Stdio}, thread, time::{Duration, Instant}};
use uuid::Uuid;

const EXE: &str = env!("CARGO_BIN_EXE_rustypresenter");

fn free_port() -> u16 { TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap().local_addr().unwrap().port() }

fn until(secs: f32, what: &str, done: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs_f32(secs);
    while !done() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

fn kill(pid: u32) {
    #[cfg(unix)]
    unsafe { libc::kill(pid as i32, libc::SIGKILL); }
    #[cfg(windows)]
    { let _ = Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status(); }
}

/// A data folder and a port, with the supervised app when it is started. Dropping it stops both.
struct Rig { data: PathBuf, port: u16, sup: Option<Child> }

impl Rig {
    fn new() -> Rig {
        let data = std::env::temp_dir().join(format!("rp-recovery-{}", Uuid::new_v4()));
        fs::create_dir_all(&data).unwrap();
        Rig { data, port: free_port(), sup: None }
    }

    fn supervisor(&self, args: &[&str], env: &[(&str, &str)]) -> Child {
        Command::new(EXE).args(args).env("RUSTYPRESENTER_HEADLESS", "1").env("RUSTYPRESENTER_DATA", &self.data)
            .env("RUSTYPRESENTER_PORT", self.port.to_string()).envs(env.iter().copied())
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap()
    }

    fn start(&mut self, args: &[&str], env: &[(&str, &str)]) { self.sup = Some(self.supervisor(args, env)); }

    /// Starts and waits until the server answers.
    fn up(&mut self) { self.up_with(&[]) }

    fn up_with(&mut self, env: &[(&str, &str)]) {
        self.start(&[], env);
        until(10.0, "the app to answer", || self.pid().is_some() && self.try_get("/remote/status").is_some());
    }

    /// Stops the supervisor and then the app, as if the computer had been switched off.
    fn stop(&mut self) {
        if let Some(mut s) = self.sup.take() { let _ = s.kill(); let _ = s.wait(); }
        if let Some(pid) = self.pid() { kill(pid); }
        let _ = fs::remove_file(self.data.join("app.pid"));
        until(5.0, "the port to come free", || TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).is_err());
    }

    fn import_deck(&self, path: &Path) {
        let body = serde_json::json!({ "path": path, "purpose": "powerpoint" }).to_string();
        self.request("POST", "/api/do/import/path", &body).expect("import/path");
    }

    /// The slide counts of the decks on disk.
    fn decks(&self) -> Vec<usize> {
        let data: serde_json::Value = serde_json::from_slice(&fs::read(self.data.join("data.json")).unwrap()).unwrap();
        data["decks"].as_array().unwrap().iter().map(|d| d["slides"].as_array().unwrap().len()).collect()
    }

    /// Everything in `tmp/`.
    fn tmp(&self) -> Vec<PathBuf> { fs::read_dir(self.data.join("tmp")).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default() }

    fn pid(&self) -> Option<u32> { fs::read_to_string(self.data.join("app.pid")).ok()?.trim().parse().ok() }

    fn request(&self, method: &str, path: &str, body: &str) -> Option<String> {
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        let head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nx-rp: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", self.port, body.len());
        s.write_all(head.as_bytes()).ok()?;
        s.write_all(body.as_bytes()).ok()?;
        let mut out = String::new();
        s.read_to_string(&mut out).ok()?;
        out.starts_with("HTTP/1.1 200").then_some(out)
    }

    fn try_get(&self, path: &str) -> Option<String> { self.request("GET", path, "") }

    /// The live view.
    fn live(&self) -> String { self.request("POST", "/api/do/live", "{}").expect("live") }

    /// Adds a text item and shows it.
    fn show_text(&self, text: &str) {
        let kind = ItemKind::Text { title: "t".into(), body: text.into() };
        let item = ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None };
        let added = self.request("POST", "/api/do/service/add", &serde_json::json!({ "item": item }).to_string()).expect("service/add");
        let reply: serde_json::Value = serde_json::from_str(added.split("\r\n\r\n").nth(1).unwrap().lines().find(|l| l.starts_with('{')).unwrap()).unwrap();
        let item_id: Uuid = serde_json::from_value(reply["id"].clone()).unwrap();
        let env = Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: LiveCommand::Show { item_id, slide: 0 } };
        self.request("POST", "/api/do/command", &serde_json::to_string(&env).unwrap()).expect("command");
        until(5.0, "the frame to be saved", || fs::read_to_string(state::path(&self.data)).is_ok_and(|s| s.contains(text)));
    }

    fn log(&self) -> String {
        let Ok(dir) = fs::read_dir(self.data.join("logs")) else { return String::new() };
        dir.flatten().map(|f| fs::read_to_string(f.path()).unwrap_or_default()).collect()
    }

    fn backups(&self) -> Vec<String> {
        fs::read_dir(self.data.join("backups")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("backup-")).collect()
    }

    fn wait(&mut self, secs: f32) -> ExitStatus {
        let sup = self.sup.as_mut().expect("started");
        let end = Instant::now() + Duration::from_secs_f32(secs);
        loop {
            if let Some(st) = sup.try_wait().unwrap() { return st; }
            assert!(Instant::now() < end, "the supervisor did not exit; log:\n{}", self.log());
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(mut s) = self.sup.take() { let _ = s.kill(); let _ = s.wait(); }
        if let Some(pid) = self.pid() { kill(pid); }
        let _ = fs::remove_dir_all(&self.data);
    }
}

/// Data with something in it, so a start makes a backup.
fn seed(root: &Path) {
    let store = Store::open(root.to_path_buf()).unwrap();
    let mut data = store.load().unwrap();
    let kind = ItemKind::Text { title: "seed".into(), body: "seed".into() };
    data.services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None });
    store.save(&data).unwrap();
}

/// A saved frame with a text slide and an audio layer, as `App` would leave it.
fn write_state(root: &Path, text: &str) {
    let json = serde_json::json!({
        "v": 1, "seq": 1, "saved_at": "now",
        "frame": { "background": null, "media": null, "logo": false,
            "slide": { "type": "text", "text": text, "footer": null, "reference": null, "scripture": false }, "audio": null },
        "cursor": null, "item": null, "positions": [], "windows": [],
    });
    let mut s: state::LiveState = serde_json::from_value(json).unwrap();
    s.frame.audio = Some(Inst { inst: 7, v: AudioLayer { asset: "a.mp3".into(), title: "A".into(), looped: false, paused: false } });
    fs::write(state::path(root), serde_json::to_vec(&s).unwrap()).unwrap();
}

#[test]
fn killed_app_is_restored_with_its_frame() {
    let mut rig = Rig::new();
    rig.up();
    rig.show_text("Amazing grace");
    let first = rig.pid().unwrap();
    kill(first);
    until(2.0, "a new pid", || rig.pid().is_some_and(|p| p != first));
    until(5.0, "the restore in the log", || rig.log().contains("restored version="));
    let log = rig.log();
    assert!(log.contains("--restore"), "{log}");
    assert!(log.contains("slide=\"Amazing grace\""), "{log}");
    assert!(log.contains("media=false"), "{log}");
}

#[test]
fn three_crashes_in_a_minute_stop_the_supervisor() {
    let mut rig = Rig::new();
    rig.start(&[], &[("RUSTYPRESENTER_TEST_CRASH", "panic"), ("RUSTYPRESENTER_TEST_CRASH_AT", "startup")]);
    assert_eq!(rig.wait(5.0).code(), Some(exitcode::GAVE_UP));
    assert!(rig.log().contains("gave up"), "{}", rig.log());
}

#[test]
fn second_attempt_restores_without_media() {
    let mut rig = Rig::new();
    write_state(&rig.data, "Still here");
    rig.start(&["--restore"], &[("RUSTYPRESENTER_TEST_CRASH", "panic"), ("RUSTYPRESENTER_TEST_CRASH_AT", "restore-media")]);
    until(10.0, "the third attempt's restore", || rig.log().contains("restored version="));
    let log = rig.log();
    assert!(log.contains("slide=\"Still here\"") && log.contains("media=false"), "{log}");
    assert!(rig.data.join("live-state.crashed.json").exists());
    let live = rig.live();
    assert!(live.contains("RustyPresenter restarted after a problem. Video/audio stopped — click it again to replay."), "{live}");
}

#[test]
fn restore_skips_startup_backup() {
    let mut rig = Rig::new();
    seed(&rig.data);
    rig.up();
    until(5.0, "the start-up backup", || rig.backups().len() == 1);
    rig.show_text("Once");
    let first = rig.pid().unwrap();
    kill(first);
    until(5.0, "the restore", || rig.log().contains("restored version="));
    thread::sleep(Duration::from_millis(1100)); // backup names go by the second
    assert_eq!(rig.backups().len(), 1, "{:?}", rig.backups());
}

#[test]
fn second_launch_exits_already_running() {
    let mut rig = Rig::new();
    rig.up();
    let first = rig.pid().unwrap();
    let mut second = rig.supervisor(&[], &[]);
    let end = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(st) = second.try_wait().unwrap() { break st; }
        assert!(Instant::now() < end, "the second launch did not exit");
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(exitcode::OK));
    assert_eq!(rig.pid(), Some(first));
    assert!(rig.try_get("/remote/status").is_some(), "the first copy is still there");
}

#[test]
fn port_taken_by_other_program_does_not_restart() {
    let mut rig = Rig::new();
    let other = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
    rig.port = other.local_addr().unwrap().port();
    rig.start(&[], &[]);
    assert_eq!(rig.wait(10.0).code(), Some(exitcode::PORT_TAKEN));
    assert_eq!(rig.log().matches("app exited").count(), 1, "{}", rig.log());
}

#[cfg(unix)]
#[test]
fn normal_quit_leaves_nothing_to_restore() {
    let mut rig = Rig::new();
    rig.up();
    rig.show_text("Goodnight");
    unsafe { libc::kill(rig.pid().unwrap() as i32, libc::SIGINT); }
    assert_eq!(rig.wait(5.0).code(), Some(exitcode::OK));
    assert!(!state::path(&rig.data).exists());
}

#[test]
fn normal_start_clears_old_state() {
    let mut rig = Rig::new();
    write_state(&rig.data, "Last week");
    rig.up();
    assert!(!state::path(&rig.data).exists());
    assert!(!rig.log().contains("restored version="), "{}", rig.log());
}

/// A Keynote package with one slide. `route` sends Keynote to LibreOffice, even on a PC with PowerPoint.
fn keynote(path: &Path) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for name in ["Index/Document.iwa", "Index/Slide.iwa"] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"iwa").unwrap();
    }
    zip.finish().unwrap();
}

/// One blank page, with no cross-reference table: pdfium rebuilds it.
const ONE_PAGE_PDF: &str = "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 320 180]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n";

/// The app's own executable as LibreOffice. On Unix a link named `soffice`, as `kill_leftover` only touches a group
/// that LibreOffice leads.
fn fake_soffice(dir: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        let link = dir.join("soffice");
        std::os::unix::fs::symlink(EXE, &link).unwrap();
        link
    }
    #[cfg(not(unix))]
    { let _ = dir; PathBuf::from(EXE) }
}

/// v1 §14.2. Needs pdfium, as without it an import stops before LibreOffice starts.
#[test]
fn killed_import_publishes_nothing() {
    let Ok(pdfium) = std::env::var("RP_PDFIUM") else { return eprintln!("skipped: RP_PDFIUM is not set") };
    let mut rig = Rig::new();
    let files = rig.data.join("files");
    fs::create_dir_all(&files).unwrap();
    let (input, fixture) = (files.join("Talk.key"), files.join("one.pdf"));
    keynote(&input);
    fs::write(&fixture, ONE_PAGE_PDF).unwrap();
    let soffice = fake_soffice(&files);
    let soffice = soffice.to_str().unwrap();

    rig.up_with(&[("RP_SOFFICE", soffice), ("RP_FAKE_SOFFICE", "hang"), ("RP_PDFIUM", &pdfium)]);
    rig.import_deck(&input);
    until(10.0, "LibreOffice to start", || rig.tmp().iter().any(|d| d.join("pdf").is_dir()) && has_group(&rig));
    #[cfg(unix)]
    let group: i32 = fs::read_to_string(rig.data.join("deps").join("libreoffice.pgid")).unwrap().split_whitespace().next().unwrap().parse().unwrap();

    let first = rig.pid().unwrap();
    kill(first);
    until(5.0, "the restart", || rig.pid().is_some_and(|p| p != first) && rig.try_get("/remote/status").is_some());
    until(5.0, "the work folder to go", || rig.tmp().is_empty());
    assert_eq!(rig.decks(), Vec::<usize>::new());
    // SAFETY: signal 0 only asks whether the group exists.
    #[cfg(unix)]
    until(5.0, "the fake LibreOffice to die", || unsafe { libc::kill(-group, 0) } != 0);

    rig.stop();
    let pdf = format!("pdf:{}", fixture.display());
    rig.up_with(&[("RP_SOFFICE", soffice), ("RP_FAKE_SOFFICE", &pdf), ("RP_PDFIUM", &pdfium)]);
    rig.import_deck(&input);
    until(15.0, "the second import", || rig.decks() == [1]);
    assert!(rig.tmp().is_empty(), "{:?}", rig.tmp());
}

/// The group is written just after the fake starts. Windows has a Job Object instead, with nothing to wait for.
fn has_group(rig: &Rig) -> bool { cfg!(not(unix)) || rig.data.join("deps").join("libreoffice.pgid").exists() }
