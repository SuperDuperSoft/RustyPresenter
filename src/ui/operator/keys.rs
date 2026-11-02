//! Operator keyboard shortcuts: which key does what, as pure functions (`operator.js:305-320`).
use crate::live::Command;
use slint::platform::Key;

#[derive(Clone, Debug, PartialEq)]
pub enum KeyAction {
    Clear(Command),
    FocusBible,
    Next,
    Prev,
    TakeArmedOrSelected,
    Blur,
    PickTranslation(usize),
    None,
}

/// Where the keyboard focus is. `dialog_open` is also true while a confirm or prompt overlay is up with no dialog
/// under it, because the web's `confirm()` and `prompt()` block every key too.
#[derive(Clone, Copy, Debug)]
pub struct KeyCtx {
    pub in_text_field: bool,
    pub in_bible_box: bool,
    pub dialog_open: bool,
}

/// The web name of a special key, from the private-use character Slint delivers for it. `None` means the text is
/// ordinary typing: route it as itself, with `key_name(text).unwrap_or(text)`.
pub fn key_name(text: &str) -> Option<&'static str> {
    let mut chars = text.chars();
    let (c, None) = (chars.next()?, chars.next()) else { return None };
    [
        (Key::RightArrow, "ArrowRight"),
        (Key::LeftArrow, "ArrowLeft"),
        (Key::DownArrow, "ArrowDown"),
        (Key::UpArrow, "ArrowUp"),
        (Key::PageDown, "PageDown"),
        (Key::PageUp, "PageUp"),
        (Key::Return, "Enter"),
        (Key::Escape, "Escape"),
        (Key::Space, " "),
        (Key::F1, "F1"),
        (Key::F2, "F2"),
        (Key::F3, "F3"),
        (Key::F4, "F4"),
        (Key::F5, "F5"),
    ]
    .into_iter()
    .find_map(|(key, name)| (char::from(key) == c).then_some(name))
}

