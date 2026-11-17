//! 1.3 song auto-follow in the operator window: the Live column's Auto chip (Manual → Suggest → Auto), the
//! listening light with the line it thinks is being sung, Hold, the Suggest glow in the slide grid, and the A and H
//! keys. The mode and Hold go to the App as commands (`Command::FollowMode`, `Command::FollowHold`), like the phone's.
use super::window::Operator;
use crate::{
    follow::{FollowState, Light, Mode, strings as tr},
    live::Command,
    ui::FollowUi,
};
use slint::ComponentHandle;
use std::rc::{Rc, Weak};
use uuid::Uuid;

/// The light as the Live column names it to Slint.
pub fn light_id(l: Light) -> &'static str {
    match l {
        Light::Off => "off",
        Light::Listening => "listening",
        Light::Sure => "sure",
        Light::Unsure => "unsure",
        Light::Holding => "holding",
        Light::Blank => "blank",
    }
}

/// The live song and how it follows (none when the live item is not a song).
fn live_follow(op: &Operator) -> Option<(Uuid, FollowState)> {
    let st = op.state.borrow();
    let v = st.live.as_ref()?.view();
    Some((v.cursor.as_ref()?.item_id, v.follow.clone()?))
}

/// Draws the row from the newest live view, and the glow on the suggested slide when the grid shows the live song.
pub(super) fn render(op: &Operator) {
    let g = op.ui.global::<FollowUi>();
    let Some((item, f)) = live_follow(op) else {
        g.set_shown(false);
        g.set_suggested(-1);
        return;
    };
    g.set_shown(true);
    g.set_mode(f.mode.index() as i32);
    g.set_mode_label(tr::mode(f.mode).into());
    g.set_mode_tip(tr::mode_tip().into());
    g.set_light(light_id(f.light).into());
    g.set_light_label(tr::light(f.light).into());
    g.set_line(f.line.clone().filter(|_| f.mode != Mode::Manual).unwrap_or_default().into());
    g.set_holding(f.holding);
    g.set_hold_tip(tr::hold_tip().into());
    let shown_here = op.state.borrow().selected == Some(item);
    g.set_suggested(f.suggest.filter(|_| shown_here && f.mode == Mode::Suggest).map_or(-1, |n| n as i32));
}

/// The chip: Manual → Suggest → Auto → Manual.
pub(crate) fn cycle_mode(op: &Rc<Operator>) { set_mode(op, Mode::next, false); }

/// The A key: Auto on, or back to Manual. False when no song is live (the key is not used).
pub(crate) fn toggle_auto(op: &Rc<Operator>, toast: bool) -> bool { set_mode(op, Mode::toggle_auto, toast) }

fn set_mode(op: &Rc<Operator>, change: fn(Mode) -> Mode, toast: bool) -> bool {
    let Some((item_id, f)) = live_follow(op) else { return false };
    let mode = change(f.mode);
    send(op, Command::FollowMode { item_id, mode: Some(mode) }, toast.then(|| tr::auto_toast(mode == Mode::Auto)));
    true
}

/// Hold on or off (the button and the H key). False when no song is live.
pub(crate) fn toggle_hold(op: &Rc<Operator>, toast: bool) -> bool {
    let Some((_, f)) = live_follow(op) else { return false };
    send(op, Command::FollowHold { on: !f.holding }, toast.then(|| tr::hold_toast(!f.holding)));
    true
}

fn send(op: &Rc<Operator>, cmd: Command, done: Option<String>) {
    let me = Rc::downgrade(op);
    let mode = matches!(cmd, Command::FollowMode { .. });
    // Suggest and Auto need a speech model: offer it the first time.
    let offer = matches!(cmd, Command::FollowMode { mode: Some(m), .. } if m != Mode::Manual);
    op.ops().cmd(cmd, None, move |o| {
        let Some(op) = me.upgrade() else { return };
        match (o.ok, o.error) {
            (false, Some(e)) => op.toast(e, true),
            (true, _) => {
                if mode { listen(&op, false) }
                if offer { crate::ui::dialogs::settings_model::offer(&op) }
                if let Some(t) = done { op.toast(t, false) }
            }
            _ => {}
        }
    });
}

/// After a mode or Settings change: start listening if a song now wants it (`again`: the sound input changed, so
/// listen to the new one). FFmpeg runs, so on its own thread; tests never start it.
pub(crate) fn listen(op: &Operator, again: bool) {
    if op.ops().is_inline() { return }
    let app = op.app().clone();
    let _ = std::thread::Builder::new().name("follow-start".into())
        .spawn(move || if again { app.listen_again() } else { app.start_follow_if_wanted() });
}

