//! Panic logging and exit (spec §9): a panic is written to the log with a backtrace, and a panic on a
//! critical thread ends the process with `exitcode::PANIC` so the supervisor restarts it.
use super::exitcode;
use std::{cell::Cell, fs::OpenOptions, io::Write, marker::PhantomData, path::PathBuf};

thread_local! {
    static CRITICAL: Cell<bool> = const { Cell::new(false) };
    static CATCHING: Cell<u32> = const { Cell::new(0) };
}

/// Every panic is appended to `log` (flushed to disk, since the process may end next) with a backtrace,
/// then reported as usual. A panic on a critical thread outside `Catching` ends the process with `PANIC`.
pub fn install(log: PathBuf) {
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = log.parent() { let _ = std::fs::create_dir_all(dir); }
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&log) {
            let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(f, "{now} PANIC thread={thread}: {info}\n{}\n", std::backtrace::Backtrace::force_capture());
            let _ = f.sync_all();
        }
        report(info);
        let critical = CRITICAL.try_with(Cell::get).unwrap_or(false);
        if critical && CATCHING.try_with(Cell::get).unwrap_or(0) == 0 { std::process::exit(exitcode::PANIC) }
    }));
}

/// A panic on this thread ends the app: the UI/main thread, `live-state` and each mpv event thread,
/// whose loss would leave the room silently stuck.
pub fn mark_critical() {
    CRITICAL.with(|c| c.set(true));
}

/// Held around a `catch_unwind` on a critical thread, so the panic it catches is only logged.
pub struct Catching(PhantomData<*const ()>); // the count is per thread, so the guard stays on its thread

impl Catching {
    pub fn new() -> Catching {
        CATCHING.with(|c| c.set(c.get() + 1));
        Catching(PhantomData)
    }
}

impl Default for Catching {
    fn default() -> Self { Catching::new() }
}

impl Drop for Catching {
    fn drop(&mut self) { let _ = CATCHING.try_with(|c| c.set(c.get().saturating_sub(1))); }
}

/// Recovery tests crash the app at a named point: `RUSTYPRESENTER_TEST_CRASH` = `panic`, `abort` or
/// `segv`, and `RUSTYPRESENTER_TEST_CRASH_AT` = `name`. Always compiled in; does nothing unless set.
pub fn test_crash_point(name: &str) {
    let Some(how) = std::env::var_os("RUSTYPRESENTER_TEST_CRASH") else { return };
    if std::env::var_os("RUSTYPRESENTER_TEST_CRASH_AT").is_none_or(|at| at != name) { return }
    match how.to_str() {
        Some("panic") => panic!("test crash at {name}"),
        Some("abort") => std::process::abort(),
        // A write to an unmapped low address: a real access violation / SIGSEGV, not a Rust abort.
        Some("segv") => unsafe { std::ptr::without_provenance_mut::<u8>(8).write_volatile(1) },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Re-runs this test binary as a helper (Plan 4 Task 2's self-invocation pattern).
    fn run_helper(log: &std::path::Path, catch: bool) -> std::process::Output {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "recovery::panic::tests::panic_helper", "--nocapture"]).env("RP_PANIC_HELPER", log);
        if catch { cmd.env("RP_PANIC_CATCH", "1"); }
        cmd.output().unwrap()
    }

    /// A no-op unless `RP_PANIC_HELPER` names a log: then it panics on a critical thread.
    #[test]
    fn panic_helper() {
        let Some(log) = std::env::var_os("RP_PANIC_HELPER") else { return };
        install(PathBuf::from(log));
        mark_critical();
        if std::env::var_os("RP_PANIC_CATCH").is_some() {
            let _c = Catching::new();
            assert!(std::panic::catch_unwind(|| panic!("caught on purpose")).is_err());
            return;
        }
        panic!("critical on purpose");
    }

    #[test]
    fn critical_thread_panic_exits_101() {
        let dir = std::env::temp_dir().join(format!("rp-panic-{}", uuid::Uuid::new_v4()));
        let log = dir.join("logs").join("native.log");
        let out = run_helper(&log, false);
        assert_eq!(out.status.code(), Some(crate::recovery::exitcode::PANIC), "{}", String::from_utf8_lossy(&out.stderr));
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("PANIC thread=") && text.contains("critical on purpose"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn caught_panic_does_not_exit() {
        let dir = std::env::temp_dir().join(format!("rp-panic-{}", uuid::Uuid::new_v4()));
        let log = dir.join("native.log");
        let out = run_helper(&log, true);
        assert!(out.status.success(), "{:?}\n{}", out.status, String::from_utf8_lossy(&out.stdout));
        assert!(std::fs::read_to_string(&log).unwrap().contains("caught on purpose"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
