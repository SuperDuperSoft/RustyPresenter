//! What the app remembers between Sundays, in its own folder on the phone: the computer it was paired with, its
//! sign-ins, and the choices on each tab.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct Saved {
    /// `192.168.1.20:4318`; empty until a computer is chosen.
    pub host: String,
    /// The computer's name, when it said it.
    pub name: String,
    /// The remote's sign-in (`rp_session`), from pairing.
    pub session: Option<String>,
    /// The stage screen's sign-in (`rp_stage`), when it has a password.
    pub stage: Option<String>,
    /// The camera the Tally tab shows (1 for the first).
    pub tally_camera: u32,
    /// The name this phone's camera has on the computer.
    pub camera_name: String,
    /// The camera on the back of the phone (or the one on the screen's side).
    pub back_camera: bool,
    /// Presenter notes shown on the Remote tab.
    pub notes: bool,
    /// Drawing on the slide: the colour (0–5), the pen (0–2, 3 the highlighter) and whether the ink fades.
    pub draw_color: u32,
    pub draw_size: u32,
    pub draw_fades: bool,
}

impl Default for Saved {
    fn default() -> Self {
        Saved { host: String::new(), name: String::new(), session: None, stage: None, tally_camera: 1, camera_name: String::new(), back_camera: true, notes: true,
            draw_color: 0, draw_size: 1, draw_fades: false }
    }
}

impl Saved {
    /// What was saved; nothing (or something unreadable) is a fresh start.
    pub fn load(path: &Path) -> Saved {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Saved by writing a new file and putting it in place, so a phone switched off mid-write keeps the old one.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }

    /// Another computer: its sign-ins are not this one's.
    pub fn choose(&mut self, host: &str, name: &str) {
        if self.host != host { self.session = None; self.stage = None; }
        self.host = host.to_string();
        if !name.is_empty() || self.name.is_empty() { self.name = name.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_kept_and_read_back() {
        let dir = std::env::temp_dir().join(format!("rp-remote-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("remote.json");
        assert_eq!(Saved::load(&path), Saved::default(), "nothing yet");
        let mut s = Saved::default();
        s.choose("192.168.1.20:4318", "CHURCH-PC");
        s.session = Some("tok".into());
        s.tally_camera = 2;
        s.save(&path).unwrap();
        assert_eq!(Saved::load(&path), s);
        std::fs::write(&path, b"{nope").unwrap();
        assert_eq!(Saved::load(&path), Saved::default(), "a broken file is a fresh start");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn another_computer_forgets_the_sign_ins() {
        let mut s = Saved { host: "a:4318".into(), session: Some("t".into()), stage: Some("s".into()), ..Default::default() };
        s.choose("a:4318", "");
        assert_eq!(s.session.as_deref(), Some("t"), "the same computer keeps them");
        s.choose("b:4318", "Hall");
        assert_eq!((s.session.clone(), s.stage.clone(), s.name.as_str()), (None, None, "Hall"));
        assert!(Saved::load(Path::new("/no/such/file")).back_camera);
    }
}
