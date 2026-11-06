// Release builds are a windowed app with no console window behind them.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! RustyPresenter: the core and phone/web server, with the operator, room and stage windows drawn natively (Slint
//! + libmpv). No browser opens; the phone remote is the `/remote` web page.
//!
//! With no `--app` the executable is the supervisor, which runs itself with `--app` and restarts it
//! with `--restore` after a crash. `RUSTYPRESENTER_HEADLESS=1` (tests only) runs the core and the
//! server with a headless room instead of any window.
use rustypresenter::{
    app::{App, Screens as _},
    import::libreoffice,
    platform::{dialog, instance::{self, Bind}},
    recovery::{self, exitcode, headless, panic, state::{self, StateWriter}, strings, supervisor},
    render::Renderer,
    startup,
    ui::{operator::window::{Operator, OperatorScreens}, ops::Ops, screens::NativeScreens},
    video,
};
use slint::{CloseRequestResponse, winit_030::invoke_from_active_event_loop};
use std::{cell::RefCell, path::Path, rc::Rc, sync::Arc};

thread_local! {
    /// The operator window, kept open on the Slint thread until the event loop ends.
    static OPERATOR: RefCell<Option<Rc<Operator>>> = const { RefCell::new(None) };
}

fn headless() -> bool { std::env::var_os("RUSTYPRESENTER_HEADLESS").is_some_and(|v| v == "1") }

/// Says why the app stops: in a dialog, or only in the log when headless (no one to click it away).
fn tell(msg: &str) {
    if headless() { eprintln!("{msg}") } else { dialog::error(msg) }
}

fn fail(msg: &str) -> ! {
    tell(msg);
    std::process::exit(exitcode::CONFIG);
}

fn main() {
    // Tests only: run as `soffice --headless …` with `RP_FAKE_SOFFICE` set, this is LibreOffice's stand-in.
    libreoffice::fake_soffice_main();
    let args = supervisor::parse_args(std::env::args_os().skip(1));
    supervisor::quiet_crash_dialogs();
    if !args.app {
        let exe = std::env::current_exe().unwrap_or_else(|e| fail(&format!("RustyPresenter could not start: {e}")));
        let passthrough: Vec<_> = std::env::args_os().skip(1).collect();
        std::process::exit(supervisor::run(&exe, &recovery::todays_log(), &passthrough));
    }
    panic::install(recovery::todays_log());
    panic::mark_critical();
    // The app's output goes to the log (the supervisor's doing): say how this run started.
    let given: Vec<_> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    println!("{} app started: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), given.join(" "));
    panic::test_crash_point("startup");

    // `--no-browser` (from when the operator was the web page) is accepted and ignored for one release.
    let port = startup::port();
    let listener = match instance::bind(port, args.restore) {
        Bind::Ok(l) => l,
        Bind::Running => { println!("RustyPresenter is already running."); std::process::exit(exitcode::ALREADY_RUNNING); }
        Bind::Taken => { tell(&strings::port_taken(port)); std::process::exit(exitcode::PORT_TAKEN); }
    };
    let started = startup::open(listener, port).unwrap_or_else(|e| fail(&e));
    // Which process to end to test a crash restart (Task Manager, `kill -9`, the recovery tests).
    if let Err(e) = std::fs::write(started.root.join("app.pid"), std::process::id().to_string()) { eprintln!("Could not write app.pid: {e}"); }
    // A LibreOffice left by a crashed app goes before anything touches its profile.
    libreoffice::kill_leftover(&started.root.join("deps"));
    startup::banner(&started, "Close the operator window (or press Ctrl+C here) to quit.", false);
    let startup::Started { app, listener, root } = started;

    // Slint runs on this (main) thread; tokio runs the server and backups on its own threads.
    let rt = match tokio::runtime::Runtime::new() { Ok(rt) => rt, Err(e) => fail(&format!("RustyPresenter could not start: {e}")) };
    rt.spawn(startup::backups(app.clone(), args.restore));
    rt.spawn(startup::serve(app.clone(), listener));

    let reopen = recovery::restore_on_start(&app, args.restore, args.attempt);
    app.set_state_writer(StateWriter::start(root.clone()));
    rt.spawn(recovery::save_while_playing(app.clone()));

    if headless() {
        let screens = Arc::new(headless::Screens::default());
        for role in reopen.iter().flatten() { let _ = screens.open(role, None); }
        app.set_screens(screens);
        let _room = headless::run(app.clone(), std::io::stdout());
        rt.block_on(async { let _ = tokio::signal::ctrl_c().await; });
        quit(&app, &root, rt);
    }
    rt.spawn(async { if tokio::signal::ctrl_c().await.is_ok() { let _ = slint::quit_event_loop(); } });

    let mode = video::choose_backend();
    let mpv = match video::ffi::Mpv::load() {
        Ok(m) => Some(m),
        Err(e) => { eprintln!("Video and audio will not play: {e}"); None }
    };
    app.ready_old_media();
    // Videos in the library without a picture get one in the background.
    if let Some(m) = &mpv {
        app.set_probe_mpv(m.clone());
        let _rt = rt.enter();
        app.fill_video_thumbs(app.videos_without_thumbs());
    }
    println!("  Video drawing: {}{}\n", if mode == video::DrawMode::OpenGl { "GPU (OpenGL)" } else { "software" }, if mpv.is_none() { ", video player missing" } else { "" });

    let native = NativeScreens::new(&app, Renderer::new(), mpv);
    app.set_screens(native.clone());
    native.restore_roles(reopen);
    native.start();
    // Queued after `start`'s reopen, so the room and stage are back before the operator picks its monitor.
    let (screens, handle, op_app) = (native.clone(), rt.handle().clone(), app.clone());
    let queued = invoke_from_active_event_loop(move |_| {
        let ops = Ops::new(op_app.clone(), handle);
        let operator = match Operator::open(op_app, ops, Rc::new(screens.clone()) as Rc<dyn OperatorScreens>) {
            Ok(op) => op,
            Err(e) => fail(&e),
        };
        screens.place_operator(&operator);
        // Closing the operator asks nothing and quits (exit 0, so the supervisor stops too); the room
        // and stage close with it below.
        operator.window().on_close_requested(|| { let _ = slint::quit_event_loop(); CloseRequestResponse::HideWindow });
        screens.set_operator(Rc::downgrade(&operator));
        OPERATOR.with(|o| *o.borrow_mut() = Some(operator));
    });
    if let Err(e) = queued { fail(&format!("RustyPresenter could not open its window: {e}")); }

    // The room and stage windows come and go; closing the operator (or Ctrl+C) quits.
    let result = slint::run_event_loop_until_quit();
    OPERATOR.with(|o| o.borrow_mut().take());
    native.shutdown();
    if let Err(e) = result { fail(&format!("RustyPresenter could not run its windows: {e}")); }
    quit(&app, &root, rt);
}

/// A normal quit: the last frame is written and then forgotten, so the next start does not restore it, and
/// the app exits 0, which stops the supervisor too.
fn quit(app: &App, root: &Path, rt: tokio::runtime::Runtime) -> ! {
    app.stop_state_writer();
    state::clear(root);
    let _ = std::fs::remove_file(root.join("app.pid"));
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    std::process::exit(exitcode::OK);
}
