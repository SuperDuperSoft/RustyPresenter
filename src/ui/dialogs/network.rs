//! Settings → Network… (N31, `ui/dialogs/network.slint`): the phone remote's password, the stage screen on phones
//! and tablets, and the TCP control. Saved with `network/save`, which hashes the passwords.
use super::DialogKind;
use crate::{network::strings as s, ui::{NetworkDlg, operator::window::Operator}};
use serde_json::{Value, json};
use slint::ComponentHandle;
use std::rc::Rc;

pub fn title() -> String { crate::tr!("Network") }

/// What the TCP control understands, for the dialog.
pub fn tcp_help() -> String {
    crate::tr!("Send one command a line: next, prev, clear, clear slide, logo, go 3 (item 3), go 3 2 (its slide 2), status. With a password, send “password …” first. Every line is answered with OK or ERR.")
}

/// Opens the dialog on what is saved now.
pub fn open(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("network/get", json!({}), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) => { op.open_dialog(DialogKind::Network); fill(&op, &v); connect(&op) }
            Err(e) => op.toast(e, true),
        }
    });
}

pub(crate) fn fill(op: &Operator, v: &Value) {
    let g = op.ui.global::<NetworkDlg>();
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

/// What Save sends, from the dialog. A port that isn't a number is an error shown in the dialog.
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

fn connect(op: &Rc<Operator>) {
    let g = op.ui.global::<NetworkDlg>();
    let me = Rc::downgrade(op);
    g.on_cancel(move || if let Some(op) = me.upgrade() { op.close_dialog() });
    let me = Rc::downgrade(op);
    g.on_save(move || if let Some(op) = me.upgrade() { save(&op) });
}

fn save(op: &Rc<Operator>) {
    let g = op.ui.global::<NetworkDlg>();
    let body = match body(&g) { Ok(b) => b, Err(e) => return g.set_error(e.into()) };
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("network/save", body, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => { op.close_dialog(); op.toast(s::saved(), false) }
            Err(e) => op.ui.global::<NetworkDlg>().set_error(e.into()),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, ui::operator::{menu::{MenuAction, action}, window::testing::{find, open as open_window, refresh}}};
    use slint::platform::PointerEventButton;

    fn settle() { for _ in 0..20 { slint::platform::update_timers_and_animations(); std::thread::sleep(std::time::Duration::from_millis(5)); } }

    fn opened() -> (std::sync::Arc<crate::app::App>, Rc<Operator>) {
        let app = test_app();
        let op = open_window(&app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        refresh(&op);
        (app, op)
    }

    /// Runs the dialog's calls until `done` (they answer on the event loop).
    fn wait(op: &Operator, done: impl Fn(&Operator) -> bool) {
        for _ in 0..400 { if done(op) { return } settle() }
        panic!("timed out");
    }

    #[test]
    fn the_settings_menu_opens_it_with_everything_off() {
        assert_eq!(action("network"), Some(MenuAction::Network));
        let (_app, op) = opened();
        op.ui.invoke_menu("network".into());
        wait(&op, |op| op.dialog() == Some(DialogKind::Network));
        assert_eq!(op.ui.get_dialog_title(), "Network");
        let g = op.ui.global::<NetworkDlg>();
        assert!(!g.get_remote_pw_on() && !g.get_stage_on() && !g.get_tcp_on());
        assert_eq!(g.get_tcp_port(), "4319");
        assert!(find(&op, "NetworkDialog::stage-check").is_some());
    }

    #[test]
    fn save_turns_the_stage_view_on_with_a_hashed_password() {
        let (app, op) = opened();
        open(&op);
        wait(&op, |op| op.dialog() == Some(DialogKind::Network));
        let g = op.ui.global::<NetworkDlg>();
        g.set_stage_on(true);
        g.set_stage_pw_on(true);
        // Asked for but not typed: the dialog says so and nothing is saved.
        find(&op, "NetworkDialog::net-save").unwrap().mock_single_click(PointerEventButton::Left);
        wait(&op, |op| op.ui.global::<NetworkDlg>().get_error() != "");
        assert_eq!(g.get_error(), "Type a password, or turn the password off.");
        assert!(!app.data.lock().unwrap().settings.network.stage_view);
        g.set_stage_pw("tenors".into());
        g.set_tcp_port("abc".into());
        g.invoke_save();
        assert_eq!(g.get_error(), "Use a port from 1024 to 65535.");
        g.set_tcp_port("4319".into());
        g.invoke_save();
        wait(&op, |op| op.dialog().is_none());
        let n = app.data.lock().unwrap().settings.network.clone();
        assert!(n.stage_view && crate::network::password::verify("tenors", n.stage_password.as_deref().unwrap()));
        assert!(!n.tcp && n.remote_password.is_none());
        // Opened again: the password shows as saved, the field is empty, and Save keeps it.
        open(&op);
        wait(&op, |op| op.dialog() == Some(DialogKind::Network));
        assert!(g.get_stage_pw_set() && g.get_stage_pw_on() && g.get_stage_pw() == "");
        g.invoke_save();
        wait(&op, |op| op.dialog().is_none());
        assert_eq!(app.data.lock().unwrap().settings.network.stage_password, n.stage_password);
    }
}
