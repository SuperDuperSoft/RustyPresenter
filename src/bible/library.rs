//! What the app knows about an installed Bible without reading its verses (Bible library spec §5.2).
//! `bibles/index.json` is only a cache: the truth is each Bible's own file, so a lost or damaged index is rebuilt
//! from the folder (`import::list`).
use super::blocklist;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io, path::Path};

/// Reading direction of a Bible's text.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Dir { Rtl, #[default] #[serde(other)] Ltr }

/// How a Bible numbers its verses (§5.4). What a newer build knows and this one doesn't reads as English.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Vsys { Original, Septuagint, Vulgate, #[default] #[serde(other)] English }

/// The terms a Bible came under. `Unknown` is an old import: the church supplied the file, so it never said.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Licence {
    Pd,
    Cc { kind: String, version: String, url: String },
    Net,
    Provider,
    #[default]
    #[serde(other)]
    Unknown,
}

impl Licence {
    /// The licence's short name for credits ("CC BY-SA 4.0", "Public domain"); empty when the Bible's own notice
    /// is all there is (an import, an online provider).
    pub fn short(&self) -> String {
        match self {
            Licence::Pd => "Public domain".into(),
            Licence::Cc { kind, version, .. } => {
                let k = kind.to_uppercase();
                if version.is_empty() { format!("CC {k}") } else { format!("CC {k} {version}") }
            }
            Licence::Net => "NET Bible licence".into(),
            Licence::Provider | Licence::Unknown => String::new(),
        }
    }

    /// Where the full licence can be read; empty when there is no page for it. NET has none here yet: its page
    /// address was not checked, and the notice in the Bible's own file says where to read it.
    pub fn link(&self) -> &str {
        match self {
            Licence::Cc { url, .. } => url,
            _ => "",
        }
    }
}

/// Where a Bible came from.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    Builtin,
    Ebible { update_date: String },
    YouVersion,
    ApiBible,
    #[default]
    #[serde(other)]
    Import,
}

/// One Bible. Every field has a default, so an older or newer file still reads.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct BibleMeta {
    /// Translation id (`bible::id`).
    pub id: String,
    /// Shown on chips, in any script ("RV1909", "ΚΔ").
    pub abbr: String,
    /// Local title.
    pub name: String,
    /// English title, may equal `name`.
    pub name_en: String,
    /// BCP 47 ("es", "hbo", "grc").
    pub lang: String,
    /// In the language itself ("Español").
    pub lang_name: String,
    /// "Spanish".
    pub lang_name_en: String,
    pub dir: Dir,
    /// ISO 15924 if known ("Hebr", "Grek", "Latn").
    pub script: String,
    pub versification: Vsys,
    /// Book indices present.
    pub books: Vec<u8>,
    /// Index, long name, short name, in the Bible's own language.
    pub book_names: Vec<(u8, String, String)>,
    /// Credit line exactly as given.
    pub copyright: String,
    pub licence: Licence,
    pub source: Source,
    /// Size of the Bible's file. The index trusts an entry only while this still matches.
    pub bytes: u64,
}

impl Default for BibleMeta {
    fn default() -> Self {
        BibleMeta {
            id: String::new(), abbr: String::new(), name: String::new(), name_en: String::new(), lang: "und".into(),
            lang_name: String::new(), lang_name_en: String::new(), dir: Dir::Ltr, script: String::new(),
            versification: Vsys::English, books: Vec::new(), book_names: Vec::new(), copyright: String::new(),
            licence: Licence::Unknown, source: Source::Import, bytes: 0,
        }
    }
}

impl BibleMeta {
    /// An import from before metadata existed, or a new one: only its id, title and notice are known.
    pub fn of_import(id: &str, name: &str, copyright: &str) -> Self {
        BibleMeta { id: id.into(), abbr: id.into(), name: name.into(), name_en: name.into(), copyright: copyright.into(), ..Default::default() }
    }

    /// True when what this says about itself names a blocked Bible. The Bible's own language code is what keeps
    /// eBible's Tlachichilco Tepehua (`tpt`) apart from The Passion Translation.
    pub fn blocked(&self) -> bool {
        blocklist::blocked_row(&self.lang, &self.abbr, &[&self.name, &self.name_en], &[&self.copyright], &[&self.id])
    }
}

/// Bumped when an older index can't be read as it stands; it is then rebuilt from the files.
// 2: every Bible's numbering is read again from its verses (0.5.4), so one added earlier needs nothing done to it.
const INDEX_VERSION: u32 = 2;

