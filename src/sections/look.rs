//! Settings → Songs → Sections (N19): the church's own name, colour and key for each kind of section, extra heading
//! words that count as a kind, and the slide labels (Blank, Instrumental…) a slide can carry.
//!
//! The look in use is held for the whole program (like the language), so every caller of `SectionKind::color`,
//! `key` and `name` follows it without being handed the settings. `App` sets it whenever the data changes.
use super::SectionKind;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[cfg(not(test))]
use std::sync::RwLock;

/// One kind's changes. An empty field keeps the usual one.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct KindLook {
    pub kind: SectionKind,
    #[serde(default)]
    pub name: String,
    /// `#rrggbb`.
    #[serde(default)]
    pub color: String,
    /// One letter, lower case.
    #[serde(default)]
    pub key: String,
}

/// A heading word the church adds ("Spontaneous", "Hook"): a section with that name counts as `kind`, with its
/// colour and key.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AddedSection { pub name: String, pub kind: SectionKind }

/// A slide label: `{Name}` on its own line at the top of a slide in the lyrics. Thumbnails show it in `color`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SlideLabel { pub name: String, pub color: String }

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct SectionLook {
    pub kinds: Vec<KindLook>,
    pub added: Vec<AddedSection>,
    /// `None`: the built-in four, in the app's language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<SlideLabel>>,
}

/// Colour families for the picker: blues, reds, oranges, yellows, greens, teals, purples, greys. Each reads with
/// dark ink on top.
pub const FAMILIES: [[&str; 4]; 8] = [
    ["#6aa8ff", "#8fbfff", "#4f8fe8", "#a9c8ff"],
    ["#ff8f93", "#ff6b70", "#ffb0b3", "#f27b9b"],
    ["#ff8a3d", "#ffa866", "#f5a35c", "#ffc08a"],
    ["#f5b94a", "#f2d08f", "#e8c547", "#c8d36a"],
    ["#3ccf91", "#6fdcaa", "#8fd36a", "#52b788"],
    ["#6fd3e0", "#4cc2c9", "#9ae3ec", "#5fb8d9"],
    ["#c9a5ff", "#b38cff", "#dcc4ff", "#e0a3e8"],
    ["#8d95a8", "#a7adbb", "#c5cad4", "#b8ad9c"],
];

/// The built-in slide labels' colours, in order: Blank, Instrumental, Repeat, Spoken Word.
const LABEL_COLORS: [&str; 4] = ["#8d95a8", "#c8d36a", "#6fd3e0", "#e0a3e8"];

pub fn default_labels() -> Vec<SlideLabel> {
    let names = [super::strings::label_blank(), super::strings::label_instrumental(), super::strings::label_repeat(), super::strings::label_spoken()];
    names.into_iter().zip(LABEL_COLORS).map(|(name, c)| SlideLabel { name, color: c.into() }).collect()
}

/// `#rrggbb`.
pub fn is_hex(c: &str) -> bool { c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|c| c.is_ascii_hexdigit()) }

impl SectionLook {
    fn kind(&self, k: SectionKind) -> Option<&KindLook> { self.kinds.iter().find(|l| l.kind == k) }

    pub fn color(&self, k: SectionKind) -> Option<String> {
        self.kind(k).map(|l| l.color.trim().to_ascii_lowercase()).filter(|c| is_hex(c))
    }

    pub fn name(&self, k: SectionKind) -> Option<String> {
        self.kind(k).map(|l| l.name.trim().to_string()).filter(|n| !n.is_empty())
    }

    /// `Some(None)`: the key is given but not a letter (no key).
    pub fn key(&self, k: SectionKind) -> Option<char> {
        let key = self.kind(k)?.key.trim().to_lowercase();
        let mut chars = key.chars();
        match (chars.next(), chars.next()) { (Some(c), None) if c.is_alphabetic() => Some(c), _ => None }
    }

    /// The slide labels in use.
    pub fn labels(&self) -> Vec<SlideLabel> { self.labels.clone().unwrap_or_else(default_labels) }

