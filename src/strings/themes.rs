//! User-visible text for themes, backgrounds and overlays (FEAT-1 spec §3 and §8).
//! Core code uses these instead of literals. Reached as `crate::theme::strings` until Part B.

// Themes
pub const THEME_NAME_REQUIRED: &str = "Give the theme a name.";
pub const THEME_NAME_RESERVED: &str = "That name is used by a built-in theme. Pick another.";
pub const THEME_GONE: &str = "That theme is gone.";
pub const BUILTIN_NO_DELETE: &str = "Built-in themes can't be deleted.";
/// Plan decision 13: Standard is only written by `theme/save-standard`.
pub const STANDARD_SAVE_ELSEWHERE: &str = "Standard is changed with Save on its own editor.";
pub const THEME_SAVED: &str = "Theme saved";
pub const THEME_DELETED: &str = "Theme deleted";
pub const STANDARD_NAME: &str = "Standard";
pub const CHURCH_DEFAULT: &str = "Church default";
pub const STANDARD_HINT: &str = "Built in — the style RustyPresenter started with";
pub const DEFAULT_HINT: &str = "Church default — used whenever nothing else is picked";
pub const NEW_THEME_NAME: &str = "New theme";
pub const BOLD_DISABLED_TIP: &str = "Text is already bold in this theme";
pub const BUNDLED_FONT_NOTE: &str = "works on every computer";

// Backgrounds
pub const BACKGROUND_KIND: &str = "Backgrounds can be pictures or videos.";
pub const BACKGROUND_NAME_REQUIRED: &str = "Give the background a name.";
pub const BACKGROUND_GONE: &str = "That background is gone.";
pub const ADDED_TO_BACKGROUNDS: &str = "Added to backgrounds";
pub const OWN_BACKGROUND_SUFFIX: &str = " · own background";
pub const FROM_THEME: &str = "From theme";
pub const STANDARD_BACKGROUND: &str = "Standard background";
pub const CURRENT_NOT_IN_LIBRARY: &str = "Current (not in your backgrounds)";
pub const COULDNT_CONVERT: &str = "Couldn't convert";
pub const DARKEN_HELP: &str = "Makes text easier to read";

// Overlays and logo
pub const OVERLAY_NAME_REQUIRED: &str = "Give the overlay a name.";
pub const OVERLAY_TEXT_REQUIRED: &str = "Type the overlay's text.";
pub const OVERLAY_GONE: &str = "That overlay is gone.";
/// The spec gives no wording for this one; it is the plan's.
pub const OVERLAY_KIND: &str = "Only text, picture and clock overlays can be saved.";
pub const OVERLAY_CAP: &str = "Up to 8 overlays can be on at once. Turn one off first.";
pub const LOGO_FIRST: &str = "Choose a church logo first (Settings → Themes).";
pub const PICTURE_GONE: &str = "That picture is gone.";
/// The reason part of `overlay_failed` when an overlay's picture can't be read.
pub const PICTURE_UNREADABLE: &str = "the picture could not be loaded.";
pub const SCREEN_CLEARED: &str = "The screen was cleared — try again.";
pub const OVERLAY_CHANGED_TIP: &str = "Changed — turn it off and on to update.";
pub const LOGO_SAVED: &str = "Logo saved";
pub const LOGO_REMOVED: &str = "Logo removed";
pub const TEXT_OVERLAY_HELP: &str = "For a sign that stays up.";
/// Shown instead of `TEXT_OVERLAY_HELP` once FEAT-2 (Message) has merged.
pub const TEXT_OVERLAY_HELP_FEAT2: &str = "For a sign that stays up. For a notice like a parent's name, use Message (once available).";

// Data and saving
pub const NEWER_DATA: &str = "This data was saved by a newer RustyPresenter. Please install the latest version.";
pub const EXPECTED_OBJECT: &str = "Expected an object.";
pub const ITEM_TWICE: &str = "The same item is listed twice.";

pub fn theme_exists(name: &str) -> String {
    format!("There's already a theme called “{name}”.")
}

pub fn overlay_failed(name: &str, reason: &str) -> String {
    format!("Couldn't show the overlay “{name}”: {reason}")
}

pub fn own_backgrounds(n: usize) -> String {
    if n == 1 {
        "1 item has its own background. Use the theme's background for it too?".into()
    } else {
        format!("{n} items have their own background. Use the theme's background for them too?")
    }
}

pub fn font_missing(font: &str) -> String {
    format!("“{font}” isn't installed on this computer, so slides use Noto Sans.")
}

pub fn used_by(n: usize) -> String {
    format!("Used by {n} {}", if n == 1 { "item" } else { "items" })
}

pub fn numbered_background(n: usize) -> String {
    format!("Background {n}")
}

