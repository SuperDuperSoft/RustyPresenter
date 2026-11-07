//! The operator's keyboard shortcuts (N28) and clear groups (N18): which keys do what by default, the church's own
//! changes (stored in `Settings.keys`), and the clashes the shortcut editor warns about. Pure: the window's key
//! scope (`ui::operator::keys`) and the editor (`ui::dialogs::settings_keys`) use it.
//!
//! The key plan (N18): F1 Clear all, F2 text, F3 media & background, F4 audio, F5 Logo, F6 every overlay (pictures,
//! clocks, signs, timers and the message), F7 the message only, F8 the timers only, F9 the first clear group
//! ("Everything but the background"). F10–F12 are left free for the church's own groups.
use crate::live::{ClearLayer, Command};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One key with its modifiers. `key` is a `keys::key_name` name (`"F2"`, `"ArrowRight"`, `" "`) or the typed
/// character in lower case. `ctrl` is Cmd on macOS, as Slint reports it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chord { pub key: String, pub ctrl: bool, pub alt: bool, pub shift: bool }

/// Named keys: the stored name, the route name (`keys::key_name`) and Slint's `Keys` name. F-keys are their own.
const NAMED: [(&str, &str, &str); 16] = [
    ("Right", "ArrowRight", "RightArrow"), ("Left", "ArrowLeft", "LeftArrow"), ("Up", "ArrowUp", "UpArrow"),
    ("Down", "ArrowDown", "DownArrow"), ("PageUp", "PageUp", "PageUp"), ("PageDown", "PageDown", "PageDown"),
    ("Enter", "Enter", "Return"), ("Escape", "Escape", "Escape"), ("Space", " ", "Space"), ("Home", "Home", "Home"),
    ("End", "End", "End"), ("Delete", "Delete", "Delete"), ("Backspace", "Backspace", "Backspace"),
    ("Tab", "Tab", "Tab"), ("Insert", "Insert", "Insert"), ("Plus", "+", "Plus"),
];

impl Chord {
    pub fn new(key: &str, ctrl: bool, alt: bool, shift: bool) -> Chord {
        let key = if key.chars().count() == 1 { key.to_lowercase() } else { key.to_string() };
        Chord { key, ctrl, alt, shift }
    }

    fn plain(key: &str) -> Chord { Chord::new(key, false, false, false) }
    fn ctrl(key: &str) -> Chord { Chord::new(key, true, false, false) }

    /// An F-key: never typed into a text box.
    pub fn is_function_key(&self) -> bool {
        self.key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=24).contains(&n))
    }

    /// Would a text box use this key for typing or moving its cursor? Then it works only outside text boxes.
    pub fn types_text(&self) -> bool { !self.ctrl && !self.alt && !self.is_function_key() }

    /// The stored form, such as `Ctrl+Shift+M`, `F2` or `Space`.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        if self.ctrl { s.push_str("Ctrl+") }
        if self.alt { s.push_str("Alt+") }
        if self.shift { s.push_str("Shift+") }
        match NAMED.iter().find(|(_, route, _)| *route == self.key) {
            Some((stored, ..)) => s.push_str(stored),
            None => s.push_str(&self.key.to_uppercase()),
        }
        s
    }

    /// Reads the stored form. `None` for an empty or unknown one.
    pub fn parse(text: &str) -> Option<Chord> {
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        let mut rest = text;
        loop {
            if let Some(r) = rest.strip_prefix("Ctrl+") { ctrl = true; rest = r }
            else if let Some(r) = rest.strip_prefix("Alt+") { alt = true; rest = r }
            else if let Some(r) = rest.strip_prefix("Shift+") { shift = true; rest = r }
            else { break }
        }
        if rest.is_empty() { return None }
        let key = match NAMED.iter().find(|(stored, ..)| *stored == rest) {
            Some((_, route, _)) => route.to_string(),
            None if rest.chars().count() == 1 || Chord::plain(rest).is_function_key() => rest.to_string(),
            None => return None,
        };
        Some(Chord::new(&key, ctrl, alt, shift))
    }

    /// The parts for `slint::Keys::from_parts` (a menu item's shortcut).
    pub fn slint_parts(&self) -> Vec<String> {
        let mut parts = vec![];
        if self.ctrl { parts.push("Control".to_string()) }
        if self.alt { parts.push("Alt".to_string()) }
        if self.shift { parts.push("Shift".to_string()) }
        parts.push(match NAMED.iter().find(|(_, route, _)| *route == self.key) {
            Some((.., slint)) => slint.to_string(),
            None if self.is_function_key() => self.key.clone(),
            None => self.key.to_lowercase(),
        });
        parts
    }

    /// The key as people read it: `Ctrl + Shift + M`, `→`, `Page Down`.
    pub fn label(&self) -> String {
        let key = match self.key.as_str() {
            "ArrowRight" => "→".to_string(),
            "ArrowLeft" => "←".to_string(),
            "ArrowUp" => "↑".to_string(),
            "ArrowDown" => "↓".to_string(),
            " " => strings::space(),
            "PageDown" => strings::page_down(),
            "PageUp" => strings::page_up(),
            "Enter" => strings::enter(),
            "Escape" => strings::esc(),
            "Delete" => strings::delete(),
            "Backspace" => strings::backspace(),
            k => k.to_uppercase(),
        };
        match (self.ctrl, self.alt, self.shift) {
            (false, false, false) => key,
            (true, false, false) => strings::ctrl(&key),
            (false, true, false) => strings::alt(&key),
            (false, false, true) => strings::shift(&key),
            (true, false, true) => strings::ctrl(&strings::shift(&key)),
            (true, true, false) => strings::ctrl(&strings::alt(&key)),
            (false, true, true) => strings::alt(&strings::shift(&key)),
            (true, true, true) => strings::ctrl(&strings::alt(&strings::shift(&key))),
        }
    }
}

