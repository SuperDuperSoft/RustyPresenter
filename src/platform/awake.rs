//! Keep the screen on and the computer awake while a presentation runs: no screen saver, no
//! dimming, no sleep. Hold a `KeepAwake` for as long as the room is showing; dropping it lets
//! the computer rest again. Each guard is independent, so holding two is fine.

/// Keeps the display on and the system from sleeping while it lives. Never fails hard: when the
/// system will not listen, the guard does nothing and `is_active` says so.
pub struct KeepAwake { inner: Option<Inner> }

impl KeepAwake {
    /// Ask the system to stay awake; `reason` is shown where the system lists who is keeping it awake.
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
enum Helper { Caffeinate, SystemdInhibit }

/// The helper's program and arguments. Both end by themselves once `pid` (ours) is gone, so a
/// crash never leaves the computer stuck awake.
#[cfg_attr(not(unix), allow(dead_code))]
fn command_line(helper: Helper, reason: &str, pid: u32) -> (String, Vec<String>) {
    match helper {
        // -d keeps the display on, -i stops idle sleep, -w exits when our process does.
        Helper::Caffeinate => ("/usr/bin/caffeinate".into(), ["-d", "-i", "-w"].map(String::from).into_iter().chain([pid.to_string()]).collect()),
        // An idle inhibitor through logind, which GNOME and KDE honour for blanking and dimming too.
        // The lock lasts while the shell runs: until we exit, or until systemd-inhibit itself (the
        // shell's parent) is killed when the guard drops. The reason is its own argument, never shell text.
        Helper::SystemdInhibit => ("systemd-inhibit".into(), vec![
            "--what=idle:sleep".into(), "--who=RustyPresenter".into(), format!("--why={reason}"), "--mode=block".into(),
            "sh".into(), "-c".into(),
            format!("while kill -0 {pid} 2>/dev/null && kill -0 $PPID 2>/dev/null; do sleep 5; done"),
        ]),
    }
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

/// macOS and Linux: a helper process holds the request while it runs.
#[cfg(unix)]
struct Inner { child: std::process::Child }

#[cfg(unix)]
impl Inner {
    fn start(reason: &str) -> Result<Inner, String> {
        use std::process::{Command, Stdio};
        let helper = if cfg!(target_os = "macos") { Helper::Caffeinate } else { Helper::SystemdInhibit };
        let (program, args) = command_line(helper, reason, std::process::id());
        // Missing (no systemd here) is the usual failure; the guard then does nothing.
        let child = Command::new(&program).args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().map_err(|e| format!("{program}: {e}"))?;
        Ok(Inner { child })
    }

    fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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

    #[test]
    fn acquire_and_drop() {
        assert_send::<KeepAwake>();
        let guard = KeepAwake::acquire("test");
        if cfg!(windows) { assert!(guard.is_active()) }
        drop(guard);
    }

    #[test]
    fn two_guards_at_once() {
        let (a, b) = (KeepAwake::acquire("one"), KeepAwake::acquire("two"));
        if cfg!(windows) { assert!(a.is_active() && b.is_active()) }
        drop(a);
        drop(b);
    }

    #[test]
    fn guard_dropped_on_another_thread() {
        let guard = KeepAwake::acquire("moved");
        let active = guard.is_active();
        let back = std::thread::spawn(move || { let a = guard.is_active(); drop(guard); a }).join().unwrap();
        assert_eq!(back, active);
        if cfg!(windows) { assert!(back) }
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
        assert_eq!(args.len(), 7);
    }
}
