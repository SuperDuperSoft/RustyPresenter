//! `rustypresenter-native`: the same core and phone/web server as the Tauri app, with the operator,
//! room and stage windows drawn natively (Slint + libmpv).
//!
//! With no `--app` the executable is the supervisor, which runs itself with `--app`.
use rustypresenter::{displays, recovery::{self, exitcode, panic, supervisor}, render::Renderer, startup, ui::{operator::window::Operator, ops::Ops, screens::NativeScreens}, video};
use std::rc::Rc;

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

    // `--no-browser` from before the native operator is still accepted, and does nothing.
    let started = match startup::start(true) {
        Ok(Some(s)) => s,
        Ok(None) => { println!("RustyPresenter is already running."); std::process::exit(exitcode::ALREADY_RUNNING); }
        Err(e) => fail(&e),
    };
    startup::banner(&started, "Close the operator window, or press Ctrl+C here, to quit.");
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
    let operator = match Operator::open(app.clone(), Ops::new(app.clone(), rt.handle().clone()), Rc::new(native.clone())) {
        Ok(op) => op,
        Err(e) => fail(&e),
    };
    native.set_operator(Rc::downgrade(&operator));
    native.place_operator();
    // Closing the operator asks nothing and quits; the room and stage windows close with it.
    operator.window().on_close_requested(|| {
        let _ = slint::quit_event_loop();
        slint::CloseRequestResponse::HideWindow
    });

    // The room and stage windows come and go; the event loop runs until the operator closes or Ctrl+C.
    let result = slint::run_event_loop_until_quit();
    drop(operator);
    native.shutdown();
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    if let Err(e) = result { fail(&format!("RustyPresenter could not run its windows: {e}")); }
}
