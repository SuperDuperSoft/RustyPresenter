//! "Is there a newer version?" for the operator (`crate::update`): the row in Settings → Help and about (the
//! switch, what the last look found, Check now, and the button to its download page), and the one question at
//! start when a newer version is found.
use crate::{
    tr,
    ui::{SettingsDlg, operator::window::Operator},
    update::{self, State},
    youtube::fetch::FetchError,
};
use slint::ComponentHandle;
use std::rc::Rc;

/// This copy's version.
const CURRENT: &str = env!("CARGO_PKG_VERSION");

fn root(op: &Operator) -> std::path::PathBuf { op.app().store.root.clone() }

fn dlg(op: &Operator) -> SettingsDlg<'static> { crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>() }

/// What the row says about the last look. Nothing before the first one.
pub fn status(state: &State, current: &str) -> String {
    match (state.newer_than(current), state.checked, &state.latest) {
        (Some(v), _, _) => tr!("Version {version} is available. You have {current}.", version = v, current = current),
        (None, Some(_), Some(_)) => tr!("You have the newest version ({current}).", current = current),
        (None, Some(_), None) => tr!("No version has been published for download yet. You have {current}.", current = current),
        (None, None, _) => String::new(),
    }
}

/// Why Check now could not check.
pub fn why_not(e: &FetchError) -> String {
    match e {
        FetchError::Offline(_) => tr!("Could not check: no internet connection."),
        e => tr!("Could not check: {reason}", reason = e.detail().unwrap_or_default()),
    }
}

/// The button to the newer version's page; no button when there is none.
fn get_label(state: &State, current: &str) -> String {
    state.newer_than(current).map(|v| tr!("Get version {version}…", version = v)).unwrap_or_default()
}

fn show(op: &Operator, state: &State) {
    let g = dlg(op);
    g.set_update_status(status(state, CURRENT).into());
    g.set_update_get(get_label(state, CURRENT).into());
}

/// Fills the row from what the last look found.
pub(super) fn fill(op: &Rc<Operator>) {
    dlg(op).set_update_busy(false);
    show(op, &update::load(&root(op)));
}

/// Check now: asks GitHub whatever the switch says, and says what came of it.
pub(super) fn check_now(op: &Rc<Operator>) {
    let g = dlg(op);
    if g.get_update_busy() { return }
    g.set_update_busy(true);
    g.set_update_status(tr!("Checking…").into());
    let me = Rc::downgrade(op);
    op.ops().update_check(move |r| {
        let Some(op) = me.upgrade() else { return };
        let g = dlg(&op);
        g.set_update_busy(false);
        match r {
            Ok(state) => show(&op, &state),
            Err(e) => {
                // What was found earlier still stands; only the line says the look failed.
                show(&op, &update::load(&root(&op)));
                g.set_update_status(why_not(&e).into());
            }
        }
    });
}

/// Opens the newer version's page in the web browser.
pub(super) fn download(op: &Rc<Operator>) {
    let state = update::load(&root(op));
    if state.newer_than(CURRENT).is_none() { return }
    let Some(tag) = state.latest else { return };
    open(op, &tag);
}

fn open(op: &Operator, tag: &str) {
    match update::open_page(tag) {
        Ok(()) => op.toast(tr!("Your web browser is opening the download page. Install it when no service is running."), false),
        Err(e) => op.toast(tr!("Could not open your web browser: {reason}", reason = e), true),
    }
}

/// Something is on the screens, or a question is already up: no time to talk about versions.
fn busy(op: &Operator) -> bool {
    let frame = op.app().live_view().frame;
    frame.slide.is_some() || frame.media.is_some() || frame.audio.is_some() || op.state.borrow().asks > 0
}

/// At start, a little after the window opens: with the switch on, asks GitHub when a day has passed, and asks the
/// person once about a newer version. Without internet, a newer version found on an earlier day is still offered.
pub fn at_start(op: &Rc<Operator>) {
    if !op.app().data.lock().unwrap().settings.check_updates { return }
    let state = update::load(&root(op));
    if !state.due(chrono::Utc::now()) { return tell(op, &state) }
    let me = Rc::downgrade(op);
    op.ops().update_check(move |r| {
        let Some(op) = me.upgrade() else { return };
        // No internet, or GitHub in trouble: not a word; what an earlier look found still counts.
        let state = r.unwrap_or_else(|_| update::load(&root(&op)));
        tell(&op, &state);
    });
}

