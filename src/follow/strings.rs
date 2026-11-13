//! Auto-follow's words for the Live column, the stage screen, the phone and Settings.
use super::{Light, Mode};
use crate::{tr, trc};

/// The Auto chip's text.
pub fn mode(m: Mode) -> String {
    match m {
        Mode::Manual => trc!("auto-follow", "Manual"),
        Mode::Suggest => trc!("auto-follow", "Suggest"),
        Mode::Auto => trc!("auto-follow", "Auto"),
    }
}

pub fn mode_tip() -> String {
    tr!("Songs follow the singers. Click: Manual → Suggest (the next slide glows) → Auto (the slides move by themselves). Key: A turns Auto on and off.")
}

/// The listening light, as the Live column says it.
pub fn light(l: Light) -> String {
    match l {
        Light::Off => tr!("Not listening"),
        Light::Listening => tr!("Listening…"),
        Light::Sure => tr!("Following the singers"),
        Light::Unsure => tr!("Not sure — staying on this slide"),
        Light::Holding => tr!("Holding — the slides stay put"),
        Light::Blank => tr!("Nobody singing — words off"),
    }
}

/// The light in a word or two, for the stage screen's line.
fn state_word(l: Light) -> String {
    match l {
        Light::Off => tr!("off"),
        Light::Listening => tr!("listening"),
        Light::Sure => tr!("following"),
        Light::Unsure => tr!("not sure"),
        Light::Holding => tr!("holding"),
        Light::Blank => tr!("words off"),
    }
}

pub fn hold_tip() -> String { tr!("Hold: auto-follow doesn't move the slides until you click again. Key: H") }

/// "Auto: following — Chorus 2" on the stage screen; nothing in Manual.
pub fn stage_line(m: Mode, l: Light, section: Option<&str>) -> Option<String> {
    if m == Mode::Manual { return None }
    let state = state_word(l);
    let mode = mode(m);
    Some(match section.filter(|s| !s.is_empty()) {
        Some(section) => tr!("{mode}: {state} — {section}", mode = mode, state = state, section = section),
        None => tr!("{mode}: {state}", mode = mode, state = state),
    })
}

/// The toast for the A key.
pub fn auto_toast(on: bool) -> String { if on { tr!("Auto-follow on") } else { tr!("Auto-follow off") } }

/// The toast for the H key.
pub fn hold_toast(on: bool) -> String { if on { tr!("Hold on: the slides stay put") } else { tr!("Hold off") } }
