//! Settings dialog logic that needs no window.
use super::{Tone, strings as tr};
use crate::{model::Settings, ui::operator::view::{BibleProvider, BibleSource}};

/// `created` from `Store::backups` (`2026-11-01_140512`) as the Backups tab shows it (`operator.js:988`):
/// "2026-11-01 at 14:05 UTC".
pub fn backup_label(created: &str) -> String {
    let text = created.replacen('_', " at ", 1);
    let cut = text.len().saturating_sub(6);
    let time = text.as_bytes().get(cut..).filter(|t| t.len() == 6 && t.iter().all(u8::is_ascii_digit));
    match time {
        Some(_) => format!("{}{}:{} UTC", &text[..cut], &text[cut..cut + 2], &text[cut + 2..cut + 4]),
        None => format!("{text} UTC"),
    }
}

/// Before Save: a translation picked or a monitor chosen while Settings was open must not be undone by the older
/// copy this dialog edited (Plan 3 Task 20).
pub fn keep_live_fields(edited: &mut Settings, latest: &Settings) {
    edited.translation = latest.translation.clone();
    edited.displays = latest.displays.clone();
}

/// A translation's status chip (`CHIP`, `operator.js:933`).
pub fn bible_chip(status: &str) -> (Tone, &'static str) {
    match status {
        "offline" => (Tone::Ok, tr::BIBLE_OFFLINE),
        "online" => (Tone::Ok, tr::BIBLE_ONLINE),
        "needs_key" => (Tone::Warn, tr::BIBLE_NEEDS_KEY),
        _ => (Tone::Warn, tr::BIBLE_NO_SOURCE),
    }
}

/// The line under a translation's name (`hint`, `operator.js:935`).
pub fn bible_hint(source: &BibleSource, providers: &[BibleProvider]) -> String {
    match source.status.as_str() {
        "needs_key" => {
            let label = |id: &String| providers.iter().find(|p| p.id == *id).map(|p| p.label.as_str());
            tr::bible_connect(&source.providers.iter().filter_map(label).collect::<Vec<_>>())
        }
        "unavailable" => tr::BIBLE_NO_SOURCE_HINT.into(),
        _ => source.provider.clone().unwrap_or_default(),
    }
}