pub(super) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<FollowUi>();
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_cycle_mode(move || if let Some(op) = me.upgrade() { cycle_mode(&op) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_toggle_hold(move || if let Some(op) = me.upgrade() { toggle_hold(&op, false); });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_open_log(move || if let Some(op) = me.upgrade() { crate::ui::dialogs::settings_follow::open_page(&op, 2) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_open_practice(move || if let Some(op) = me.upgrade() { crate::ui::dialogs::settings_follow::open_page(&op, 1) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_open_settings(move || if let Some(op) = me.upgrade() { crate::ui::dialogs::settings_follow::open_page(&op, 0) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, LiveUpdate, test_app},
        model::{ItemKind, ServiceItem, Song},
        ui::operator::window::testing::{find, open, refresh},
    };
    use slint::{SharedString, platform::{PointerEventButton, WindowEvent}};
    use std::sync::Arc;

    /// A song (two slides) in the library and the service; returns its item id.
    fn add_song(app: &App) -> Uuid {
        let song = Song { id: Uuid::new_v4(), title: "Grace".into(), lyrics: "Verse 1\nAmazing grace how sweet the sound\n\nChorus\nMy chains are gone".into(),
            arrangement: vec!["Verse 1".into(), "Chorus".into()], ..Default::default() };
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None };
        let mut d = app.data.lock().unwrap();
        d.songs.push(song);
        d.services[0].items.push(item.clone());
        item.id
    }

    fn add_text(app: &App) -> Uuid {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "Welcome".into(), body: "Hello".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        item.id
    }

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op
    }

    fn live(op: &Operator) { op.apply_live(LiveUpdate::new(op.app().live_view())); }

    fn show(op: &Operator, item_id: Uuid) {
        op.ops().cmd(Command::Show { item_id, slide: 0 }, None, |o| assert!(o.ok, "{o:?}"));
        refresh(op);
        live(op);
    }

    fn click(op: &Operator, id: &str) {
        find(op, id).unwrap_or_else(|| panic!("{id}")).mock_single_click(PointerEventButton::Left);
        refresh(op);
        live(op);
    }

    fn press(op: &Operator, text: &str) {
        let text: SharedString = text.into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text });
        refresh(op);
        live(op);
    }

    fn label(op: &Operator, id: &str) -> String { find(op, id).and_then(|e| e.accessible_label()).unwrap_or_default().to_string() }

    fn saved(app: &App, item: Uuid) -> Option<Mode> { app.data.lock().unwrap().automation.follow.get(&item).copied() }

    #[test]
    fn auto_chip_cycles_modes() {
        crate::ui::ui_test_init();
        let app = test_app();
        let (song, text) = (add_song(&app), add_text(&app));
        let op = opened(&app);
        show(&op, text);
        assert!(find(&op, "FollowRow::follow-chip").is_none(), "no chip for a text item");
        show(&op, song);
        assert_eq!(label(&op, "FollowRow::follow-chip"), "Manual");
        assert!(find(&op, "FollowRow::follow-hold").is_none(), "no Hold in Manual");
        click(&op, "FollowRow::follow-chip");
        assert_eq!((label(&op, "FollowRow::follow-chip"), saved(&app, song)), ("Suggest".into(), Some(Mode::Suggest)));
        // No speech model yet: the download is offered, once.
        assert_eq!(op.state.borrow().asks, 1, "Auto-follow needs a speech model (378 MB). Download now?");
        crate::ui::dialogs::ask::answer(&op, false, String::new());
        assert!(op.dialog().is_none(), "Not now: nothing opens");
        click(&op, "FollowRow::follow-chip");
        assert_eq!((label(&op, "FollowRow::follow-chip"), saved(&app, song)), ("Auto".into(), Some(Mode::Auto)));
        assert!(find(&op, "FollowRow::follow-hold").is_some(), "Hold shows while it follows");
        click(&op, "FollowRow::follow-chip");
        assert_eq!((label(&op, "FollowRow::follow-chip"), saved(&app, song)), ("Manual".into(), Some(Mode::Manual)));
        // A: Auto on, then off again.
        press(&op, "a");
        assert_eq!((label(&op, "FollowRow::follow-chip"), saved(&app, song)), ("Auto".into(), Some(Mode::Auto)));
        assert!(op.toast_log().iter().any(|(t, err)| t == "Auto-follow on" && !err));
        press(&op, "a");
        assert_eq!(saved(&app, song), Some(Mode::Manual));
        assert_eq!(op.state.borrow().asks, 0, "asked only once");
        // Settings → Auto-follow's mode is used by a song with none of its own.
        let fresh = add_song(&app);
        app.data.lock().unwrap().settings.follow.mode = Mode::Suggest;
        show(&op, fresh);
        assert_eq!(label(&op, "FollowRow::follow-chip"), "Suggest");
    }

    #[test]
    fn options_button_opens_the_menu() {
        crate::ui::ui_test_init();
        let app = test_app();
        let song = add_song(&app);
        let op = opened(&app);
        show(&op, song);
        let more = find(&op, "FollowRow::follow-more").expect("the ⋯ button shows beside the chip");
        more.mock_single_click(PointerEventButton::Left);
        slint::platform::update_timers_and_animations();
        let pick = |id: &str, page: i32| {
            let line = find(&op, id).unwrap_or_else(|| panic!("{id} in the menu"));
            line.invoke_accessible_default_action();
            use crate::ui::dialogs::{SettingsTab, settings};
            let want = match page { 1 => SettingsTab::FollowPractice, 2 => SettingsTab::FollowReview, _ => SettingsTab::AutoFollow };
            assert_eq!(settings::shown(&op), Some(want), "{id}");
            settings::close(&op);
        };
        pick("FollowRow::follow-menu-log", 2);
        more.mock_single_click(PointerEventButton::Left);
        pick("FollowRow::follow-menu-practice", 1);
        more.mock_single_click(PointerEventButton::Left);
        pick("FollowRow::follow-menu-settings", 0);
    }

    #[test]
    fn hold_key_holds() {
        crate::ui::ui_test_init();
        let app = test_app();
        app.start_follower();
        let song = add_song(&app);
        app.data.lock().unwrap().automation.follow.insert(song, Mode::Auto);
        let op = opened(&app);
        show(&op, song);
        assert!(find(&op, "FollowRow::follow-hold").is_some() && !op.ui.global::<FollowUi>().get_holding());
        press(&op, "h");
        assert!(app.follower().unwrap().holding());
        assert!(op.ui.global::<FollowUi>().get_holding());
        assert_eq!(op.ui.global::<FollowUi>().get_light(), "holding");
        assert!(op.toast_log().iter().any(|(t, _)| t == "Hold on: the slides stay put"));
        click(&op, "FollowRow::follow-hold");
        assert!(!app.follower().unwrap().holding(), "the button lets go");
        assert!(!op.ui.global::<FollowUi>().get_holding());
    }

    #[test]
    fn light_shows_state() {
        crate::ui::ui_test_init();
        let app = test_app();
        let song = add_song(&app);
        app.data.lock().unwrap().automation.follow.insert(song, Mode::Suggest);
        let op = opened(&app);
        show(&op, song);
        op.ui.invoke_service_select(song.to_string().into());
        let put = |light: Light, line: Option<&str>, suggest: Option<usize>| {
            let mut v = app.live_view();
            v.follow = Some(FollowState { mode: Mode::Suggest, light, line: line.map(str::to_string), holding: false, suggest });
            op.apply_live(LiveUpdate::new(v));
        };
        put(Light::Sure, Some("Amazing grace how sweet the sound"), Some(1));
        let light = find(&op, "FollowRow::follow-light").unwrap();
        assert_eq!(light.accessible_value().unwrap_or_default(), "sure");
        assert_eq!(light.accessible_label().unwrap_or_default(), "Following the singers");
        assert_eq!(label(&op, "FollowRow::follow-line"), "Amazing grace how sweet the sound");
        assert_eq!(op.ui.global::<FollowUi>().get_suggested(), 1, "Suggest: the next slide glows");
        put(Light::Unsure, None, None);
        assert_eq!(find(&op, "FollowRow::follow-light").unwrap().accessible_value().unwrap_or_default(), "unsure");
        assert_eq!(label(&op, "FollowRow::follow-line"), "Not sure — staying on this slide", "no line: the light says it");
        assert_eq!(op.ui.global::<FollowUi>().get_suggested(), -1);
        put(Light::Blank, None, None);
        assert_eq!(label(&op, "FollowRow::follow-line"), "Nobody singing — words off");
    }
}
