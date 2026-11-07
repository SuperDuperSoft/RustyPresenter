//! FEAT-5 user-facing text (spec §3, §5, §8). Everything the scripture core or UI
//! shows a volunteer comes from here; `tr` is a translation code such as "KJV".
use crate::{tr, trn};

// ---------- Fixed messages ----------

pub fn empty_search() -> String { tr!("Type at least one word to search for.") }
pub fn add_a_chapter() -> String { tr!("Add a chapter, like Job 1 or Job 1:1.") }
pub fn pick_different_second() -> String { tr!("Pick a different second translation.") }
pub fn second_turned_off() -> String { tr!("Second translation turned off (same as main).") }
pub fn too_long_chip() -> String { tr!("Too long — will split inside a verse") }
pub fn join_too_long_note() -> String { tr!("Keeping these together makes a slide too long, so it will split inside a verse.") }
pub fn upgrade_toast() -> String { tr!("Bible slides are now fitted to the screen, so some passages may break in different places.") }
pub fn fitting_chip() -> String { tr!("Fitting slides…") }
pub fn unknown_translation() -> String { tr!("Unknown translation.") }
pub fn blocked() -> String { tr!("This translation can't be used in RustyPresenter. Please choose another Bible.") }
pub fn unparseable_verse_tooltip() -> String { tr!("This verse came from an older version; look the passage up again to choose breaks here.") }

/// A saved service lost passages to the blocklist (Bible library spec §4).
pub fn passages_removed(n: usize) -> String {
    trn!("One passage used a translation that can't be used in RustyPresenter and was removed.",
        "{n} passages used a translation that can't be used in RustyPresenter and were removed.", n)
}

// ---------- Settings → Bible ----------

pub fn status_offline() -> String { tr!("Ready on this computer") }
pub fn status_online() -> String { tr!("Ready (needs internet)") }
pub fn status_needs_key() -> String { tr!("Needs a key — Get one") }
pub fn status_unavailable() -> String { tr!("Not available") }
pub fn search_chip_ready() -> String { tr!("Search: Ready") }
pub fn search_chip_esv() -> String { tr!("Search: online (ESV)") }

/// The search rules for the Help tab (spec §3.1).
pub fn help_search_rules() -> Vec<String> { vec![
    tr!("Search works for the translations stored on this computer (KJV and any bundled or imported Bible). Online translations can't be searched, because their owners don't allow it (ESV may offer its own online search)."),
    tr!("Every word must appear in the verse (love neighbour finds verses with both)."),
    tr!("Words match their beginnings: love finds love, loved, loveth, lovest, loving. Words shorter than 3 letters must match exactly."),
    tr!("Put words in quotes for an exact phrase: \"be still\"."),
    tr!("Capitals and punctuation do not matter. lord finds LORD."),
    tr!("Best matches come first: verses with your exact words in order, then verses where the words are close together, then the rest in Bible order. Switch to Bible order to see Genesis first."),
    tr!("Ctrl+Enter in the Bible box always searches, even for a book name like \"Job\"."),
] }

// ---------- Labels ----------

pub fn also_show() -> String { tr!("Also show…") }
pub fn search_by_words() -> String { tr!("Search by words…") }
pub fn slide_breaks() -> String { tr!("Slide breaks") }
pub fn choose_slide_breaks() -> String { tr!("Choose slide breaks…") }
pub fn reset_to_automatic() -> String { tr!("Reset to automatic") }
pub fn gap_auto() -> String { tr!("New slide (automatic)") }
pub fn gap_added() -> String { tr!("New slide (you added)") }
pub fn gap_kept() -> String { tr!("Kept together") }
pub fn gap_none_hover() -> String { tr!("+ New slide here") }
pub fn lower_third_stacks() -> String { tr!("Lower-third themes always stack two translations.") }
pub fn search_these_words() -> String { tr!("Search for these words") }

// ---------- Messages with values ----------

pub fn no_hits(tr: &str) -> String { tr!("No verses in {bible} have all of those words.", bible = tr) }

