//! Settings → Auto-follow → Speech model: Fast (English) or Best (all languages), downloaded off the UI thread
//! with progress and Cancel (a cut-off download goes on where it stopped), Installed ✓ with Remove, and Test
//! speed, which says which model suits this computer. A finished download makes auto-follow listen again with it.
use crate::{
    follow::{ear::Choice, fetch},
    ui::{SettingsDlg, operator::window::Operator},
};
use slint::ComponentHandle;
use std::{cell::{Cell, RefCell}, path::{Path, PathBuf}, rc::Rc};
use tokio_util::sync::CancellationToken;

thread_local! {
    /// The download running, and which model it is.
    static DOWNLOAD: RefCell<Option<(Choice, CancellationToken)>> = const { RefCell::new(None) };
    /// The "needs a speech model" question was asked this session.
    static OFFERED: Cell<bool> = const { Cell::new(false) };
    /// Tests keep models in a folder of their own.
    #[cfg(test)]
    pub(crate) static TEST_DEPS: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// `<data>/deps`, or a test's own folder.
fn deps() -> PathBuf {
    // A test never looks in this computer's own data folder: with a model downloaded there (as on Jake's PC since
    // November 14) "no model yet" would not be true. Without a folder of its own it gets one that is not there.
    #[cfg(test)]
    return TEST_DEPS.with(|d| d.borrow().clone()).unwrap_or_else(|| std::env::temp_dir().join("rp-tests-have-no-speech-model"));
    #[cfg(not(test))]
    fetch::deps_dir()
}

/// The model of the choice on screen: 0 Fast, 1 Best.
pub fn pick(i: i32) -> Choice { if i == 1 { Choice::Best } else { Choice::Fast } }

fn model(c: Choice) -> &'static fetch::Model { c.model().unwrap_or(&fetch::FAST) }

/// "378 MB", "1.26 GB".
pub fn size_text(bytes: u64) -> String {
    let mb = bytes as f64 / 1_000_000.0;
    if mb >= 1000.0 { crate::tr!("{size} GB", size = format!("{:.2}", mb / 1000.0)) } else { crate::tr!("{size} MB", size = format!("{mb:.0}")) }
}

/// Some speech model is downloaded.
pub fn any_installed(deps: &Path) -> bool { [&fetch::FAST, &fetch::BEST].into_iter().any(|m| fetch::installed(deps, m).is_some()) }

/// What Test speed says, in plain words.
pub fn speed_words(choice: Choice, deps: &Path) -> String {
    let best = fetch::installed(deps, &fetch::BEST).is_some();
    match choice {
        Choice::Best => crate::tr!("Best runs fine here."),
        Choice::Fast if best => crate::tr!("Use Fast on this computer: Best is too slow here."),
        Choice::Fast => crate::tr!("Fast runs fine here. Download Best to check it too."),
        Choice::Off if !any_installed(deps) => crate::tr!("Download a model first, then test its speed."),
        Choice::Off => crate::tr!("This computer is too slow for the speech models: auto-follow can't hear the words here, so it can't follow the songs."),
    }
}

fn error_words(e: &crate::youtube::fetch::FetchError) -> String {
    use crate::youtube::fetch::FetchError as E;
    match e {
        E::Offline(_) | E::Http(..) => crate::tr!("Couldn't reach the download site. Check the internet and try again: it goes on where it stopped."),
        E::Checksum => crate::tr!("The download was damaged and was thrown away. Try again."),
        E::DiskFull => crate::tr!("The disk is full. Make room and try again."),
        E::Cancelled => crate::tr!("Download stopped. Download again to go on where it stopped."),
        E::NoBuild | E::Other(_) => crate::tr!("The download didn't work. Try again."),
    }
}

/// Shows the chosen model: installed, downloading or not yet.
pub(super) fn render(g: &SettingsDlg) {
    let c = pick(g.get_model_pick());
    let m = model(c);
    g.set_model_size(size_text(m.size).into());
    g.set_model_installed(fetch::installed(&deps(), m).is_some());
    let running = DOWNLOAD.with(|d| d.borrow().as_ref().map(|(c, _)| *c));
    g.set_model_downloading(running == Some(c));
    g.set_model_busy(running.is_some());
}

/// Connects the model buttons and shows the state (Settings opened).
pub(super) fn load(op: &Rc<Operator>) {
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    if DOWNLOAD.with(|d| d.borrow().is_none()) { g.set_model_status("".into()) }
    g.set_model_speed("".into());
    render(&g);
    let me = Rc::downgrade(op);
    g.on_model_pick_changed(move |_| if let Some(op) = me.upgrade() { render(&crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>()) });
    let me = Rc::downgrade(op);
    g.on_model_download(move || if let Some(op) = me.upgrade() { download(&op, pick(crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>().get_model_pick())) });
    g.on_model_cancel(|| DOWNLOAD.with(|d| if let Some((_, c)) = d.borrow().as_ref() { c.cancel() }));
    let me = Rc::downgrade(op);
    g.on_model_remove(move || if let Some(op) = me.upgrade() { remove(&op) });
    let me = Rc::downgrade(op);
    g.on_model_speed_check(move || if let Some(op) = me.upgrade() { speed_check(&op) });
}

/// Downloads `c` from Hugging Face on its own thread; the progress shows in Settings. When it is done, auto-follow
/// listens again with the new model.
pub(crate) fn download(op: &Rc<Operator>, c: Choice) {
    if DOWNLOAD.with(|d| d.borrow().is_some()) { return }
    let cancel = CancellationToken::new();
    DOWNLOAD.with(|d| *d.borrow_mut() = Some((c, cancel.clone())));
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    g.set_model_progress(0.0);
    g.set_model_status(crate::tr!("Downloading…").into());
    render(&g);
    let (ui, app, deps, m) = (crate::ui::dialogs::settings_window::win(&op).as_weak(), op.app().clone(), deps(), model(c));
    let total = m.size;
    let progress = {
        let ui = ui.clone();
        move |done: u64, all: u64| {
            let f = if all == 0 { 0.0 } else { done as f32 / all as f32 };
            let words = crate::tr!("Downloading… {done} of {total}", done = size_text(done), total = size_text(all.max(total)));
            let _ = ui.upgrade_in_event_loop(move |ui| {
                let g = ui.global::<SettingsDlg>();
                g.set_model_progress(f);
                g.set_model_status(words.into());
            });
        }
    };
    let work = move || {
        let r = tokio::runtime::Builder::new_current_thread().enable_all().build()
            .map_err(|e| crate::youtube::fetch::FetchError::Other(e.to_string()))
            .and_then(|rt| rt.block_on(fetch::ensure(&deps, m, fetch::HUGGING_FACE, &progress, &cancel)));
        let ok = r.is_ok();
        if ok { app.listen_again() }
        let words = match &r { Ok(_) => crate::tr!("Installed. Auto-follow now listens with it."), Err(e) => error_words(e) };
        let _ = ui.upgrade_in_event_loop(move |ui| finished(&ui.global::<SettingsDlg>(), words));
    };
    if let Err(e) = std::thread::Builder::new().name("follow-model".into()).spawn(work) {
        finished(&g, e.to_string());
    }
}

fn finished(g: &SettingsDlg, words: String) {
    DOWNLOAD.with(|d| d.borrow_mut().take());
    g.set_model_status(words.into());
    render(g);
}

/// Remove: the model's folder goes, and auto-follow listens again (with the other model, or none).
fn remove(op: &Rc<Operator>) {
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let m = model(pick(g.get_model_pick()));
    let dir = fetch::models_dir(&deps()).join(m.folder);
    let words = match std::fs::remove_dir_all(&dir) {
        Ok(()) => crate::tr!("Removed."),
        Err(e) => crate::tr!("Couldn't remove it: {error}", error = e),
    };
    // The speed check runs again for what is left.
    let _ = std::fs::remove_file(fetch::models_dir(&deps()).join("choice.json"));
    g.set_model_status(words.into());
    render(&g);
    if !op.ops().is_inline() {
        let app = op.app().clone();
        let _ = std::thread::Builder::new().name("follow-start".into()).spawn(move || app.listen_again());
    }
}

/// Test speed: runs each downloaded model on a little sound, off the UI thread, and says which suits this PC.
fn speed_check(op: &Rc<Operator>) {
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    let deps = deps();
    if !any_installed(&deps) { return g.set_model_speed(speed_words(Choice::Off, &deps).into()) }
    g.set_model_checking(true);
    g.set_model_speed(crate::tr!("Testing… this takes a few seconds.").into());
    let ui = crate::ui::dialogs::settings_window::win(&op).as_weak();
    let _ = std::thread::Builder::new().name("follow-speed".into()).spawn(move || {
        let words = speed_words(crate::follow::model::speed_check(), &deps);
        let _ = ui.upgrade_in_event_loop(move |ui| {
            let g = ui.global::<SettingsDlg>();
            g.set_model_checking(false);
            g.set_model_speed(words.into());
        });
    });
}

/// The first time this session a song is set to Suggest or Auto with no model downloaded: offer Fast.
pub(crate) fn offer(op: &Rc<Operator>) {
    if OFFERED.with(|o| o.get()) || any_installed(&deps()) || DOWNLOAD.with(|d| d.borrow().is_some()) { return }
    OFFERED.with(|o| o.set(true));
    let question = crate::tr!("Auto-follow needs a speech model ({size}). Download now?", size = size_text(fetch::FAST.size));
    let me = Rc::downgrade(op);
    super::confirm(op, &question, &crate::tr!("Download"), &crate::tr!("Not now"), false, move |yes| {
        let Some(op) = me.upgrade().filter(|_| yes) else { return };
        super::settings_follow::open_page(&op, 0);
        crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>().set_model_pick(0);
        download(&op, Choice::Fast);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, ui::{dialogs::{SettingsTab, settings}, operator::window::testing::{open, refresh}}};

    fn temp_deps() -> PathBuf {
        let d = std::env::temp_dir().join(format!("rp-model-{}", uuid::Uuid::new_v4()));
        TEST_DEPS.with(|t| *t.borrow_mut() = Some(d.clone()));
        d
    }

    /// A model's files, as a finished download leaves them (tiny stand-ins).
    fn put(deps: &Path, m: &fetch::Model) {
        let dir = fetch::models_dir(deps).join(m.folder);
        std::fs::create_dir_all(&dir).unwrap();
        for f in [m.weights, fetch::LICENCE_FILE].into_iter().chain(fetch::SMALL_FILES) { std::fs::write(dir.join(f), "x").unwrap() }
    }

    #[test]
    fn sizes_and_speed_words_are_plain() {
        assert_eq!((size_text(fetch::FAST.size), size_text(fetch::BEST.size)), ("378 MB".to_string(), "1.26 GB".to_string()));
        let deps = temp_deps();
        assert_eq!(speed_words(Choice::Off, &deps), "Download a model first, then test its speed.");
        put(&deps, &fetch::FAST);
        assert_eq!(speed_words(Choice::Fast, &deps), "Fast runs fine here. Download Best to check it too.");
        put(&deps, &fetch::BEST);
        assert_eq!(speed_words(Choice::Best, &deps), "Best runs fine here.");
        assert_eq!(speed_words(Choice::Fast, &deps), "Use Fast on this computer: Best is too slow here.");
        assert!(speed_words(Choice::Off, &deps).starts_with("This computer is too slow"));
        TEST_DEPS.with(|t| t.borrow_mut().take());
    }

    #[test]
    fn model_choice_shows_installed_and_removes() {
        crate::ui::ui_test_init();
        let deps = temp_deps();
        put(&deps, &fetch::BEST);
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        settings::open(&op, SettingsTab::AutoFollow);
        let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
        assert_eq!((g.get_model_pick(), g.get_model_size().as_str(), g.get_model_installed()), (0, "378 MB", false), "Fast: not yet");
        assert!(!g.get_model_downloading() && !g.get_model_busy(), "Download shows");
        g.set_model_pick(1);
        g.invoke_model_pick_changed(1);
        assert_eq!((g.get_model_size().as_str(), g.get_model_installed()), ("1.26 GB", true), "Best: installed ✓");
        g.invoke_model_remove();
        assert!(!g.get_model_installed());
        assert!(fetch::installed(&deps, &fetch::BEST).is_none(), "its folder is gone");
        assert_eq!(g.get_model_status(), "Removed.");
        g.invoke_model_speed_check();
        assert_eq!(g.get_model_speed(), "Download a model first, then test its speed.");
        TEST_DEPS.with(|t| t.borrow_mut().take());
    }
}
