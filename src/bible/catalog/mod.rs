//! Catalogs of Bibles the church can add (Bible library spec §3, three-Bibles spec §4). Each source is one enum
//! variant and one module, as `sources::Provider` is, because the project has no `async-trait`.
//!
//! A `CatalogRow` can only be made by `CatalogRow::checked`, which runs the blocklist and the licence rule, and its
//! fields are private, so no source can list a blocked Bible by forgetting a check. Downloads and imports still run
//! the content check of `blocklist` on the text itself.
pub mod ebible;
pub mod install;
pub mod licence;
#[path = "../../strings/catalog.rs"]
pub mod strings;

use std::path::PathBuf;

use super::{blocklist, library::{BibleMeta, Dir, Licence}};

/// Where a catalog row comes from. YouVersion, API.Bible, Door43 and CrossWire SWORD join here as they are built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CatalogSource {
    Ebible,
    /// Online stand-ins for the sources still to come, so `merge` can be tested now.
    #[cfg(test)]
    Test(u8),
}

impl CatalogSource {
    /// The id prefix of its Bibles (`ebible:spaRV1909`).
    pub fn id(self) -> &'static str {
        match self {
            CatalogSource::Ebible => "ebible",
            #[cfg(test)]
            CatalogSource::Test(_) => "test",
        }
    }

    /// Its name in a row's tag ("Also on eBible.org").
    pub fn label(self) -> &'static str {
        match self {
            CatalogSource::Ebible => strings::EBIBLE,
            #[cfg(test)]
            CatalogSource::Test(_) => "Test",
        }
    }

    /// True when its Bibles are downloaded and work without internet.
    pub fn offline(self) -> bool {
        match self {
            CatalogSource::Ebible => true,
            #[cfg(test)]
            CatalogSource::Test(_) => false,
        }
    }

    /// The current list. Falls back to the last saved list when the source can't be reached.
    pub async fn refresh(self, ctx: &Ctx) -> Result<Listing, CatalogError> {
        match self {
            CatalogSource::Ebible => ebible::refresh(ctx).await,
            #[cfg(test)]
            CatalogSource::Test(_) => Ok(Listing::default()),
        }
    }

    /// Downloads one of its rows and adds it to the installed Bibles. `progress` gets 0 to 100.
    pub async fn install(self, ctx: &Ctx, row: &CatalogRow, cancel: &tokio_util::sync::CancellationToken,
        progress: &(dyn Fn(u8) + Send + Sync)) -> Result<BibleMeta, install::InstallError> {
        match self {
            CatalogSource::Ebible => install::ebible(ctx, row, cancel, progress).await,
            #[cfg(test)]
            CatalogSource::Test(_) => Err(install::InstallError::Unusable("a test source installs nothing".into())),
        }
    }
}

/// What a refresh needs to know. `base` is the source's address, so tests can point it at a local server.
#[derive(Clone, Debug)]
pub struct Ctx {
    /// The app's data folder; catalogs live in `bibles/catalog/`.
    pub root: PathBuf,
    /// Seconds since 1970, so tests can move the clock.
    pub now: i64,
    /// Ask the source even if the saved list is less than a day old.
    pub force: bool,
    pub base: String,
}

impl Ctx {
    pub fn new(root: PathBuf, source: CatalogSource) -> Self {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let base = match source {
            CatalogSource::Ebible => ebible::BASE.to_string(),
            #[cfg(test)]
            CatalogSource::Test(_) => String::new(),
        };
        Ctx { root, now, force: false, base }
    }

    pub(crate) fn dir(&self) -> PathBuf { self.root.join("bibles/catalog") }
}

/// The rows a source offers. `offline` is true when the source couldn't be reached and the rows are the last saved
/// list, so Settings can say "You're offline" and turn off Add.
#[derive(Debug, Default)]
pub struct Listing { pub rows: Vec<CatalogRow>, pub offline: bool }

