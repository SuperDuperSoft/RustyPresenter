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