/// Asks once about a newer version; never while something is on the screens (it asks at the next start instead).
fn tell(op: &Rc<Operator>, state: &State) {
    let Some(version) = state.to_tell(CURRENT) else { return };
    let Some(tag) = state.latest.clone() else { return };
    if busy(op) { return }
    update::mark_told(&root(op), &version);
    let text = tr!("RustyPresenter {version} is available. You have {current}. Get it now? Your web browser opens its download page.", version = version, current = CURRENT);
    let me = Rc::downgrade(op);
    super::ask::confirm(op, &text, &tr!("Get it"), &crate::video::bench::not_now(), false, move |yes| {
        let Some(op) = me.upgrade() else { return };
        if yes { open(&op, &tag) }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::test_app,
        ui::{dialogs::{SettingsTab, ask, settings, settings::testing::find_any as find}, operator::window::testing::{open as open_window, refresh}},
        update::testing::{BASE, OPENED},
        youtube::testing::FakeReleases,
    };
    use slint::platform::PointerEventButton;

    const LATEST: &str = "/SuperDuperSoft/RustyPresenter/releases/latest";

    /// A stand-in GitHub whose newest release is `tag`, looked at by this thread's checks.
    fn github(tag: Option<&str>) -> FakeReleases {
        let web = crate::ui::ops::block_on_inline(FakeReleases::start());
        if let Some(tag) = tag { web.point(LATEST, &format!("/SuperDuperSoft/RustyPresenter/releases/tag/{tag}")) }
        BASE.with(|b| *b.borrow_mut() = Some(web.base("/SuperDuperSoft/RustyPresenter/releases")));
        OPENED.with(|o| o.borrow_mut().clear());
        web
    }

    fn opened_pages() -> Vec<String> { OPENED.with(|o| o.borrow().clone()) }

    fn toasts(op: &Operator) -> Vec<String> { op.state.borrow().toasts.iter().map(|t| t.0.clone()).collect() }

    #[test]
    fn the_row_says_what_the_last_look_found() {
        let now = Some(chrono::Utc::now());
        assert_eq!(status(&State::default(), "1.4.6"), "", "never looked");
        assert_eq!(status(&State { checked: now, latest: None, told: None }, "1.4.6"), "No version has been published for download yet. You have 1.4.6.");
        let same = State { checked: now, latest: Some("v1.4.6".into()), told: None };
        assert_eq!(status(&same, "1.4.6"), "You have the newest version (1.4.6).");
        assert_eq!(get_label(&same, "1.4.6"), "");
        // GitHub behind this copy: nothing to offer.
        assert_eq!(status(&State { latest: Some("v1.3.0".into()), ..same.clone() }, "1.4.6"), "You have the newest version (1.4.6).");
        let newer = State { latest: Some("v1.5.0".into()), ..same };
        assert_eq!(status(&newer, "1.4.6"), "Version 1.5.0 is available. You have 1.4.6.");
        assert_eq!(get_label(&newer, "1.4.6"), "Get version 1.5.0…");
        assert_eq!(why_not(&FetchError::Offline("x".into())), "Could not check: no internet connection.");
        assert_eq!(why_not(&FetchError::Http(503, "https://x".into())), "Could not check: HTTP 503 for https://x");
    }

    #[test]
    fn check_now_finds_a_newer_version_and_opens_its_page() {
        let app = test_app();
        let op = open_window(&app);
        refresh(&op);
        let _web = github(Some("v99.0.0"));
        settings::open(&op, SettingsTab::About);
        let g = dlg(&op);
        assert!(g.get_check_updates(), "on unless switched off");
        assert_eq!(g.get_update_status(), "", "never looked");
        assert!(find(&op, "SettingsAbout::update-download").is_none());
        find(&op, "SettingsAbout::update-check").unwrap().mock_single_click(PointerEventButton::Left);
        slint::platform::update_timers_and_animations();
        assert_eq!(g.get_update_status(), format!("Version 99.0.0 is available. You have {CURRENT}.").as_str());
        assert!(!g.get_update_busy());
        find(&op, "SettingsAbout::update-download").unwrap().mock_single_click(PointerEventButton::Left);
        assert_eq!(opened_pages(), ["https://github.com/SuperDuperSoft/RustyPresenter/releases/tag/v99.0.0"]);
        assert!(toasts(&op).iter().any(|t| t.starts_with("Your web browser is opening the download page.")));
        // Shown again when Settings is opened the next time, without asking GitHub.
        BASE.with(|b| *b.borrow_mut() = None);
        fill(&op);
        assert_eq!(g.get_update_get(), "Get version 99.0.0…");
        // Without internet Check now says so, and the version found earlier stays on offer.
        check_now(&op);
        assert_eq!(g.get_update_status(), "Could not check: no internet connection.");
        assert_eq!(g.get_update_get(), "Get version 99.0.0…");
    }

    #[test]
    fn check_now_with_nothing_newer_or_nothing_published() {
        let app = test_app();
        let op = open_window(&app);
        refresh(&op);
        let web = github(None);
        settings::open(&op, SettingsTab::About);
        check_now(&op);
        assert_eq!(dlg(&op).get_update_status(), format!("No version has been published for download yet. You have {CURRENT}.").as_str());
        web.point(LATEST, "/SuperDuperSoft/RustyPresenter/releases/tag/v0.9.0");
        check_now(&op);
        assert_eq!(dlg(&op).get_update_status(), format!("You have the newest version ({CURRENT}).").as_str());
        assert_eq!(dlg(&op).get_update_get(), "");
        download(&op);
        assert!(opened_pages().is_empty(), "nothing to get");
    }

    #[test]
    fn at_start_a_newer_version_is_offered_once() {
        let app = test_app();
        let op = open_window(&app);
        refresh(&op);
        let web = github(Some("v99.0.0"));
        at_start(&op);
        assert_eq!(op.state.borrow().asks, 1, "asked");
        ask::answer(&op, false, String::new());
        assert!(opened_pages().is_empty(), "Not now opens nothing");
        let hits = web.hits();
        at_start(&op);
        assert_eq!(op.state.borrow().asks, 0, "told once");
        assert_eq!(web.hits(), hits, "and GitHub is asked once a day");
        // A day later, a newer one again: asked, and Get it opens its page.
        let dir = root(&op);
        let mut state = update::load(&dir);
        state.checked = Some(chrono::Utc::now() - chrono::Duration::hours(25));
        update::save(&dir, &state);
        web.point(LATEST, "/SuperDuperSoft/RustyPresenter/releases/tag/v99.1.0");
        at_start(&op);
        assert_eq!(op.state.borrow().asks, 1);
        ask::answer(&op, true, String::new());
        assert_eq!(opened_pages(), ["https://github.com/SuperDuperSoft/RustyPresenter/releases/tag/v99.1.0"]);
    }

    #[test]
    fn at_start_nothing_is_said_when_off_offline_or_up_to_date() {
        let app = test_app();
        let op = open_window(&app);
        refresh(&op);
        // Switched off: GitHub is not asked.
        let web = github(Some("v99.0.0"));
        app.data.lock().unwrap().settings.check_updates = false;
        at_start(&op);
        assert_eq!((web.hits(), op.state.borrow().asks), (0, 0));
        app.data.lock().unwrap().settings.check_updates = true;
        // No internet: silence.
        BASE.with(|b| *b.borrow_mut() = None);
        at_start(&op);
        assert_eq!(op.state.borrow().asks, 0);
        assert!(toasts(&op).is_empty());
        // The project not public yet, then nothing newer.
        let web = github(None);
        at_start(&op);
        assert_eq!((web.hits(), op.state.borrow().asks), (1, 0));
        assert!(toasts(&op).is_empty());
    }

    #[test]
    fn the_switch_is_saved_with_the_settings() {
        let app = test_app();
        let op = open_window(&app);
        refresh(&op);
        settings::open(&op, SettingsTab::About);
        let g = dlg(&op);
        g.set_check_updates(false);
        g.invoke_save();
        assert!(!app.data.lock().unwrap().settings.check_updates);
        // Settings saved before 1.4.6 have no such line: on.
        let old: crate::model::Settings = serde_json::from_str("{}").unwrap();
        assert!(old.check_updates);
    }
}