#[derive(Debug, PartialEq, Eq)]
pub enum CatalogError {
    /// No internet and no saved list.
    Offline,
    /// The source answered, but not with a list we can read.
    Bad(String),
    /// The data folder couldn't be written.
    Disk(String),
}

impl CatalogError {
    pub fn message(&self) -> String {
        match self {
            CatalogError::Offline => strings::CATALOG_OFFLINE.into(),
            CatalogError::Bad(why) => format!("{} ({why})", strings::CATALOG_BAD),
            CatalogError::Disk(why) => format!("{} ({why})", strings::CATALOG_DISK),
        }
    }
}

/// How much of the Bible a row has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope { Full, NewTestament, OldTestament, Portions }

impl Scope {
    /// The row's tag in Settings → Bibles.
    pub fn label(self) -> &'static str {
        match self {
            Scope::Full => strings::SCOPE_FULL,
            Scope::NewTestament => strings::SCOPE_NT,
            Scope::OldTestament => strings::SCOPE_OT,
            Scope::Portions => strings::SCOPE_PORTIONS,
        }
    }
}

/// What is known about a row's licence before download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LicenceState {
    /// From the shipped seed or an earlier download of the same edition.
    Known(Licence),
    /// Not known yet; it is read from the Bible's own notice when it is downloaded ("licence checked on download").
    OnDownload,
    /// An earlier download of this edition had no licence we may use. `checked` drops such rows.
    Refused,
}

/// Everything a source says about one Bible. Turned into a row only by `CatalogRow::checked`.
#[derive(Clone, Debug, Default)]
pub struct Fields {
    /// The source's own id for it, without the prefix (`spaRV1909`).
    pub source_id: String,
    pub abbr: String,
    pub title: String,
    pub title_en: String,
    pub description: String,
    pub copyright: String,
    pub lang: String,
    pub lang_name: String,
    pub lang_name_en: String,
    pub dir: Dir,
    pub script: String,
    pub update_date: String,
    pub has_dc: bool,
}

/// One Bible a catalog offers.
#[derive(Clone, Debug)]
pub struct CatalogRow {
    source: CatalogSource,
    id: String,
    f: Fields,
    scope: Scope,
    licence: LicenceState,
    /// Every searchable name, folded once by `blocklist::normalise`.
    search: Vec<String>,
    /// Other sources that offer the same Bible (`merge`).
    also_from: Vec<CatalogSource>,
}

impl CatalogRow {
    /// The only way to make a row. `None` when the Bible is blocked or its licence is known to be refused.
    pub fn checked(source: CatalogSource, f: Fields, scope: Scope, licence: LicenceState) -> Option<CatalogRow> {
        if licence == LicenceState::Refused { return None; }
        let id = format!("{}:{}", source.id(), f.source_id);
        if !super::id::is_prefixed(&id) { return None; }
        let titles = [f.title.as_str(), f.title_en.as_str()];
        let notices = [f.copyright.as_str(), f.description.as_str()];
        let blocked = if source.offline() {
            blocklist::blocked_row(&f.lang, &f.abbr, &titles, &notices, &[&id])
        } else {
            blocklist::blocked_online(&f.abbr, &titles, &notices, &[&id])
        };
        if blocked { return None; }
        let search = [&f.abbr, &f.title, &f.title_en, &f.lang_name, &f.lang_name_en, &f.lang, &f.source_id]
            .iter().map(|s| blocklist::normalise(s)).filter(|s| !s.is_empty()).collect();
        Some(CatalogRow { source, id, f, scope, licence, search, also_from: vec![] })
    }

