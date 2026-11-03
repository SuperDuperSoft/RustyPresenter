//! The supervisor (spec §9): the executable started without `--app` runs only this loop. It launches
//! itself with `--app`, and restarts a crashed app with `--restore`, giving up after 3 crashes in 60 s.
use super::exitcode;
use std::{
    collections::VecDeque,
    ffi::{OsStr, OsString},
    fmt,
    fs::OpenOptions,
    io::{self, Write},
    path::Path,
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

/// How long crashes are remembered, and how many inside it end the restarts.
const WINDOW: Duration = Duration::from_secs(60);
const MAX_CRASHES: usize = 3;

/// What an `--app` exit means: stop with this code, or a crash (`"code=0xC0000005"`, `"signal=11"`).
#[derive(Debug, PartialEq)]
pub enum Exit {
    Stop(i32),
    Crash(String),
}

impl fmt::Display for Exit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Exit::Stop(c) => write!(f, "stop code={c}"),
            Exit::Crash(why) => write!(f, "crash {why}"),
        }
    }
}

/// 0, `CONFIG`, `ALREADY_RUNNING` and `PORT_TAKEN` stop, and so do Ctrl+C / console close
/// (`0xC000013A`) and SIGTERM, SIGHUP, SIGINT (a quit, logoff or shutdown). Everything else is a crash.
pub fn classify(st: ExitStatus) -> Exit {
    #[cfg(unix)]
    {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = st.signal() {
            return if [SIGTERM, SIGHUP, SIGINT].contains(&sig) { Exit::Stop(128 + sig) } else { Exit::Crash(format!("signal={sig}")) };
        }
    }
    let Some(code) = st.code() else { return Exit::Crash(format!("status={st}")) };
    match code {
        exitcode::OK | exitcode::CONFIG | exitcode::ALREADY_RUNNING | exitcode::PORT_TAKEN => Exit::Stop(code),
        #[cfg(windows)]
        c if c as u32 == 0xC000013A => Exit::Stop(c), // STATUS_CONTROL_C_EXIT
        // NTSTATUS codes read best in hex; small codes (`PANIC`, 1) in decimal.
        c if !(0..0x10000).contains(&c) => Exit::Crash(format!("code=0x{:08X}", c as u32)),
        c => Exit::Crash(format!("code={c}")),
    }
}

#[derive(Debug, PartialEq)]
pub enum Next {
    Restart { attempt: u32 },
    GiveUp,
}

/// Counts recent crashes, so a state that crashes the app on every start cannot loop forever.
#[derive(Default)]
pub struct Guard {
    crashes: VecDeque<Instant>,
}

impl Guard {
    /// Records a crash at `now`. The attempt is the number of crashes in the last 60 s (1 or 2);
    /// the third gives up.
    pub fn record(&mut self, now: Instant) -> Next {
        self.crashes.retain(|&t| now.saturating_duration_since(t) < WINDOW);
        self.crashes.push_back(now);
        match self.crashes.len() {
            n if n >= MAX_CRASHES => Next::GiveUp,
            n => Next::Restart { attempt: n as u32 },
        }
    }
}

/// The `--app` arguments for an attempt: 0 runs as asked, 1 restores, 2 restores without media.
/// A `--restore` already passed through is not repeated.
pub fn app_args(attempt: u32, passthrough: &[OsString]) -> Vec<OsString> {
    if attempt == 0 { return passthrough.to_vec() }
    let mut args: Vec<OsString> = vec!["--restore".into()];
    if attempt >= 2 { args.extend(["--attempt".into(), attempt.to_string().into()]) }
    args.extend(passthrough.iter().filter(|a| a.as_os_str() != OsStr::new("--restore")).cloned());
    args
}

/// The supervisor loop: runs `exe --app …` until it stops, restarting it after a crash. Returns the
/// supervisor's own exit code.
pub fn run(exe: &Path, log: &Path, passthrough: &[OsString]) -> i32 {
    quiet_crash_dialogs();
    #[cfg(unix)]
    unix::watch_signals();
    let mut guard = Guard::default();
    let mut attempt = 0;
    loop {
        let status = match launch(exe, log, &app_args(attempt, passthrough)) {
            Ok(st) => st,
            Err(e) => {
                note(log, &format!("could not start the app: {e}"));
                return exitcode::CONFIG;
            }
        };
        let exit = classify(status);
        note(log, &format!("app exited: {exit}"));
        if let Exit::Stop(code) = exit { return if code == exitcode::ALREADY_RUNNING { exitcode::OK } else { code } }
        match guard.record(Instant::now()) {
            Next::Restart { attempt: a } => attempt = a,
            Next::GiveUp => {
                note(log, "gave up after 3 crashes");
                give_up_dialog(log);
                return exitcode::GAVE_UP;
            }
        }
        std::thread::sleep(Duration::from_millis(250));
        // An app killed by a logoff or shutdown looks like a crash; never relaunch into one.
        if shutting_down() { return exitcode::OK }
    }
}

/// Starts the app with its output appended to the log, and waits for it.
fn launch(exe: &Path, log: &Path, args: &[OsString]) -> io::Result<ExitStatus> {
    if let Some(dir) = log.parent() { std::fs::create_dir_all(dir)?; }
    let out = OpenOptions::new().create(true).append(true).open(log)?;
    let err = out.try_clone()?;
    let mut child = Command::new(exe).arg("--app").args(args).env("RUSTYPRESENTER_SUPERVISED", "1")
        .stdin(Stdio::null()).stdout(out).stderr(err).spawn()?;
    #[cfg(unix)]
    unix::set_child(child.id() as i32);
    let status = child.wait();
    #[cfg(unix)]
    unix::set_child(0);
    status
}

