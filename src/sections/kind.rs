//! Section kinds, their short names, jump keys and the fixed colour palette (FEAT-4 spec §4.1, §6.1).
use super::strings;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind { Verse, Chorus, PreChorus, Bridge, Tag, Intro, Ending, Interlude, Other }

impl SectionKind {
    pub const ALL: [SectionKind; 9] = [
        SectionKind::Verse, SectionKind::Chorus, SectionKind::PreChorus, SectionKind::Bridge, SectionKind::Tag,
        SectionKind::Intro, SectionKind::Ending, SectionKind::Interlude, SectionKind::Other,
    ];

    /// "Chorus 2" → (Chorus, Some(2)). Brackets and a trailing `:` are ignored; Other never has a number.
    pub fn from_label(label: &str) -> (SectionKind, Option<u8>) {
        let lower = label.trim().trim_end_matches(':').trim_matches(['[', ']']).trim().trim_end_matches(':').trim().to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();
        let Some(first) = words.first() else { return (SectionKind::Other, None) };
        let (kind, rest) = match *first {
            "verse" => (SectionKind::Verse, 1),
            "chorus" | "refrain" => (SectionKind::Chorus, 1),
            "pre-chorus" | "prechorus" => (SectionKind::PreChorus, 1),
            "pre" if words.get(1) == Some(&"chorus") => (SectionKind::PreChorus, 2),
            "bridge" => (SectionKind::Bridge, 1),
            "tag" => (SectionKind::Tag, 1),
            "intro" => (SectionKind::Intro, 1),
            "ending" | "end" | "outro" | "coda" => (SectionKind::Ending, 1),
            "interlude" | "instrumental" | "turnaround" | "vamp" => (SectionKind::Interlude, 1),
            _ => return match translated_head(&lower) {
                Some((kind, rest)) => (kind, section_number(rest.split_whitespace().next())),
                None => (SectionKind::Other, None),
            },
        };
        (kind, section_number(words.get(rest).copied()))
    }

    /// "V2", "C", "PC", "B", "T", "I", "E", "Int"; Other is always "•".
    pub fn short(self, number: Option<u8>) -> String {
        let letter = match self {
            SectionKind::Verse => "V",
            SectionKind::Chorus => "C",
            SectionKind::PreChorus => "PC",
            SectionKind::Bridge => "B",
            SectionKind::Tag => "T",
            SectionKind::Intro => "I",
            SectionKind::Ending => "E",
            SectionKind::Interlude => "Int",
            SectionKind::Other => return "•".into(),
        };
        match number { Some(n) => format!("{letter}{n}"), None => letter.into() }
    }

    /// The colour: the church's (Settings → Sections), else the usual one.
    pub fn color(self) -> String { super::look::current().color(self).unwrap_or_else(|| self.default_color().into()) }

    /// The usual colour. `ui/theme.slint` copies these; a test compares the two.
    pub fn default_color(self) -> &'static str {
        match self {
            SectionKind::Verse => "#6aa8ff",
            SectionKind::Chorus => "#ff8f93",
            SectionKind::PreChorus => "#f5b94a",
            SectionKind::Bridge => "#3ccf91",
            SectionKind::Tag => "#ff8a3d",
            SectionKind::Intro => "#6fd3e0",
            SectionKind::Ending => "#c9a5ff",
            SectionKind::Interlude => "#c8d36a",
            SectionKind::Other => "#5d6576",
        }
    }

    /// The jump key, lower case: the church's (Settings → Sections), else the usual one. Other has none.
    pub fn key(self) -> Option<char> {
        if self == SectionKind::Other { return None }
        super::look::current().key(self).or_else(|| self.default_key())
    }

    /// The usual jump key: Verse `v` (next verse), Interlude `l`; Other has none.
    pub fn default_key(self) -> Option<char> {
        Some(match self {
            SectionKind::Verse => 'v',
            SectionKind::Chorus => 'c',
            SectionKind::PreChorus => 'p',
            SectionKind::Bridge => 'b',
            SectionKind::Tag => 't',
            SectionKind::Intro => 'i',
            SectionKind::Ending => 'e',
            SectionKind::Interlude => 'l',
            SectionKind::Other => return None,
        })
    }

    /// The name the app gives this kind: the church's (Settings → Sections), else the usual one.
    pub fn name(self) -> String { super::look::current().name(self).unwrap_or_else(|| self.default_name()) }

    /// The usual name, in the app's language.
    pub fn default_name(self) -> String {
        match self {
            SectionKind::Verse => strings::verse(),
            SectionKind::Chorus => strings::chorus(),
            SectionKind::PreChorus => strings::pre_chorus(),
            SectionKind::Bridge => strings::bridge(),
            SectionKind::Tag => strings::tag(),
            SectionKind::Intro => strings::intro(),
            SectionKind::Ending => strings::ending(),
            SectionKind::Interlude => strings::interlude(),
            SectionKind::Other => strings::other(),
        }
    }

    /// The serde name, which `Theme.section-color` takes: "verse", "pre_chorus", …
    pub fn id(self) -> &'static str {
        match self {
            SectionKind::Verse => "verse",
            SectionKind::Chorus => "chorus",
            SectionKind::PreChorus => "pre_chorus",
            SectionKind::Bridge => "bridge",
            SectionKind::Tag => "tag",
            SectionKind::Intro => "intro",
            SectionKind::Ending => "ending",
            SectionKind::Interlude => "interlude",
            SectionKind::Other => "other",
        }
    }
}

