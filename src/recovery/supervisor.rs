//! The executable starts as this small supervisor: it runs itself with `--app`, restarts the app
//! after a crash with `--restore`, and gives up after three crashes in a minute.
use super::exitcode;
use std::{collections::VecDeque, ffi::OsString, fs, io::{self, Write}, path::Path, process::{Child, Command, ExitStatus, Stdio}, time::{Duration, Instant}};

/// Windows' exit code for a console Ctrl+C or a closed console.
const CTRL_C: u32 = 0xC000013A;
const WINDOW: Duration = Duration::from_secs(60);
const MAX_CRASHES: usize = 3;
const RESTART_PAUSE: Duration = Duration::from_millis(250);

#[derive(Debug, PartialEq)]
pub enum Exit { Stop(i32), Crash(String) }

pub fn classify(st: ExitStatus) -> Exit {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        match st.signal() {
            Some(libc::SIGTERM | libc::SIGHUP | libc::SIGINT) => return Exit::Stop(exitcode::OK),
            Some(s) => return Exit::Crash(format!("signal={s}")),
            None => {}
        }
    }
    st.code().map_or(Exit::Crash("no exit code".into()), from_code)
}

fn from_code(code: i32) -> Exit {
    match code {
        exitcode::OK | exitcode::CONFIG | exitcode::ALREADY_RUNNING | exitcode::PORT_TAKEN => Exit::Stop(code),
        c if c as u32 == CTRL_C => Exit::Stop(exitcode::OK),
        c => Exit::Crash(format!("code=0x{:08X}", c as u32)),
    }
}

#[derive(Debug, PartialEq)]
pub enum Next { Restart { attempt: u32 }, GiveUp }

/// Counts crashes in the last minute.
#[derive(Default)]
pub struct Guard { crashes: VecDeque<Instant> }

impl Guard {
    pub fn record(&mut self, now: Instant) -> Next {
        self.crashes.retain(|t| now.duration_since(*t) < WINDOW);
        self.crashes.push_back(now);
        if self.crashes.len() >= MAX_CRASHES { Next::GiveUp } else { Next::Restart { attempt: self.crashes.len() as u32 } }
    }
}

/// The app's arguments on each attempt: the first restart restores everything, the second leaves
/// out what plays, in case that is what crashed.
pub fn app_args(attempt: u32, passthrough: &[OsString]) -> Vec<OsString> {
    let restoring = passthrough.iter().any(|a| a == "--restore");
    let mut args = vec![];
    if attempt >= 1 && !restoring { args.push("--restore".into()); }
    if attempt >= 2 { args.extend(["--attempt".into(), attempt.to_string().into()]); }
    args.extend_from_slice(passthrough);
    args
}

#[derive(Debug, PartialEq, Default)]
pub struct Args { pub app: bool, pub restore: bool, pub attempt: u32, pub no_browser: bool, pub rest: Vec<OsString> }

pub fn parse_args(mut it: impl Iterator<Item = OsString>) -> Args {
    let mut a = Args::default();
    while let Some(arg) = it.next() {
        match arg.to_str() {
            Some("--app") => a.app = true,
            Some("--restore") => a.restore = true,
            Some("--no-browser") => a.no_browser = true,
            Some("--attempt") => a.attempt = it.next().and_then(|n| n.to_str()?.parse().ok()).unwrap_or(0),
            _ => a.rest.push(arg),
        }
    }
    a
}

/// What the loop needs from the world.
trait Host {
    fn launch(&mut self, args: &[OsString]) -> io::Result<ExitStatus>;
    fn log(&mut self, line: &str);
    fn give_up(&mut self);
    fn shutting_down(&self) -> bool;
}

fn stamp() -> String { chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string() }

fn supervise(host: &mut impl Host, passthrough: &[OsString], pause: Duration) -> i32 {
    let mut guard = Guard::default();
    let mut attempt = 0;
    loop {
        let status = match host.launch(&app_args(attempt, passthrough)) {
            Ok(s) => s,
            Err(e) => {
                host.log(&format!("{} could not start the app: {e}", stamp()));
                return exitcode::CONFIG;
            }
        };
        match classify(status) {
            Exit::Stop(code) => {
                host.log(&format!("{} app exited: stop code={code}", stamp()));
                return if code == exitcode::ALREADY_RUNNING { exitcode::OK } else { code };
            }
            Exit::Crash(how) => host.log(&format!("{} app exited: {how}", stamp())),
        }
        match guard.record(Instant::now()) {
            Next::GiveUp => {
                host.log(&format!("{} gave up after 3 crashes", stamp()));
                host.give_up();
                return exitcode::GAVE_UP;
            }
            Next::Restart { attempt: a } => attempt = a,
        }
        std::thread::sleep(pause);
        if host.shutting_down() { return exitcode::OK; }
    }
}

