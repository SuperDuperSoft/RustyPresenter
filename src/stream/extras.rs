//! The livestream's extras, kept together in Settings (`RecordPrefs::extras`): live captions and the website
//! player (design: `docs/superpowers/specs/2026-11-17-livestream-extras-design.md`).
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Extras {
    #[serde(skip_serializing_if = "is_default")]
    pub captions: super::captions::Prefs,
    #[serde(skip_serializing_if = "is_default")]
    pub website: super::website::Prefs,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool { *v == T::default() }

impl Extras {
    pub fn is_default(&self) -> bool { *self == Extras::default() }
}
