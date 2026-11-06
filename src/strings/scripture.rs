//! FEAT-5 user-facing text (spec §3, §5, §8). Everything the scripture core or UI
//! shows a volunteer comes from here; `tr` is a translation code such as "KJV".

// ---------- Fixed messages ----------

pub const EMPTY_SEARCH: &str = "Type at least one word to search for.";
pub const ADD_A_CHAPTER: &str = "Add a chapter, like Job 1 or Job 1:1.";
pub const PICK_DIFFERENT_SECOND: &str = "Pick a different second translation.";
pub const SECOND_TURNED_OFF: &str = "Second translation turned off (same as main).";
pub const TOO_LONG_CHIP: &str = "Too long — will split inside a verse";
pub const JOIN_TOO_LONG_NOTE: &str = "Keeping these together makes a slide too long, so it will split inside a verse.";
pub const UPGRADE_TOAST: &str = "Bible slides are now fitted to the screen, so some passages may break in different places.";
pub const FITTING_CHIP: &str = "Fitting slides…";
pub const UNKNOWN_TRANSLATION: &str = "Unknown translation.";
pub const BLOCKED: &str = "This translation can't be used in RustyPresenter. Please choose another Bible.";
pub const UNPARSEABLE_VERSE_TOOLTIP: &str = "This verse came from an older version; look the passage up again to choose breaks here.";

/// A saved service lost passages to the blocklist (Bible library spec §4).
pub fn passages_removed(n: usize) -> String {
    if n == 1 { "One passage used a translation that can't be used in RustyPresenter and was removed.".into() }
    else { format!("{n} passages used a translation that can't be used in RustyPresenter and were removed.") }
}

// ---------- Settings → Bible ----------

pub const STATUS_OFFLINE: &str = "Ready on this computer";
pub const STATUS_ONLINE: &str = "Ready (needs internet)";
pub const STATUS_NEEDS_KEY: &str = "Needs a key — Get one";
pub const STATUS_UNAVAILABLE: &str = "Not available";
pub const SEARCH_CHIP_READY: &str = "Search: Ready";
pub const SEARCH_CHIP_ESV: &str = "Search: online (ESV)";

/// The search rules for the Help tab (spec §3.1).
pub const HELP_SEARCH_RULES: &[&str] = &[
    "Search works for the translations stored on this computer (KJV and any bundled or imported Bible). Online translations can't be searched, because their owners don't allow it (ESV may offer its own online search).",
    "Every word must appear in the verse (love neighbour finds verses with both).",
    "Words match their beginnings: love finds love, loved, loveth, lovest, loving. Words shorter than 3 letters must match exactly.",
    "Put words in quotes for an exact phrase: \"be still\".",
    "Capitals and punctuation do not matter. lord finds LORD.",
    "Best matches come first: verses with your exact words in order, then verses where the words are close together, then the rest in Bible order. Switch to Bible order to see Genesis first.",
    "Ctrl+Enter in the Bible box always searches, even for a book name like \"Job\".",
];

// ---------- Labels ----------

pub const ALSO_SHOW: &str = "Also show…";
pub const SEARCH_BY_WORDS: &str = "Search by words…";
pub const SLIDE_BREAKS: &str = "Slide breaks";
pub const CHOOSE_SLIDE_BREAKS: &str = "Choose slide breaks…";
pub const RESET_TO_AUTOMATIC: &str = "Reset to automatic";
pub const GAP_AUTO: &str = "New slide (automatic)";
pub const GAP_ADDED: &str = "New slide (you added)";
pub const GAP_KEPT: &str = "Kept together";
pub const GAP_NONE_HOVER: &str = "+ New slide here";
pub const LOWER_THIRD_STACKS: &str = "Lower-third themes always stack two translations.";
pub const SEARCH_THESE_WORDS: &str = "Search for these words";

// ---------- Messages with values ----------

pub fn no_hits(tr: &str) -> String { format!("No verses in {tr} have all of those words.") }

pub fn no_hits_one_word(tr: &str) -> String {
    format!("No verses in {tr} have that word. If it's a book name, check the spelling and add a chapter, like John 3.")
}