/// Several keys as the Help table shows them: `→  ↓  Space  Page Down`; empty for none.
pub fn labels(chords: &[Chord]) -> String { chords.iter().map(Chord::label).collect::<Vec<_>>().join("  ") }

/// Something a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Next, Back, Take, FocusBible,
    ClearAll, ClearSlide, ClearMedia, ClearAudio, Logo, ClearOverlays, ClearMessage, ClearTimers,
    /// A clear group (its id).
    Group(Uuid),
    PasteLink, Undo, Redo, Find, ShowMessage, FindMouse,
}

/// Where a shortcut works.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Everywhere, over dialogs and while typing (clears, Logo, Find my mouse), when its key types nothing.
    Anywhere,
    /// While typing too, but not over a dialog (Bible box, Find, Show message), when its key types nothing.
    Window,
    /// Only when no text box has the keyboard and no dialog is open.
    Free,
}

/// The fixed actions in the Help table's order; the clear groups follow `ClearTimers`.
const FIXED: [Action; 18] = [
    Action::Next, Action::Back, Action::Take, Action::FocusBible, Action::ClearAll, Action::ClearSlide, Action::ClearMedia,
    Action::ClearAudio, Action::Logo, Action::ClearOverlays, Action::ClearMessage, Action::ClearTimers,
    Action::PasteLink, Action::Undo, Action::Redo, Action::Find, Action::ShowMessage, Action::FindMouse,
];

/// The clear group every church starts with: "Everything but the background" on F9.
pub const BUILTIN_GROUP: Uuid = Uuid::from_u128(0x6b1e_0c1e_a400_4000_8000_0000_0000_0b09);

impl Action {
    /// The stored id.
    pub fn id(&self) -> String {
        let id = match self {
            Action::Group(id) => return format!("group:{id}"),
            Action::Next => "next", Action::Back => "back", Action::Take => "show_selected", Action::FocusBible => "bible_box",
            Action::ClearAll => "clear_all", Action::ClearSlide => "clear_slide", Action::ClearMedia => "clear_media",
            Action::ClearAudio => "clear_audio", Action::Logo => "logo", Action::ClearOverlays => "clear_overlays",
            Action::ClearMessage => "clear_message", Action::ClearTimers => "clear_timers", Action::PasteLink => "paste_link",
            Action::Undo => "undo", Action::Redo => "redo", Action::Find => "find", Action::ShowMessage => "show_message",
            Action::FindMouse => "find_mouse",
        };
        id.to_string()
    }

    pub fn from_id(id: &str) -> Option<Action> {
        if let Some(g) = id.strip_prefix("group:") { return Uuid::parse_str(g).ok().map(Action::Group) }
        FIXED.into_iter().find(|a| a.id() == id)
    }