    pub fn source(&self) -> CatalogSource { self.source }
    /// The translation id it installs as (`ebible:spaRV1909`).
    pub fn id(&self) -> &str { &self.id }
    pub fn source_id(&self) -> &str { &self.f.source_id }
    pub fn abbr(&self) -> &str { &self.f.abbr }
    pub fn title(&self) -> &str { &self.f.title }
    pub fn title_en(&self) -> &str { &self.f.title_en }
    pub fn description(&self) -> &str { &self.f.description }
    pub fn copyright(&self) -> &str { &self.f.copyright }
    pub fn lang(&self) -> &str { &self.f.lang }
    pub fn lang_name(&self) -> &str { &self.f.lang_name }
    pub fn lang_name_en(&self) -> &str { &self.f.lang_name_en }
    pub fn dir(&self) -> Dir { self.f.dir }
    pub fn script(&self) -> &str { &self.f.script }
    pub fn update_date(&self) -> &str { &self.f.update_date }
    pub fn has_dc(&self) -> bool { self.f.has_dc }
    pub fn scope(&self) -> Scope { self.scope }
    pub fn licence(&self) -> &LicenceState { &self.licence }
    /// Other sources that offer the same Bible; the row's tag says "Also on …".
    pub fn also_from(&self) -> &[CatalogSource] { &self.also_from }

    /// True when every word of the query is in one of its names, ignoring case, accents, spaces and punctuation.
    pub fn matches(&self, query: &str) -> bool {
        query.split_whitespace().map(blocklist::normalise).filter(|w| !w.is_empty())
            .all(|w| self.search.iter().any(|s| s.contains(&w)))
    }
}

/// One row per Bible across sources (Nov 5 spec §4): rows of different sources with the same language and the same
/// title (the English one, else its own, ignoring case, accents and spacing) become one. The row kept is the offline
/// source's, else one whose key is saved (`has_key`), else the first listed; it names the others in `also_from`.
/// Rows of one source never merge: the source lists them apart for a reason (two editions, say).
pub fn merge(rows: Vec<CatalogRow>, has_key: &dyn Fn(CatalogSource) -> bool) -> Vec<CatalogRow> {
    let key = |r: &CatalogRow| (r.lang().to_lowercase(), blocklist::normalise(if r.title_en().is_empty() { r.title() } else { r.title_en() }));
    let rank = |r: &CatalogRow| if r.source.offline() { 0 } else if has_key(r.source) { 1 } else { 2 };
    let mut out: Vec<CatalogRow> = Vec::new();
    for r in rows {
        let k = key(&r);
        let same = out.iter_mut().find(|o| key(o) == k && o.source != r.source && !o.also_from.contains(&r.source));
        match same {
            None => out.push(r),
            Some(o) if rank(&r) < rank(&*o) => {
                let old = std::mem::replace(o, r);
                o.also_from = old.also_from;
                o.also_from.insert(0, old.source);
            }
            Some(o) => o.also_from.push(r.source),
        }
    }
    out
}

/// The rows that match `query`, in the order given. An empty query matches everything.
pub fn search<'a>(rows: &'a [CatalogRow], query: &str) -> Vec<&'a CatalogRow> {
    rows.iter().filter(|r| r.matches(query)).collect()
}

/// One language's rows, for the grouped list in Settings → Bibles.
#[derive(Debug)]
pub struct LanguageGroup<'a> {
    pub lang: String,
    pub name: String,
    pub name_en: String,
    pub rows: Vec<&'a CatalogRow>,
}

