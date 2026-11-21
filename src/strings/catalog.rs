//! What Settings → Bibles says about catalogs (Bible library spec §3.1, §7.1, §13).
use crate::tr;

pub fn catalog_offline() -> String { tr!("You're offline. Bibles you've added still work.") }
pub fn catalog_bad() -> String { tr!("The list of Bibles couldn't be read. Try again later.") }
pub fn catalog_disk() -> String { tr!("The list of Bibles couldn't be saved on this computer.") }
pub fn licence_on_download() -> String { tr!("Licence checked on download") }
pub fn scope_full() -> String { tr!("Full Bible") }
pub fn scope_nt() -> String { tr!("New Testament") }
pub fn scope_ot() -> String { tr!("Old Testament") }
pub fn scope_portions() -> String { tr!("Portions") }
pub fn includes_apocrypha() -> String { tr!("Includes Apocrypha") }

// Downloads (§13). `name` is the Bible's title.
pub fn downloading_name(name: &str) -> String { tr!("Downloading {name}…", name = name) }
pub fn waiting(name: &str) -> String { tr!("Waiting to download {name}…", name = name) }
pub fn checking(name: &str) -> String { tr!("Checking {name}…", name = name) }
pub fn download_failed(name: &str) -> String { tr!("Couldn't download {name}. Check the internet and try again.", name = name) }
pub fn licence_refused() -> String { tr!("This Bible's licence doesn't allow us to keep it. It has been removed.") }
pub fn update_unusable(name: &str) -> String {
    tr!("The update for {name} couldn't be used. The copy you have still works.", name = name)
}
pub fn too_large() -> String { tr!("This Bible is larger than expected, so it wasn't added.") }
/// `mb`: megabytes it needs.
pub fn no_space(mb: u64) -> String { tr!("Not enough disk space for this Bible (needs about {n} MB).", n = mb) }
pub fn unusable(name: &str) -> String { tr!("{name} couldn't be used, so it wasn't added.", name = name) }
pub fn not_in_list() -> String { tr!("That Bible is no longer in the list. Open Settings → Bible again to see the current list.") }
pub fn offline_no_add() -> String { tr!("You're offline, so Bibles can't be added right now.") }

// Settings → Bible, the library list (§7.1).
pub fn list_loading() -> String { tr!("Loading the list of Bibles…") }
pub fn all_languages() -> String { tr!("All languages") }
pub fn no_match() -> String { tr!("No Bibles match. Try fewer words, or All.") }
pub fn none_installed() -> String { tr!("No Bibles added yet. Choose All to see the Bibles you can add.") }
pub fn tag_offline() -> String { tr!("Offline") }
pub fn tag_online() -> String { tr!("Online") }
pub fn add() -> String { tr!("Add") }
pub fn update() -> String { tr!("Update") }
pub fn remove() -> String { tr!("Remove") }
pub fn cancel() -> String { tr!("Cancel") }
pub fn added_tag() -> String { tr!("Added") }
/// Under a row whose title or language name has a character no font on this computer can draw (spec §9.1).
pub fn no_font() -> String { tr!("Your computer may not have a font for this language.") }
pub fn waiting_short() -> String { tr!("Waiting") }
pub fn update_ready() -> String { tr!("A newer edition is ready") }
pub fn default_bible() -> String { tr!("Default Bible") }
pub fn default_set() -> String { tr!("Default Bible changed") }
pub fn also_full() -> String { tr!("You can show up to two more Bibles. Turn one off first.") }
pub fn credits() -> String { tr!("Credits") }
/// The line under a downloaded Bible in Settings → Bible.
pub fn from_ebible(name: &str) -> String { tr!("{name} — downloaded from eBible.org", name = name) }
/// A Bible in the Default Bible list: its abbreviation, then its title when that says more.
pub fn bible_option(abbr: &str, name: &str) -> String {
    if name.is_empty() || name == abbr { abbr.to_string() } else { format!("{abbr} — {name}") }
}
/// One credit: the Bible's own notice (or its name), then the licence's short name when it has one.
pub fn credit(abbr: &str, copyright: &str, licence: &str) -> String {
    let notice = if copyright.is_empty() { abbr.to_string() } else if copyright.contains(abbr) { copyright.to_string() } else { format!("{abbr}: {copyright}") };
    if licence.is_empty() || notice.contains(licence) { notice } else { format!("{notice} ({licence})") }
}
/// The list is cut at `shown` rows so the page stays quick; search narrows it.
pub fn showing(shown: usize, of: usize) -> String { tr!("Showing {shown} of {total} Bibles. Search to find others.", shown = shown, total = of) }
pub fn downloading(percent: u8) -> String { tr!("Downloading… {percent}%", percent = percent) }
pub fn added(abbr: &str) -> String { tr!("Added {bible}", bible = abbr) }
pub fn removed(abbr: &str) -> String { tr!("Removed {bible}", bible = abbr) }
/// A language in the filter: its English name, then its own name when that differs.
pub fn language(name_en: &str, name: &str, code: &str) -> String {
    let en = if name_en.is_empty() { code } else { name_en };
    if name.is_empty() || name == en { en.to_string() } else { format!("{en} · {name}") }
}
/// A source's name in a row's tag.
pub const EBIBLE: &str = "eBible.org";
pub const YOUVERSION: &str = "YouVersion";
pub fn from_youversion(name: &str) -> String { tr!("{name} — online, from YouVersion", name = name) }
/// The row's tag when other libraries offer the same Bible; empty when none do.
pub fn also_on(sources: &[&str]) -> String { if sources.is_empty() { String::new() } else { tr!("Also on {sources}", sources = sources.join(" · ")) } }
