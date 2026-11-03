// Release builds are a windowed app with no console window behind them.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rustypresenter::{displays, startup};
use tauri::Manager;

fn fail(msg: &str) -> ! {
    displays::error_box(msg);
    std::process::exit(1);
}

fn main() {
    let headless = std::env::args().any(|a| a == "--no-open");
    // Already running? `start` asked it to bring up its operator window.
    let started = match startup::start(!headless) {
        Ok(Some(s)) => s,
        Ok(None) => { println!("RustyPresenter is already running."); return; }
        Err(e) => fail(&e),
    };
    startup::banner(&started, "Close the operator window to quit.", true);
    let startup::Started { app, listener, .. } = started;

    let setup_app = app.clone();
    let result = tauri::Builder::default()
        .setup(move |t| {
            let app = setup_app;
            app.set_probe_mpv(rustypresenter::video::ffi::Mpv::load().ok());
            tauri::async_runtime::spawn(rustypresenter::import::init(&app));
            let screens = displays::TauriScreens::new(t.handle().clone(), &app);
            app.set_screens(screens.clone());
            tauri::async_runtime::spawn(startup::backups(app.clone()));
            tauri::async_runtime::spawn(screens.watchdog());
            tauri::async_runtime::spawn(startup::serve(app.clone(), listener));

            if !headless {
                app.open_window("operator", None)?;
                // Reopen the room/stage screens where they were last time, if those screens are plugged in.
                let roles = app.data.lock().unwrap().settings.displays.clone();
                let mons = displays::monitors();
                for (role, id) in [("room", roles.room), ("stage", roles.stage)] {
                    if let Some(id) = id { if mons.iter().any(|m| m.id == id && !m.primary) { let _ = app.open_window(role, Some(&id)); } }
                }
            }
            Ok(())
        })
        .on_window_event(|w, ev| {
            // Closing the operator window quits, taking the room and stage screens with it.
            if matches!(ev, tauri::WindowEvent::CloseRequested { .. }) && w.label().starts_with("operator-") {
                w.app_handle().exit(0);
            }
        })
        .run(tauri::generate_context!());
    if let Err(e) = result { fail(&format!("RustyPresenter could not start its windows: {e}")); }
}
