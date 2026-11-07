//! User-visible text for themes, backgrounds and overlays (FEAT-1 spec §3 and §8).
//! Core code uses these instead of literals. Reached as `crate::theme::strings` until Part B.
use crate::{tr, trn};

// Themes
pub fn theme_name_required() -> String { tr!("Give the theme a name.") }
pub fn theme_name_reserved() -> String { tr!("That name is used by a built-in theme. Pick another.") }
pub fn theme_gone() -> String { tr!("That theme is gone.") }
pub fn builtin_no_delete() -> String { tr!("Built-in themes can't be deleted.") }
/// Plan decision 13: Standard is only written by `theme/save-standard`.
pub fn standard_save_elsewhere() -> String { tr!("Standard is changed with Save on its own editor.") }
pub fn theme_saved() -> String { tr!("Theme saved") }
pub fn theme_deleted() -> String { tr!("Theme deleted") }
pub fn standard_name() -> String { tr!("Standard") }
pub fn church_default() -> String { tr!("Church default") }
/// The built-in lower-third theme's name.
pub fn lower_third_name() -> String { tr!("Lower third") }
pub fn standard_hint() -> String { tr!("Built in — the style RustyPresenter started with") }
pub fn default_hint() -> String { tr!("Church default — used whenever nothing else is picked") }
pub fn new_theme_name() -> String { tr!("New theme") }
pub fn service_gone() -> String { tr!("That service is gone.") }
pub fn item_gone() -> String { tr!("That item is gone.") }
/// Decks and media items draw no text, so they take a background but no theme.
pub fn item_no_theme() -> String { tr!("Only songs, Bible passages and announcements use a theme.") }
pub fn bold_disabled_tip() -> String { tr!("Text is already bold in this theme") }
pub fn bundled_font_note() -> String { tr!("works on every computer") }

// Backgrounds
pub fn background_kind() -> String { tr!("Backgrounds can be pictures or videos.") }
pub fn background_name_required() -> String { tr!("Give the background a name.") }
pub fn background_gone() -> String { tr!("That background is gone.") }
pub fn added_to_backgrounds() -> String { tr!("Added to backgrounds") }
pub fn own_background_suffix() -> String { tr!(" · own background") }
pub fn from_theme() -> String { tr!("From theme") }
pub fn standard_background() -> String { tr!("Standard background") }
pub fn current_not_in_library() -> String { tr!("Current (not in your backgrounds)") }
pub fn couldnt_convert() -> String { tr!("Couldn't convert") }
/// Name for an imported background whose file name has no usable stem.
pub fn unnamed_background() -> String { tr!("Background") }
pub fn darken_help() -> String { tr!("Makes text easier to read") }

// Overlays and logo
pub fn overlay_name_required() -> String { tr!("Give the overlay a name.") }
pub fn overlay_text_required() -> String { tr!("Type the overlay's text.") }
pub fn overlay_gone() -> String { tr!("That overlay is gone.") }
/// The spec gives no wording for this one; it is the plan's.
pub fn overlay_kind() -> String { tr!("Only text, picture and clock overlays can be saved.") }
pub fn overlay_cap() -> String { tr!("Up to 8 overlays can be on at once. Turn one off first.") }
pub fn logo_first() -> String { tr!("Choose a church logo first (Settings → Themes).") }
pub fn picture_gone() -> String { tr!("That picture is gone.") }
/// The reason part of `overlay_failed` when an overlay's picture can't be read.
pub fn picture_unreadable() -> String { tr!("the picture could not be loaded.") }
pub fn screen_cleared() -> String { tr!("The screen was cleared — try again.") }
pub fn overlay_changed_tip() -> String { tr!("Changed — turn it off and on to update.") }
pub fn logo_saved() -> String { tr!("Logo saved") }
pub fn logo_removed() -> String { tr!("Logo removed") }
pub fn text_overlay_help() -> String { tr!("For a sign that stays up.") }
/// Shown instead of `text_overlay_help` once FEAT-2 (Message) has merged.
pub fn text_overlay_help_feat2() -> String { tr!("For a sign that stays up. For a notice like a parent's name, use Message (once available).") }

