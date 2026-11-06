//! What Settings → Bibles says about catalogs (Bible library spec §3.1, §7.1, §13).

pub const CATALOG_OFFLINE: &str = "You're offline. Bibles you've added still work.";
pub const CATALOG_BAD: &str = "The list of Bibles couldn't be read. Try again later.";
pub const CATALOG_DISK: &str = "The list of Bibles couldn't be saved on this computer.";
pub const LICENCE_ON_DOWNLOAD: &str = "Licence checked on download";
pub const SCOPE_FULL: &str = "Full Bible";
pub const SCOPE_NT: &str = "New Testament";
pub const SCOPE_OT: &str = "Old Testament";
pub const SCOPE_PORTIONS: &str = "Portions";
pub const INCLUDES_APOCRYPHA: &str = "Includes Apocrypha";

// Downloads (§13). `{name}` is the Bible's title, `{n}` megabytes.
pub const DOWNLOADING: &str = "Downloading {name}…";
pub const WAITING: &str = "Waiting to download {name}…";
pub const CHECKING: &str = "Checking {name}…";
pub const DOWNLOAD_FAILED: &str = "Couldn't download {name}. Check the internet and try again.";
pub const LICENCE_REFUSED: &str = "This Bible's licence doesn't allow us to keep it. It has been removed.";
pub const UPDATE_UNUSABLE: &str = "The update for {name} couldn't be used. The copy you have still works.";
pub const TOO_LARGE: &str = "This Bible is larger than expected, so it wasn't added.";
pub const NO_SPACE: &str = "Not enough disk space for this Bible (needs about {n} MB).";
pub const UNUSABLE: &str = "{name} couldn't be used, so it wasn't added.";
pub const NOT_IN_LIST: &str = "That Bible is no longer in the list. Open Settings → Bibles again to see the current list.";
pub const OFFLINE_NO_ADD: &str = "You're offline, so Bibles can't be added right now.";

// Settings → Bible, the library list (§7.1).
pub const LIST_LOADING: &str = "Loading the list of Bibles…";
pub const ALL_LANGUAGES: &str = "All languages";
pub const NO_MATCH: &str = "No Bibles match. Try fewer words, or All.";
pub const NONE_INSTALLED: &str = "No Bibles added yet. Choose All to see the Bibles you can add.";
pub const TAG_OFFLINE: &str = "Offline";
pub const ADD: &str = "Add";
pub const UPDATE: &str = "Update";
pub const REMOVE: &str = "Remove";
pub const CANCEL: &str = "Cancel";
pub const ADDED: &str = "Added";
pub const WAITING_SHORT: &str = "Waiting";
pub const UPDATE_READY: &str = "A newer edition is ready";
pub const DEFAULT_BIBLE: &str = "Default Bible";
pub const DEFAULT_SET: &str = "Default Bible changed";
pub const CREDITS: &str = "Credits";
/// The line under a downloaded Bible in Settings → Bible.
pub fn from_ebible(name: &str) -> String { format!("{name} — downloaded from eBible.org") }
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
pub fn showing(shown: usize, of: usize) -> String { format!("Showing {shown} of {of} Bibles. Search to find others.") }
pub fn downloading(percent: u8) -> String { format!("Downloading… {percent}%") }
pub fn added(abbr: &str) -> String { format!("Added {abbr}") }
pub fn removed(abbr: &str) -> String { format!("Removed {abbr}") }
/// A language in the filter: its English name, then its own name when that differs.
pub fn language(name_en: &str, name: &str, code: &str) -> String {
    let en = if name_en.is_empty() { code } else { name_en };
    if name.is_empty() || name == en { en.to_string() } else { format!("{en} · {name}") }
}
