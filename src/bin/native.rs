//! `rustypresenter-native`: the same core and phone/web server as the Tauri app, with the room and
//! stage screens drawn natively (Slint + libmpv). Until the native operator window exists (Plan 3),
//! the operator is today's web page, opened once in the system browser. Started without `--app` it is
//! only the supervisor (`recovery::supervisor`), which runs itself with `--app` and restarts it after a crash.
use rustypresenter::{displays, recovery::{exitcode, panic, supervisor}, render::Renderer, server, startup, ui::screens::{self, NativeScreens}, video};
use std::{ffi::OsString, path::{Path, PathBuf}};

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub app: bool,
    pub restore: bool,
    pub attempt: u32,
    pub no_browser: bool,
    pub rest: Vec<OsString>,
}

/// The command line without the program name. `--app` runs the app itself; without it this process is
/// the supervisor. `--attempt` with a bad number counts as 0. Anything unknown is kept in `rest`.
pub fn parse_args(mut it: impl Iterator<Item = OsString>) -> Args {
    let mut args = Args::default();
    while let Some(a) = it.next() {
        match a.to_str() {
            Some("--app") => args.app = true,
            Some("--restore") => args.restore = true,
            Some("--attempt") => args.attempt = it.next().and_then(|n| n.to_str()?.parse().ok()).unwrap_or(0),
            Some("--no-browser") => args.no_browser = true,
            _ => args.rest.push(a),
        }
    }
    args
}

fn fail(msg: &str) -> ! {
    displays::error_box(msg);
    std::process::exit(exitcode::CONFIG);
}

/// `<data>/logs/native-YYYYMMDD.log` for `date`.
fn log_path(root: &Path, date: chrono::NaiveDate) -> PathBuf {
    root.join("logs").join(format!("native-{}.log", date.format("%Y%m%d")))
}

/// The supervisor and the app share the log of the day the supervisor started. The supervisor only
/// needs the data folder for this; it never reads `data.json`.
fn todays_log() -> PathBuf {
    let root = server::data_dir().unwrap_or_else(|| std::env::temp_dir().join("RustyPresenter"));
    log_path(&root, chrono::Local::now().date_naive())
}

fn main() {
    let args = parse_args(std::env::args_os().skip(1));
    if !args.app {
        let exe = match std::env::current_exe() { Ok(p) => p, Err(e) => fail(&format!("RustyPresenter could not start: {e}")) };
        std::process::exit(supervisor::run(&exe, &todays_log(), &std::env::args_os().skip(1).collect::<Vec<_>>()));
    }
    supervisor::quiet_crash_dialogs();
    panic::install(todays_log());
    panic::mark_critical();
    let no_browser = args.no_browser;
    let started = match startup::start(!no_browser) {
        Ok(Some(s)) => s,
        Ok(None) => { println!("RustyPresenter is already running."); std::process::exit(exitcode::ALREADY_RUNNING); }
        Err(e) => fail(&e),
    };
    startup::banner(&started, "Press Ctrl+C here to quit.");
    let startup::Started { app, listener, .. } = started;

    // Slint runs on this (main) thread; tokio runs the server and backups on its own threads.
    let rt = match tokio::runtime::Runtime::new() { Ok(rt) => rt, Err(e) => fail(&format!("RustyPresenter could not start: {e}")) };
    rt.spawn(startup::backups(app.clone()));
    rt.spawn(startup::serve(app.clone(), listener));
    rt.spawn(async { if tokio::signal::ctrl_c().await.is_ok() { let _ = slint::quit_event_loop(); } });

    let mode = video::choose_backend();
    let mpv = match video::ffi::Mpv::load() {
        Ok(m) => Some(m),
        Err(e) => { eprintln!("Video and audio will not play: {e}"); None }
    };
    println!("  Video drawing: {}{}\n", if mode == video::DrawMode::OpenGl { "GPU (OpenGL)" } else { "software" }, if mpv.is_none() { ", video player missing" } else { "" });

    let native = NativeScreens::new(&app, Renderer::new(), mpv);
    app.set_screens(native.clone());
    native.start();
    if !no_browser { screens::open_browser(&format!("http://127.0.0.1:{}/", app.port)); }

    // The room and stage windows come and go; only Ctrl+C (or closing the console) quits.
    let result = slint::run_event_loop_until_quit();
    native.shutdown();
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    if let Err(e) = result { fail(&format!("RustyPresenter could not run its windows: {e}")); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_cases() {
        let parse = |v: &[&str]| parse_args(v.iter().map(std::ffi::OsString::from));
        assert_eq!(parse(&[]), Args::default());
        assert_eq!(parse(&["--app", "--restore", "--attempt", "2", "--no-browser"]), Args { app: true, restore: true, attempt: 2, no_browser: true, rest: vec![] });
        assert_eq!(parse(&["--app", "--attempt", "x", "show.json"]), Args { app: true, attempt: 0, rest: vec!["show.json".into()], ..Args::default() });
        assert_eq!(parse(&["--restore", "--other"]), Args { restore: true, rest: vec!["--other".into()], ..Args::default() });
    }

    #[test]
    fn log_file_is_per_day_in_the_logs_folder() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        assert_eq!(log_path(Path::new("data"), d), Path::new("data").join("logs").join("native-20261101.log"));
    }
}
