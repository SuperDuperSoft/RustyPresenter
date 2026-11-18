//! The Bibles shown with an item's main one (Nov 5 spec §2): at most `MAX_ALSO`, never the main one, never twice,
//! never a blocked translation. One rule set for items and for the church default.
use super::AlsoShow;
use crate::bible::id::same_id;

/// At most this many Also Bibles: three Bibles in all.
pub const MAX_ALSO: usize = 2;

/// Tidies an item's Also Bibles in place, keeping their order: drops the main Bible, repeats of an id and blocked
/// ids, then keeps the first `MAX_ALSO`. Returns the ids dropped for being blocked, so the caller can say so.
pub fn tidy(main: &str, also: &mut Vec<AlsoShow>, blocked: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut kept: Vec<AlsoShow> = Vec::new();
    for a in also.drain(..) {
        if blocked(&a.translation) { out.push(a.translation); continue }
        if same_id(&a.translation, main) || kept.iter().any(|k| same_id(&k.translation, &a.translation)) { continue }
        kept.push(a);
    }
    kept.truncate(MAX_ALSO);
    *also = kept;
    out
}

/// The same rules for a list of ids (`Settings.also_show`).
pub fn tidy_ids(main: &str, ids: &mut Vec<String>, blocked: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut also: Vec<AlsoShow> = ids.drain(..).map(|translation| AlsoShow { translation, verses: vec![] }).collect();
    let out = tidy(main, &mut also, blocked);
    *ids = also.into_iter().map(|a| a.translation).collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripture::test_support::{also, also_ids};

    #[test]
    fn tidy_drops_main_repeats_and_blocked_then_trims() {
        let mut a = also(&["KJV", "NIV", "NIV", "TPT", "RVR", "ESV"]);
        let blocked = tidy("KJV", &mut a, &|id| id == "TPT");
        assert_eq!(also_ids(&a), ["NIV", "RVR"]);
        assert_eq!(blocked, ["TPT"]);
    }

    #[test]
    fn same_abbr_different_ids_both_kept() {
        let mut a = also(&["ebible:spaRV1909", "imp:RV1909"]);
        tidy("KJV", &mut a, &|_| false);
        assert_eq!(also_ids(&a), ["ebible:spaRV1909", "imp:RV1909"]);
    }

    #[test]
    fn tidy_ids_same_rules_for_settings() {
        let mut v = vec!["KJV".to_string(), "NIV".into(), "WEB".into(), "BSB".into()];
        tidy_ids("KJV", &mut v, &|_| false);
        assert_eq!(v, ["NIV", "WEB"]);
    }

    #[test]
    fn tidy_keeps_stored_verses() {
        let mut a = vec![AlsoShow { translation: "WEB".into(), verses: vec![crate::model::VerseText::new("John 3:16", "For God")] }];
        tidy("KJV", &mut a, &|_| false);
        assert_eq!(a[0].verses.len(), 1);
    }
}