/// Appends `"{time} {line}"` to the log; the supervisor has nowhere else to write.
fn note(log: &Path, line: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log) {
        let _ = writeln!(f, "{} {line}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}

/// Windows is logging off or shutting down, or (Unix) the supervisor was asked to stop.
#[cfg(windows)]
pub fn shutting_down() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SHUTTINGDOWN};
    unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) != 0 }
}

#[cfg(unix)]
pub fn shutting_down() -> bool { unix::STOPPING.load(std::sync::atomic::Ordering::SeqCst) }

#[cfg(not(any(windows, unix)))]
pub fn shutting_down() -> bool { false }

/// No Windows "has stopped working" or missing-disk boxes: a crash goes straight to the supervisor.
/// The mode is inherited by the app, and the app sets it again for itself.
pub fn quiet_crash_dialogs() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX, SetErrorMode};
        unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX); }
    }
}

/// Tells the operator the app will not come back, and where the log is. Not shown in headless tests.
// Plan 4 names `rfd::MessageDialog`; rfd arrives with Plan 3, so this uses `displays::error_box` until then.
pub fn give_up_dialog(log: &Path) {
    if std::env::var_os("RUSTYPRESENTER_HEADLESS").is_some_and(|v| v == "1") { return }
    crate::displays::error_box(&super::strings::gave_up(log));
}

/// SIGTERM, SIGHUP and SIGINT set the stop flag and are passed on to the app as SIGTERM, so a
/// logoff ends both processes and the app's exit is not taken for a crash.
#[cfg(unix)]
mod unix {
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    pub static STOPPING: AtomicBool = AtomicBool::new(false);
    static CHILD: AtomicI32 = AtomicI32::new(0);

    pub fn set_child(pid: i32) {
        CHILD.store(pid, Ordering::SeqCst);
        if pid > 0 && STOPPING.load(Ordering::SeqCst) { forward() } // the signal came during the spawn
    }

    fn forward() {
        let pid = CHILD.load(Ordering::SeqCst);
        if pid > 0 { unsafe { libc::kill(pid, libc::SIGTERM); } }
    }

    pub fn watch_signals() {
        use signal_hook::{consts::{SIGHUP, SIGINT, SIGTERM}, iterator::Signals};
        let Ok(mut signals) = Signals::new([SIGTERM, SIGHUP, SIGINT]) else { return };
        let _ = std::thread::Builder::new().name("supervisor-signals".into()).spawn(move || {
            for _ in signals.forever() {
                STOPPING.store(true, Ordering::SeqCst);
                forward();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[cfg(windows)]
    fn status(raw: u32) -> ExitStatus { std::os::windows::process::ExitStatusExt::from_raw(raw) }
    #[cfg(unix)]
    fn status(code: i32) -> ExitStatus { std::os::unix::process::ExitStatusExt::from_raw(code << 8) }
    #[cfg(unix)]
    fn signalled(sig: i32) -> ExitStatus { std::os::unix::process::ExitStatusExt::from_raw(sig) }

    #[test]
    fn classify_normal_codes_stop() {
        for c in [0, 2, 3, 4] {
            assert_eq!(classify(status(c)), Exit::Stop(c as i32));
        }
        assert!(matches!(classify(status(101)), Exit::Crash(_)));
        assert!(matches!(classify(status(1)), Exit::Crash(_)));
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
        assert_eq!(classify(status(0xC000013A)), Exit::Stop(0xC000013Au32 as i32));
    }

    #[cfg(unix)]
    #[test]
    fn classify_signals() {
        for sig in [11, 6, 9] {
            assert_eq!(classify(signalled(sig)), Exit::Crash(format!("signal={sig}")));
        }
        for sig in [15, 1, 2] {
            assert!(matches!(classify(signalled(sig)), Exit::Stop(_)), "signal {sig}");
        }
    }

    #[test]
    fn guard_gives_up_on_third_crash_within_60s() {
        let t = Instant::now();
        let mut g = Guard::default();
        assert_eq!(g.record(t), Next::Restart { attempt: 1 });
        assert_eq!(g.record(t + Duration::from_secs(20)), Next::Restart { attempt: 2 });
        assert_eq!(g.record(t + Duration::from_secs(59)), Next::GiveUp);
    }

    #[test]
    fn guard_forgets_crashes_older_than_60s() {
        let t = Instant::now();
        let mut g = Guard::default();
        assert_eq!(g.record(t), Next::Restart { attempt: 1 });
        assert_eq!(g.record(t + Duration::from_secs(30)), Next::Restart { attempt: 2 });
        assert_eq!(g.record(t + Duration::from_secs(61)), Next::Restart { attempt: 2 });
        assert_eq!(g.record(t + Duration::from_secs(200)), Next::Restart { attempt: 1 });
    }

    #[test]
    fn app_args_escalate() {
        let os = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        let pass = os(&["--no-browser"]);
        assert_eq!(app_args(0, &pass), os(&["--no-browser"]));
        assert_eq!(app_args(1, &pass), os(&["--restore", "--no-browser"]));
        assert_eq!(app_args(2, &pass), os(&["--restore", "--attempt", "2", "--no-browser"]));
        // A supervisor started with `--restore` restores on attempt 0 and never repeats the flag.
        let pass = os(&["--restore", "--no-browser"]);
        assert_eq!(app_args(0, &pass), os(&["--restore", "--no-browser"]));
        assert_eq!(app_args(1, &pass), os(&["--restore", "--no-browser"]));
        assert_eq!(app_args(2, &pass), os(&["--restore", "--attempt", "2", "--no-browser"]));
    }
}