/// Rows grouped by language: the church's own language first, then A to Z by English name. Inside a group, full
/// Bibles come first, then New Testaments, then the rest, each by title.
pub fn by_language<'a>(rows: &[&'a CatalogRow], church_lang: &str) -> Vec<LanguageGroup<'a>> {
    let mut groups: Vec<LanguageGroup<'a>> = Vec::new();
    for &r in rows {
        match groups.iter_mut().find(|g| g.lang == r.lang()) {
            Some(g) => g.rows.push(r),
            None => groups.push(LanguageGroup {
                lang: r.lang().into(), name: r.lang_name().into(), name_en: r.lang_name_en().into(), rows: vec![r],
            }),
        }
    }
    let first = |g: &LanguageGroup| !g.lang.eq_ignore_ascii_case(church_lang);
    let english = |g: &LanguageGroup| {
        let n = if g.name_en.is_empty() { &g.lang } else { &g.name_en };
        blocklist::normalise(n)
    };
    groups.sort_by(|a, b| first(a).cmp(&first(b)).then_with(|| english(a).cmp(&english(b))).then_with(|| a.lang.cmp(&b.lang)));
    for g in &mut groups {
        g.rows.sort_by(|a, b| a.scope().cmp(&b.scope())
            .then_with(|| blocklist::normalise(a.title()).cmp(&blocklist::normalise(b.title())))
            .then_with(|| a.id().cmp(b.id())));
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(id: &str, abbr: &str, title: &str, lang: &str) -> Fields {
        Fields { source_id: id.into(), abbr: abbr.into(), title: title.into(), title_en: title.into(), lang: lang.into(),
            lang_name_en: lang.into(), ..Default::default() }
    }

    fn row(id: &str, abbr: &str, title: &str, lang: &str) -> Option<CatalogRow> {
        CatalogRow::checked(CatalogSource::Ebible, fields(id, abbr, title, lang), Scope::Full, LicenceState::OnDownload)
    }

    fn from(source: CatalogSource, id: &str, title: &str, lang: &str) -> CatalogRow {
        CatalogRow::checked(source, fields(id, "X", title, lang), Scope::Full, LicenceState::OnDownload).unwrap()
    }

    fn sources_of(rows: &[CatalogRow]) -> Vec<(CatalogSource, Vec<CatalogSource>)> { rows.iter().map(|r| (r.source(), r.also_from().to_vec())).collect() }

    #[test]
    fn same_bible_two_sources_one_row_offline_wins() {
        let online = from(CatalogSource::Test(1), "web", "World English Bible", "eng");
        let offline = from(CatalogSource::Ebible, "engwebp", "World English  bible", "eng");
        let rows = merge(vec![online, offline], &|_| true);
        assert_eq!(sources_of(&rows), [(CatalogSource::Ebible, vec![CatalogSource::Test(1)])]);
        assert_eq!(rows[0].id(), "ebible:engwebp", "the offline row is the one Add installs");
    }

    #[test]
    fn different_language_same_title_kept_apart() {
        let rows = merge(vec![from(CatalogSource::Ebible, "a", "Bible", "eng"), from(CatalogSource::Test(1), "b", "Bible", "spa")], &|_| false);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn keyed_online_beats_unkeyed_online() {
        let rows = merge(vec![from(CatalogSource::Test(1), "a", "NIV", "eng"), from(CatalogSource::Test(2), "b", "NIV", "eng")], &|s| s == CatalogSource::Test(2));
        assert_eq!(sources_of(&rows), [(CatalogSource::Test(2), vec![CatalogSource::Test(1)])]);
        let rows = merge(vec![from(CatalogSource::Test(1), "a", "NIV", "eng"), from(CatalogSource::Test(2), "b", "NIV", "eng")], &|_| false);
        assert_eq!(sources_of(&rows), [(CatalogSource::Test(1), vec![CatalogSource::Test(2)])], "else the first listed");
    }

    #[test]
    fn rows_of_one_source_never_merge() {
        let rows = merge(vec![from(CatalogSource::Ebible, "a", "Reina Valera", "spa"), from(CatalogSource::Ebible, "b", "Reina Valera", "spa")], &|_| false);
        assert_eq!(rows.len(), 2, "eBible lists two editions apart for a reason");
        assert_eq!(strings::also_on(&rows[0].also_from().iter().map(|s| s.label()).collect::<Vec<_>>()), "");
        assert_eq!(strings::also_on(&["YouVersion", "API.Bible"]), "Also on YouVersion · API.Bible");
    }

    #[test]
    fn no_row_can_be_made_for_a_blocked_bible() {
        assert!(row("engnwt", "NWT", "Some Bible", "en").is_none(), "abbreviation");
        assert!(row("engx", "X", "New World Translation of the Holy Scriptures", "en").is_none(), "title");
        assert!(row("spax", "X", "Traducción del Nuevo Mundo", "es").is_none(), "translated title");
        assert!(row("engj", "JST", "Bible", "en").is_none(), "Joseph Smith Translation");
        assert!(row("engp", "X", "The Passion Translation", "en").is_none(), "The Passion Translation");
        let mut f = fields("engw", "W", "Bible", "en");
        f.copyright = "© Watch Tower Bible and Tract Society".into();
        assert!(CatalogRow::checked(CatalogSource::Ebible, f, Scope::Full, LicenceState::OnDownload).is_none(), "notice");
        let mut f = fields("engd", "D", "Bible", "en");
        f.description = "Published by Passion & Fire Ministries".into();
        assert!(CatalogRow::checked(CatalogSource::Ebible, f, Scope::Full, LicenceState::OnDownload).is_none(), "description");
    }

    #[test]
    fn tepehua_is_listed() {
        let r = row("tptNT", "NT", "Tlachichilco Tepehua New Testament", "tpt").expect("Tepehua is not The Passion Translation");
        assert_eq!(r.id(), "ebible:tptNT");
        assert!(row("tptNT", "TPT", "Nuevo Testamento", "tpt").is_some(), "an abbreviation that is only the language code");
    }

    #[test]
    fn a_refused_licence_or_a_bad_id_makes_no_row() {
        assert!(CatalogRow::checked(CatalogSource::Ebible, fields("engx", "X", "Bible", "en"), Scope::Full, LicenceState::Refused).is_none());
        assert!(row("", "X", "Bible", "en").is_none());
        assert!(row(" x", "X", "Bible", "en").is_none());
    }

    #[test]
    fn search_ignores_case_accents_and_spaces_and_needs_every_word() {
        let mut f = fields("spaRV1909", "RV1909", "Santa Biblia Reina-Valera 1909", "spa");
        f.lang_name = "Español".into();
        f.lang_name_en = "Spanish".into();
        let r = CatalogRow::checked(CatalogSource::Ebible, f, Scope::Full, LicenceState::OnDownload).unwrap();
        for q in ["", "reina valera", "REINA", "espanol", "Español", "spanish 1909", "rv1909", "spaRV"] { assert!(r.matches(q), "{q}"); }
        for q in ["english", "reina english"] { assert!(!r.matches(q), "{q}"); }
    }

    #[test]
    fn groups_put_the_church_language_first_then_english_names_a_to_z() {
        let mk = |id: &str, title: &str, lang: &str, en: &str, scope: Scope| {
            let mut f = fields(id, id, title, lang);
            f.lang_name_en = en.into();
            CatalogRow::checked(CatalogSource::Ebible, f, scope, LicenceState::OnDownload).unwrap()
        };
        let rows = [
            mk("deu1", "Zeta", "deu", "German", Scope::Full),
            mk("eng2", "World English New Testament", "eng", "English", Scope::NewTestament),
            mk("spa1", "Biblia", "spa", "Spanish", Scope::Full),
            mk("eng1", "World English Bible", "eng", "English", Scope::Full),
            mk("afr1", "Bybel", "afr", "Afrikaans", Scope::Full),
            mk("eng3", "A Psalter", "eng", "English", Scope::Portions),
        ];
        let all: Vec<&CatalogRow> = rows.iter().collect();
        let g = by_language(&all, "spa");
        assert_eq!(g.iter().map(|g| g.lang.as_str()).collect::<Vec<_>>(), ["spa", "afr", "eng", "deu"]);
        assert_eq!(g[2].rows.iter().map(|r| r.source_id()).collect::<Vec<_>>(), ["eng1", "eng2", "eng3"]);
        assert_eq!(by_language(&all, "eng")[0].lang, "eng");
    }
}
