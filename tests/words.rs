//! One word for each thing (docs/superpowers/specs/2026-11-17-words-design.md): no text in
//! `translations/rustypresenter.pot` uses a word the glossary retired, so new texts keep calling the audience screen the
//! audience screen (Jake, November 17, 2026: "I like Stage Screen and audience screen better"). Fails with the text, the retired word and the word to use instead.
use rustypresenter::i18n::po;
use std::path::Path;

/// `(retired word, the word to use)`. Matched as whole words, ignoring capitals unless the word has some.
const RETIRED: &[(&str, &str)] = &[
    // The screens.
    ("room screen", "audience screen (the screen the people in the room see)"),
    ("stage display", "stage screen"),
    ("stage view", "stage screen (on a phone: the stage screen for phones)"),
    ("Screen Configuration", "Screens (the window's name in the Window menu)"),
    ("confidence monitor", "stage screen"),
    // The livestream.
    ("live stream", "livestream"),
    ("live-stream", "livestream"),
    // Songs and the Bible.
    ("lyrics", "words (song words)"),
    ("lyric", "song"),
    ("scripture", "Bible passage"),
    ("Bible reading", "Bible passage"),
    ("Bible readings", "Bible passages"),
    ("order of service", "service order"),
    // The clear keys.
    ("Clear All", "Clear all"),
    ("Clear everything", "Clear all"),
    ("Stop audio", "Clear audio"),
    ("Clear words", "Clear text"),
    // Sound.
    ("sound desk", "sound board"),
    ("soundboard", "sound board"),
    ("mixing desk", "sound board"),
    // Plain words.
    ("opacity", "see-through"),
    ("transparent", "see-through"),
    ("transparency", "see-through"),
    ("this PC", "this computer"),
    ("e-mail", "email"),
    // British spelling, as most of the app is.
    ("color", "colour"),
    ("colors", "colours"),
    ("center", "centre"),
    ("centered", "centred"),
    ("license", "licence (the noun; \"licensed\" is fine)"),
    ("licenses", "licences"),
    ("favorite", "favourite"),
    ("gray", "grey"),
    ("recognize", "recognise"),
    ("organize", "organise"),
    // Settings paths: the tab's name, then the section, with →.
    ("Settings → Bibles", "Settings → Bible"),
    ("Settings → Recording", "Settings → Media → Recording"),
    ("Settings → Livestream", "Settings → Media → Livestream"),
    ("Settings → Stream", "Settings → Media → Livestream"),
    ("Settings ->", "Settings →"),
    ("Settings >", "Settings →"),
];

/// Other people's names that hold a retired word: removed from a text before it is checked.
const ALLOWED: &[&str] = &[
    // SongSelect's own download and file names.
    "SongSelect lyrics (.txt)",
    "Download → Lyrics",
    "\"Lyrics (.txt)\"",
    // CCLI's licence for streaming, by its own name.
    "Streaming License",
    // TikTok's page for going live, by its own name.
    "LIVE Center",
    // Twitch's own settings page (the Twitch website, not ours).
    "choose Settings → Stream",
];

/// The text without `{…}` gaps (like `{color}` or `{{license}}`, filled in by the code) and without `ALLOWED` names.
fn checked_text(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    ALLOWED.iter().fold(out, |t, a| t.replace(a, " "))
}

/// Whether `word` is in `text` as a whole word (not inside a longer one). Capitals matter only if `word` has some
/// after its first letter or is all capitals in a name like "Clear All".
fn has_word(text: &str, word: &str) -> bool {
    let exact = word.chars().skip(1).any(char::is_uppercase) || word.contains('→');
    let (t, w) = if exact { (text.to_string(), word.to_string()) } else { (text.to_lowercase(), word.to_lowercase()) };
    let word_char = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    let mut from = 0;
    while let Some(at) = t[from..].find(&w) {
        let start = from + at;
        let end = start + w.len();
        let before = t[..start].chars().next_back();
        let after = t[end..].chars().next();
        if !before.is_some_and(word_char) && !after.is_some_and(word_char) { return true }
        from = start + w.chars().next().map_or(1, char::len_utf8);
    }
    false
}

#[test]
fn texts_use_the_glossary_words() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("translations/rustypresenter.pot");
    let pot = po::parse(&std::fs::read_to_string(&path).unwrap()).expect("the template reads");
    let mut bad = Vec::new();
    for e in &pot.entries {
        for text in std::iter::once(&e.id).chain(e.plural.as_ref()) {
            let t = checked_text(text);
            for (old, new) in RETIRED {
                if has_word(&t, old) { bad.push(format!("{text:?}: says \"{old}\"; the app says {new}")) }
            }
            if t.contains("...") { bad.push(format!("{text:?}: three dots; use the one character …")) }
        }
    }
    assert!(bad.is_empty(), "Texts that use a retired word (see docs/superpowers/specs/2026-11-17-words-design.md):\n{}", bad.join("\n"));
}

#[test]
fn the_checks_find_whole_words_only() {
    assert!(has_word("The Room screen", "room screen"));
    assert!(!has_word("Audiences", "audience"), "only whole words");
    assert!(has_word("Clear All", "Clear All") && !has_word("Clear all", "Clear All"), "capitals matter in names");
    assert!(!has_word(&checked_text("Use {color} here"), "color"), "gaps are not words");
    assert!(!has_word(&checked_text("under your Streaming License"), "license"), "other people's names are allowed");
    assert!(has_word(&checked_text("a license"), "license"));
    assert!(!has_word("on-air see-through", "through"), "a hyphen joins words");
}