pub fn too_many_hits(shown: usize, total: usize) -> String {
    format!("Showing the first {} of {} verses — add more words to narrow it down.", thousands(shown), thousands(total))
}

/// "3 verses found in KJV"; "1 verse found in KJV".
pub fn found(n: usize, tr: &str) -> String {
    format!("{} {} found in {tr}", thousands(n), if n == 1 { "verse" } else { "verses" })
}

pub fn index_building(tr: &str) -> String { format!("Getting {tr} ready to search…") }

pub fn search_unavailable(tr: &str) -> String {
    format!("Search isn't available for {tr}. Try a translation stored on this computer, like KJV.")
}

/// The Bible box translation can't be searched, so the dialog searches `local` instead.
pub fn searching_instead(tr: &str, local: &str) -> String { format!("Search isn't available for {tr}, so this searches {local}.") }

pub fn esv_offline() -> String { "Searching ESV needs the internet. Check the connection, or search in KJV.".into() }

/// Settings search chip, and the tooltip on a greyed translation in the search dialog.
pub fn search_chip_none(tr: &str) -> String { format!("Search isn't available for {tr}") }

pub fn needs_key(tr: &str) -> String { format!("{tr} needs a key. Add one in Settings → Bible.") }

pub fn not_available(tr: &str) -> String { format!("{tr} isn't available.") }

pub fn text_unreachable(tr: &str) -> String { format!("Couldn't get {tr} text — check the internet.") }

pub fn main_only_added(second: &str, main: &str) -> String {
    format!("{second} isn't available right now, so only {main} was added. Turn off the second translation with the X.")
}

pub fn couldnt_get(tr: &str, msg: &str) -> String { format!("Couldn't get {tr}: {msg}") }

/// "NIV doesn't include Acts 8:37."
pub fn missing_verse(tr: &str, reference: &str) -> String { format!("{tr} doesn't include {reference}.") }

/// "Psalm 46:10–11 added — press Enter to show it"
pub fn added_toast(label: &str) -> String { format!("{label} added — press Enter to show it") }

/// "Add Psalm 46:10–11"
pub fn add_button(label: &str) -> String { format!("Add {label}") }

/// `Search for "job" instead`
pub fn search_for_instead(word: &str) -> String { format!("Search for \"{word}\" instead") }

pub fn passage_too_long(limit: usize) -> String {
    format!("That passage is too long. Choose up to about {} verses.", thousands(limit))
}

/// 1240 → "1,240".
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 { out.push(','); }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_many_hits_formats_thousands() {
        assert_eq!(too_many_hits(200, 1240), "Showing the first 200 of 1,240 verses — add more words to narrow it down.");
    }
    #[test]
    fn found_is_singular_for_one() { assert_eq!(found(1, "KJV"), "1 verse found in KJV"); assert_eq!(found(3, "KJV"), "3 verses found in KJV"); }
    #[test]
    fn main_only_message_names_both() {
        assert_eq!(main_only_added("NIV", "KJV"), "NIV isn't available right now, so only KJV was added. Turn off the second translation with the X.");
    }
}

/// An Also Bible was picked as the main one (Nov 5 spec §3).
pub fn also_turned_off(abbr: &str) -> String { format!("{abbr} turned off (same as main).") }

/// Some Also Bibles could not be looked up for a new passage; it was added with the rest.
pub fn also_left_out(left: &str, kept: &str) -> String { format!("{left} isn't available right now, so only {kept} was added.") }

/// A lower-third theme draws two Bibles; the third is named in the passage's note (Nov 5 spec §3).
pub fn lower_third_left_out(abbr: &str) -> String { format!("Lower-third themes show two Bibles. {abbr} is left out.") }

/// An online Also Bible has no text on this computer for the passage, so its column is left out.
pub fn needs_internet(abbr: &str) -> String { format!("{abbr} needs the internet for this passage.") }

/// An Also Bible that is no longer in the library (Bible library spec §13).
pub const BIBLE_REMOVED: &str = "That Bible was removed. Add it again in Settings → Bibles.";