// Settings → Themes, the background library and the overlays strip
pub fn reset() -> String { tr!("Reset") }
pub fn delete() -> String { tr!("Delete") }
pub fn remove() -> String { tr!("Remove") }
/// The OK button of the star's question.
pub fn make_default() -> String { tr!("Make default") }
pub fn theme_reset() -> String { tr!("Theme put back as it came") }
pub fn rename_background() -> String { tr!("New name for this background") }
pub fn kind_color() -> String { tr!("Color") }
pub fn kind_gradient() -> String { tr!("Gradient") }
pub fn kind_picture() -> String { tr!("Picture") }
pub fn kind_video() -> String { tr!("Video") }
pub fn kind_text() -> String { tr!("Text") }
pub fn kind_clock() -> String { tr!("Clock") }
pub fn use_theme_background() -> String { tr!("Use theme background") }
pub fn keep_theirs() -> String { tr!("Keep theirs") }
pub fn manage_themes() -> String { tr!("Manage themes…") }
pub fn overlays_title() -> String { tr!("Overlays") }
pub fn overlay_saved() -> String { tr!("Overlay saved") }
pub fn overlay_deleted() -> String { tr!("Overlay deleted") }
pub fn new_overlay_name() -> String { tr!("New overlay") }
pub fn clear_overlays_help() -> String { tr!("Clear overlays") }
pub fn add_overlays_tip() -> String { tr!("A logo in the corner, a clock or a short sign") }
pub fn overlay_text_sample() -> String { tr!("Welcome!") }

pub fn reset_question(name: &str) -> String {
    tr!("Put “{name}” back the way it came? Your changes to it are lost.", name = name)
}

pub fn used_by_services(n: usize) -> String {
    trn!("{n} service", "{n} services", n)
}

/// The first line of the service's theme menu, and of Item edit's.
pub fn church_default_named(name: &str) -> String {
    tr!("Church default ({name})", name = name)
}

pub fn same_as_service(name: &str) -> String {
    tr!("Same as service ({name})", name = name)
}

pub fn delete_overlay_question(name: &str) -> String {
    tr!("Delete the overlay “{name}”? If it is on the screen, it stays there until you turn it off.", name = name)
}

// Data and saving
pub fn newer_data() -> String { tr!("This data was saved by a newer RustyPresenter. Please install the latest version.") }
pub fn expected_object() -> String { tr!("Expected an object.") }
pub fn item_twice() -> String { tr!("The same item is listed twice.") }

pub fn theme_exists(name: &str) -> String {
    tr!("There's already a theme called “{name}”.", name = name)
}

pub fn overlay_failed(name: &str, reason: &str) -> String {
    tr!("Couldn't show the overlay “{name}”: {reason}", name = name, reason = reason)
}

/// The phone's name for an overlay that is still on screen but was deleted from the library.
pub fn deleted_overlay(name: &str) -> String {
    tr!("(deleted) {name}", name = name)
}

pub fn own_backgrounds(n: usize) -> String {
    trn!("{n} item has its own background. Use the theme's background for it too?",
        "{n} items have their own background. Use the theme's background for them too?", n)
}

pub fn font_missing(font: &str) -> String {
    tr!("“{font}” isn't installed on this computer, so slides use Noto Sans.", font = font)
}

pub fn used_by(n: usize) -> String {
    trn!("Used by {n} item", "Used by {n} items", n)
}

pub fn numbered_background(n: usize) -> String {
    tr!("Background {n}", n = n)
}

pub fn make_default_question(name: &str) -> String {
    tr!("Make “{name}” the church default? Everything without its own theme will use it. You can switch back any time.", name = name)
}

pub fn now_default(name: &str) -> String {
    tr!("“{name}” is now the church default", name = name)
}