/// What a key press does. The order of these checks is the behaviour; it follows the web handler, with the
/// Alt+digit translation shortcut added inside the Bible box.
pub fn route(key: &str, ctrl_or_cmd: bool, alt: bool, ctx: KeyCtx) -> KeyAction {
    let clear = match key {
        "F1" => Some(Command::ClearAll),
        "F2" => Some(Command::ClearSlide),
        "F3" => Some(Command::ClearMedia),
        "F4" => Some(Command::ClearAudio),
        "F5" => Some(Command::Logo),
        _ => None,
    };
    if let Some(command) = clear {
        return KeyAction::Clear(command);
    }
    if ctrl_or_cmd && key.eq_ignore_ascii_case("b") {
        return if ctx.dialog_open { KeyAction::None } else { KeyAction::FocusBible };
    }
    if ctx.dialog_open {
        return KeyAction::None;
    }
    if alt && ctx.in_bible_box && matches!(key.as_bytes(), [b'1'..=b'6']) {
        return KeyAction::PickTranslation(usize::from(key.as_bytes()[0] - b'1'));
    }
    if ctx.in_text_field || ctx.in_bible_box {
        return if key == "Escape" { KeyAction::Blur } else { KeyAction::None };
    }
    match key {
        "ArrowRight" | "ArrowDown" | "PageDown" | " " => KeyAction::Next,
        "ArrowLeft" | "ArrowUp" | "PageUp" => KeyAction::Prev,
        "Enter" => KeyAction::TakeArmedOrSelected,
        _ => KeyAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::platform::Key;

    const PLAIN: KeyCtx = KeyCtx { in_text_field: false, in_bible_box: false, dialog_open: false };
    const TYPING: KeyCtx = KeyCtx { in_text_field: true, in_bible_box: false, dialog_open: false };
    const BIBLE: KeyCtx = KeyCtx { in_text_field: true, in_bible_box: true, dialog_open: false };
    const DIALOG: KeyCtx = KeyCtx { in_text_field: false, in_bible_box: false, dialog_open: true };

    #[test]
    fn f_keys_clear_even_while_typing_or_in_a_dialog() {
        let ctx = KeyCtx { in_text_field: true, in_bible_box: true, dialog_open: true };
        assert_eq!(route("F1", false, false, ctx), KeyAction::Clear(Command::ClearAll));
        assert_eq!(route("F2", false, false, ctx), KeyAction::Clear(Command::ClearSlide));
        assert_eq!(route("F3", false, false, ctx), KeyAction::Clear(Command::ClearMedia));
        assert_eq!(route("F4", false, false, ctx), KeyAction::Clear(Command::ClearAudio));
        assert_eq!(route("F5", false, false, ctx), KeyAction::Clear(Command::Logo));
    }

    #[test]
    fn ctrl_b_focuses_the_bible_box_from_anywhere_but_a_dialog() {
        assert_eq!(route("b", true, false, PLAIN), KeyAction::FocusBible);
        assert_eq!(route("b", true, false, TYPING), KeyAction::FocusBible);
        assert_eq!(route("B", true, false, PLAIN), KeyAction::FocusBible, "Caps Lock or Shift, like the web's toLowerCase");
    }

    #[test]
    fn ctrl_b_ignored_while_a_dialog_is_open() {
        assert_eq!(route("b", true, false, DIALOG), KeyAction::None);
    }

    #[test]
    fn plain_b_is_not_a_shortcut() {
        assert_eq!(route("b", false, false, PLAIN), KeyAction::None);
    }

    #[test]
    fn a_dialog_blocks_every_other_key() {
        for key in ["ArrowRight", "ArrowLeft", " ", "PageDown", "Enter", "Escape"] {
            assert_eq!(route(key, false, false, DIALOG), KeyAction::None, "{key:?}");
        }
    }

    #[test]
    fn alt_digit_picks_translation_only_in_bible_box() {
        assert_eq!(route("1", false, true, BIBLE), KeyAction::PickTranslation(0));
        assert_eq!(route("6", false, true, BIBLE), KeyAction::PickTranslation(5));
        assert_eq!(route("7", false, true, BIBLE), KeyAction::None);
        assert_eq!(route("0", false, true, BIBLE), KeyAction::None);
        assert_eq!(route("2", false, false, BIBLE), KeyAction::None, "plain digits are typed");
        assert_eq!(route("2", false, true, TYPING), KeyAction::None);
        assert_eq!(route("2", false, true, PLAIN), KeyAction::None);
        let dialog_over_box = KeyCtx { dialog_open: true, ..BIBLE };
        assert_eq!(route("2", false, true, dialog_over_box), KeyAction::None);
    }

    #[test]
    fn esc_blurs_text_field() {
        assert_eq!(route("Escape", false, false, TYPING), KeyAction::Blur);
        assert_eq!(route("Escape", false, false, BIBLE), KeyAction::Blur);
        assert_eq!(route("Escape", false, false, PLAIN), KeyAction::None);
    }

    #[test]
    fn arrows_go_to_the_text_field_while_typing() {
        assert_eq!(route("ArrowRight", false, false, TYPING), KeyAction::None);
        assert_eq!(route(" ", false, false, TYPING), KeyAction::None);
        assert_eq!(route("Enter", false, false, TYPING), KeyAction::None);
    }

    #[test]
    fn the_bible_box_counts_as_a_text_field_on_its_own() {
        // Its focus comes from `has-focus`, not `OperatorState`, so the caller may set only `in_bible_box`.
        let ctx = KeyCtx { in_text_field: false, in_bible_box: true, dialog_open: false };
        assert_eq!(route("ArrowRight", false, false, ctx), KeyAction::None);
        assert_eq!(route("Escape", false, false, ctx), KeyAction::Blur);
    }

    #[test]
    fn space_pagedown_down_right_are_next() {
        for key in ["ArrowRight", "ArrowDown", "PageDown", " "] {
            assert_eq!(route(key, false, false, PLAIN), KeyAction::Next, "{key:?}");
        }
    }

    #[test]
    fn pageup_up_left_are_prev() {
        for key in ["ArrowLeft", "ArrowUp", "PageUp"] {
            assert_eq!(route(key, false, false, PLAIN), KeyAction::Prev, "{key:?}");
        }
    }

    #[test]
    fn enter_takes_armed_or_selected() {
        assert_eq!(route("Enter", false, false, PLAIN), KeyAction::TakeArmedOrSelected);
    }

    #[test]
    fn other_keys_do_nothing() {
        assert_eq!(route("x", false, false, PLAIN), KeyAction::None);
        assert_eq!(route("F6", false, false, PLAIN), KeyAction::None);
    }

    #[test]
    fn slint_key_characters_map_to_route_names() {
        let name = |k: Key| key_name(&String::from(char::from(k)));
        assert_eq!(name(Key::RightArrow), Some("ArrowRight"));
        assert_eq!(name(Key::LeftArrow), Some("ArrowLeft"));
        assert_eq!(name(Key::UpArrow), Some("ArrowUp"));
        assert_eq!(name(Key::DownArrow), Some("ArrowDown"));
        assert_eq!(name(Key::PageUp), Some("PageUp"));
        assert_eq!(name(Key::PageDown), Some("PageDown"));
        assert_eq!(name(Key::Return), Some("Enter"));
        assert_eq!(name(Key::Escape), Some("Escape"));
        assert_eq!(name(Key::Space), Some(" "));
        assert_eq!(name(Key::F1), Some("F1"));
        assert_eq!(name(Key::F2), Some("F2"));
        assert_eq!(name(Key::F3), Some("F3"));
        assert_eq!(name(Key::F4), Some("F4"));
        assert_eq!(name(Key::F5), Some("F5"));
    }

    #[test]
    fn ordinary_text_has_no_special_name() {
        // The caller routes `key_name(text).unwrap_or(text)`, so typed text passes through as itself.
        assert_eq!(key_name("b"), None);
        assert_eq!(key_name("1"), None);
        assert_eq!(key_name(""), None);
        assert_eq!(key_name(&String::from(char::from(Key::Home))), None);
    }
}