    /// A label's colour (any letter case); `None` for a name not in the list.
    pub fn label_color(&self, name: &str) -> Option<String> {
        self.labels().into_iter().find(|l| l.name.trim().eq_ignore_ascii_case(name.trim())).map(|l| l.color).filter(|c| is_hex(c))
    }

    /// The heading words this look adds, lower case, with their kinds: each renamed kind's new name, then the
    /// added sections.
    pub fn words(&self) -> Vec<(String, SectionKind)> {
        let renamed = SectionKind::ALL.iter().filter_map(|k| self.name(*k).map(|n| (n, *k)));
        let added = self.added.iter().map(|a| (a.name.trim().to_string(), a.kind));
        renamed.chain(added).map(|(n, k)| (n.to_lowercase(), k)).filter(|(n, k)| !n.is_empty() && *k != SectionKind::Other).collect()
    }

    /// What is wrong with this look, for Save: a key two kinds share, or a colour that is not `#rrggbb`.
    pub fn problem(&self) -> Option<String> {
        for l in &self.kinds {
            if !l.color.trim().is_empty() && !is_hex(l.color.trim()) { return Some(super::strings::bad_color(&l.color)) }
            if !l.key.trim().is_empty() && self.key(l.kind).is_none() { return Some(super::strings::bad_key(&l.key)) }
        }
        let keys: Vec<(SectionKind, char)> = SectionKind::ALL.iter().filter_map(|k| Some((*k, effective_key(self, *k)?))).collect();
        for (i, (a, ka)) in keys.iter().enumerate() {
            if let Some((b, _)) = keys[i + 1..].iter().find(|(_, kb)| kb == ka) {
                return Some(super::strings::key_taken(ka.to_ascii_uppercase(), &name_in(self, *a), &name_in(self, *b)))
            }
        }
        None
    }
}

fn effective_key(look: &SectionLook, k: SectionKind) -> Option<char> { look.key(k).or_else(|| k.default_key()) }
fn name_in(look: &SectionLook, k: SectionKind) -> String { look.name(k).unwrap_or_else(|| k.default_name()) }

#[cfg(not(test))]
static CURRENT: RwLock<Option<Arc<SectionLook>>> = RwLock::new(None);

#[cfg(test)]
thread_local! {
    /// Unit tests: each test thread has its own look, so one test's colours never reach another's.
    static LOCAL: std::cell::RefCell<Option<Arc<SectionLook>>> = const { std::cell::RefCell::new(None) };
}

/// Uses `look` from now on (for the whole program; for this thread only in unit tests).
pub fn set_look(look: &SectionLook) {
    if *current() == *look { return }
    let look = (*look != SectionLook::default()).then(|| Arc::new(look.clone()));
    #[cfg(test)]
    LOCAL.with(|l| *l.borrow_mut() = look);
    #[cfg(not(test))]
    { *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = look; }
}