/// Spec §3.1 step 6 with plan decision 12: each count is named only when it is not 0, singular for 1,
/// and the verb agrees with the total.
pub fn delete_theme_question(name: &str, items: usize, services: usize, is_default: bool) -> String {
    let mut s = tr!("Delete the theme “{name}”?", name = name);
    let item_part = || trn!("{n} item", "{n} items", items);
    let service_part = || trn!("{n} service", "{n} services", services);
    let users = match (items, services) {
        (0, 0) => None,
        (n, 0) => Some(trn!("{things} uses it; they'll use the church default instead.", "{things} use it; they'll use the church default instead.", n, things = item_part())),
        (0, n) => Some(trn!("{things} uses it; they'll use the church default instead.", "{things} use it; they'll use the church default instead.", n, things = service_part())),
        _ => Some(tr!("{items} and {services} use it; they'll use the church default instead.", items = item_part(), services = service_part())),
    };
    if let Some(users) = users {
        s.push(' ');
        s.push_str(&users);
    }
    if is_default {
        s.push(' ');
        s.push_str(&tr!("It's the church default, so Standard becomes the church default."));
    }
    s
}

pub fn remove_background_question(name: &str) -> String {
    tr!("Remove “{name}” from backgrounds? Items that use it keep it.", name = name)
}

pub fn used_by_screen_views(names: &[String]) -> String {
    tr!("Used by screen view(s): {views}", views = names.join(", "))
}

pub fn converting(percent: u8) -> String {
    tr!("Converting {percent} %", percent = percent)
}

pub fn theme_chip(name: &str) -> String {
    tr!("Theme: {name}", name = name)
}

/// A Formatting chip under the rich text box: "Gold, Large: ‘Pancake breakfast’". Parts go Bold, Italic,
/// Underline, colour, size; `text` is already cut to length.
pub fn range_chip(style: &crate::theme::richtext::RunStyle, text: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (on, name) in [(style.bold, tr!("Bold")), (style.italic, tr!("Italic")), (style.underline, tr!("Underline"))] {
        if on {
            parts.push(name);
        }
    }
    if let Some(c) = &style.color {
        parts.push(match c.to_ascii_lowercase().as_str() {
            "#f5b94a" => tr!("Gold"),
            "#ff4f55" => tr!("Red"),
            _ => tr!("Color"),
        });
    }
    match style.size {
        75 => parts.push(tr!("Small")),
        125 => parts.push(tr!("Large")),
        150 => parts.push(tr!("Huge")),
        _ => {}
    }
    format!("{}: ‘{text}’", parts.join(", "))
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
        assert_eq!(overlay_failed("Logo", &picture_unreadable()), "Couldn't show the overlay “Logo”: the picture could not be loaded.");
        assert_eq!(font_missing("Lato"), "“Lato” isn't installed on this computer, so slides use Noto Sans.");
        assert_eq!(converting(40), "Converting 40 %");
        assert_eq!((numbered_background(1), numbered_background(12)), ("Background 1".to_string(), "Background 12".to_string()));
        assert_eq!((used_by(1), used_by(3)), ("Used by 1 item".to_string(), "Used by 3 items".to_string()));
    }

    #[test]
    fn text_overlay_help_lines_match_spec() {
        assert!(text_overlay_help_feat2().starts_with(&text_overlay_help()));
        assert!(text_overlay_help_feat2().ends_with("use Message (once available)."));
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
        assert_eq!(theme_chip(&church_default()), "Theme: Church default");
    }

    #[test]
    fn range_chip_names_gold_and_large() {
        use crate::theme::richtext::RunStyle;
        let st = RunStyle { color: Some("#f5b94a".into()), size: 125, ..Default::default() };
        assert_eq!(range_chip(&st, "Pancake breakfast"), "Gold, Large: ‘Pancake breakfast’");
        let st = RunStyle { bold: true, italic: true, underline: true, color: Some("#123456".into()), size: 75, ..Default::default() };
        assert_eq!(range_chip(&st, "x"), "Bold, Italic, Underline, Color, Small: ‘x’");
        let st = RunStyle { color: Some("#FF4F55".into()), size: 150, ..Default::default() };
        assert_eq!(range_chip(&st, "y"), "Red, Huge: ‘y’");
    }
}
