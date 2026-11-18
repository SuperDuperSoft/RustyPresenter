//! The phone's language → the app's words. The app has the desktop app's 13 languages besides English (its words
//! are the desktop's own where they are the same, `translations/` in this folder).

/// The languages the app's words come in (folder names under `translations/`).
pub const LANGUAGES: [&str; 13] = ["de", "es", "fil", "fr", "ht", "ja", "ko", "pt-BR", "ru", "sw", "uk", "vi", "zh-CN"];

/// Android's language and country (`en`, `US`; old codes such as `in` and `iw` included) → one of `LANGUAGES`, or
/// None for English and languages the app does not have.
pub fn pick(language: &str, country: &str) -> Option<&'static str> {
    let l = language.trim().to_ascii_lowercase();
    let _ = country;
    let code = match l.as_str() {
        "tl" | "fil" => "fil",
        "pt" => "pt-BR",
        "zh" => "zh-CN",
        other => other,
    };
    LANGUAGES.iter().copied().find(|c| *c == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `@tr("…")` text of the screens (`ui/*.slint`), as written there.
    fn screen_texts() -> Vec<String> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui");
        let mut out = vec![];
        for f in std::fs::read_dir(dir).unwrap().flatten() {
            let src = std::fs::read_to_string(f.path()).unwrap();
            for part in src.split("@tr(\"").skip(1) {
                let mut text = String::new();
                let mut chars = part.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => if let Some(n) = chars.next() { text.push(n) },
                        c => text.push(c),
                    }
                }
                if !out.contains(&text) { out.push(text) }
            }
        }
        out
    }

    /// Every word on the screens is translated in every language (the `.po` files in `translations/`).
    #[test]
    fn every_screen_word_is_translated() {
        let texts = screen_texts();
        assert!(texts.len() > 50, "found {}", texts.len());
        for l in LANGUAGES {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("translations/{l}/LC_MESSAGES/rustypresenter-remote.po"));
            let po = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("no {}", path.display()));
            for t in &texts {
                let id = format!("msgid \"{}\"\nmsgstr \"", t.replace('\\', "\\\\").replace('"', "\\\""));
                let at = po.find(&id).unwrap_or_else(|| panic!("“{t}” is not in {l}"));
                assert!(!po[at + id.len()..].starts_with('"'), "“{t}” is empty in {l}");
            }
        }
    }

    #[test]
    fn the_phones_language_picks_the_words() {
        assert_eq!(pick("es", "MX"), Some("es"));
        assert_eq!(pick("pt", "PT"), Some("pt-BR"), "one Portuguese");
        assert_eq!(pick("zh", "TW"), Some("zh-CN"), "one Chinese");
        assert_eq!(pick("tl", "PH"), Some("fil"), "Tagalog phones say tl");
        assert_eq!(pick("en", "US"), None);
        assert_eq!(pick("nl", ""), None);
        assert_eq!(pick("DE", ""), Some("de"));
    }
}
