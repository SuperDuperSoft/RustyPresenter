//! Online Bible text (Nov 5 spec §5). Publishers cap how much text an app may keep, so the text of a Bible read over
//! the internet lives only in memory and in the capped `bible-cache/`: never in `data.json`, backups or service files.
//! The item keeps its reference and Bible; the text comes back from the cache when the service opens.
use crate::{bible, model::{Data, ItemKind, ServiceItem}};
use std::{borrow::Cow, path::Path};

/// True for a Bible whose text is kept on this computer: built in, imported or downloaded.
pub fn stored(root: &Path, translation: &str) -> bool {
    // The built-in Bibles by name, so nothing is parsed here.
    ["KJV", "WEB", "BSB"].iter().any(|b| bible::id::same_id(b, translation)) || bible::import::list(root).iter().any(|m| bible::id::same_id(&m.id, translation))
}

/// True for a Bible whose text may not be saved with a service.
pub fn online_only(root: &Path, translation: &str) -> bool { bible::sources::is_online(translation) && !stored(root, translation) }

/// Each Bible of a passage that has text it may not keep, as (Bible, has text) pairs.
fn bibles(item: &ServiceItem) -> Vec<(&str, bool)> {
    let ItemKind::Scripture { translation, verses, also, .. } = &item.kind else { return vec![] };
    std::iter::once((translation.as_str(), !verses.is_empty())).chain(also.iter().map(|a| (a.translation.as_str(), !a.verses.is_empty()))).collect()
}

/// Clears the online text of one item.
pub fn strip_item(item: &mut ServiceItem, root: &Path) {
    if let ItemKind::Scripture { translation, verses, also, .. } = &mut item.kind {
        if online_only(root, translation) { verses.clear() }
        for a in also.iter_mut().filter(|a| online_only(root, &a.translation)) { a.verses.clear() }
    }
}

/// `data` as it may be saved: the same when it holds no online text, else a copy without it.
pub fn without_online_text<'a>(data: &'a Data, root: &Path) -> Cow<'a, Data> {
    let has = |i: &ServiceItem| bibles(i).into_iter().any(|(t, text)| text && online_only(root, t));
    if !data.services.iter().flat_map(|s| &s.items).any(has) { return Cow::Borrowed(data) }
    let mut copy = data.clone();
    for item in copy.services.iter_mut().flat_map(|s| s.items.iter_mut()) { strip_item(item, root) }
    Cow::Owned(copy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::VerseText, scripture::AlsoShow};

    fn passage(main: &str, also: &[&str]) -> ServiceItem {
        let v = |t: &str| vec![VerseText::new("John 3:16", format!("{t} words"))];
        let also = also.iter().map(|t| AlsoShow { translation: t.to_string(), verses: v(t) }).collect();
        ServiceItem { id: uuid::Uuid::new_v4(), kind: ItemKind::Scripture { reference: "John 3:16".into(), translation: main.into(), verses: v(main), per_slide: 0, text_size: 0, also, layout: Default::default() }, background: None, notes: String::new() }
    }

    #[test]
    fn only_online_text_is_left_out() {
        let root = crate::store::test_dir();
        let mut data = Data::default();
        data.services[0].items = vec![passage("KJV", &["NIV", "WEB"]), passage("ESV", &["yv:111"])];
        let saved = without_online_text(&data, &root);
        let json = serde_json::to_string(&*saved).unwrap();
        assert!(json.contains("KJV words") && json.contains("WEB words"), "stored Bibles keep their text");
        for gone in ["NIV words", "ESV words", "yv:111 words"] { assert!(!json.contains(gone), "{gone}") }
        assert!(json.contains(r#""translation":"NIV""#), "the Bible itself stays");
        assert!(serde_json::to_string(&data).unwrap().contains("NIV words"), "what is in memory is untouched");
    }

    #[test]
    fn nothing_online_is_not_copied() {
        let mut data = Data::default();
        data.services[0].items = vec![passage("KJV", &["WEB"])];
        assert!(matches!(without_online_text(&data, &crate::store::test_dir()), Cow::Borrowed(_)));
    }

    #[test]
    fn stored_bibles() {
        let root = crate::store::test_dir();
        assert!(stored(&root, "KJV") && stored(&root, "web"));
        assert!(!stored(&root, "NIV") && online_only(&root, "NIV") && online_only(&root, "yv:111"));
        assert!(!online_only(&root, "ebible:spaRV1909"), "a downloaded Bible is never online");
    }
}
