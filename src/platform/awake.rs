//! Keep the screen on and the computer awake while a presentation runs: no screen saver, no
//! dimming, no sleep. Hold a `KeepAwake` for as long as the room is showing; dropping it lets
//! the computer rest again. Each guard is independent, so holding two is fine.
//!
//! How each system is asked:
//! - Windows: `SetThreadExecutionState` from a small thread of our own.
//! - macOS: the `caffeinate` helper.
//! - Linux: two requests side by side. `org.freedesktop.ScreenSaver.Inhibit` over the session
//!   D-Bus stops screen blanking on KDE, XFCE, Cinnamon, MATE and others; a helper process
//!   (`gnome-session-inhibit`, else `systemd-inhibit`) keeps the computer from sleeping, and on
//!   GNOME also keeps the screen on. Either one working counts as held. None of the Linux paths
//!   has been tried on a real Linux desktop yet: they compile and their pure parts are tested.

/// Keeps the display on and the system from sleeping while it lives. Never fails hard: when the
/// system will not listen, the guard does nothing and `is_active` says so.
pub struct KeepAwake { inner: Option<Inner> }

impl KeepAwake {
    /// Ask the system to stay awake. On Linux `reason` is shown where the desktop lists who is
    /// keeping it awake; Windows and macOS have no place for it and ignore it.
    pub fn acquire(reason: &str) -> KeepAwake {
        let inner = Inner::start(reason).map_err(|e| eprintln!("could not keep the screen awake: {e}")).ok();
        KeepAwake { inner }
    }

    /// Whether the system accepted the request.
    pub fn is_active(&self) -> bool { self.inner.is_some() }
}

impl Drop for KeepAwake {
    fn drop(&mut self) { if let Some(inner) = self.inner.take() { inner.stop() } }
}

/// The helper programs that hold the request on macOS and Linux.
#[derive(Clone, Copy)]
#[cfg_attr(not(unix), allow(dead_code))]
enum Helper { Caffeinate, GnomeSessionInhibit, SystemdInhibit }

/// The helper's program and arguments. Both end by themselves once `pid` (ours) is gone, so a
/// crash never leaves the computer stuck awake.
#[cfg_attr(not(unix), allow(dead_code))]
fn command_line(helper: Helper, reason: &str, pid: u32) -> (String, Vec<String>) {
    match helper {
        // -d keeps the display on, -i stops idle sleep, -w exits when our process does.
        Helper::Caffeinate => ("/usr/bin/caffeinate".into(), ["-d", "-i", "-w"].map(String::from).into_iter().chain([pid.to_string()]).collect()),
        // GNOME blanks the screen on its own session-manager inhibitors, not logind's, so on GNOME
        // this is the one that keeps the screen on (idle) and the computer up (suspend). Elsewhere
        // it is missing or exits at once, and we fall back to systemd-inhibit. Not yet tried on a
        // real GNOME machine.
        Helper::GnomeSessionInhibit => ("gnome-session-inhibit".into(), vec![
            "--inhibit".into(), "idle:suspend".into(), "--app-id".into(), "RustyPresenter".into(),
            "--reason".into(), reason.into(), "sh".into(), "-c".into(), watch_script(pid),
        ]),
        // An idle and sleep inhibitor through logind. KDE honours it for blanking and dimming too;
        // GNOME only for sleep, hence the helper above. The reason is its own argument, never shell text.
        Helper::SystemdInhibit => ("systemd-inhibit".into(), vec![
            "--what=idle:sleep".into(), "--who=RustyPresenter".into(), format!("--why={reason}"), "--mode=block".into(),
            "sh".into(), "-c".into(), watch_script(pid),
        ]),
    }
}

/// The command the Linux helpers hold their lock around. It runs until we exit, or until the
/// helper itself (the shell's parent) is killed when the guard drops.
#[cfg_attr(not(unix), allow(dead_code))]
fn watch_script(pid: u32) -> String {
    format!("while kill -0 {pid} 2>/dev/null && kill -0 $PPID 2>/dev/null; do sleep 5; done")
}

/// Windows: the request belongs to the thread that made it, so a small thread makes it, waits
/// for the guard to drop, then clears it.
#[cfg(windows)]
struct Inner { stop: std::sync::mpsc::Sender<()>, thread: std::thread::JoinHandle<()> }