/// The look in use.
pub fn current() -> Arc<SectionLook> {
    #[cfg(test)]
    let now = LOCAL.with(|l| l.borrow().clone());
    #[cfg(not(test))]
    let now = CURRENT.read().unwrap_or_else(|e| e.into_inner()).clone();
    now.unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn with_look<R>(look: SectionLook, f: impl FnOnce() -> R) -> R {
    let before = LOCAL.with(|l| l.replace(Some(Arc::new(look))));
    let out = f();
    LOCAL.with(|l| *l.borrow_mut() = before);
    out
}

/// A slide label line: `{Instrumental}` on its own, 1–30 characters with no braces, `:` or `;` (ChordPro's
/// `{title: …}` is not one). The name, trimmed.
pub fn label_line(line: &str) -> Option<&str> {
    let inner = line.trim().strip_prefix('{')?.strip_suffix('}')?.trim();
    let ok = !inner.is_empty() && inner.chars().count() <= 30 && !inner.contains(['{', '}', ':', ';']);
    ok.then_some(inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use SectionKind::*;

    fn look() -> SectionLook {
        SectionLook {
            kinds: vec![
                KindLook { kind: Chorus, name: "Refrain".into(), color: "#FF6B70".into(), key: "R".into() },
                KindLook { kind: Bridge, name: String::new(), color: "nope".into(), key: String::new() },
            ],
            added: vec![AddedSection { name: "Spontaneous".into(), kind: Interlude }],
            labels: None,
        }
    }

    #[test]
    fn overrides_and_defaults() {
        let l = look();
        assert_eq!(l.color(Chorus).as_deref(), Some("#ff6b70"));
        assert_eq!(l.color(Bridge), None, "not a colour: the usual one");
        assert_eq!(l.key(Chorus), Some('r'));
        assert_eq!(l.name(Chorus).as_deref(), Some("Refrain"));
        assert_eq!(l.name(Verse), None);
        assert_eq!(l.words(), [("refrain".to_string(), Chorus), ("spontaneous".to_string(), Interlude)]);
    }

    #[test]
    fn kinds_follow_the_look_in_use() {
        assert_eq!(Chorus.color(), "#ff8f93");
        with_look(look(), || {
            assert_eq!(Chorus.color(), "#ff6b70");
            assert_eq!(Chorus.key(), Some('r'));
            assert_eq!(Chorus.name(), "Refrain");
            assert_eq!(Bridge.color(), "#3ccf91");
            assert_eq!(Verse.key(), Some('v'));
            assert_eq!(SectionKind::from_label("Spontaneous"), (Interlude, None), "an added word");
            assert_eq!(SectionKind::from_label("Refrain 2"), (Chorus, Some(2)));
            assert_eq!(crate::slides::section_label("Spontaneous").as_deref(), Some("Spontaneous"), "a heading in the lyrics");
            assert_eq!(super::super::jump_for_key("r", false, false), Some((Chorus, None)));
            assert_eq!(super::super::jump_for_key("c", false, false), None, "C is free now");
        });
        assert_eq!(Chorus.key(), Some('c'), "only inside");
    }

    #[test]
    fn set_look_is_per_test_thread() {
        set_look(&look());
        assert_eq!(Chorus.name(), "Refrain");
        set_look(&SectionLook::default());
        assert_eq!(Chorus.name(), "Chorus");
    }

    #[test]
    fn problems_name_shared_keys_and_bad_colours() {
        assert_eq!(SectionLook::default().problem(), None);
        let mut l = look();
        assert_eq!(l.problem().as_deref(), Some("“nope” is not a colour. Use #rrggbb, like #6aa8ff."));
        l.kinds[1].color = String::new();
        assert_eq!(l.problem(), None);
        l.kinds[0].key = "b".into();
        assert_eq!(l.problem().as_deref(), Some("Refrain and Bridge both use the key B."));
        l.kinds[0].key = "7".into();
        assert_eq!(l.problem().as_deref(), Some("“7” is not a letter. Section keys are one letter each."));
    }

    #[test]
    fn labels_default_and_colours() {
        let l = SectionLook::default();
        assert_eq!(l.labels().iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Blank", "Instrumental", "Repeat", "Spoken Word"]);
        assert_eq!(l.label_color("instrumental").as_deref(), Some("#c8d36a"));
        assert_eq!(l.label_color("Choir"), None);
    }

    #[test]
    fn label_lines() {
        assert_eq!(label_line("{Instrumental}"), Some("Instrumental"));
        assert_eq!(label_line("  { Spoken Word } "), Some("Spoken Word"));
        assert_eq!(label_line("{title: Grace}"), None, "a ChordPro directive");
        assert_eq!(label_line("{}"), None);
        assert_eq!(label_line("Instrumental"), None);
        assert_eq!(label_line("{a} b"), None);
    }

    #[test]
    fn family_colours_read_with_dark_ink() {
        for c in FAMILIES.iter().flatten().chain(LABEL_COLORS.iter()) {
            assert!(super::super::kind::colour::contrast(c, "#0d0f13") >= 4.5, "{c}");
            assert!(is_hex(c));
        }
    }
}
