//! RustyPresenter in any language (I18N spec `docs/superpowers/specs/2026-11-06-i18n-any-language-design.md`).
//!
//! One gettext catalog per language, `translations/<code>/LC_MESSAGES/rustypresenter.po`, built into the program
//! (`build.rs`). `.slint` text uses Slint's `@tr(...)`; text made in Rust uses the macros below, over the same files.
//! A missing, empty or fuzzy translation shows the English text.
//!
//! # Writing translatable Rust text
//!
//! ```ignore
//! use crate::i18n::{tr, trn, trc};
//!
//! tr!("Settings saved")                                   // -> String
//! tr!("Delete “{name}”?", name = song.title)              // named gaps; any `Display` value
//! trn!("{n} song", "{n} songs", count)                    // plural; `{n}` is filled with `count` for you
//! trn!("{n} verse from {book}", "{n} verses from {book}", count, book = name)
//! trc!("verb", "Clear")                                   // a context, when one English word needs two translations
//! trc!("screen", "Clear {what}", what = label)
//! ```
//!
//! - The English text must be a string literal (the extractor reads it from the source). Never build it with
//!   `format!` first, and never pass a variable: translate the fixed words, put the variable in a named gap.
//! - Gaps are `{name}` (letters, digits, `_`). A gap with no argument stays as written. `{{` and `}}` give `{` `}`.
//! - Keep the English byte-for-byte when converting old text, so tests stay green.
//! - A user string `pub const SAVED: &str = "Settings saved";` becomes
//!   `pub fn saved() -> String { tr!("Settings saved") }` (callers change `SAVED` to `saved()`). Keep a `const` for
//!   things that are not words for people: ids, keys, file names, log lines, font names.
//! - After adding or changing text, run `cargo run --example i18n_extract` to update `translations/` (the
//!   `i18n_pot_is_current` test fails until then).

pub mod plural;
pub mod po;

use plural::Plurals;
use std::{collections::{HashMap, HashSet}, fmt, sync::{Arc, OnceLock, RwLock}};

mod bundle {
    // `BUNDLE: &[(code, .po text)]` sorted by code, and `POT: &str`.
    include!(concat!(env!("OUT_DIR"), "/i18n_bundle.rs"));
}

/// The source language's code. English needs no catalog.
pub const ENGLISH: &str = "en";

/// One language's translations.
#[derive(Debug)]
pub struct Catalog {
    pub code: String,
    pub native_name: String,
    pub english_name: String,
    /// `X-Reviewed: yes` in the header: a native speaker checked it.
    pub reviewed: bool,
    pub plurals: Plurals,
    map: HashMap<(String, String), Vec<String>>,
}

impl Catalog {
    /// Reads a `.po`. Untranslated and fuzzy entries are left out, and so is any whose gaps (`{name}`, `{}`, `{0}`)
    /// differ from the English, so the English shows instead of a broken sentence.
    pub fn from_po(code: &str, text: &str) -> Result<Catalog, po::ParseError> {
        let file = po::parse(text)?;
        let name = |key: &str| file.header(key).filter(|v| !v.is_empty()).unwrap_or(code).to_string();
        let mut map = HashMap::new();
        for e in file.entries.iter().filter(|e| e.translated() && gaps_kept(e)) {
            map.insert(e.key(), e.strs.clone());
        }
        Ok(Catalog {
            code: code.to_string(),
            native_name: name("X-Native-Name"),
            english_name: name("X-English-Name"),
            reviewed: file.header("X-Reviewed").is_some_and(|v| v.eq_ignore_ascii_case("yes")),
            plurals: Plurals::from_header(file.header("Plural-Forms")),
            map,
        })
    }

    fn get(&self, ctx: &str, id: &str) -> Option<&Vec<String>> { self.map.get(&(ctx.to_string(), id.to_string())) }

    /// How many entries have a usable translation.
    pub fn translated(&self) -> usize { self.map.len() }
}

/// Whether every translated form keeps the English gaps. Plural forms may leave out `{n}` (e.g. "one song").
pub fn gaps_kept(e: &po::Entry) -> bool {
    let mut source: HashSet<String> = placeholders(&e.id).into_iter().collect();
    if let Some(p) = &e.plural { source.extend(placeholders(p)) }
    e.strs.iter().all(|s| {
        let got: HashSet<String> = placeholders(s).into_iter().collect();
        if e.plural.is_some() {
            got.is_subset(&source) && source.iter().filter(|g| *g != "{n}").all(|g| got.contains(g))
        } else { got == source }
    })
}