#[cfg(windows)]
mod win {
    pub const ES_CONTINUOUS: u32 = 0x8000_0000;
    pub const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
    pub const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        /// Returns the previous state, or 0 on failure.
        pub fn SetThreadExecutionState(flags: u32) -> u32;
    }

    pub fn set(flags: u32) -> bool { unsafe { SetThreadExecutionState(flags) != 0 } }
}

#[cfg(windows)]
impl Inner {
    fn start(_reason: &str) -> Result<Inner, String> {
        use std::sync::mpsc;
        let (stop, wait) = mpsc::channel::<()>();
        let (ready, started) = mpsc::channel::<bool>();
        let thread = std::thread::Builder::new().name("keep-awake".into()).spawn(move || {
            let ok = win::set(win::ES_CONTINUOUS | win::ES_DISPLAY_REQUIRED | win::ES_SYSTEM_REQUIRED);
            let _ = ready.send(ok);
            if !ok { return }
            // A message or a dropped sender both mean the guard is gone.
            let _ = wait.recv();
            win::set(win::ES_CONTINUOUS);
        }).map_err(|e| e.to_string())?;
        match started.recv() {
            Ok(true) => Ok(Inner { stop, thread }),
            _ => { let _ = thread.join(); Err("SetThreadExecutionState failed".into()) }
        }
    }

    fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.thread.join();
    }
}

/// Linux holds two requests at once; the guard is held if either was granted. Both failing is
/// an error that says why each failed; one failing is only logged.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn either<A, B>(a: Result<A, String>, b: Result<B, String>) -> Result<(Option<A>, Option<B>), String> {
    match (a, b) {
        (Err(a), Err(b)) => Err(format!("{a}; {b}")),
        (a, b) => {
            if let Err(e) = &a { eprintln!("keep awake: {e}") }
            if let Err(e) = &b { eprintln!("keep awake: {e}") }
            Ok((a.ok(), b.ok()))
        }
    }
}

/// macOS and Linux: a helper process holds the request while it runs. On Linux a D-Bus screen
/// saver request is held next to it.
#[cfg(unix)]
struct Inner {
    child: Option<std::process::Child>,
    #[cfg(target_os = "linux")]
    screensaver: Option<screensaver::Hold>,
}

#[cfg(unix)]
impl Inner {
    #[cfg(target_os = "linux")]
    fn start(reason: &str) -> Result<Inner, String> {
        let screensaver = screensaver::Hold::start(reason);
        let (child, screensaver) = either(Self::start_helper(reason), screensaver)?;
        Ok(Inner { child, screensaver })
    }

    #[cfg(not(target_os = "linux"))]
    fn start(reason: &str) -> Result<Inner, String> {
        Ok(Inner { child: Some(Self::start_helper(reason)?) })
    }

    fn start_helper(reason: &str) -> Result<std::process::Child, String> {
        let helpers: &[Helper] = if cfg!(target_os = "macos") { &[Helper::Caffeinate] }
            else { &[Helper::GnomeSessionInhibit, Helper::SystemdInhibit] };
        let mut errors = Vec::new();
        for &helper in helpers {
            match Self::spawn(helper, reason) {
                Ok(inner) => return Ok(inner),
                Err(e) => errors.push(e),
            }
        }
        Err(errors.join("; "))
    }

    fn spawn(helper: Helper, reason: &str) -> Result<std::process::Child, String> {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let (program, args) = command_line(helper, reason, std::process::id());
        // Missing (no systemd here) is the usual failure; the guard then does nothing. Its own
        // process group keeps Ctrl-C in a terminal from ending the hold behind the app's back.
        let mut child = Command::new(&program).args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .process_group(0).spawn().map_err(|e| format!("{program}: {e}"))?;
        // Started is not holding: with no logind or D-Bus, or when the session may not inhibit
        // (SSH, an inactive seat), the helper exits straight away. Give it a moment to refuse.
        let deadline = Instant::now() + Duration::from_millis(150);
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(status)) => return Err(format!("{program} exited at once ({status})")),
                Ok(None) => std::thread::sleep(Duration::from_millis(15)),
                Err(e) => { let _ = child.kill(); let _ = child.wait(); return Err(format!("{program}: {e}")) }
            }
        }
        Ok(child)
    }

    fn stop(self) {
        if let Some(mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        #[cfg(target_os = "linux")]
        if let Some(hold) = self.screensaver { hold.stop() }
    }
}

