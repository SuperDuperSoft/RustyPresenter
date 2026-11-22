//! Settings → Remote and network (N31; `ui/dialogs/settings-remote.slint`, was Settings → Network…): the remote's
//! password, the stage screen on phones and tablets, and the TCP control. Loaded with `network/get` as Settings
//! opens, and saved with `network/save` (which hashes the passwords) when Settings is saved and they changed.
use super::settings_window::win;
use crate::{network::strings as s, ui::{NetworkDlg, operator::window::Operator}};
use serde_json::{Value, json};
use slint::ComponentHandle;
use std::{cell::RefCell, rc::Rc};

pub fn title() -> String { crate::tr!("Network") }

/// What the TCP control understands, for the page.
pub fn tcp_help() -> String {
    crate::tr!("Send one command a line: next, prev, clear, clear slide, logo, go 3 (item 3), go 3 2 (its slide 2), status. With a password, send “password …” first. Every line is answered with OK or ERR.")
}

thread_local! {
    /// What Save would send for the rows as they were loaded, so an untouched page saves nothing.
    static LOADED: RefCell<Option<Value>> = const { RefCell::new(None) };
}

/// Fills the rows from what is saved now (Settings is opening).
pub(crate) fn load(op: &Rc<Operator>) {
    LOADED.with(|l| *l.borrow_mut() = None);
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("network/get", json!({}), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) => {
                fill(&op, &v);
                let loaded = body(&win(&op).global::<NetworkDlg>()).ok();
                LOADED.with(|l| *l.borrow_mut() = loaded);
            }
            Err(e) => op.toast(e, true),
        }
    });
}

pub(crate) fn fill(op: &Operator, v: &Value) {
    let g = win(op).global::<NetworkDlg>();
    let b = |k: &str| v[k].as_bool().unwrap_or(false);
    let st = |k: &str| slint::SharedString::from(v[k].as_str().unwrap_or(""));
    g.set_remote_pw_on(b("remote_password"));
    g.set_remote_pw_set(b("remote_password"));
    g.set_remote_pw("".into());
    g.set_stage_on(b("stage_view"));
    g.set_stage_pw_on(b("stage_password"));
    g.set_stage_pw_set(b("stage_password"));
    g.set_stage_pw("".into());
    g.set_stage_url(st("stage_url"));
    g.set_tcp_on(b("tcp"));
    g.set_tcp_port(v["tcp_port"].as_u64().unwrap_or(u64::from(crate::network::TCP_PORT)).to_string().into());
    g.set_tcp_pw_on(b("tcp_password"));
    g.set_tcp_pw_set(b("tcp_password"));
    g.set_tcp_pw("".into());
    g.set_tcp_address(st("tcp_address"));
    g.set_tcp_help(tcp_help().into());
    g.set_error("".into());
}

/// One password as `network/save` wants it: off, kept (asked for, nothing typed) or set.
fn change(on: bool, typed: &str) -> Value {
    match (on, typed.is_empty()) {
        (false, _) => json!({ "do": "off" }),
        (true, true) => json!({ "do": "set", "password": "" }),
        (true, false) => json!({ "do": "set", "password": typed }),
    }
}

/// What Save sends, from the page. A port that isn't a number is an error shown on the page.
pub(crate) fn body(g: &NetworkDlg) -> Result<Value, String> {
    let port: u16 = g.get_tcp_port().trim().parse().map_err(|_| s::port_range())?;
    Ok(json!({
        "remote_password": change(g.get_remote_pw_on(), &g.get_remote_pw()),
        "stage_view": g.get_stage_on(),
        "stage_password": if g.get_stage_on() { change(g.get_stage_pw_on(), &g.get_stage_pw()) } else { json!({ "do": "keep" }) },
        "tcp": g.get_tcp_on(),
        "tcp_port": port,
        "tcp_password": if g.get_tcp_on() { change(g.get_tcp_pw_on(), &g.get_tcp_pw()) } else { json!({ "do": "keep" }) },
    }))
}

/// Whether the rows differ from how they were loaded (a port that is not a number counts as a change).
pub(crate) fn changed(op: &Operator) -> bool {
    let now = body(&win(op).global::<NetworkDlg>()).ok();
    LOADED.with(|l| l.borrow().is_some() && *l.borrow() != now)
}

/// What to send with Save: `None` when nothing changed; an error for a port that is not a number.
pub(crate) fn body_if_changed(op: &Operator) -> Result<Option<Value>, String> {
    if !changed(op) { return Ok(None) }
    body(&win(op).global::<NetworkDlg>()).map(Some)
}

