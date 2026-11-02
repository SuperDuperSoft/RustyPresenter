//! `rustypresenter-native`: the same core and phone/web server as the Tauri app, with the room and
//! stage screens drawn natively (Slint + libmpv). Until the native operator window exists (Plan 3),
//! the operator is today's web page, opened once in the system browser.
use rustypresenter::{displays, render::Renderer, startup, ui::screens::{self, NativeScreens}, video};
use std::{fs::OpenOptions, io::Write, path::{Path, PathBuf}};

fn fail(msg: &str) -> ! {
    displays::error_box(msg);
    std::process::exit(1);
}

/// `<data>/logs/native-YYYYMMDD.log` for `date`.
fn log_path(root: &Path, date: chrono::NaiveDate) -> PathBuf {
    root.join("logs").join(format!("native-{}.log", date.format("%Y%m%d")))
}

/// Panics are written to the day's log with a backtrace, then reported as usual.
fn log_panics(root: PathBuf) {
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let now = chrono::Local::now();
        let path = log_path(&root, now.date_naive());
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(&root));
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
            let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
            let _ = writeln!(f, "[{}] panic on thread '{thread}': {info}\n{}\n", now.format("%Y-%m-%d %H:%M:%S"), std::backtrace::Backtrace::force_capture());
        }
        report(info);
    }));
}

fn main() {
    let no_browser = std::env::args().any(|a| a == "--no-browser");
    let started = match startup::start(!no_browser) {
        Ok(Some(s)) => s,
        Ok(None) => { println!("RustyPresenter is already running."); return; }
        Err(e) => fail(&e),
    };
    log_panics(started.root.clone());
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
    fn log_file_is_per_day_in_the_logs_folder() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        assert_eq!(log_path(Path::new("data"), d), Path::new("data").join("logs").join("native-20261101.log"));
    }
}
