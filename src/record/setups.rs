//! Saved setups: a whole livestream setup (destinations on or off, cameras, sound, scenes, cards, quality and every
//! More setting) kept under a name, "Sunday" or "Wedding", and used again in one go. Stream keys and sign-ins are
//! not part of it (they stay in the computer's password store, by destination), nor is what belongs to one
//! broadcast or to this computer (the YouTube broadcast made ahead, the licence note, what was learnt about its
//! encoder).
use super::RecordPrefs;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Setup {
    pub name: String,
    pub prefs: RecordPrefs,
}

/// `prefs` without what a setup does not keep.
fn kept(prefs: &RecordPrefs) -> RecordPrefs {
    RecordPrefs {
        setups: vec![], licence_noted: false, stream_scheduled: String::new(), stream_scheduled_link: String::new(), x264_too_slow: false,
        ..prefs.clone()
    }
}

fn same(a: &str, b: &str) -> bool { a.trim().to_lowercase() == b.trim().to_lowercase() }

/// The setups' names, in the order they were first saved.
pub fn names(prefs: &RecordPrefs) -> Vec<String> { prefs.setups.iter().map(|s| s.name.clone()).collect() }

/// Keeps the setup as it is now under `name` (a setup of that name is replaced, in its place). False for an empty
/// name.
pub fn save(prefs: &mut RecordPrefs, name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() { return false }
    let setup = Setup { name: name.to_string(), prefs: kept(prefs) };
    match prefs.setups.iter_mut().find(|s| same(&s.name, name)) {
        Some(s) => *s = setup,
        None => prefs.setups.push(setup),
    }
    true
}

/// Uses setup `name`: everything it keeps replaces what is set now. False when there is none of that name.
pub fn apply(prefs: &mut RecordPrefs, name: &str) -> bool {
    let Some(setup) = prefs.setups.iter().find(|s| same(&s.name, name)).cloned() else { return false };
    let mine = std::mem::take(&mut prefs.setups);
    *prefs = RecordPrefs {
        setups: mine, licence_noted: prefs.licence_noted, stream_scheduled: std::mem::take(&mut prefs.stream_scheduled),
        stream_scheduled_link: std::mem::take(&mut prefs.stream_scheduled_link), x264_too_slow: prefs.x264_too_slow,
        ..kept(&setup.prefs)
    };
    true
}

/// Forgets setup `name`.
pub fn remove(prefs: &mut RecordPrefs, name: &str) { prefs.setups.retain(|s| !same(&s.name, name)) }

/// The setup whose settings are the ones now, if any (shown as chosen).
pub fn current(prefs: &RecordPrefs) -> Option<String> {
    let now = kept(prefs);
    prefs.setups.iter().find(|s| kept(&s.prefs) == now).map(|s| s.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{Preset, advanced::{Format, Keep, More}};

    #[test]
    fn a_setup_is_saved_used_again_and_forgotten() {
        let mut p = RecordPrefs { size: Preset::P1080, camera: "Cam A".into(), licence_noted: true, stream_scheduled: "yt1".into(), ..Default::default() };
        assert!(save(&mut p, " Sunday "));
        assert_eq!(names(&p), ["Sunday"]);
        assert_eq!(current(&p).as_deref(), Some("Sunday"));
        // A wedding: 720p, another camera, an MKV recording.
        p.size = Preset::P720;
        p.camera = "Cam B".into();
        p.more = More { keep: Keep { format: Format::Mkv, ..Keep::default() }, ..More::default() };
        assert_eq!(current(&p), None, "changed since");
        assert!(save(&mut p, "Wedding"));
        assert!(!save(&mut p, "  "), "a setup needs a name");
        // Back to Sunday: its settings, but what belongs to this computer and this broadcast stays.
        assert!(apply(&mut p, "sunday"));
        assert_eq!((p.size, p.camera.as_str(), p.more.keep.format), (Preset::P1080, "Cam A", Format::Mp4));
        assert!(p.licence_noted && p.stream_scheduled == "yt1");
        assert_eq!(names(&p), ["Sunday", "Wedding"]);
        assert!(p.setups.iter().all(|s| s.prefs.setups.is_empty() && !s.prefs.licence_noted && s.prefs.stream_scheduled.is_empty()), "a setup holds no setups");
        assert!(!apply(&mut p, "Easter"));
        // Saving again under a name replaces it in its place.
        p.camera = "Cam C".into();
        save(&mut p, "WEDDING");
        assert_eq!(names(&p), ["Sunday", "WEDDING"]);
        assert_eq!(p.setups[1].prefs.camera, "Cam C");
        remove(&mut p, "sunday");
        assert_eq!(names(&p), ["WEDDING"]);
        // Kept with the settings.
        let back: RecordPrefs = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }
}
