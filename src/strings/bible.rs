//! User-facing strings of the Bible core (lookups, the online catalogs, the blocklist; Bible library design §13).
//! Declared from `src/bible.rs` with a `#[path]` line until a shared `src/strings/mod.rs` exists.

/// `bible.blocked` (design §4).
pub const BLOCKED: &str = "This translation can't be used in RustyPresenter. Please choose another Bible.";
pub const NO_CREDIT: &str = "This Bible has no copyright line, so it can't be shown.";
pub const UNKNOWN_BIBLE: &str = "That Bible isn't in the list any more. Press Refresh list and try again.";
pub const REMOVED: &str = "That Bible was removed. Add it again in Settings → Bible.";
pub fn needs_key(abbr: &str, provider: &str) -> String {
    format!("{abbr} needs the internet and a free key from {provider}. Add the key in Settings → Bible, or choose another Bible.")
}

// ---- online catalogs (Settings → Bible) ----
pub const TAG_YOUVERSION: &str = "Needs internet · YouVersion";
pub const TAG_API_BIBLE: &str = "Needs internet · API.Bible";
pub const YOUVERSION_BUSY: &str = "YouVersion is busy. Try again in a minute.";
pub const API_BIBLE_BUSY: &str = "API.Bible is busy. Try again in a minute.";
pub fn offline(provider: &str) -> String { format!("Couldn't reach {provider}. You're offline? Bibles you've added still work.") }
pub fn key_refused(provider: &str) -> String { format!("{provider} refused the key. Check it below.") }
pub fn list_failed(provider: &str) -> String { format!("The {provider} list couldn't be read. Showing the last saved list.") }
/// The credits page lines each provider's terms ask for.
pub const CREDIT_YOUVERSION: &str = "Bible text from the YouVersion Platform (YouVersion.com).";
pub const CREDIT_API_BIBLE: &str = "Scripture text from API.Bible, https://api.bible";