fn append(log: &Path) -> io::Result<fs::File> {
    if let Some(dir) = log.parent() { fs::create_dir_all(dir)?; }
    fs::OpenOptions::new().create(true).append(true).open(log)
}

/// The app's output goes to the log. Logging is best effort: a log that cannot be opened (a read-only or missing
/// data folder) must not stop the app, which then explains the folder problem itself.
fn app_output(log: &Path) -> (Stdio, Stdio) {
    match append(log).and_then(|out| Ok((out.try_clone()?, out))) {
        Ok((out, err)) => (out.into(), err.into()),
        Err(e) => {
            eprintln!("RustyPresenter could not open its log {}: {e}", log.display());
            (Stdio::null(), Stdio::null())
        }
    }
}

struct Real<'a> { exe: &'a Path, log: &'a Path }

impl Host for Real<'_> {
    fn launch(&mut self, args: &[OsString]) -> io::Result<ExitStatus> {
        let (stdout, stderr) = app_output(self.log);
        let mut child = Command::new(self.exe).arg("--app").args(args).env("RUSTYPRESENTER_SUPERVISED", "1")
            .stdin(Stdio::null()).stdout(stdout).stderr(stderr).spawn()?;
        forward_to(&child);
        let status = child.wait();
        forget_child();
        status
    }

    fn log(&mut self, line: &str) {
        if let Ok(mut f) = append(self.log) { let _ = writeln!(f, "{line}"); }
    }

    fn give_up(&mut self) { give_up_dialog(self.log); }

    fn shutting_down(&self) -> bool { shutting_down() }
}

/// Runs the app until it stops for good. Returns the supervisor's own exit code.
pub fn run(exe: &Path, log: &Path, passthrough: &[OsString]) -> i32 {
    watch_signals();
    supervise(&mut Real { exe, log }, passthrough, RESTART_PAUSE)
}

#[cfg(unix)]
static STOPPING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(unix)]
static CHILD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// A logout or `kill` reaches the supervisor: remember it and pass it on, so nothing relaunches.
#[cfg(unix)]
fn watch_signals() {
    use std::sync::atomic::Ordering;
    for signal in [libc::SIGTERM, libc::SIGHUP] {
        // Only atomics and `kill`, which are safe in a signal handler.
        let hooked = unsafe {
            signal_hook_registry::register(signal, move || {
                STOPPING.store(true, Ordering::SeqCst);
                let child = CHILD.load(Ordering::SeqCst);
                if child > 0 { libc::kill(child, libc::SIGTERM); }
            })
        };
        if let Err(e) = hooked { eprintln!("RustyPresenter could not watch for shutdown: {e}"); }
    }
}

/// Publishes the new app's pid for the signal handler. A signal that came just before then reached only the
/// old app, so the new one is stopped here.
#[cfg(unix)]
fn forward_to(child: &Child) {
    use std::sync::atomic::Ordering;
    let pid = child.id() as i32;
    CHILD.store(pid, Ordering::SeqCst);
    if STOPPING.load(Ordering::SeqCst) { unsafe { libc::kill(pid, libc::SIGTERM); } }
}

/// The app has been waited for, so its pid may be reused: never signal it again.
#[cfg(unix)]
fn forget_child() { CHILD.store(0, std::sync::atomic::Ordering::SeqCst); }

#[cfg(not(unix))]
fn watch_signals() {}

#[cfg(not(unix))]
fn forward_to(_child: &Child) {}

#[cfg(not(unix))]
fn forget_child() {}

#[cfg(windows)]
pub fn shutting_down() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SHUTTINGDOWN};
    unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) != 0 }
}

#[cfg(unix)]
pub fn shutting_down() -> bool { STOPPING.load(std::sync::atomic::Ordering::SeqCst) }

#[cfg(not(any(windows, unix)))]
pub fn shutting_down() -> bool { false }

/// No Windows crash dialog for the volunteer to click away: the supervisor restarts the app.
pub fn quiet_crash_dialogs() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX, SetErrorMode};
        unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX); }
    }
}

fn give_up_text(log: &Path) -> String {
    format!("RustyPresenter stopped after crashing 3 times in a minute. The log is at {}.", log.display())
}