pub fn no_hits_one_word(tr: &str) -> String {
    tr!("No verses in {bible} have that word. If it's a book name, check the spelling and add a chapter, like John 3.", bible = tr)
}

pub fn too_many_hits(shown: usize, total: usize) -> String {
    tr!("Showing the first {shown} of {total} verses — add more words to narrow it down.", shown = thousands(shown), total = thousands(total))
}

/// "3 verses found in KJV"; "1 verse found in KJV".
pub fn found(n: usize, tr: &str) -> String {
    trn!("{count} verse found in {bible}", "{count} verses found in {bible}", n, count = thousands(n), bible = tr)
}

pub fn index_building(tr: &str) -> String { tr!("Getting {bible} ready to search…", bible = tr) }

pub fn search_unavailable(tr: &str) -> String {
    tr!("Search isn't available for {bible}. Try a translation stored on this computer, like KJV.", bible = tr)
}

/// The Bible box translation can't be searched, so the dialog searches `local` instead.
pub fn searching_instead(tr: &str, local: &str) -> String { tr!("Search isn't available for {bible}, so this searches {local}.", bible = tr, local = local) }

pub fn esv_offline() -> String { tr!("Searching ESV needs the internet. Check the connection, or search in KJV.") }

/// Settings search chip, and the tooltip on a greyed translation in the search dialog.
pub fn search_chip_none(tr: &str) -> String { tr!("Search isn't available for {bible}", bible = tr) }

pub fn needs_key(tr: &str) -> String { tr!("{bible} needs a key. Add one in Settings → Bible.", bible = tr) }

pub fn not_available(tr: &str) -> String { tr!("{bible} isn't available.", bible = tr) }

pub fn text_unreachable(tr: &str) -> String { tr!("Couldn't get {bible} text — check the internet.", bible = tr) }

pub fn main_only_added(second: &str, main: &str) -> String {
    tr!("{second} isn't available right now, so only {main} was added. Turn off the second translation with the X.", second = second, main = main)
}

pub fn couldnt_get(tr: &str, msg: &str) -> String { tr!("Couldn't get {bible}: {error}", bible = tr, error = msg) }

/// "NIV doesn't include Acts 8:37."
pub fn missing_verse(tr: &str, reference: &str) -> String { tr!("{bible} doesn't include {passage}.", bible = tr, passage = reference) }

/// "Psalm 46:10–11 added — press Enter to show it"
pub fn added_toast(label: &str) -> String { tr!("{passage} added — press Enter to show it", passage = label) }

/// "Add Psalm 46:10–11"
pub fn add_button(label: &str) -> String { tr!("Add {passage}", passage = label) }

/// `Search for "job" instead`
pub fn search_for_instead(word: &str) -> String { tr!("Search for \"{word}\" instead", word = word) }

pub fn passage_too_long(limit: usize) -> String {
    tr!("That passage is too long. Choose up to about {n} verses.", n = thousands(limit))
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
pub fn also_turned_off(abbr: &str) -> String { tr!("{bible} turned off (same as main).", bible = abbr) }

/// Some Also Bibles could not be looked up for a new passage; it was added with the rest.
pub fn also_left_out(left: &str, kept: &str) -> String { tr!("{left} isn't available right now, so only {kept} was added.", left = left, kept = kept) }

/// A lower-third theme draws two Bibles; the third is named in the passage's note (Nov 5 spec §3).
pub fn lower_third_left_out(abbr: &str) -> String { tr!("Lower-third themes show two Bibles. {bible} is left out.", bible = abbr) }

/// An online Also Bible has no text on this computer for the passage, so its column is left out.
pub fn needs_internet(abbr: &str) -> String { tr!("{bible} needs the internet for this passage.", bible = abbr) }

/// An Also Bible that is no longer in the library (Bible library spec §13).
pub fn bible_removed() -> String { tr!("That Bible was removed. Add it again in Settings → Bibles.") }
