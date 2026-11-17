//! "Restart now" for settings used from the next start (Settings → General → Screens and graphics → Graphics): `request()` quits through the
//! normal quit path (`main`), which then starts a new copy (`relaunch`) with this run's arguments, minus the ones
//! for one run only, plus `--wait-for <this pid>`. The new copy waits for this one to end before it takes the port
//! (`wait_for`), so it doesn't find "RustyPresenter is already running".
use std::{ffi::OsString, path::Path, process::{Command, Stdio}, sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};

pub const WAIT_FLAG: &str = "--wait-for";
/// The longest the new copy waits for the old one.
pub const WAIT_LIMIT: Duration = Duration::from_secs(20);

static REQUESTED: AtomicBool = AtomicBool::new(false);

/// Quits the app, which then starts again (see the top of this file).
pub fn request() {
    #[cfg(test)]
    REQUESTS.with(|r| r.set(r.get() + 1));
    REQUESTED.store(true, Ordering::SeqCst);
    let _ = slint::quit_event_loop();
}

pub fn requested() -> bool { REQUESTED.load(Ordering::SeqCst) }

#[cfg(test)]
thread_local! { static REQUESTS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }

/// Tests: how many times this thread asked for a restart.
#[cfg(test)]
pub fn requests_here() -> u32 { REQUESTS.with(|r| r.get()) }

/// This run's arguments for the next copy: without the supervisor's and one-run flags (`--app`, `--restore`,
/// `--attempt n`, `--safe-mode`, `--wait-for n`, the graphics test's), with `--wait-for pid`.
pub fn relaunch_args(args: &[OsString], pid: u32) -> Vec<OsString> {
    let mut out = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.to_str() {
            Some("--app" | "--restore" | "--safe-mode") => {}
            Some("--attempt" | WAIT_FLAG | crate::video::bench::FLAG | crate::video::bench::OUT_FLAG | crate::video::bench::ROOM_FLAG) => { it.next(); }
            _ => out.push(a.clone()),
        }
    }
    out.extend([WAIT_FLAG.into(), pid.to_string().into()]);
    out
}

/// The new copy's command: `exe` with `relaunch_args`, on its own (no input or output of ours).
pub fn command(exe: &Path, args: &[OsString], pid: u32) -> Command {
    let mut c = Command::new(exe);
    c.args(relaunch_args(args, pid)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    c
}

/// Starts the new copy. Called by `main` last, after the normal quit.
pub fn relaunch() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let started = std::env::current_exe().and_then(|exe| command(&exe, &args, std::process::id()).spawn());
    if let Err(e) = started { eprintln!("RustyPresenter could not start again: {e}") }
}

/// `--wait-for n` among the arguments.
pub fn wait_arg(args: &[OsString]) -> Option<u32> {
    let i = args.iter().position(|a| a == WAIT_FLAG)?;
    args.get(i + 1)?.to_str()?.parse().ok()
}

/// Waits (at most `limit`) until `alive` says the old copy has ended.
pub fn wait_until_gone(mut alive: impl FnMut() -> bool, limit: Duration, poll: Duration) -> bool {
    let start = Instant::now();
    while alive() {
        if start.elapsed() >= limit { return false }
        std::thread::sleep(poll);
    }
    true
}

/// Waits for process `pid` (the copy that restarted this one) to end.
pub fn wait_for(pid: u32) {
    if !wait_until_gone(|| running(pid), WAIT_LIMIT, Duration::from_millis(100)) { eprintln!("The previous RustyPresenter (process {pid}) is still running.") }
}

#[cfg(unix)]
fn running(pid: u32) -> bool {
    // Signal 0 only asks whether the process is there.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(windows)]
fn running(pid: u32) -> bool {
    use windows_sys::Win32::{Foundation::{CloseHandle, WAIT_TIMEOUT}, System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject}};
    let h = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if h.is_null() { return false }
    let alive = unsafe { WaitForSingleObject(h, 0) } == WAIT_TIMEOUT;
    unsafe { CloseHandle(h) };
    alive
}

#[cfg(not(any(unix, windows)))]
fn running(_pid: u32) -> bool { false }

#[cfg(test)]
mod tests {
    use super::*;

    fn os(v: &[&str]) -> Vec<OsString> { v.iter().map(OsString::from).collect() }

    #[test]
    fn the_new_copy_gets_the_lasting_arguments_and_waits_for_this_one() {
        let args = os(&["--app", "--restore", "--attempt", "2", "--safe-mode", "--no-browser", "--wait-for", "9", "extra"]);
        assert_eq!(relaunch_args(&args, 42), os(&["--no-browser", "extra", "--wait-for", "42"]));
        let test = os(&["--graphics-bench", "vulkan", "--bench-out", "/tmp/o.json"]);
        assert_eq!(relaunch_args(&test, 7), os(&["--wait-for", "7"]));
        assert_eq!(relaunch_args(&[], 1), os(&["--wait-for", "1"]));
    }

    #[test]
    fn the_command_runs_this_program_on_its_own() {
        let c = command(Path::new("/opt/rp/rustypresenter"), &os(&["--app", "--no-browser"]), 5);
        assert_eq!(c.get_program(), "/opt/rp/rustypresenter");
        assert_eq!(c.get_args().collect::<Vec<_>>(), ["--no-browser", "--wait-for", "5"]);
    }

    #[test]
    fn the_wait_flag_is_read() {
        assert_eq!(wait_arg(&os(&["--no-browser", "--wait-for", "123"])), Some(123));
        assert_eq!(wait_arg(&os(&["--wait-for"])), None);
        assert_eq!(wait_arg(&os(&["--wait-for", "x"])), None);
        assert_eq!(wait_arg(&os(&["--app"])), None);
    }

    #[test]
    fn waiting_ends_when_the_old_copy_does_or_at_the_limit() {
        let mut left = 3;
        assert!(wait_until_gone(|| { left -= 1; left > 0 }, Duration::from_secs(5), Duration::from_millis(1)));
        assert!(!wait_until_gone(|| true, Duration::from_millis(20), Duration::from_millis(1)));
    }

    #[cfg(unix)]
    #[test]
    fn a_process_is_running_until_it_ends() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        assert!(running(child.id()));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!running(child.id()));
        assert!(running(std::process::id()));
    }
}