/// The freedesktop screen saver interface: `Inhibit(app, reason) -> cookie` and
/// `UnInhibit(cookie)`. Most desktops serve it at the first path; some older ones only at the second.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod screensaver_names {
    pub const SERVICE: &str = "org.freedesktop.ScreenSaver";
    pub const INTERFACE: &str = "org.freedesktop.ScreenSaver";
    pub const PATHS: [&str; 2] = ["/org/freedesktop/ScreenSaver", "/ScreenSaver"];
    pub const APP: &str = "RustyPresenter";
}

/// Linux: a thread owns a session D-Bus connection holding `Inhibit` until the guard drops, then
/// sends `UnInhibit`. The desktop also lifts the request when the connection closes, so a crash
/// never leaves the screen stuck on. Untested on a real desktop.
#[cfg(target_os = "linux")]
mod screensaver {
    use super::screensaver_names::{APP, INTERFACE, PATHS, SERVICE};
    use std::sync::mpsc;
    use std::time::Duration;
    use zbus::blocking::Connection;

    /// How long to wait for the desktop to answer before going on without it.
    const ANSWER_WITHIN: Duration = Duration::from_secs(3);

    pub struct Hold { stop: mpsc::Sender<()>, thread: std::thread::JoinHandle<()> }

    /// Connect and ask at each path in turn. Returns the connection, the path that answered and the cookie.
    fn inhibit(reason: &str) -> Result<(Connection, &'static str, u32), String> {
        let connection = Connection::session().map_err(|e| format!("session D-Bus: {e}"))?;
        let mut errors = Vec::new();
        for path in PATHS {
            let cookie = connection.call_method(Some(SERVICE), path, Some(INTERFACE), "Inhibit", &(APP, reason))
                .and_then(|reply| reply.body().deserialize::<u32>());
            match cookie {
                Ok(cookie) => return Ok((connection, path, cookie)),
                Err(e) => errors.push(format!("{path}: {e}")),
            }
        }
        Err(format!("{SERVICE}.Inhibit: {}", errors.join("; ")))
    }

    impl Hold {
        pub fn start(reason: &str) -> Result<Hold, String> {
            let reason = reason.to_owned();
            let (stop, wait) = mpsc::channel::<()>();
            let (ready, started) = mpsc::channel::<Result<(), String>>();
            let thread = std::thread::Builder::new().name("keep-awake-dbus".into()).spawn(move || {
                let (connection, path, cookie) = match inhibit(&reason) {
                    Ok(held) => held,
                    Err(e) => { let _ = ready.send(Err(e)); return }
                };
                let _ = ready.send(Ok(()));
                // A message or a dropped sender both mean the guard is gone (or we gave up waiting).
                let _ = wait.recv();
                if let Err(e) = connection.call_method(Some(SERVICE), path, Some(INTERFACE), "UnInhibit", &(cookie,)) {
                    // Closing the connection below lifts it anyway.
                    eprintln!("keep awake: {SERVICE}.UnInhibit: {e}");
                }
            }).map_err(|e| e.to_string())?;
            match started.recv_timeout(ANSWER_WITHIN) {
                Ok(Ok(())) => Ok(Hold { stop, thread }),
                Ok(Err(e)) => { let _ = thread.join(); Err(e) }
                // Slow bus: leave the thread behind. `stop` drops here, so if the request is
                // granted later the thread lifts it straight away.
                Err(mpsc::RecvTimeoutError::Timeout) => Err(format!("{SERVICE}: no answer within {ANSWER_WITHIN:?}")),
                Err(mpsc::RecvTimeoutError::Disconnected) => { let _ = thread.join(); Err(format!("{SERVICE}: thread ended")) }
            }
        }

        pub fn stop(self) {
            let _ = self.stop.send(());
            let _ = self.thread.join();
        }
    }
}