pub(crate) fn show_error(op: &Operator, e: &str) { win(op).global::<NetworkDlg>().set_error(e.into()) }

/// Saves the rows, then `done`; a refusal (a password asked for but not typed) shows on the page instead.
pub(crate) fn save(op: &Rc<Operator>, body: Value, done: impl FnOnce(&Rc<Operator>) + 'static) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("network/save", body.clone(), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => {
                LOADED.with(|l| *l.borrow_mut() = Some(body));
                done(&op)
            }
            Err(e) => {
                super::settings::go(&op, super::SettingsTab::Remote, "");
                show_error(&op, &e);
                op.toast(e, true);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, ui::{SettingsDlg, dialogs::{SettingsTab, settings}, operator::{menu::{MenuAction, action}, window::testing::{open as open_window, refresh}}}};

    fn settle() { for _ in 0..20 { slint::platform::update_timers_and_animations(); std::thread::sleep(std::time::Duration::from_millis(5)); } }

    fn opened() -> (std::sync::Arc<crate::app::App>, Rc<Operator>) {
        let app = test_app();
        let op = open_window(&app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        refresh(&op);
        (app, op)
    }

    /// Runs the page's calls until `done` (they answer on the event loop).
    fn wait(op: &Operator, done: impl Fn(&Operator) -> bool) {
        for _ in 0..400 { if done(op) { return } settle() }
        panic!("timed out");
    }

    #[test]
    fn the_settings_menu_opens_it_with_everything_off() {
        assert_eq!(action("network"), Some(MenuAction::Network));
        let (_app, op) = opened();
        op.ui.invoke_menu("network".into());
        assert_eq!(settings::shown(&op), Some(SettingsTab::Remote));
        assert_eq!(win(&op).global::<SettingsDlg>().get_page_title(), "Remote and network → Remote");
        let g = win(&op).global::<NetworkDlg>();
        wait(&op, |op| win(op).global::<NetworkDlg>().get_tcp_help() != "");
        assert!(!g.get_remote_pw_on() && !g.get_stage_on() && !g.get_tcp_on());
        assert_eq!(g.get_tcp_port(), "4319");
        settings::go(&op, SettingsTab::PhoneStage, "");
        assert!(settings::testing::find(&op, "PhoneStagePage::stage-check").is_some());
    }

    #[test]
    fn save_turns_the_stage_screen_for_phones_on_with_a_hashed_password() {
        let (app, op) = opened();
        settings::open(&op, SettingsTab::PhoneStage);
        wait(&op, |op| win(op).global::<NetworkDlg>().get_tcp_help() != "");
        let g = win(&op).global::<NetworkDlg>();
        let sg = win(&op).global::<SettingsDlg>();
        assert!(!settings::changed(&op, &settings::editing().unwrap()), "nothing changed yet");
        g.set_stage_on(true);
        g.set_stage_pw_on(true);
        // Asked for but not typed: the page says so and nothing is saved.
        sg.invoke_save();
        wait(&op, |op| win(op).global::<NetworkDlg>().get_error() != "");
        assert_eq!(g.get_error(), "Type a password, or turn the password off.");
        assert!(!app.data().settings.network.stage_view);
        assert!(settings::is_open(&op), "Settings stays open to fix it");
        g.set_stage_pw("tenors".into());
        g.set_tcp_port("abc".into());
        sg.invoke_save();
        assert_eq!(g.get_error(), "Use a port from 1024 to 65535.");
        assert_eq!(settings::shown(&op), Some(SettingsTab::OtherGear), "the page with the port");
        g.set_tcp_port("4319".into());
        sg.invoke_save();
        wait(&op, |op| !settings::is_open(op));
        let n = app.data().settings.network.clone();
        assert!(n.stage_view && crate::network::password::verify("tenors", n.stage_password.as_deref().unwrap()));
        assert!(!n.tcp && n.remote_password.is_none());
        // Opened again: the password shows as saved, the field is empty, and Save keeps it.
        settings::open(&op, SettingsTab::PhoneStage);
        wait(&op, |op| win(op).global::<NetworkDlg>().get_stage_pw_set());
        assert!(g.get_stage_pw_on() && g.get_stage_pw() == "");
        sg.invoke_save();
        wait(&op, |op| !settings::is_open(op));
        assert_eq!(app.data().settings.network.stage_password, n.stage_password);
    }
}
