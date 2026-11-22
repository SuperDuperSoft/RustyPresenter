//! The online Bibles a church added from a catalog (YouVersion's): `bibles/online.json`. Only what names and credits
//! the Bible is kept. Its text is never stored here; it is read over the internet as each passage is looked up
//! (`sources::lookup`) and lives only in the capped cache (`scripture::online`).
use super::blocklist;
use serde::{Deserialize, Serialize};
use std::{io, path::Path, sync::Mutex};

const FILE: &str = "bibles/online.json";
/// One writer at a time, so two adds never lose each other.
static WRITING: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct OnlineBible {
    /// Translation id (`yv:1588`).
    pub id: String,
    pub abbr: String,
    pub name: String,
    pub lang: String,
    pub lang_name: String,
    pub lang_name_en: String,
    /// The publisher's notice, as YouVersion gives it.
    pub copyright: String,
}

impl OnlineBible {
    fn blocked(&self) -> bool { blocklist::blocked_online(&self.abbr, &[&self.name], &[&self.copyright], &[&self.id]) }
}

/// `bibles/online.json` as last read, by data folder (`kept`).
static LISTS: super::kept::Kept<Vec<OnlineBible>> = super::kept::Kept::new();

/// The Bibles added, in the order they were added. A blocked one is never listed, whatever the file says. Asked
/// for many times per look at the Bible lists, so the file is read once and again only when it changed.
pub fn list(root: &Path) -> Vec<OnlineBible> {
    LISTS.get(&root.join(FILE), |bytes| {
        let all: Vec<OnlineBible> = bytes.and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        all.into_iter().filter(|b| super::id::is_prefixed(&b.id) && !b.blocked()).collect()
    })
}

pub fn get(root: &Path, id: &str) -> Option<OnlineBible> { list(root).into_iter().find(|b| b.id == id) }

fn save(root: &Path, all: &[OnlineBible]) -> io::Result<()> {
    std::fs::create_dir_all(root.join("bibles"))?;
    let saved = crate::store::write_atomic(&root.join(FILE), &serde_json::to_vec_pretty(all).map_err(io::Error::other)?);
    LISTS.forget(&root.join(FILE));
    saved
}

/// Adds a Bible, or replaces what is known about one already added.
pub fn add(root: &Path, b: OnlineBible) -> io::Result<()> {
    if !super::id::is_prefixed(&b.id) || b.blocked() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "this Bible can't be added")); }
    let _g = WRITING.lock().unwrap();
    let mut all = list(root);
    match all.iter_mut().find(|x| x.id == b.id) {
        Some(x) => *x = b,
        None => all.push(b),
    }
    save(root, &all)
}

/// Takes a Bible off the list. True when it was there.
pub fn remove(root: &Path, id: &str) -> io::Result<bool> {
    let _g = WRITING.lock().unwrap();
    let mut all = list(root);
    let n = all.len();
    all.retain(|b| b.id != id);
    if all.len() == n { return Ok(false) }
    save(root, &all).map(|()| true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bible(id: &str, abbr: &str, name: &str) -> OnlineBible { OnlineBible { id: id.into(), abbr: abbr.into(), name: name.into(), ..Default::default() } }

    #[test]
    fn bibles_are_added_replaced_and_removed() {
        let root = std::env::temp_dir().join(format!("rp-test-{}", uuid::Uuid::new_v4()));
        assert!(list(&root).is_empty());
        add(&root, bible("yv:1588", "AMP", "Amplified Bible")).unwrap();
        add(&root, bible("yv:89", "LBLA", "La Biblia de las Américas")).unwrap();
        add(&root, bible("yv:1588", "AMP", "Amplified Bible (2015)")).unwrap();
        assert_eq!(list(&root).iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["Amplified Bible (2015)", "La Biblia de las Américas"]);
        assert_eq!(get(&root, "yv:89").map(|b| b.abbr), Some("LBLA".into()));
        assert!(remove(&root, "yv:1588").unwrap() && !remove(&root, "yv:1588").unwrap());
        assert_eq!(list(&root).len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_blocked_bible_is_neither_added_nor_listed() {
        let root = std::env::temp_dir().join(format!("rp-test-{}", uuid::Uuid::new_v4()));
        assert!(add(&root, bible("yv:1849", "X", "A Bible")).is_err());
        assert!(add(&root, bible("yv:5", "TPT", "The Passion Translation")).is_err());
        assert!(add(&root, bible("not an id", "X", "A Bible")).is_err());
        // Written by hand, or by an older build: still not listed.
        std::fs::create_dir_all(root.join("bibles")).unwrap();
        std::fs::write(root.join(FILE), serde_json::to_vec(&[bible("yv:1849", "TPT", "The Passion Translation"), bible("yv:12", "ASV", "American Standard Version")]).unwrap()).unwrap();
        assert_eq!(list(&root).iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["yv:12"]);
        let _ = std::fs::remove_dir_all(root);
    }
}