/// "2" in "Verse 2": one or two digits, not 0.
fn section_number(word: Option<&str>) -> Option<u8> {
    word.filter(|w| (1..=2).contains(&w.len()) && w.chars().all(|c| c.is_ascii_digit())).and_then(|w| w.parse::<u8>().ok()).filter(|n| *n >= 1)
}

/// The section heading words of the language the app is shown in, longest first, with their kinds. Translators
/// may give several words for one, split by `|` ("Coro|Estribillo"). English words are left to the callers, which
/// always know them.
fn translated_heads() -> Vec<(String, SectionKind)> {
    use crate::trc;
    let words = [
        (SectionKind::Verse, "verse", trc!("song section heading", "Verse")),
        (SectionKind::Chorus, "chorus", trc!("song section heading", "Chorus")),
        (SectionKind::PreChorus, "pre-chorus", trc!("song section heading", "Pre-chorus")),
        (SectionKind::Bridge, "bridge", trc!("song section heading", "Bridge")),
        (SectionKind::Tag, "tag", trc!("song section heading", "Tag")),
        (SectionKind::Intro, "intro", trc!("song section heading", "Intro")),
        (SectionKind::Ending, "ending", trc!("song section heading", "Ending")),
        (SectionKind::Interlude, "interlude", trc!("song section heading", "Interlude")),
    ];
    let mut out: Vec<(String, SectionKind)> = words.iter().flat_map(|(kind, english, t)| {
        t.split('|').map(|w| w.trim().to_lowercase()).filter(|w| !w.is_empty() && w != english).map(|w| (w, *kind)).collect::<Vec<_>>()
    }).collect();
    // The church's own words (Settings → Sections).
    out.extend(super::look::current().words());
    out.sort_by_key(|(w, _)| std::cmp::Reverse(w.chars().count()));
    out
}

/// A lower-case heading that starts with a section word of the app's language ("coro", "verso 1"): its kind and
/// what follows the word.
pub(crate) fn translated_head(lower: &str) -> Option<(SectionKind, &str)> {
    translated_heads().into_iter().find_map(|(word, kind)| {
        let rest = lower.strip_prefix(word.as_str())?;
        (rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace() || c.is_ascii_digit())).then(|| (kind, rest.trim_start()))
    })
}

/// The colour for a slide label, `None` for Other (for the stage and the phone).
pub fn color_for_label(label: &str) -> Option<String> {
    let (kind, _) = SectionKind::from_label(label);
    (kind != SectionKind::Other).then(|| kind.color())
}

#[cfg(test)]
pub(crate) mod colour {
    //! CIEDE2000 and WCAG contrast, for the palette tests.
    fn srgb(hex: &str) -> [f64; 3] {
        let h = hex.trim_start_matches('#');
        let c = |i: usize| f64::from(u8::from_str_radix(&h[i..i + 2], 16).unwrap()) / 255.0;
        [c(0), c(2), c(4)]
    }

