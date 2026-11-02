//! `rustypresenter-native`: the same core and phone/web server as the Tauri app, with the room and
//! stage screens drawn natively (Slint + libmpv). Until the native operator window exists (Plan 3),
//! the operator is today's web page, opened once in the system browser.
//!
//! With no `--app` the executable is the supervisor, which runs itself with `--app`.
use rustypresenter::{displays, recovery::{self, exitcode, panic, supervisor}, render::Renderer, startup, ui::screens::{self, NativeScreens}, video};

fn fail(msg: &str) -> ! {
    displays::error_box(msg);
    std::process::exit(exitcode::CONFIG);
}

fn main() {
    let args = supervisor::parse_args(std::env::args_os().skip(1));
    supervisor::quiet_crash_dialogs();
    if !args.app {
        let exe = std::env::current_exe().unwrap_or_else(|e| fail(&format!("RustyPresenter could not start: {e}")));
        let passthrough: Vec<_> = std::env::args_os().skip(1).collect();
        std::process::exit(supervisor::run(&exe, &recovery::todays_log(), &passthrough));
    }
    panic::install(recovery::todays_log());
    panic::mark_critical();
    panic::test_crash_point("startup");

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
