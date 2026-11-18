//! RustyPresenter: the operator, room and stage windows are drawn natively (Slint + libmpv). The phone
//! remote is the `/remote` web page.
//!
//! With no `--app` the executable is the supervisor, which runs itself with `--app`.

// Release builds are a windowed app with no console window behind them.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rustypresenter::{app::App, i18n, platform::{dialog, instance::{self, Bind}}, recovery::{self, exitcode, headless, panic, safe_mode::{self, Notice}, state::{self, StateWriter}, supervisor}, render::Renderer, startup, ui::{operator::window::{Operator, OperatorScreens}, ops::Ops, screens::NativeScreens}, video};
use slint::{CloseRequestResponse, winit_030::invoke_from_active_event_loop};
use std::{cell::RefCell, fs, path::Path, rc::Rc, sync::Arc};

thread_local! {
    /// The operator window, kept open on the Slint thread until the event loop ends.
    static OPERATOR: RefCell<Option<Rc<Operator>>> = const { RefCell::new(None) };
}

/// How long after the operator opens the graphics test may be offered.
const OFFER_TEST_AFTER: std::time::Duration = std::time::Duration::from_secs(4);

/// Tests only: no windows, the core and the server with a stand-in room. A dialog would wait for nobody.
fn headless_mode() -> bool { std::env::var_os("RUSTYPRESENTER_HEADLESS").is_some_and(|v| v == "1") }

fn tell(msg: &str) {
    if headless_mode() { eprintln!("{msg}") } else { dialog::error(msg) }
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
        Err(e) => fail(&i18n::tr!("RustyPresenter could not open its log: {reason}", reason = e)),
    };
    app.set_screens(Arc::new(headless::NoScreens));
    headless::run(app.clone(), log);
    let _ = rt.block_on(tokio::signal::ctrl_c());
}