    fn lab(hex: &str) -> [f64; 3] {
        let lin = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let [r, g, b] = srgb(hex).map(lin);
        let x = 0.4124 * r + 0.3576 * g + 0.1805 * b;
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let z = 0.0193 * r + 0.1192 * g + 0.9505 * b;
        let f = |t: f64| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
        let (fx, fy, fz) = (f(x / 0.95047), f(y), f(z / 1.08883));
        [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
    }

    pub fn de2000_lab(a: [f64; 3], b: [f64; 3]) -> f64 {
        let ([l1, a1, b1], [l2, a2, b2]) = (a, b);
        let cb = (a1.hypot(b1) + a2.hypot(b2)) / 2.0;
        let g = 0.5 * (1.0 - (cb.powi(7) / (cb.powi(7) + 25f64.powi(7))).sqrt());
        let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
        let (c1, c2) = (a1p.hypot(b1), a2p.hypot(b2));
        let hue = |x: f64, y: f64| if x == 0.0 && y == 0.0 { 0.0 } else { y.atan2(x).to_degrees().rem_euclid(360.0) };
        let (h1, h2) = (hue(a1p, b1), hue(a2p, b2));
        let dh = if c1 * c2 == 0.0 { 0.0 } else if (h2 - h1).abs() <= 180.0 { h2 - h1 } else if h2 - h1 > 180.0 { h2 - h1 - 360.0 } else { h2 - h1 + 360.0 };
        let d_h = 2.0 * (c1 * c2).sqrt() * (dh / 2.0).to_radians().sin();
        let (lb, cbp) = ((l1 + l2) / 2.0, (c1 + c2) / 2.0);
        let hb = if c1 * c2 == 0.0 { h1 + h2 } else if (h1 - h2).abs() <= 180.0 { (h1 + h2) / 2.0 } else if h1 + h2 < 360.0 { (h1 + h2 + 360.0) / 2.0 } else { (h1 + h2 - 360.0) / 2.0 };
        let cos = |d: f64| d.to_radians().cos();
        let t = 1.0 - 0.17 * cos(hb - 30.0) + 0.24 * cos(2.0 * hb) + 0.32 * cos(3.0 * hb + 6.0) - 0.20 * cos(4.0 * hb - 63.0);
        let dth = 30.0 * (-((hb - 275.0) / 25.0).powi(2)).exp();
        let rc = 2.0 * (cbp.powi(7) / (cbp.powi(7) + 25f64.powi(7))).sqrt();
        let sl = 1.0 + 0.015 * (lb - 50.0).powi(2) / (20.0 + (lb - 50.0).powi(2)).sqrt();
        let sc = 1.0 + 0.045 * cbp;
        let sh = 1.0 + 0.015 * cbp * t;
        let rt = -(2.0 * dth).to_radians().sin() * rc;
        let (x, y, z) = ((l2 - l1) / sl, (c2 - c1) / sc, d_h / sh);
        (x * x + y * y + z * z + rt * y * z).sqrt()
    }

    pub fn delta_e2000(a: &str, b: &str) -> f64 { de2000_lab(lab(a), lab(b)) }

    pub fn contrast(a: &str, b: &str) -> f64 {
        let lum = |h: &str| {
            let [r, g, b] = srgb(h).map(|c| if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) });
            0.2126 * r + 0.7152 * g + 0.0722 * b
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{colour::*, *};

    #[test]
    fn from_label_kinds_and_numbers() {
        use SectionKind::*;
        let t = SectionKind::from_label;
        assert_eq!(t("Verse 1"), (Verse, Some(1)));
        assert_eq!(t("Chorus 2"), (Chorus, Some(2)));
        assert_eq!(t("Pre-Chorus"), (PreChorus, None));
        assert_eq!(t("pre chorus"), (PreChorus, None));
        assert_eq!(t("Pre Chorus 2"), (PreChorus, Some(2)));
        assert_eq!(t("Refrain"), (Chorus, None));
        for l in ["Outro", "End", "Coda", "Ending"] { assert_eq!(t(l), (Ending, None)); }
        assert_eq!(t("Vamp"), (Interlude, None));
        assert_eq!(t("Part 3"), (Other, None));
        assert_eq!(t("Spontaneous"), (Other, None));
        assert_eq!(t("Post-Chorus"), (Other, None));
        assert_eq!(t("Verse 2a"), (Verse, None));
        assert_eq!(t("Verse 0"), (Verse, None));
        assert_eq!(t("Verse 100"), (Verse, None));
        assert_eq!(t("[Bridge]:"), (Bridge, None));
    }

    /// Spanish heading words, as a translator would give them.
    pub(crate) const ES: &str = r#"
msgid ""
msgstr ""
"Language: es\n"

msgctxt "song section heading"
msgid "Verse"
msgstr "Verso"

msgctxt "song section heading"
msgid "Chorus"
msgstr "Coro|Estribillo"

msgctxt "song section heading"
msgid "Pre-chorus"
msgstr "Pre-coro"

msgctxt "song section heading"
msgid "Bridge"
msgstr "Puente"

msgctxt "song section heading"
msgid "Tag"
msgstr "Tag"
"#;

    #[test]
    fn from_label_knows_the_ui_languages_heading_words() {
        use SectionKind::*;
        crate::i18n::testing::with_catalog(ES, || {
            let t = SectionKind::from_label;
            assert_eq!(t("Coro"), (Chorus, None));
            assert_eq!(t("Verso 1"), (Verse, Some(1)));
            assert_eq!(t("[Estribillo 2]"), (Chorus, Some(2)));
            assert_eq!(t("Pre-coro"), (PreChorus, None), "the longer word wins over Coro");
            assert_eq!(t("puente:"), (Bridge, None));
            assert_eq!(t("Verse 2"), (Verse, Some(2)), "English is always known");
            assert_eq!(t("Tag"), (Tag, None));
            assert_eq!(t("Coronas"), (Other, None), "a word that only starts like one");
        });
        assert_eq!(SectionKind::from_label("Coro"), (SectionKind::Other, None), "only in that language");
    }

    #[test]
    fn heading_words_reach_the_translation_template() {
        let msgs = crate::i18n::extract::from_rust(include_str!("kind.rs"), "src/sections/kind.rs").unwrap();
        let heads: Vec<&str> = msgs.iter().filter(|m| m.ctx.as_deref() == Some("song section heading")).map(|m| m.id.as_str()).collect();
        assert_eq!(heads, ["Verse", "Chorus", "Pre-chorus", "Bridge", "Tag", "Intro", "Ending", "Interlude"]);
    }

    #[test]
    fn short_names() {
        use SectionKind::*;
        assert_eq!(Verse.short(Some(2)), "V2");
        assert_eq!(Chorus.short(None), "C");
        assert_eq!(PreChorus.short(None), "PC");
        assert_eq!(Bridge.short(None), "B");
        assert_eq!(Interlude.short(None), "Int");
        assert_eq!(Other.short(Some(3)), "•");
    }

    #[test]
    fn keys_and_names() {
        let keys: Vec<_> = SectionKind::ALL.iter().map(|k| k.key()).collect();
        assert_eq!(keys, [Some('v'), Some('c'), Some('p'), Some('b'), Some('t'), Some('i'), Some('e'), Some('l'), None]);
        assert_eq!(SectionKind::PreChorus.name(), "Pre-chorus");
    }

    #[test]
    fn color_for_label_matches_kind_color() {
        assert_eq!(color_for_label("Chorus 2"), Some(SectionKind::Chorus.color()));
        assert_eq!(color_for_label("Part 1"), None);
    }

    #[test]
    fn serde_is_snake_case_and_matches_id() {
        for k in SectionKind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), k.id());
        }
        assert_eq!(SectionKind::PreChorus.id(), "pre_chorus");
    }

    #[test]
    fn ciede2000_reference_pairs() {
        // Sharma, Wu, Dalal (2005) test data, pairs 1 and 17.
        assert!((de2000_lab([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485]) - 2.0425).abs() < 1e-3);
        assert!((de2000_lab([50.0, 2.5, 0.0], [73.0, 25.0, -18.0]) - 27.1492).abs() < 1e-3);
        assert!((delta_e2000("#000000", "#ffffff") - 100.0).abs() < 0.05);
    }

    #[test]
    fn palette_is_distinct() {
        for (i, a) in SectionKind::ALL.iter().enumerate() {
            for b in &SectionKind::ALL[i + 1..] {
                let d = delta_e2000(&a.color(), &b.color());
                assert!(d >= 15.0, "{a:?}/{b:?} ΔE {d:.1}");
            }
        }
    }

    #[test]
    fn section_colours_read_with_dark_ink_and_on_the_panel() {
        for k in SectionKind::ALL.iter().filter(|k| **k != SectionKind::Other) {
            assert!(contrast(&k.color(), "#0d0f13") >= 4.5, "{k:?} with sec-ink");
            assert!(contrast(&k.color(), "#15181e") >= 3.0, "{k:?} on the panel");
        }
    }
}
