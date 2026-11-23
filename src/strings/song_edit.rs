//! Words for editing a song from its slides (`src/song_edit`, the slide editor and the order on the Slides panel).
use crate::tr;

pub fn gone() -> String { tr!("That slide is not in the song any more. Try again.") }
pub fn same_group(lines: usize) -> String {
    tr!("These two slides are one part shown {lines} lines at a time. Use Lines + to show more lines on each slide.", lines = lines)
}
pub fn last_of_part() -> String { tr!("This is the last slide of its part, so there is nothing to join it with.") }
pub fn no_split() -> String { tr!("Put the cursor on the line that should start the new slide.") }
pub fn only_slide() -> String { tr!("This is the song’s only slide. Delete the song in the library instead.") }

/// The slide editor's heading: "Verse 1 · slide 2 of 3".
pub fn where_slide(label: &str, n: usize, of: usize) -> String {
    if of <= 1 { label.to_string() } else { tr!("{part} · slide {n} of {count}", part = label, n = n, count = of) }
}
/// A part played more than once: the change shows every time.
pub fn plays_times(label: &str, times: usize) -> String {
    tr!("{part} plays {times} times in this song. Your change shows every time.", part = label, times = times)
}
pub fn chords_only_note() -> String { tr!("This slide is chords only. Type the chords in square brackets, like [G] [D].") }
pub fn chords_kept_note() -> String { tr!("The chords stay over the same words. To change them, tick Edit chords too.") }
pub fn new_slide() -> String { tr!("New slide") }
/// Toasts.
pub fn saved() -> String { tr!("Slide saved") }
pub fn split_done() -> String { tr!("Split into two slides") }
pub fn joined() -> String { tr!("Joined with the next slide") }
pub fn added() -> String { tr!("Slide added") }
pub fn deleted() -> String { tr!("Slide deleted") }
pub fn duplicated() -> String { tr!("Slide copied") }
/// Copy, Cut and Paste of one slide (the menu, or the keys with the mouse over a slide).
pub fn copied(paste_key: &str) -> String { tr!("Slide copied. Point at a slide and press {key} to paste it after that one.", key = paste_key) }
pub fn cut(paste_key: &str) -> String { tr!("Slide cut. Point at a slide and press {key} to paste it after that one.", key = paste_key) }
pub fn pasted() -> String { tr!("Slide pasted") }
pub fn nothing_to_paste() -> String { tr!("There is no slide or text to paste.") }
pub fn section_changed(name: &str) -> String { tr!("This part is now {part}", part = name) }
pub fn order_changed() -> String { tr!("Order changed for this service") }
pub fn order_usual() -> String { tr!("This is now the song’s usual order") }
pub fn order_back() -> String { tr!("Back to the song’s usual order") }
pub fn no_chorus() -> String { tr!("This song has no chorus.") }
pub fn delete_repeated(label: &str, times: usize) -> String {
    tr!("{part} plays {times} times in this song. Delete this slide every time it plays?", part = label, times = times)
}
pub fn delete_yes() -> String { tr!("Delete slide") }
pub fn not_a_song() -> String { tr!("Only song slides can be edited here. Use Edit for this item.") }