/// Tells the operator the app will not come back, and waits for them. Windows calls `MessageBoxW` directly, not
/// `rfd`, whose dialog needs the application manifest: this box is what shows when the app is at its most broken.
pub fn give_up_dialog(log: &Path) {
    if std::env::var_os("RUSTYPRESENTER_HEADLESS").is_some_and(|v| v == "1") { return; }
    let text = give_up_text(log);
    #[cfg(not(windows))]
    crate::platform::dialog::error(&text);
    #[cfg(windows)]
    {
        eprintln!("{text}");
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        unsafe { MessageBoxW(std::ptr::null_mut(), wide(&text).as_ptr(), wide("RustyPresenter").as_ptr(), MB_OK | MB_ICONERROR); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(v: &[&str]) -> Vec<OsString> { v.iter().map(OsString::from).collect() }

    #[cfg(windows)]
    fn status(code: u32) -> ExitStatus { use std::os::windows::process::ExitStatusExt; ExitStatus::from_raw(code) }
    #[cfg(unix)]
    fn status(code: u32) -> ExitStatus { use std::os::unix::process::ExitStatusExt; ExitStatus::from_raw((code as i32) << 8) }

    #[test]
    fn classify_normal_codes_stop() {
        for c in [0, 2, 3, 4] { assert_eq!(classify(status(c)), Exit::Stop(c as i32)); }
    }

    #[test]
    fn classify_a_panic_as_a_crash() {
        assert!(matches!(classify(status(101)), Exit::Crash(_)));
    }

    #[test]
    fn windows_crash_codes_crash() {
        assert_eq!(from_code(0xC0000005u32 as i32), Exit::Crash("code=0xC0000005".into()));
        assert_eq!(from_code(0xC0000409u32 as i32), Exit::Crash("code=0xC0000409".into()));
    }

    #[test]
    fn windows_ctrl_c_stops() {
        assert_eq!(from_code(0xC000013Au32 as i32), Exit::Stop(exitcode::OK));
    }

    #[cfg(windows)]
    #[test]
    fn classify_access_violation_and_fastfail_crash() {
        assert_eq!(classify(status(0xC0000005)), Exit::Crash("code=0xC0000005".into()));
        assert_eq!(classify(status(0xC0000409)), Exit::Crash("code=0xC0000409".into()));
    }

    #[cfg(windows)]
    #[test]
    fn classify_ctrl_c_stops() {
        assert_eq!(classify(status(0xC000013A)), Exit::Stop(exitcode::OK));
    }

    #[cfg(unix)]
    #[test]
    fn classify_signals() {
        use std::os::unix::process::ExitStatusExt;
        for s in [libc::SIGSEGV, libc::SIGABRT, libc::SIGKILL] {
            assert_eq!(classify(ExitStatus::from_raw(s)), Exit::Crash(format!("signal={s}")));
        }
        for s in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
            assert_eq!(classify(ExitStatus::from_raw(s)), Exit::Stop(exitcode::OK));
        }
    }

    #[test]
    fn guard_gives_up_on_third_crash_within_60s() {
        let (mut g, t) = (Guard::default(), Instant::now());
        assert_eq!(g.record(t), Next::Restart { attempt: 1 });
        assert_eq!(g.record(t + Duration::from_secs(20)), Next::Restart { attempt: 2 });
        assert_eq!(g.record(t + Duration::from_secs(59)), Next::GiveUp);
    }

    #[test]
    fn guard_forgets_crashes_older_than_60s() {
        let (mut g, t) = (Guard::default(), Instant::now());
        g.record(t);
        g.record(t + Duration::from_secs(30));
        assert_eq!(g.record(t + Duration::from_secs(61)), Next::Restart { attempt: 2 });
        assert_eq!(g.record(t + Duration::from_secs(200)), Next::Restart { attempt: 1 });
    }

    #[test]
    fn app_args_escalate() {
        let rest = os(&["--no-browser"]);
        assert_eq!(app_args(0, &rest), os(&["--no-browser"]));
        assert_eq!(app_args(1, &rest), os(&["--restore", "--no-browser"]));
        assert_eq!(app_args(2, &rest), os(&["--restore", "--attempt", "2", "--no-browser"]));
    }

    #[test]
    fn app_args_do_not_repeat_restore() {
        let rest = os(&["--restore"]);
        assert_eq!(app_args(0, &rest), os(&["--restore"]));
        assert_eq!(app_args(1, &rest), os(&["--restore"]));
        assert_eq!(app_args(2, &rest), os(&["--attempt", "2", "--restore"]));
    }

    #[test]
    fn parse_args_cases() {
        assert_eq!(parse_args(os(&[]).into_iter()), Args::default());
        let a = parse_args(os(&["--app", "--restore", "--attempt", "2", "--no-browser", "song.txt"]).into_iter());
        assert_eq!(a, Args { app: true, restore: true, attempt: 2, no_browser: true, rest: os(&["song.txt"]) });
    }

    #[test]
    fn parse_args_with_a_bad_attempt_means_zero() {
        assert_eq!(parse_args(os(&["--attempt", "x", "--app"]).into_iter()).attempt, 0);
        assert_eq!(parse_args(os(&["--attempt"]).into_iter()).attempt, 0);
        assert!(parse_args(os(&["--attempt", "x", "--app"]).into_iter()).app);
    }

    #[derive(Default)]
    struct Fake { exits: VecDeque<u32>, launched: Vec<Vec<OsString>>, lines: Vec<String>, gave_up: bool, down_after: Option<usize>, fail: bool }

    impl Host for Fake {
        fn launch(&mut self, args: &[OsString]) -> io::Result<ExitStatus> {
            if self.fail { return Err(io::ErrorKind::NotFound.into()); }
            self.launched.push(args.to_vec());
            Ok(status(self.exits.pop_front().expect("no more exits scripted")))
        }
        fn log(&mut self, line: &str) { self.lines.push(line.into()); }
        fn give_up(&mut self) { self.gave_up = true; }
        fn shutting_down(&self) -> bool { self.down_after.is_some_and(|n| self.launched.len() >= n) }
    }

    fn fake(exits: &[u32]) -> Fake { Fake { exits: exits.iter().copied().collect(), ..Fake::default() } }
    fn go(f: &mut Fake, rest: &[&str]) -> i32 { supervise(f, &os(rest), Duration::ZERO) }

    #[test]
    fn give_up_text_names_the_log() {
        assert_eq!(give_up_text(Path::new("x.log")), "RustyPresenter stopped after crashing 3 times in a minute. The log is at x.log.");
    }

    #[test]
    fn normal_quit_stops_at_once() {
        let mut f = fake(&[0]);
        assert_eq!(go(&mut f, &["--no-browser"]), exitcode::OK);
        assert_eq!(f.launched, [os(&["--no-browser"])]);
    }

    #[test]
    fn already_running_is_a_success_for_the_supervisor() {
        assert_eq!(go(&mut fake(&[3]), &[]), exitcode::OK);
    }

    #[test]
    fn config_and_port_errors_are_passed_on_without_a_restart() {
        let mut f = fake(&[2]);
        assert_eq!(go(&mut f, &[]), exitcode::CONFIG);
        assert_eq!(f.launched.len(), 1);
        let mut f = fake(&[4]);
        assert_eq!(go(&mut f, &[]), exitcode::PORT_TAKEN);
        assert_eq!(f.launched.len(), 1);
    }

    #[test]
    fn a_crash_restarts_with_restore_and_then_without_media() {
        let mut f = fake(&[101, 101, 0]);
        assert_eq!(go(&mut f, &[]), exitcode::OK);
        assert_eq!(f.launched, [os(&[]), os(&["--restore"]), os(&["--restore", "--attempt", "2"])]);
    }

    #[test]
    fn three_crashes_give_up_and_say_so() {
        let mut f = fake(&[101, 101, 101]);
        assert_eq!(go(&mut f, &[]), exitcode::GAVE_UP);
        assert_eq!(f.launched.len(), 3);
        assert!(f.gave_up);
        assert!(f.lines.last().unwrap().contains("gave up after 3 crashes"), "{:?}", f.lines);
    }

    #[test]
    fn every_exit_is_logged() {
        let mut f = fake(&[101, 0]);
        go(&mut f, &[]);
        assert_eq!(f.lines.len(), 2, "{:?}", f.lines);
        assert!(f.lines[0].contains("app exited: code=0x00000065"), "{:?}", f.lines);
        assert!(f.lines[1].contains("app exited: stop code=0"), "{:?}", f.lines);
    }

    #[test]
    fn no_restart_during_shutdown() {
        let mut f = fake(&[101, 0]);
        f.down_after = Some(1);
        assert_eq!(go(&mut f, &[]), exitcode::OK);
        assert_eq!(f.launched.len(), 1);
    }

    /// A log that cannot be opened must not stop the app: here the log's path is a folder.
    #[test]
    fn the_app_starts_when_its_log_cannot_be_opened() {
        let dir = std::env::temp_dir().join(format!("rp-supervisor-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        assert!(append(&dir).is_err());
        // This test binary stands in for the app: it refuses `--app` and exits, which shows it was started.
        let exe = std::env::current_exe().unwrap();
        let status = Real { exe: &exe, log: &dir }.launch(&os(&["--list"]));
        fs::remove_dir_all(&dir).unwrap();
        assert!(status.is_ok(), "{status:?}");
    }

    #[test]
    fn an_app_that_cannot_start_is_logged_not_retried() {
        let mut f = fake(&[]);
        f.fail = true;
        assert_eq!(go(&mut f, &[]), exitcode::CONFIG);
        assert!(f.lines[0].contains("could not start the app"), "{:?}", f.lines);
    }
}
