//! `rustypresenter-native`: the same core and phone/web server as the Tauri app, with the operator,
//! room and stage windows drawn natively (Slint + libmpv). No browser opens; the phone remote is
//! still the `/remote` web page, and the web operator page is still served for the Tauri app.
//!
//! With no `--app` the executable is the supervisor, which runs itself with `--app`.
use rustypresenter::{app::App, displays, platform::instance::{self, Bind}, recovery::{self, exitcode, headless, panic, state::{self, StateWriter}, supervisor}, render::Renderer, startup, ui::{operator::window::{Operator, OperatorScreens}, ops::Ops, screens::NativeScreens}, video};
use slint::{CloseRequestResponse, winit_030::invoke_from_active_event_loop};
use std::{cell::RefCell, fs, path::Path, rc::Rc, sync::Arc};

thread_local! {
    /// The operator window, kept open on the Slint thread until the event loop ends.
    static OPERATOR: RefCell<Option<Rc<Operator>>> = const { RefCell::new(None) };
}

/// Tests only: no windows, the core and the server with a stand-in room. A dialog would wait for nobody.
fn headless_mode() -> bool { std::env::var_os("RUSTYPRESENTER_HEADLESS").is_some_and(|v| v == "1") }

fn tell(msg: &str) {
    if headless_mode() { eprintln!("{msg}") } else { displays::error_box(msg) }
}

fn fail(msg: &str) -> ! {
    tell(msg);
    std::process::exit(exitcode::CONFIG);
}

/// A normal quit: the frame on air is not to come back next time.
fn forget_live_state(app: &App, root: &Path) {
    app.stop_state_writer();
    state::clear(root);
}

/// Runs until Ctrl+C with the stand-in room. Crashes and kills are what the tests want from it.
fn run_headless(app: &Arc<App>, rt: &tokio::runtime::Runtime) {
    let log = match fs::OpenOptions::new().create(true).append(true).open(recovery::todays_log()) {
        Ok(f) => f,
        Err(e) => fail(&format!("RustyPresenter could not open its log: {e}")),
    };
    app.set_screens(Arc::new(headless::NoScreens));
    headless::run(app.clone(), log);
    let _ = rt.block_on(tokio::signal::ctrl_c());
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
    let port = startup::port();
    let root = startup::data_root().unwrap_or_else(|e| fail(&e));
    let listener = match instance::bind(port, args.restore) {
        Bind::Ok(l) => l,
        Bind::Running => { println!("RustyPresenter is already running."); std::process::exit(exitcode::ALREADY_RUNNING); }
        Bind::Taken => { tell(&instance::taken_message(port)); std::process::exit(exitcode::PORT_TAKEN); }
    };
    let started = startup::open(listener, root, port).unwrap_or_else(|e| fail(&e));
    if let Err(e) = fs::write(started.root.join("app.pid"), std::process::id().to_string()) { eprintln!("Could not write app.pid: {e}"); }
    startup::banner(&started, "Close the operator window (or press Ctrl+C here) to quit.", false);
    let startup::Started { app, listener, root } = started;

    if args.restore { println!("Started with --restore, attempt {}.", args.attempt); }
    let restored = recovery::restore::at_start(&app, args.restore, args.attempt);
    app.set_state_writer(StateWriter::start(root.clone()));

    // Slint runs on this (main) thread; tokio runs the server and backups on its own threads.
    let rt = match tokio::runtime::Runtime::new() { Ok(rt) => rt, Err(e) => fail(&format!("RustyPresenter could not start: {e}")) };
    rt.spawn(startup::backups(app.clone(), args.restore));
    rt.spawn(startup::serve(app.clone(), listener));
    rt.spawn(startup::save_playing(app.clone()));
    if headless_mode() {
        run_headless(&app, &rt);
        forget_live_state(&app, &root);
        rt.shutdown_timeout(std::time::Duration::from_secs(2));
        return;
    }
    rt.spawn(async { if tokio::signal::ctrl_c().await.is_ok() { let _ = slint::quit_event_loop(); } });

    let mode = video::choose_backend();
    let mpv = match video::ffi::Mpv::load() {
        Ok(m) => Some(m),
        Err(e) => { eprintln!("Video and audio will not play: {e}"); None }
    };
    println!("  Video drawing: {}{}\n", if mode == video::DrawMode::OpenGl { "GPU (OpenGL)" } else { "software" }, if mpv.is_none() { ", video player missing" } else { "" });

    rustypresenter::import::media::at_start(&app, mpv.clone(), rt.handle());
    rt.spawn(rustypresenter::import::init(&app));

    let native = NativeScreens::new(&app, Renderer::new(), mpv);
    app.set_screens(native.clone());
    if let Some(roles) = restored { native.set_restored(roles); }
    native.start();
    // Queued after `start`'s reopen, so the room and stage are back before the operator picks its monitor.
    let (screens, handle, op_app) = (native.clone(), rt.handle().clone(), app.clone());
    let queued = invoke_from_active_event_loop(move |_| {
        let ops = Ops::new(op_app.clone(), handle);
        let operator = match Operator::open(op_app, ops, Rc::new(screens.clone()) as Rc<dyn OperatorScreens>) {
            Ok(op) => op,
            Err(e) => fail(&e),
        };
        screens.place_operator(operator.window());
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
    forget_live_state(&app, &root);
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
}
