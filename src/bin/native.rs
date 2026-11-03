//! `rustypresenter-native`: the same core and phone/web server as the Tauri app, with the operator,
//! room and stage windows drawn natively (Slint + libmpv). No browser opens; the phone remote is
//! still the `/remote` web page, and the web operator page is still served for the Tauri app.
use rustypresenter::{displays, render::Renderer, startup, ui::{operator::window::{Operator, OperatorScreens}, ops::Ops, screens::NativeScreens}, video};
use slint::{CloseRequestResponse, winit_030::invoke_from_active_event_loop};
use std::{cell::RefCell, fs::OpenOptions, io::Write, path::{Path, PathBuf}, rc::Rc};

thread_local! {
    /// The operator window, kept open on the Slint thread until the event loop ends.
    static OPERATOR: RefCell<Option<Rc<Operator>>> = const { RefCell::new(None) };
}

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
    // `--no-browser` (from when the operator was the web page) is accepted and ignored for one release.
    let started = match startup::start(true) {
        Ok(Some(s)) => s,
        Ok(None) => { println!("RustyPresenter is already running."); return; }
        Err(e) => fail(&e),
    };
    log_panics(started.root.clone());
    startup::banner(&started, "Close the operator window (or press Ctrl+C here) to quit.");
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
    // Queued after `start`'s reopen, so the room and stage are back before the operator picks its monitor.
    let (screens, handle) = (native.clone(), rt.handle().clone());
    let queued = invoke_from_active_event_loop(move |_| {
        let ops = Ops::new(app.clone(), handle);
        let operator = match Operator::open(app, ops, Rc::new(screens.clone()) as Rc<dyn OperatorScreens>) {
            Ok(op) => op,
            Err(e) => fail(&e),
        };
        screens.place_operator(operator.window());
        // Closing the operator asks nothing and quits; the room and stage close with it below.
        operator.window().on_close_requested(|| { let _ = slint::quit_event_loop(); CloseRequestResponse::HideWindow });
        screens.set_operator(Rc::downgrade(&operator));
        OPERATOR.with(|o| *o.borrow_mut() = Some(operator));
    });
    if let Err(e) = queued { fail(&format!("RustyPresenter could not open its window: {e}")); }

    // The room and stage windows come and go; closing the operator (or Ctrl+C) quits.
    let result = slint::run_event_loop_until_quit();
    OPERATOR.with(|o| o.borrow_mut().take());
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