    /// The keys it has until the church changes them.
    pub fn defaults(&self) -> Vec<Chord> {
        let f = |k: &str| vec![Chord::plain(k)];
        match self {
            Action::Next => ["ArrowRight", "ArrowDown", " ", "PageDown"].into_iter().map(Chord::plain).collect(),
            Action::Back => ["ArrowLeft", "ArrowUp", "PageUp"].into_iter().map(Chord::plain).collect(),
            Action::Take => f("Enter"),
            Action::FocusBible => vec![Chord::ctrl("b")],
            Action::ClearAll => f("F1"),
            Action::ClearSlide => f("F2"),
            Action::ClearMedia => f("F3"),
            Action::ClearAudio => f("F4"),
            Action::Logo => f("F5"),
            Action::ClearOverlays => f("F6"),
            Action::ClearMessage => f("F7"),
            Action::ClearTimers => f("F8"),
            Action::Group(id) if *id == BUILTIN_GROUP => f("F9"),
            Action::Group(_) => vec![],
            Action::PasteLink => vec![Chord::ctrl("v")],
            Action::Undo => vec![Chord::ctrl("z")],
            Action::Redo => vec![Chord::ctrl("y")],
            Action::Find => vec![Chord::ctrl("f")],
            Action::ShowMessage => vec![Chord::new("m", true, false, true)],
            Action::FindMouse => vec![Chord::new("m", false, true, false)],
        }
    }

    pub fn reach(&self) -> Reach {
        match self {
            Action::ClearAll | Action::ClearSlide | Action::ClearMedia | Action::ClearAudio | Action::Logo | Action::ClearOverlays
            | Action::ClearMessage | Action::ClearTimers | Action::Group(_) | Action::FindMouse => Reach::Anywhere,
            Action::FocusBible | Action::Find | Action::ShowMessage => Reach::Window,
            Action::Next | Action::Back | Action::Take | Action::PasteLink | Action::Undo | Action::Redo => Reach::Free,
        }
    }

    /// The live command of a clear key (not a group: its layers are in the settings).
    pub fn clear_command(&self) -> Option<Command> {
        Some(match self {
            Action::ClearAll => Command::ClearAll,
            Action::ClearSlide => Command::ClearSlide,
            Action::ClearMedia => Command::ClearMedia,
            Action::ClearAudio => Command::ClearAudio,
            Action::Logo => Command::Logo,
            Action::ClearOverlays => Command::ClearOverlays,
            Action::ClearMessage => Command::ClearLayers { layers: vec![ClearLayer::Message] },
            Action::ClearTimers => Command::ClearLayers { layers: vec![ClearLayer::Timers] },
            _ => return None,
        })
    }
}

/// A church's own mix of clears (N18), run by its key, the View menu or the phone's clear buttons' neighbours.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ClearGroup {
    pub id: Uuid,
    pub name: String,
    pub layers: Vec<ClearLayer>,
}

impl ClearGroup {
    /// Everything but the background (and video): text, audio, logo, overlays, the message and the timers.
    pub fn builtin() -> ClearGroup {
        ClearGroup { id: BUILTIN_GROUP, name: strings::all_but_background(),
            layers: vec![ClearLayer::Slide, ClearLayer::Audio, ClearLayer::Logo, ClearLayer::Overlays, ClearLayer::Message, ClearLayer::Timers] }
    }

    pub fn command(&self) -> Command { Command::ClearLayers { layers: self.layers.clone() } }
}

/// One action's keys as the church set them (`keys`: stored chords; empty = no key).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Binding { pub action: String, pub keys: Vec<String> }

/// `Settings.keys`: the changed shortcuts (only those that differ from the defaults) and the clear groups.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct KeySettings {
    pub changed: Vec<Binding>,
    pub clear_groups: Vec<ClearGroup>,
}

impl Default for KeySettings {
    fn default() -> Self { KeySettings { changed: vec![], clear_groups: vec![ClearGroup::builtin()] } }
}

impl KeySettings {
    /// Gives `action` the keys `chords` (stored only when they differ from its defaults).
    pub fn set(&mut self, action: Action, chords: Vec<Chord>) {
        let id = action.id();
        self.changed.retain(|b| b.action != id);
        if chords != action.defaults() {
            self.changed.push(Binding { action: id, keys: chords.iter().map(Chord::to_text).collect() });
        }
    }

    /// Back to the default keys for `action`.
    pub fn reset(&mut self, action: Action) { let id = action.id(); self.changed.retain(|b| b.action != id) }

    /// Puts `chord` on `action` in place of its keys, taking it off whatever had it.
    pub fn assign(&mut self, action: Action, chord: Chord) {
        let map = Keymap::new(self);
        for (other, keys) in map.entries() {
            if *other != action && keys.contains(&chord) {
                let rest = keys.iter().filter(|k| **k != chord).cloned().collect();
                self.set(*other, rest);
            }
        }
        self.set(action, vec![chord]);
    }
}

/// Every action with the keys it has now, in the Help table's order.
#[derive(Clone, Debug, PartialEq)]
pub struct Keymap { entries: Vec<(Action, Vec<Chord>)> }

impl Default for Keymap {
    fn default() -> Self { Keymap::new(&KeySettings::default()) }
}

