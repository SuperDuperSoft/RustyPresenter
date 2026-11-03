//! Panics go to the log with a backtrace. On a critical thread the app then exits with
//! `exitcode::PANIC`, so the supervisor restarts it; elsewhere they stay that thread's problem.
use super::exitcode;
use std::{backtrace::Backtrace, cell::Cell, fs, io::Write, path::{Path, PathBuf}};

thread_local! {
    static CRITICAL: Cell<bool> = const { Cell::new(false) };
    static CATCHING: Cell<u32> = const { Cell::new(0) };
}

fn write_panic(log: &Path, thread: &str, info: &dyn std::fmt::Display) {
    if let Some(dir) = log.parent() { let _ = fs::create_dir_all(dir); }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(log) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(f, "{now} PANIC thread={thread}: {info}\n{}\n", Backtrace::force_capture());
        let _ = f.sync_all();
    }
}

pub fn install(log: PathBuf) {
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_panic(&log, std::thread::current().name().unwrap_or("unnamed"), info);
        report(info);
        let critical = CRITICAL.try_with(Cell::get).unwrap_or(false);
        let caught = CATCHING.try_with(Cell::get).unwrap_or(0) > 0;
        if critical && !caught { std::process::exit(exitcode::PANIC); }
    }));
}

/// The app cannot go on without this thread.
pub fn mark_critical() { CRITICAL.set(true); }

/// While one is held on a thread, a panic there is being caught by `catch_unwind`.
pub struct Catching(());

impl Catching {
    pub fn new() -> Self { CATCHING.set(CATCHING.get() + 1); Catching(()) }
}

impl Default for Catching { fn default() -> Self { Self::new() } }

impl Drop for Catching {
    fn drop(&mut self) { CATCHING.set(CATCHING.get() - 1); }
}

/// Crashes here when `RUSTYPRESENTER_TEST_CRASH` (`panic`, `abort` or `segv`) and
/// `RUSTYPRESENTER_TEST_CRASH_AT` ask for it. Does nothing otherwise, in every build.
pub fn test_crash_point(name: &str) {
    if std::env::var_os("RUSTYPRESENTER_TEST_CRASH_AT").is_none_or(|at| at != name) { return; }
    match std::env::var("RUSTYPRESENTER_TEST_CRASH").as_deref() {
        Ok("panic") => panic!("test crash at {name}"),
        Ok("abort") => std::process::abort(),
        // A low address that is never mapped; a null write would stop at Rust's debug check instead.
        Ok("segv") => unsafe { std::ptr::write_volatile(std::hint::black_box(8usize as *mut u8), 1) },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, process::{Command, Output}};

    const HELPER: &str = "recovery::panic::tests::panic_helper";

    /// Runs `panic_helper` in a process of its own. A panic hook lasts for the whole process.
    fn helper(mode: &str, envs: &[(&str, &str)]) -> (Output, String) {
        let log = env::temp_dir().join(format!("rp-panic-{}.log", uuid::Uuid::new_v4()));
        let out = Command::new(env::current_exe().unwrap()).args(["--exact", HELPER, "--nocapture"])
            .env("RP_PANIC_HELPER", &log).env("RP_PANIC_MODE", mode).envs(envs.iter().copied()).output().unwrap();
        (out, std::fs::read_to_string(&log).unwrap_or_default())
    }

    /// The harness also exits 101 for a failed test; only the hook exits before it reports.
    fn left_early(out: &Output) -> bool { !String::from_utf8_lossy(&out.stdout).contains("test result:") }

    #[test]
    fn panic_helper() {
        let Some(log) = env::var_os("RP_PANIC_HELPER") else { return };
        install(log.into());
        match env::var("RP_PANIC_MODE").unwrap().as_str() {
            "critical" => { mark_critical(); panic!("boom"); }
            "caught" => {
                mark_critical();
                let _catching = Catching::new();
                assert!(std::panic::catch_unwind(|| panic!("boom")).is_err());
            }
            "other" => assert!(std::thread::spawn(|| panic!("boom")).join().is_err()),
            "mark-elsewhere" => {
                mark_critical();
                assert!(std::thread::spawn(|| panic!("boom")).join().is_err());
            }
            "point" => test_crash_point("here"),
            mode => unreachable!("{mode}"),
        }
    }

    #[test]
    fn critical_thread_panic_exits_101() {
        let (out, log) = helper("critical", &[]);
        assert_eq!(out.status.code(), Some(101));
        assert!(left_early(&out), "the hook exits before the test harness reports");
        assert!(log.contains("PANIC thread="), "{log}");
        assert!(log.contains("boom"), "{log}");
    }

    #[test]
    fn caught_panic_does_not_exit() {
        let (out, log) = helper("caught", &[]);
        assert!(out.status.success(), "{out:?}");
        assert!(log.contains("PANIC thread="), "still logged: {log}");
    }

    #[test]
    fn panic_on_another_thread_is_only_logged() {
        let (out, log) = helper("other", &[]);
        assert!(out.status.success(), "{out:?}");
        assert!(log.contains("PANIC thread="), "{log}");
    }

    #[test]
    fn being_critical_belongs_to_one_thread() {
        let (out, _) = helper("mark-elsewhere", &[]);
        assert!(out.status.success(), "{out:?}");
    }

    #[test]
    fn crash_point_panics_when_named() {
        let (out, log) = helper("point", &[("RUSTYPRESENTER_TEST_CRASH", "panic"), ("RUSTYPRESENTER_TEST_CRASH_AT", "here")]);
        assert!(!out.status.success());
        assert!(log.contains("test crash at here"), "{log}");
    }

    #[test]
    fn crash_point_is_silent_for_another_name() {
        let (out, _) = helper("point", &[("RUSTYPRESENTER_TEST_CRASH", "panic"), ("RUSTYPRESENTER_TEST_CRASH_AT", "elsewhere")]);
        assert!(out.status.success(), "{out:?}");
    }

    #[test]
    fn crash_point_is_silent_when_unset() {
        let (out, _) = helper("point", &[]);
        assert!(out.status.success(), "{out:?}");
    }

    #[cfg(unix)]
    #[test]
    fn crash_points_die_by_signal() {
        use std::os::unix::process::ExitStatusExt;
        for (how, signal) in [("abort", libc::SIGABRT), ("segv", libc::SIGSEGV)] {
            let (out, _) = helper("point", &[("RUSTYPRESENTER_TEST_CRASH", how), ("RUSTYPRESENTER_TEST_CRASH_AT", "here")]);
            assert_eq!(out.status.signal(), Some(signal), "{how}");
        }
    }
}