/// The gaps in a text, sorted, once each: `{name}`, `{}`, `{0}`. `{{` is not a gap.
pub fn placeholders(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'{' {
            if b.get(i + 1) == Some(&b'{') { i += 2; continue }
            if let Some(len) = s[i + 1..].find('}') {
                let inner = &s[i + 1..i + 1 + len];
                if inner.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    out.push(format!("{{{inner}}}"));
                    i += len + 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    out.sort();
    out.dedup();
    out
}

/// A language the app can show, for Settings → Language.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageInfo {
    pub code: String,
    /// Its name in itself ("Español").
    pub native_name: String,
    /// Its name in English ("Spanish").
    pub english_name: String,
    /// How much of the app is translated, 0–100.
    pub percent: u8,
    pub reviewed: bool,
}

/// `set_language` was given a code that is not built in.
#[derive(Clone, Debug, PartialEq)]
pub struct UnknownLanguage(pub String);

impl fmt::Display for UnknownLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Unknown language: {}", self.0) }
}

impl std::error::Error for UnknownLanguage {}

/// The built-in catalogs, read once. One that fails to read (the tests make that impossible) is left out.
fn builtins() -> &'static [Arc<Catalog>] {
    static ALL: OnceLock<Vec<Arc<Catalog>>> = OnceLock::new();
    ALL.get_or_init(|| bundle::BUNDLE.iter().filter_map(|(code, text)| match Catalog::from_po(code, text) {
        Ok(c) => Some(Arc::new(c)),
        Err(e) => { eprintln!("The {code} translation could not be read ({e}); it is left out."); None }
    }).collect())
}

/// The template's `(context, msgid)` keys, for "how complete".
fn template_keys() -> &'static HashSet<(String, String)> {
    static KEYS: OnceLock<HashSet<(String, String)>> = OnceLock::new();
    KEYS.get_or_init(|| po::parse(bundle::POT).map(|f| f.entries.iter().map(po::Entry::key).collect()).unwrap_or_default())
}

fn builtin(code: &str) -> Option<Arc<Catalog>> { builtins().iter().find(|c| c.code == code).cloned() }

/// English first, then every built-in language by code.
pub fn languages() -> Vec<LanguageInfo> {
    let keys = template_keys();
    let mut out = vec![LanguageInfo { code: ENGLISH.into(), native_name: "English".into(), english_name: "English".into(), percent: 100, reviewed: true }];
    out.extend(builtins().iter().map(|c| {
        let done = keys.iter().filter(|(ctx, id)| c.get(ctx, id).is_some()).count();
        let percent = if keys.is_empty() { 100 } else { (done * 100 / keys.len()) as u8 };
        LanguageInfo { code: c.code.clone(), native_name: c.native_name.clone(), english_name: c.english_name.clone(), percent, reviewed: c.reviewed }
    }));
    out
}

/// Whether `code` is English or a built-in language.
pub fn is_known(code: &str) -> bool { code == ENGLISH || builtin(code).is_some() }

static CURRENT: RwLock<Option<Arc<Catalog>>> = RwLock::new(None);

#[cfg(test)]
thread_local! {
    /// Unit tests: each test thread has its own language (English unless a test sets one), so a test that switches
    /// language never changes another test's text.
    static LOCAL: std::cell::RefCell<Option<Arc<Catalog>>> = const { std::cell::RefCell::new(None) };
}

fn active() -> Option<Arc<Catalog>> {
    #[cfg(test)]
    return LOCAL.with(|l| l.borrow().clone());
    #[cfg(not(test))]
    CURRENT.read().unwrap_or_else(|e| e.into_inner()).clone()
}

fn install(c: Option<Arc<Catalog>>) {
    #[cfg(test)]
    { LOCAL.with(|l| *l.borrow_mut() = c); let _ = &CURRENT; }
    #[cfg(not(test))]
    { *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = c; }
}