/// Elsewhere there is nothing to ask.
#[cfg(not(any(windows, unix)))]
struct Inner;

#[cfg(not(any(windows, unix)))]
impl Inner {
    fn start(_reason: &str) -> Result<Inner, String> { Err("not supported on this system".into()) }
    fn stop(self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}

    /// Systems where the request always works: Windows always answers, and macOS always ships
    /// caffeinate. Linux depends on the desktop (CI runners often have no logind), so it is not asserted.
    const HOLDS_HERE: bool = cfg!(any(windows, target_os = "macos"));

    #[test]
    fn acquire_and_drop() {
        assert_send::<KeepAwake>();
        let guard = KeepAwake::acquire("test");
        if HOLDS_HERE { assert!(guard.is_active()) }
        drop(guard);
    }

    #[test]
    fn two_guards_at_once() {
        let (a, b) = (KeepAwake::acquire("one"), KeepAwake::acquire("two"));
        if HOLDS_HERE { assert!(a.is_active() && b.is_active()) }
        drop(a);
        drop(b);
    }

    #[test]
    fn guard_dropped_on_another_thread() {
        let guard = KeepAwake::acquire("moved");
        let active = guard.is_active();
        let back = std::thread::spawn(move || { let a = guard.is_active(); drop(guard); a }).join().unwrap();
        assert_eq!(back, active);
        if HOLDS_HERE { assert!(back) }
    }

    #[test]
    fn either_holds_if_one_side_did() {
        assert_eq!(either::<u8, u8>(Ok(1), Ok(2)), Ok((Some(1), Some(2))));
        assert_eq!(either::<u8, u8>(Ok(1), Err("no bus".into())), Ok((Some(1), None)));
        assert_eq!(either::<u8, u8>(Err("no helper".into()), Ok(2)), Ok((None, Some(2))));
        assert_eq!(either::<u8, u8>(Err("no helper".into()), Err("no bus".into())), Err("no helper; no bus".to_string()));
    }

    #[test]
    fn screensaver_asks_the_freedesktop_paths_in_order() {
        use screensaver_names::*;
        assert_eq!(SERVICE, "org.freedesktop.ScreenSaver");
        assert_eq!(INTERFACE, "org.freedesktop.ScreenSaver");
        assert_eq!(PATHS, ["/org/freedesktop/ScreenSaver", "/ScreenSaver"]);
        assert_eq!(APP, "RustyPresenter");
    }

    #[test]
    fn macos_command_watches_our_pid() {
        let (program, args) = command_line(Helper::Caffeinate, "Presenting", 4242);
        assert_eq!(program, "/usr/bin/caffeinate");
        assert_eq!(args, ["-d", "-i", "-w", "4242"]);
    }

    #[test]
    fn linux_command_inhibits_idle_and_sleep_until_we_exit() {
        let (program, args) = command_line(Helper::SystemdInhibit, "Presenting \"Sunday\"; rm -rf", 4242);
        assert_eq!(program, "systemd-inhibit");
        assert_eq!(&args[..5], ["--what=idle:sleep", "--who=RustyPresenter", "--why=Presenting \"Sunday\"; rm -rf", "--mode=block", "sh"]);
        assert_eq!(args[5], "-c");
        // The reason is its own argument, never part of the shell script.
        assert!(!args[6].contains("Sunday"));
        assert!(args[6].contains("kill -0 4242"));
        // Also ends when systemd-inhibit itself is killed.
        assert!(args[6].contains("kill -0 $PPID"));
        assert_eq!(args.len(), 7);
    }

    #[test]
    fn gnome_command_inhibits_idle_and_suspend_until_we_exit() {
        let (program, args) = command_line(Helper::GnomeSessionInhibit, "Presenting \"Sunday\"; rm -rf", 4242);
        assert_eq!(program, "gnome-session-inhibit");
        assert_eq!(&args[..8], ["--inhibit", "idle:suspend", "--app-id", "RustyPresenter", "--reason", "Presenting \"Sunday\"; rm -rf", "sh", "-c"]);
        assert!(!args[8].contains("Sunday"));
        assert!(args[8].contains("kill -0 4242") && args[8].contains("kill -0 $PPID"));
        assert_eq!(args.len(), 9);
    }
}