pub fn make_default_question(name: &str) -> String {
    format!("Make “{name}” the church default? Everything without its own theme will use it. You can switch back any time.")
}

pub fn now_default(name: &str) -> String {
    format!("“{name}” is now the church default")
}

/// Spec §3.1 step 6 with plan decision 12: each count is named only when it is not 0, singular for 1,
/// and the verb agrees with the total.
pub fn delete_theme_question(name: &str, items: usize, services: usize, is_default: bool) -> String {
    let mut s = format!("Delete the theme “{name}”?");
    let parts: Vec<String> = [(items, "item", "items"), (services, "service", "services")]
        .into_iter()
        .filter(|&(n, ..)| n > 0)
        .map(|(n, one, many)| format!("{n} {}", if n == 1 { one } else { many }))
        .collect();
    if !parts.is_empty() {
        let verb = if items + services == 1 { "uses" } else { "use" };
        s.push_str(&format!(" {} {verb} it; they'll use the church default instead.", parts.join(" and ")));
    }
    if is_default {
        s.push_str(" It's the church default, so Standard becomes the church default.");
    }
    s
}

pub fn remove_background_question(name: &str) -> String {
    format!("Remove “{name}” from backgrounds? Items that use it keep it.")
}

pub fn used_by_screen_views(names: &[String]) -> String {
    format!("Used by screen view(s): {}", names.join(", "))
}

pub fn converting(percent: u8) -> String {
    format!("Converting {percent} %")
}

pub fn theme_chip(name: &str) -> String {
    format!("Theme: {name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_question_leaves_out_zero_counts_and_adds_default_line() {
        assert_eq!(delete_theme_question("Christmas", 0, 0, false), "Delete the theme “Christmas”?");
        assert_eq!(delete_theme_question("Christmas", 3, 1, true), "Delete the theme “Christmas”? 3 items and 1 service use it; they'll use the church default instead. It's the church default, so Standard becomes the church default.");
    }

    #[test]
    fn delete_question_singular_and_one_sided_counts() {
        assert_eq!(delete_theme_question("A", 1, 0, false), "Delete the theme “A”? 1 item uses it; they'll use the church default instead.");
        assert_eq!(delete_theme_question("A", 2, 0, false), "Delete the theme “A”? 2 items use it; they'll use the church default instead.");
        assert_eq!(delete_theme_question("A", 0, 1, false), "Delete the theme “A”? 1 service uses it; they'll use the church default instead.");
        assert_eq!(delete_theme_question("A", 1, 1, false), "Delete the theme “A”? 1 item and 1 service use it; they'll use the church default instead.");
        assert_eq!(delete_theme_question("A", 0, 0, true), "Delete the theme “A”? It's the church default, so Standard becomes the church default.");
    }

    #[test]
    fn formatted_strings_match_spec() {
        assert_eq!(theme_exists("Easter"), "There's already a theme called “Easter”.");
        assert_eq!(overlay_failed("Logo", PICTURE_UNREADABLE), "Couldn't show the overlay “Logo”: the picture could not be loaded.");
        assert_eq!(font_missing("Lato"), "“Lato” isn't installed on this computer, so slides use Noto Sans.");
        assert_eq!(converting(40), "Converting 40 %");
        assert_eq!((numbered_background(1), numbered_background(12)), ("Background 1".to_string(), "Background 12".to_string()));
        assert_eq!((used_by(1), used_by(3)), ("Used by 1 item".to_string(), "Used by 3 items".to_string()));
    }

    #[test]
    fn text_overlay_help_lines_match_spec() {
        assert!(TEXT_OVERLAY_HELP_FEAT2.starts_with(TEXT_OVERLAY_HELP));
        assert!(TEXT_OVERLAY_HELP_FEAT2.ends_with("use Message (once available)."));
    }

    #[test]
    fn own_backgrounds_and_screen_views_match_spec() {
        assert_eq!(own_backgrounds(1), "1 item has its own background. Use the theme's background for it too?");
        assert_eq!(own_backgrounds(4), "4 items have their own background. Use the theme's background for them too?");
        assert_eq!(used_by_screen_views(&["Stream".to_string(), "Lobby".to_string()]), "Used by screen view(s): Stream, Lobby");
        assert_eq!(remove_background_question("Sky"), "Remove “Sky” from backgrounds? Items that use it keep it.");
    }

    #[test]
    fn default_and_chip_strings_match_spec() {
        assert_eq!(make_default_question("Easter"), "Make “Easter” the church default? Everything without its own theme will use it. You can switch back any time.");
        assert_eq!(now_default("Easter"), "“Easter” is now the church default");
        assert_eq!(theme_chip(CHURCH_DEFAULT), "Theme: Church default");
    }
}