/// Shows the app in `code` (`""` or `"en"` = English). Rust text changes at once; the Slint text on this thread
/// too, if it has windows (other threads' windows call `sync_slint`).
pub fn set_language(code: &str) -> Result<(), UnknownLanguage> {
    if code.is_empty() || code == ENGLISH { install(None) } else {
        install(Some(builtin(code).ok_or_else(|| UnknownLanguage(code.to_string()))?));
    }
    sync_slint();
    Ok(())
}

/// The language shown now, e.g. `"en"` or `"pt-BR"`.
pub fn current() -> String { active().map_or_else(|| ENGLISH.to_string(), |c| c.code.clone()) }

/// Gives this thread's Slint windows the current language. Call after making a window: Slint picks the computer's
/// language for the first one by itself. Does nothing on a thread with no Slint windows.
pub fn sync_slint() {
    let code = current();
    let _ = slint::select_bundled_translation(if code == ENGLISH { "" } else { &code });
}

/// The computer's language, if the app has it.
pub fn system_language() -> Option<String> { sys_locale::get_locale().and_then(|l| system_language_from(&l)) }

/// `system_language` for a given locale such as `es-MX`, `pt_BR.UTF-8` or `zh-Hans-CN`.
pub fn system_language_from(locale: &str) -> Option<String> {
    let mut codes = vec![ENGLISH];
    codes.extend(builtins().iter().map(|c| c.code.as_str()));
    match_language(locale, &codes)
}

/// The best of `codes` for `locale`: the same code, then without a script (`zh-Hans-CN` → `zh-CN`), then Chinese
/// scripts (`zh-Hans` → `zh-CN`, `zh-Hant` → `zh-TW`), then the same base language (`es-MX` → `es`).
pub fn match_language(locale: &str, codes: &[&str]) -> Option<String> {
    let loc = locale.split(['.', '@']).next().unwrap_or_default().replace('_', "-");
    if loc.is_empty() || loc.eq_ignore_ascii_case("C") || loc.eq_ignore_ascii_case("POSIX") { return None }
    let parts: Vec<&str> = loc.split('-').collect();
    let find = |want: &str| codes.iter().find(|c| c.eq_ignore_ascii_case(want)).map(|c| c.to_string());
    let base = |c: &str| c.split('-').next().unwrap_or_default().to_string();
    find(&loc)
        .or_else(|| (parts.len() >= 3 && parts[1].len() == 4).then(|| find(&format!("{}-{}", parts[0], parts[2]))).flatten())
        .or_else(|| {
            let script = parts.get(1).filter(|s| s.len() == 4)?;
            if !parts[0].eq_ignore_ascii_case("zh") { return None }
            if script.eq_ignore_ascii_case("Hans") { find("zh-CN") } else if script.eq_ignore_ascii_case("Hant") { find("zh-TW") } else { None }
        })
        .or_else(|| codes.iter().find(|c| base(c).eq_ignore_ascii_case(parts[0])).map(|c| c.to_string()))
}

/// The language to show for the `Settings.ui_language` value: `""` follows the computer (English if the app does
/// not have its language); a code that is not built in gives English.
pub fn resolve(setting: &str) -> String {
    if setting.is_empty() { return system_language().unwrap_or_else(|| ENGLISH.to_string()) }
    if is_known(setting) { setting.to_string() } else { ENGLISH.to_string() }
}

/// `msgid` in the current language. Use `tr!`.
pub fn tr(msgid: &str) -> String { trc("", msgid) }

/// `msgid` with a context. Use `trc!`.
pub fn trc(ctx: &str, msgid: &str) -> String {
    active().and_then(|c| c.get(ctx, msgid).and_then(|f| f.first().cloned())).unwrap_or_else(|| msgid.to_string())
}

/// The form of `one`/`many` for `n`, gaps not filled. Use `trn!`.
pub fn trn(one: &str, many: &str, n: u64) -> String {
    if let Some(c) = active() && let Some(forms) = c.get("", one) {
        let i = c.plurals.index(n);
        if let Some(f) = forms.get(i).or(forms.last()) { return f.clone() }
    }
    if n == 1 { one.to_string() } else { many.to_string() }
}