/// The copyright lines at the foot of the Bible tab: online or imported translations that have one (`operator.js:958`).
pub fn bible_credits(sources: &[BibleSource]) -> Vec<&str> {
    sources.iter()
        .filter(|b| b.status == "online" || b.imported)
        .filter_map(|b| b.copyright.as_deref())
        .filter(|c| !c.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        live::Command,
        ui::operator::keys::{KeyAction, KeyCtx, route},
    };

    #[test]
    fn backup_label_formats_like_web() {
        assert_eq!(backup_label("2026-11-01_140512"), "2026-11-01 at 14:05 UTC");
    }

    #[test]
    fn backup_label_leaves_an_unexpected_stamp_alone() {
        assert_eq!(backup_label("oddly named"), "oddly named UTC");
        assert_eq!(backup_label("12345"), "12345 UTC");
        assert_eq!(backup_label("é12345"), "é12345 UTC", "must not cut inside a character");
    }

    #[test]
    fn backup_contents_like_web() {
        assert_eq!(tr::backup_contents(12, 3, 40), "12 songs · 3 services · 40 files");
    }

    #[test]
    fn save_keeps_translation_and_displays_changed_meanwhile() {
        let mut edited = Settings { lines_per_slide: 4, ..Settings::default() };
        let mut latest = Settings { translation: "ESV".into(), ..Settings::default() };
        latest.displays.room = Some("tv".into());
        keep_live_fields(&mut edited, &latest);
        assert_eq!(edited.translation, "ESV");
        assert_eq!(edited.displays.room.as_deref(), Some("tv"));
        assert_eq!(edited.lines_per_slide, 4, "what the user edited stays");
    }

    fn source(status: &str) -> BibleSource {
        BibleSource { translation: "ESV".into(), status: status.into(), ..BibleSource::default() }
    }

    fn provider(id: &str, label: &str) -> BibleProvider {
        BibleProvider { id: id.into(), label: label.into(), ..BibleProvider::default() }
    }

    #[test]
    fn bible_chips_match_the_web() {
        assert_eq!(bible_chip("offline"), (Tone::Ok, "Ready — offline"));
        assert_eq!(bible_chip("online"), (Tone::Ok, "Ready — online"));
        assert_eq!(bible_chip("needs_key"), (Tone::Warn, "Needs a free key"));
        assert_eq!(bible_chip("unavailable"), (Tone::Warn, "No source yet"));
    }

    #[test]
    fn bible_hint_names_the_services_to_connect() {
        let all = [provider("esv", "Crossway"), provider("api_bible", "API.Bible")];
        let needs = |ids: &[&str]| BibleSource { providers: ids.iter().map(|s| s.to_string()).collect(), ..source("needs_key") };
        assert_eq!(bible_hint(&needs(&["esv", "api_bible"]), &all), "Connect Crossway or API.Bible below");
        assert_eq!(bible_hint(&needs(&["api_bible", "gone"]), &all), "Connect API.Bible below");
    }

    #[test]
    fn bible_hint_for_other_states() {
        assert_eq!(bible_hint(&source("unavailable"), &[]), "No online source offers this one. Import a licensed file, or paste verses when you add them.");
        let online = BibleSource { provider: Some("Crossway".into()), ..source("online") };
        assert_eq!(bible_hint(&online, &[]), "Crossway");
        assert_eq!(bible_hint(&source("offline"), &[]), "");
    }

    #[test]
    fn bible_credits_only_for_online_or_imported_with_text() {
        let credit = |status: &str, imported: bool, copyright: Option<&str>| BibleSource {
            imported, copyright: copyright.map(String::from), ..source(status)
        };
        let all = [
            credit("online", false, Some("ESV text")),
            credit("offline", true, Some("Imported text")),
            credit("offline", false, Some("Built in")),
            credit("online", false, None),
            credit("online", false, Some("")),
        ];
        assert_eq!(bible_credits(&all), ["ESV text", "Imported text"]);
    }

    // ----- Help -----

    const PLAIN: KeyCtx = KeyCtx { in_text_field: false, in_bible_box: false, dialog_open: false };
    const TYPING: KeyCtx = KeyCtx { in_text_field: true, in_bible_box: false, dialog_open: false };
    const BIBLE: KeyCtx = KeyCtx { in_text_field: true, in_bible_box: true, dialog_open: false };

    /// What the Help table calls a key.
    fn help_name(key: &str, ctrl: bool, alt: bool) -> String {
        let plain = match key {
            "ArrowRight" => "→",
            "ArrowDown" => "↓",
            "ArrowLeft" => "←",
            "ArrowUp" => "↑",
            " " => "Space",
            "PageDown" => "Page Down",
            "PageUp" => "Page Up",
            "Escape" => "Esc",
            other => other,
        };
        // `route` ignores modifiers on every other key, so Ctrl+F1 is simply F1.
        if ctrl && key == "b" {
            "Ctrl + B".into()
        } else if alt && key.len() == 1 && key.as_bytes()[0].is_ascii_digit() {
            "Alt + 1–6".into()
        } else {
            plain.into()
        }
    }

    fn help_keys() -> Vec<&'static str> {
        tr::HELP_KEYS.iter().flat_map(|(keys, _)| keys.split("  ")).collect()
    }

    #[test]
    fn help_lists_every_bound_key() {
        let mut keys: Vec<String> = ["ArrowRight", "ArrowLeft", "ArrowDown", "ArrowUp", "PageDown", "PageUp", "Enter", "Escape", " ", "Tab", "Home", "End"]
            .map(String::from).to_vec();
        keys.extend((1..=12).map(|n| format!("F{n}")));
        keys.extend(('a'..='z').chain('0'..='9').map(String::from));
        let listed = help_keys();
        let mut bound = 0;
        for key in &keys {
            for (ctrl, alt) in [(false, false), (true, false), (false, true)] {
                for ctx in [PLAIN, TYPING, BIBLE] {
                    if route(key, ctrl, alt, ctx) != KeyAction::None {
                        bound += 1;
                        let name = help_name(key, ctrl, alt);
                        assert!(listed.contains(&name.as_str()), "{name} works but Help does not list it");
                    }
                }
            }
        }
        assert!(bound > 20, "the loop must have found the bindings, got {bound}");
    }

    #[test]
    fn every_help_row_does_what_it_says() {
        let key_for = |name: &str| match name {
            "→" => "ArrowRight", "↓" => "ArrowDown", "←" => "ArrowLeft", "↑" => "ArrowUp", "Space" => " ",
            "Page Down" => "PageDown", "Page Up" => "PageUp", "Esc" => "Escape", other => other,
        }.to_string();
        for (keys, what) in tr::HELP_KEYS {
            for name in keys.split("  ") {
                let action = match name {
                    "Ctrl + B" => route("b", true, false, PLAIN),
                    "Alt + 1–6" => route("1", false, true, BIBLE),
                    "Esc" => route("Escape", false, false, TYPING),
                    _ => route(&key_for(name), false, false, PLAIN),
                };
                let expected = match *what {
                    "Next" => KeyAction::Next,
                    "Back" => KeyAction::Prev,
                    "Show the selected item" => KeyAction::TakeArmedOrSelected,
                    "Bible box" => KeyAction::FocusBible,
                    "Clear all" => KeyAction::Clear(Command::ClearAll),
                    "Clear text" => KeyAction::Clear(Command::ClearSlide),
                    "Clear media & background" => KeyAction::Clear(Command::ClearMedia),
                    "Stop audio" => KeyAction::Clear(Command::ClearAudio),
                    "Logo" => KeyAction::Clear(Command::Logo),
                    "Leave a text box" => KeyAction::Blur,
                    "Bible translation (in the Bible box)" => KeyAction::PickTranslation(0),
                    other => panic!("no expectation for Help row {other:?}"),
                };
                assert_eq!(action, expected, "{name}");
            }
        }
    }
}