#[derive(Serialize)]
struct IndexRef<'a> { v: u32, bibles: &'a [BibleMeta] }

#[derive(Deserialize)]
struct Index { v: u32, bibles: Vec<BibleMeta> }

/// The saved index by id. Missing, damaged or from another version is empty, and the caller rebuilds it.
pub fn read_index(root: &Path) -> BTreeMap<String, BibleMeta> {
    fs::read(root.join("bibles/index.json")).ok()
        .and_then(|b| serde_json::from_slice::<Index>(&b).ok())
        .filter(|i| i.v == INDEX_VERSION)
        .map(|i| i.bibles.into_iter().map(|m| (m.id.clone(), m)).collect())
        .unwrap_or_default()
}

pub fn write_index(root: &Path, bibles: &[BibleMeta]) -> io::Result<()> {
    let bytes = serde_json::to_vec(&IndexRef { v: INDEX_VERSION, bibles }).map_err(io::Error::other)?;
    crate::store::write_atomic(&root.join("bibles/index.json"), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_from_a_newer_build_still_reads() {
        let m: BibleMeta = serde_json::from_str(r#"{"id":"x:y","lang":"es","dir":"up","versification":"masoretic2","licence":{"type":"future"},"source":{"type":"door43"},"new":1}"#).unwrap();
        assert_eq!((m.dir, m.versification, m.licence, m.source), (Dir::Ltr, Vsys::English, Licence::Unknown, Source::Import));
        assert_eq!(m.lang, "es");
    }

    #[test]
    fn an_empty_object_is_an_old_import() {
        let m: BibleMeta = serde_json::from_str("{}").unwrap();
        assert_eq!(m, BibleMeta::default());
        assert_eq!((m.lang.as_str(), m.dir, m.versification, m.source), ("und", Dir::Ltr, Vsys::English, Source::Import));
    }

    #[test]
    fn licences_and_sources_keep_their_details() {
        let m = BibleMeta {
            licence: Licence::Cc { kind: "by-sa".into(), version: "4.0".into(), url: "https://creativecommons.org/licenses/by-sa/4.0/".into() },
            source: Source::Ebible { update_date: "2026-03-01".into() }, dir: Dir::Rtl, versification: Vsys::Septuagint, ..Default::default()
        };
        assert_eq!(serde_json::from_str::<BibleMeta>(&serde_json::to_string(&m).unwrap()).unwrap(), m);
    }

    #[test]
    fn licences_have_a_short_name_and_a_link_for_credits() {
        let cc = Licence::Cc { kind: "by-nc-nd".into(), version: "4.0".into(), url: "https://creativecommons.org/licenses/by-nc-nd/4.0/".into() };
        assert_eq!((cc.short().as_str(), cc.link()), ("CC BY-NC-ND 4.0", "https://creativecommons.org/licenses/by-nc-nd/4.0/"));
        assert_eq!((Licence::Pd.short().as_str(), Licence::Pd.link()), ("Public domain", ""));
        assert_eq!((Licence::Net.short().as_str(), Licence::Net.link()), ("NET Bible licence", ""));
        assert_eq!((Licence::Unknown.short().as_str(), Licence::Provider.short().as_str()), ("", ""));
    }

    #[test]
    fn a_tepehua_row_is_allowed_and_the_passion_translation_is_not() {
        let m = |lang: &str, name: &str| BibleMeta { id: "ebible:tptNT".into(), abbr: "TPT".into(), name: name.into(), lang: lang.into(), ..Default::default() };
        assert!(!m("tpt", "Tlachichilco Tepehua New Testament").blocked());
        assert!(m("en", "Some Bible").blocked());
        assert!(m("tpt", "The Passion Translation").blocked());
        assert!(BibleMeta::of_import("TPT", "", "").blocked(), "an import doesn't know its language");
    }

    #[test]
    fn a_damaged_index_or_one_from_another_version_is_empty() {
        let root = crate::store::test_dir();
        fs::create_dir_all(root.join("bibles")).unwrap();
        assert!(read_index(&root).is_empty());
        write_index(&root, &[BibleMeta::of_import("WEB", "World English Bible", "")]).unwrap();
        assert_eq!(read_index(&root)["WEB"].name, "World English Bible");
        fs::write(root.join("bibles/index.json"), b"{ nope").unwrap();
        assert!(read_index(&root).is_empty());
        fs::write(root.join("bibles/index.json"), br#"{"v":99,"bibles":[{"id":"WEB"}]}"#).unwrap();
        assert!(read_index(&root).is_empty());
    }
}