/// Fills `{name}` gaps from `args`; a gap with no argument stays as written. `{{` and `}}` are `{` and `}`.
pub fn fill(template: &str, args: &[(&str, &dyn fmt::Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if tail.starts_with('{') && let Some(end) = tail.find('}') {
            let name = &tail[1..end];
            if let Some((_, v)) = args.iter().find(|(k, _)| *k == name) {
                use fmt::Write;
                let _ = write!(out, "{v}");
                rest = &tail[end + 1..];
                continue;
            }
        }
        out.push_str(&tail[..1]);
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// `tr!("English text")` or `tr!("Hello {name}", name = value)`: the text in the current language, as a `String`.
/// See the module docs.
#[macro_export]
macro_rules! tr {
    ($msg:literal $(,)?) => { $crate::i18n::tr($msg) };
    ($msg:literal $(, $name:ident = $val:expr)+ $(,)?) => {
        $crate::i18n::fill(&$crate::i18n::tr($msg), &[$((stringify!($name), &$val as &dyn ::std::fmt::Display)),+])
    };
}

/// `trn!("{n} song", "{n} songs", count)` (more named gaps may follow): the plural form for `count`, `{n}` filled.
#[macro_export]
macro_rules! trn {
    ($one:literal, $many:literal, $n:expr $(, $name:ident = $val:expr)* $(,)?) => {{
        let n = $n;
        $crate::i18n::fill(&$crate::i18n::trn($one, $many, n as u64), &[("n", &n as &dyn ::std::fmt::Display) $(, (stringify!($name), &$val as &dyn ::std::fmt::Display))*])
    }};
}

/// `trc!("context", "English text")` (named gaps may follow): like `tr!`, for a word that needs a different
/// translation in another place.
#[macro_export]
macro_rules! trc {
    ($ctx:literal, $msg:literal $(,)?) => { $crate::i18n::trc($ctx, $msg) };
    ($ctx:literal, $msg:literal $(, $name:ident = $val:expr)+ $(,)?) => {
        $crate::i18n::fill(&$crate::i18n::trc($ctx, $msg), &[$((stringify!($name), &$val as &dyn ::std::fmt::Display)),+])
    };
}

pub use crate::{tr, trc, trn};

/// Unit tests: a language from `.po` text on this test thread only.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Runs `f` with `po` as this thread's language, then goes back to the one before.
    pub(crate) fn with_catalog<R>(po: &str, f: impl FnOnce() -> R) -> R {
        let cat = Arc::new(Catalog::from_po("xx", po).expect("test catalog"));
        let before = LOCAL.with(|l| l.replace(Some(cat)));
        let out = f();
        LOCAL.with(|l| *l.borrow_mut() = before);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::with_catalog, *};

    const XX: &str = r#"
msgid ""
msgstr ""
"Language: xx\n"
"Plural-Forms: nplurals=3; plural=(n==1 ? 0 : n==2 ? 1 : 2);\n"
"X-Native-Name: Ixx\n"
"X-English-Name: Test\n"
"X-Reviewed: yes\n"

msgid "Settings saved"
msgstr "Gespeichert"

msgid "Delete “{name}”?"
msgstr "“{name}” weg?"

msgid "{n} song"
msgid_plural "{n} songs"
msgstr[0] "ein Lied"
msgstr[1] "{n} Liedchen"
msgstr[2] "{n} Lieder"

msgctxt "verb"
msgid "Clear"
msgstr "Leeren"

msgid "Empty"
msgstr ""

#, fuzzy
msgid "Fuzzy"
msgstr "Flausch"

msgid "Lost {gap}"
msgstr "Verloren"
"#;

    #[test]
    fn english_by_default() {
        assert_eq!(current(), "en");
        assert_eq!(tr!("Settings saved"), "Settings saved");
        assert_eq!(tr!("Delete “{name}”?", name = "Amazing Grace"), "Delete “Amazing Grace”?");
        assert_eq!(trn!("{n} song", "{n} songs", 1), "1 song");
        assert_eq!(trn!("{n} song", "{n} songs", 3usize), "3 songs");
        assert_eq!(trc!("verb", "Clear"), "Clear");
    }

    #[test]
    fn translated_with_named_gaps_plurals_and_context() {
        with_catalog(XX, || {
            assert_eq!(current(), "xx");
            assert_eq!(tr!("Settings saved"), "Gespeichert");
            let title = String::from("Amazing Grace");
            assert_eq!(tr!("Delete “{name}”?", name = title), "“Amazing Grace” weg?");
            assert_eq!(trn!("{n} song", "{n} songs", 1), "ein Lied");
            assert_eq!(trn!("{n} song", "{n} songs", 2), "2 Liedchen");
            assert_eq!(trn!("{n} song", "{n} songs", 7u32), "7 Lieder");
            assert_eq!(trc!("verb", "Clear"), "Leeren");
            assert_eq!(tr!("Clear"), "Clear", "no context is another entry");
        });
        assert_eq!(tr!("Settings saved"), "Settings saved", "back to English after");
    }

    #[test]
    fn missing_empty_fuzzy_and_broken_entries_show_english() {
        with_catalog(XX, || {
            assert_eq!(tr!("Not there"), "Not there");
            assert_eq!(tr!("Empty"), "Empty");
            assert_eq!(tr!("Fuzzy"), "Fuzzy");
            assert_eq!(tr!("Lost {gap}", gap = 1), "Lost 1", "a translation that drops a gap is not used");
            assert_eq!(trn!("{n} file", "{n} files", 2), "2 files");
        });
    }

    #[test]
    fn named_args_and_braces() {
        assert_eq!(tr!("{a} and {b}", b = 2, a = "one"), "one and 2");
        assert_eq!(tr!("Keep {missing} as written", other = 1), "Keep {missing} as written");
        assert_eq!(fill("{{literal}} {x}", &[("x", &5)]), "{literal} 5");
        assert_eq!(fill("odd { and } ok", &[]), "odd { and } ok");
        assert_eq!(trn!("{n} verse from {book}", "{n} verses from {book}", 2, book = "John"), "2 verses from John");
        assert_eq!(trc!("screen", "Clear {what}", what = "text"), "Clear text");
    }

    #[test]
    fn placeholders_found() {
        assert_eq!(placeholders("{name} {} {0} {n} {{x}} {name} { no }"), ["{0}", "{name}", "{n}", "{}"]);
    }

    #[test]
    fn header_names_and_review() {
        let c = Catalog::from_po("xx", XX).unwrap();
        assert_eq!((c.native_name.as_str(), c.english_name.as_str(), c.reviewed, c.plurals.count), ("Ixx", "Test", true, 3));
        assert_eq!(c.translated(), 4);
        let bare = Catalog::from_po("yy", "").unwrap();
        assert_eq!((bare.native_name.as_str(), bare.reviewed, bare.plurals.count), ("yy", false, 2));
    }

    #[test]
    fn language_matching() {
        let codes = ["en", "es", "pt-BR", "zh-CN", "zh-TW", "fr"];
        let m = |l: &str| match_language(l, &codes);
        assert_eq!(m("es-MX").as_deref(), Some("es"));
        assert_eq!(m("es_MX.UTF-8").as_deref(), Some("es"));
        assert_eq!(m("pt-BR").as_deref(), Some("pt-BR"));
        assert_eq!(m("pt_br").as_deref(), Some("pt-BR"));
        assert_eq!(m("pt-PT").as_deref(), Some("pt-BR"));
        assert_eq!(m("zh-Hans-CN").as_deref(), Some("zh-CN"));
        assert_eq!(m("zh-Hans").as_deref(), Some("zh-CN"));
        assert_eq!(m("zh-Hant-HK").as_deref(), Some("zh-TW"));
        assert_eq!(m("en-US").as_deref(), Some("en"));
        assert_eq!(m("de-DE"), None);
        assert_eq!(m("C"), None);
        assert_eq!(m("POSIX"), None);
        assert_eq!(m(""), None);
        assert_eq!(system_language_from("es-MX").as_deref(), Some("es"), "Spanish is built in");
        assert_eq!(system_language_from("xx-YY"), None);
    }

    #[test]
    fn built_in_languages_and_switching() {
        let langs = languages();
        assert_eq!(langs[0].code, "en");
        let es = langs.iter().find(|l| l.code == "es").expect("Spanish is built in");
        assert_eq!((es.native_name.as_str(), es.english_name.as_str(), es.reviewed), ("Español", "Spanish", false));
        assert!(is_known("es") && is_known("en") && !is_known("xx"));
        assert_eq!(set_language("xx"), Err(UnknownLanguage("xx".into())));
        assert_eq!(current(), "en");
        set_language("es").unwrap();
        assert_eq!(current(), "es");
        set_language("").unwrap();
        assert_eq!(current(), "en");
        assert_eq!(resolve("es"), "es");
        assert_eq!(resolve("xx"), "en");
    }
}