fn main() {
    rustypresenter::import::libreoffice::fake_soffice_main();
    // Settings → General → Screens and graphics → "Test which is fastest" runs this program once per graphics choice.
    if let Some(test) = video::bench::child_args(std::env::args_os().skip(1)) { std::process::exit(rustypresenter::ui::graphics_test::run(test)); }
    let args = supervisor::parse_args(std::env::args_os().skip(1));
    // The computer's language until the church's saved one is read, so a problem at start is in it too.
    let _ = i18n::set_language(&i18n::resolve(""));
    supervisor::quiet_crash_dialogs();
    if !args.app {
        let exe = std::env::current_exe().unwrap_or_else(|e| fail(&i18n::tr!("RustyPresenter could not start: {reason}", reason = e)));
        let passthrough: Vec<_> = std::env::args_os().skip(1).collect();
        std::process::exit(supervisor::run(&exe, &recovery::todays_log(), &passthrough));
    }
    panic::install(recovery::todays_log());
    panic::mark_critical();
    panic::test_crash_point("startup");

    // `--no-browser` from before the native operator is still accepted, and does nothing.
    // "Restart now" (`recovery::restart`): the copy that started this one lets go of the port first.
    if let Some(pid) = recovery::restart::wait_arg(&args.rest) { recovery::restart::wait_for(pid); }
    let port = startup::port();
    let root = startup::data_root().unwrap_or_else(|e| fail(&e));
    let listener = match instance::bind(port, args.restore) {
        Bind::Ok(l) => l,
        Bind::Running => { println!("RustyPresenter is already running."); std::process::exit(exitcode::ALREADY_RUNNING); }
        Bind::Taken => { tell(&instance::taken_message(port)); std::process::exit(exitcode::PORT_TAKEN); }
    };
    let started = startup::open(listener, root, port).unwrap_or_else(|e| fail(&e));
    if let Err(e) = fs::write(started.root.join("app.pid"), std::process::id().to_string()) { eprintln!("Could not write app.pid: {e}"); }
    startup::banner(&started, "Close the operator window (or press Ctrl+C here) to quit.");
    let startup::Started { app, listener, root } = started;

    // The app's language (I18N spec D6): the one saved in Settings, or the computer's. Before any window, so the
    // first one is drawn in it.
    // The church's own translations (Settings → Translations…) over the built-in ones; a file that cannot be read
    // is left out, and the operator window says so.
    for p in i18n::load_user_folder(&root.join("translations")) { eprintln!("{p}"); }
    let saved_lang = app.data.lock().unwrap().settings.ui_language.clone();
    let lang = i18n::resolve(&saved_lang);
    let _ = i18n::set_language(&lang);
    let lang_missing = !saved_lang.is_empty() && lang != saved_lang;
    if lang_missing { eprintln!("The saved language {saved_lang} is not built in or in the translations folder; showing English."); }

    if args.restore { println!("Started with --restore, attempt {}.", args.attempt); }
    let restored = recovery::restore::at_start(&app, args.restore, args.attempt);
    app.set_state_writer(StateWriter::start(root.clone()));

    // Slint runs on this (main) thread; tokio runs the server and backups on its own threads.
    let rt = match tokio::runtime::Runtime::new() { Ok(rt) => rt, Err(e) => fail(&i18n::tr!("RustyPresenter could not start: {reason}", reason = e)) };
    rt.spawn(startup::backups(app.clone(), args.restore));
    rt.spawn(startup::serve(app.clone(), listener));
    rt.spawn(startup::save_playing(app.clone()));
    rt.spawn(startup::watch_sound(app.clone()));
    rt.spawn(rustypresenter::automation::task::ticker(app.clone()));
    // 1.3: songs set to follow the singers start listening now (FFmpeg lists the inputs: off the main thread).
    { let app = app.clone(); let _ = std::thread::Builder::new().name("follow-start".into()).spawn(move || app.start_follow_if_wanted()); }
    rt.spawn(rustypresenter::import::init(&app));
    // The open service's online Bible text that isn't in the cache yet (Nov 5 spec §5).
    { let app = app.clone(); rt.spawn(async move { app.fetch_online_text().await }); }
    if headless_mode() {
        run_headless(&app, &rt);
        forget_live_state(&app, &root);
        rt.shutdown_timeout(std::time::Duration::from_secs(2));
        return;
    }
    rt.spawn(async { if tokio::signal::ctrl_c().await.is_ok() { let _ = slint::quit_event_loop(); } });

    // Settings → General → Screens and graphics → Graphics, or software after two failed starts or with `--safe-mode`.
    let graphics = safe_mode::start(&app, &root, args.safe_mode);
    let mode = graphics.chosen.draw;
    let mpv = match video::ffi::Mpv::load() {
        Ok(m) => Some(m),
        Err(e) => { eprintln!("Video and audio will not play: {e}"); None }
    };
    println!("  Video drawing: {}{}\n", if mode == video::DrawMode::OpenGl { "GPU (OpenGL)" } else { "software" }, if mpv.is_none() { ", video player missing" } else { "" });

    rustypresenter::import::media::at_start(&app, mpv.clone(), rt.handle());
    // N13: the walk-in music's own player.
    rustypresenter::automation::music_out::start(&app, mpv.clone());

    let native = NativeScreens::new(&app, Renderer::new(), mpv);
    app.set_screens(native.clone());
    if let Some(roles) = restored { native.set_restored(roles); }
    native.start();
    // Queued after `start`'s reopen, so the room and stage are back before the operator picks its monitor.
    let (screens, handle, op_app, notices, settle_root) = (native.clone(), rt.handle().clone(), app.clone(), graphics.notices, root.clone());
    let offer_test = !args.restore;
    let queued = invoke_from_active_event_loop(move |_| {
        let ops = Ops::new(op_app.clone(), handle);
        let operator = match Operator::open(op_app, ops, Rc::new(screens.clone()) as Rc<dyn OperatorScreens>) {
            Ok(op) => op,
            Err(e) => fail(&e),
        };
        screens.place_operator(&operator);
        if lang_missing { operator.toast(rustypresenter::tr!("The language chosen in Settings is not in this version, so RustyPresenter is in English."), true); }
        for n in notices {
            match n { Notice::Dialog(t) => rustypresenter::ui::dialogs::ask::notice(&operator, &t), Notice::Toast(t) => operator.toast(t, true) }
        }
        // Stopped in the middle of a recording or a livestream last time: its part kept, and Go Live offered.
        operator.after_unfinished_recording();
        slint::Timer::single_shot(safe_mode::SETTLE, move || safe_mode::passed(&settle_root));
        // The graphics test is offered once the windows have drawn (they report the card), never after a crash.
        if offer_test {
            let me = Rc::downgrade(&operator);
            slint::Timer::single_shot(OFFER_TEST_AFTER, move || if let Some(op) = me.upgrade() { rustypresenter::ui::dialogs::settings_graphics::offer_at_start(&op) });
        }
        // Closing the operator asks nothing and quits (exit 0, so the supervisor stops too); the room
        // and stage close with it below.
        operator.window().on_close_requested(|| { let _ = slint::quit_event_loop(); CloseRequestResponse::HideWindow });
        screens.set_operator(Rc::downgrade(&operator));
        OPERATOR.with(|o| *o.borrow_mut() = Some(operator));
    });
    if let Err(e) = queued { fail(&i18n::tr!("RustyPresenter could not open its window: {reason}", reason = e)); }

    // The room and stage windows come and go; closing the operator (or Ctrl+C) quits.
    let result = slint::run_event_loop_until_quit();
    OPERATOR.with(|o| o.borrow_mut().take());
    // Closed as it should be: a recording under way then is no recording the app stopped in the middle of.
    rustypresenter::record::unfinished::clear(&app.store.root);
    native.shutdown();
    if let Err(e) = result { fail(&i18n::tr!("RustyPresenter could not run its windows: {reason}", reason = e)); }
    safe_mode::passed(&root);
    forget_live_state(&app, &root);
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    if recovery::restart::requested() { recovery::restart::relaunch(); }
}