impl Keymap {
    pub fn new(s: &KeySettings) -> Keymap {
        let mut actions: Vec<Action> = FIXED[..12].to_vec();
        actions.extend(s.clear_groups.iter().map(|g| Action::Group(g.id)));
        actions.extend_from_slice(&FIXED[12..]);
        let entries = actions.into_iter().map(|a| {
            let id = a.id();
            let keys = match s.changed.iter().find(|b| b.action == id) {
                Some(b) => b.keys.iter().filter_map(|k| Chord::parse(k)).collect(),
                None => a.defaults(),
            };
            (a, keys)
        }).collect();
        Keymap { entries }
    }

    pub fn entries(&self) -> &[(Action, Vec<Chord>)] { &self.entries }

    pub fn keys(&self, action: Action) -> &[Chord] {
        self.entries.iter().find(|(a, _)| *a == action).map_or(&[], |(_, k)| k.as_slice())
    }

    /// The action a key press runs: an exact match first, then (with Shift held) a key that does not ask for Shift,
    /// so Shift+→ is still Next.
    pub fn action_for(&self, pressed: &Chord) -> Option<Action> {
        let find = |c: &Chord| self.entries.iter().find(|(_, keys)| keys.contains(c)).map(|(a, _)| *a);
        find(pressed).or_else(|| pressed.shift.then(|| find(&Chord { shift: false, ..pressed.clone() })).flatten())
    }

    /// The menu item's shortcut for `action`: its first key that a text box never types. A typing key works only
    /// through the window's key scope, which knows when a text box has the keyboard.
    pub fn menu_key(&self, action: Action) -> Option<&Chord> { self.keys(action).iter().find(|c| !c.types_text()) }
}

/// Why a key is a poor choice for an action, for the editor to ask about before using it.
#[derive(Clone, Debug, PartialEq)]
pub enum Clash {
    /// Another action has it; using it here takes it off that one.
    Action(Action),
    /// A plain letter or digit that jumps the live song to a section (Settings → Songs → Section keys).
    SectionKey,
    /// Alt + 1–5 picks a Bible chip in the Bible box.
    BibleChip,
    /// Ctrl + A, C, X, V, Z or Y: text boxes use it for editing.
    TextEditing,
}

/// Keys the editor never takes: Esc (it cancels the change and leaves text boxes) and Tab (moves the focus).
pub fn reserved(chord: &Chord) -> bool { matches!(chord.key.as_str(), "Escape" | "Tab") }

/// What is wrong with giving `action` the key `chord`, the worst first. `section_keys`: the Settings switch.
pub fn clashes(map: &Keymap, action: Action, chord: &Chord, section_keys: bool) -> Vec<Clash> {
    let mut out = vec![];
    if let Some((other, _)) = map.entries().iter().find(|(a, keys)| *a != action && keys.contains(chord)) { out.push(Clash::Action(*other)) }
    if section_keys && !chord.ctrl && !chord.alt && crate::sections::jump_for_key(&chord.key, false, false).is_some() { out.push(Clash::SectionKey) }
    if chord.alt && !chord.ctrl && matches!(chord.key.as_str(), "1" | "2" | "3" | "4" | "5") { out.push(Clash::BibleChip) }
    let editing = matches!(chord.key.as_str(), "a" | "c" | "x" | "v" | "z" | "y");
    let its_own = matches!((action, chord.key.as_str()), (Action::Undo, "z") | (Action::Redo, "y") | (Action::PasteLink, "v"));
    if chord.ctrl && !chord.alt && editing && !its_own { out.push(Clash::TextEditing) }
    out
}

pub mod strings {
    use crate::tr;
    pub fn ctrl(key: &str) -> String { tr!("Ctrl + {key}", key = key) }
    pub fn alt(key: &str) -> String { tr!("Alt + {key}", key = key) }
    pub fn shift(key: &str) -> String { tr!("Shift + {key}", key = key) }
    pub fn space() -> String { tr!("Space") }
    pub fn page_down() -> String { tr!("Page Down") }
    pub fn page_up() -> String { tr!("Page Up") }
    pub fn enter() -> String { tr!("Enter") }
    pub fn esc() -> String { tr!("Esc") }
    pub fn delete() -> String { tr!("Delete") }
    pub fn backspace() -> String { tr!("Backspace") }
    pub fn clear_message() -> String { tr!("Clear the message") }
    pub fn clear_timers() -> String { tr!("Clear the timers") }
    pub fn clear_group(name: &str) -> String { tr!("Clear group: {name}", name = name) }
    pub fn find_mouse() -> String { tr!("Find my mouse") }
    pub fn all_but_background() -> String { tr!("Everything but the background") }
}

#[cfg(test)]
#[path = "shortcuts_tests.rs"]
mod tests;
