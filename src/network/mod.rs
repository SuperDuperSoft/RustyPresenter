//! N31: the network extras, every one off until turned on: a password for the phone remote (asked when a phone
//! pairs), the stage screen on a phone or tablet (`stage_view`, read only, password optional) and a plain-text
//! TCP control for other gear (`tcp`, see `docs/tcp-control.md`). Passwords are kept only as hashes (`password`).
pub mod actions;
pub mod guest;
pub mod https;
pub mod phone_camera;
pub mod websocket;
pub mod password;
pub mod stage_view;
pub mod phone_mic;
pub mod live_anim;
pub mod tcp;
#[path = "../strings/network.rs"]
pub mod strings;

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::IpAddr, sync::Mutex, time::Instant};

/// The TCP control's port when none is set.
pub const TCP_PORT: u16 = 4319;

/// `Settings.network`. Old data (and data this version can't read) loads as all off.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct NetworkSettings {
    /// A hash (`password::hash`): a phone must type this password to pair.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_password: Option<String>,
    /// The stage screen at `/stage-view` for phones and tablets.
    pub stage_view: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_password: Option<String>,
    /// The TCP control on `tcp_port`.
    pub tcp: bool,
    pub tcp_port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_password: Option<String>,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        NetworkSettings { remote_password: None, stage_view: false, stage_password: None, tcp: false, tcp_port: TCP_PORT, tcp_password: None }
    }
}

/// What the network extras keep while the app runs (`App::net`): stage view sign-ins, the TCP listener and the
/// last password try from each address.
#[derive(Default)]
pub struct NetState {
    /// Hashes of the stage view's sign-in cookies, with when each was made. Cleared when its password changes.
    pub(crate) stage_tokens: Mutex<Vec<(String, Instant)>>,
    pub(crate) tcp: Mutex<Option<tcp::Running>>,
    pub(crate) tries: Mutex<HashMap<IpAddr, Instant>>,
    /// The port the phone pages are served on over https (`https`, for the phone camera), once they are.
    pub(crate) https: Mutex<Option<u16>>,
}

impl NetState {
    /// One password try a second from each address. False = too fast.
    pub(crate) fn may_try(&self, ip: IpAddr) -> bool {
        let mut t = self.tries.lock().unwrap();
        let now = Instant::now();
        if t.get(&ip).is_some_and(|at| now.duration_since(*at).as_millis() < 1000) { return false }
        if t.len() > 256 { t.clear() }
        t.insert(ip, now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_and_bad_settings_load_with_everything_off() {
        use crate::model::Settings;
        let old: Settings = serde_json::from_str(r#"{"lines_per_slide":3}"#).unwrap();
        assert_eq!(old.network, NetworkSettings::default());
        assert!(!old.network.stage_view && !old.network.tcp && old.network.remote_password.is_none());
        let bad: Settings = serde_json::from_str(r#"{"lines_per_slide":3,"network":{"tcp":"yes"}}"#).unwrap();
        assert_eq!((bad.network, bad.lines_per_slide), (NetworkSettings::default(), 3));
        let v = serde_json::to_value(NetworkSettings::default()).unwrap();
        assert_eq!(v, serde_json::json!({ "stage_view": false, "tcp": false, "tcp_port": 4319 }), "no password keys when there is none");
    }

    #[test]
    fn one_try_a_second_from_each_address() {
        let s = NetState::default();
        let (a, b): (IpAddr, IpAddr) = ("192.168.1.2".parse().unwrap(), "192.168.1.3".parse().unwrap());
        assert!(s.may_try(a));
        assert!(!s.may_try(a));
        assert!(s.may_try(b));
    }
}
