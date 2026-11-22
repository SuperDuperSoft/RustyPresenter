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

pub mod extract;
pub mod languages;
pub mod plural;
pub mod po;
pub mod remote_strings;
pub mod user;

use plural::Plurals;
use std::{collections::{BTreeMap, HashMap, HashSet}, fmt, path::{Path, PathBuf}, sync::{Arc, OnceLock, RwLock}};

mod bundle {
    // `BUNDLE: &[(code, .po text)]` sorted by code, and `POT: &str`.
    include!(concat!(env!("OUT_DIR"), "/i18n_bundle.rs"));
}

/// The source language's code. English needs no catalog.
pub const ENGLISH: &str = "en";

/// One language's translations.
#[derive(Clone, Debug)]
pub struct Catalog {
    pub code: String,
    pub native_name: String,
    pub english_name: String,
    /// `X-Reviewed: yes` in the header: a native speaker checked it.
    pub reviewed: bool,
    pub plurals: Plurals,
    /// Built into the program (`translations/`), so Slint's bundle has it too.
    pub builtin: bool,
    /// Context, then msgid, then one string per plural form. Two levels, so a lookup takes the context and the
    /// msgid as they come (`&str`) and builds no key: every `tr!` in every redraw comes through here.
    map: HashMap<String, HashMap<String, Vec<String>>>,
    /// The entries a user file gave (`<data>/translations/<code>.po`): they win over the built-in ones.
    user_keys: HashSet<(String, String)>,
    header: Vec<(String, String)>,
}

impl Catalog {
    /// Reads a `.po`. Untranslated and fuzzy entries are left out, and so is any whose gaps (`{name}`, `{}`, `{0}`)
    /// differ from the English, so the English shows instead of a broken sentence.
    pub fn from_po(code: &str, text: &str) -> Result<Catalog, po::ParseError> {
        let file = po::parse(text)?;
        let name = |key: &str| file.header(key).filter(|v| !v.is_empty()).unwrap_or(code).to_string();
        let mut map: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
        for e in file.entries.iter().filter(|e| e.translated() && gaps_kept(e)) {
            let (ctx, id) = e.key();
            map.entry(ctx).or_default().insert(id, e.strs.clone());
        }
        Ok(Catalog {
            code: code.to_string(),
            native_name: name("X-Native-Name"),
            english_name: name("X-English-Name"),
            reviewed: file.header("X-Reviewed").is_some_and(|v| v.eq_ignore_ascii_case("yes")),
            plurals: Plurals::from_header(file.header("Plural-Forms")),
            builtin: false,
            map,
            user_keys: HashSet::new(),
            header: file.header,
        })
    }

    /// A user file's catalog over the built-in one (if the language is built in): a user entry wins, and so does a
    /// header field the user file gives.
    fn layered(base: Option<&Catalog>, user: &Catalog) -> Catalog {
        let mut out = user.clone();
        out.user_keys = user.map.iter().flat_map(|(ctx, ids)| ids.keys().map(move |id| (ctx.clone(), id.clone()))).collect();
        let Some(base) = base else { return out };
        let has = |key: &str| user.header.iter().any(|(k, v)| k.eq_ignore_ascii_case(key) && !v.is_empty());
        if !has("X-Native-Name") { out.native_name = base.native_name.clone() }
        if !has("X-English-Name") { out.english_name = base.english_name.clone() }
        if !has("X-Reviewed") { out.reviewed = base.reviewed }
        if !has("Plural-Forms") { out.plurals = base.plurals.clone() }
        out.builtin = true;
        out.map = base.map.clone();
        for (ctx, ids) in &user.map { out.map.entry(ctx.clone()).or_default().extend(ids.iter().map(|(id, forms)| (id.clone(), forms.clone()))) }
        out
    }

    fn get(&self, ctx: &str, id: &str) -> Option<&Vec<String>> { self.map.get(ctx)?.get(id) }

    /// The usable translation of an entry (one string per plural form).
    pub fn forms(&self, ctx: &str, id: &str) -> Option<&[String]> { self.get(ctx, id).map(Vec::as_slice) }

    /// Whether the entry's translation came from the user's file.
    pub fn is_users(&self, ctx: &str, id: &str) -> bool { self.user_keys.contains(&(ctx.to_string(), id.to_string())) }

    /// How many entries have a usable translation.
    pub fn translated(&self) -> usize { self.map.values().map(HashMap::len).sum() }
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

/// The codes of the built-in languages, without reading one catalog.
fn builtin_codes() -> impl Iterator<Item = &'static str> { bundle::BUNDLE.iter().map(|(code, _)| *code) }

/// A built-in catalog, read the first time that language is asked for and kept: a church that only ever shows
/// English, or one language, reads no other. One that fails to read (the tests make that impossible) is left out.
fn builtin(code: &str) -> Option<Arc<Catalog>> {
    static READ: OnceLock<Vec<OnceLock<Option<Arc<Catalog>>>>> = OnceLock::new();
    let at = bundle::BUNDLE.iter().position(|(c, _)| *c == code)?;
    let read = READ.get_or_init(|| bundle::BUNDLE.iter().map(|_| OnceLock::new()).collect());
    read[at].get_or_init(|| {
        let (code, text) = bundle::BUNDLE[at];
        #[cfg(test)]
        BUILTINS_READ_HERE.with(|n| n.set(n.get() + 1));
        match Catalog::from_po(code, text) {
            Ok(c) => Some(Arc::new(Catalog { builtin: true, ..c })),
            Err(e) => { eprintln!("The {code} translation could not be read ({e}); it is left out."); None }
        }
    }).clone()
}

#[cfg(test)]
thread_local! {
    /// How many built-in catalogs this thread has read (the first to ask for a language reads it).
    static BUILTINS_READ_HERE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod lazy_tests {
    use super::*;

    /// Q16: finding the computer's language, or checking a code, reads no catalog; showing a language reads one.
    #[test]
    fn a_catalog_is_read_only_when_its_language_is_shown() {
        let read = || BUILTINS_READ_HERE.with(|n| n.get());
        let before = read();
        assert_eq!(system_language_from("es-MX").as_deref(), Some("es"));
        assert_eq!(system_language_from("en-US").as_deref(), Some("en"));
        assert!(is_known("de") && is_builtin("fr") && !is_builtin("xx") && !is_known("xx"));
        assert_eq!(resolve("ko"), "ko");
        assert_eq!(read(), before, "codes only: nothing was read");
        // Asking for the catalog itself reads it, at most once (another test may have read it first).
        assert!(catalog("es").is_some_and(|c| c.builtin && c.translated() > 1000));
        let after = read();
        assert!(after - before <= 1);
        assert!(catalog("es").is_some());
        assert_eq!(read(), after, "kept, not read again");
    }
}

/// The template's entries, for "how complete" and the translation editor.
pub fn template() -> &'static [po::Entry] {
    static ENTRIES: OnceLock<Vec<po::Entry>> = OnceLock::new();
    ENTRIES.get_or_init(|| po::parse(bundle::POT).map(|f| f.entries).unwrap_or_default())
}

/// The template's `(context, msgid)` keys, for "how complete".
fn template_keys() -> &'static HashSet<(String, String)> {
    static KEYS: OnceLock<HashSet<(String, String)>> = OnceLock::new();
    KEYS.get_or_init(|| template().iter().map(po::Entry::key).collect())
}

/// The built-in `.po` text of a language, as shipped.
pub fn builtin_po(code: &str) -> Option<&'static str> { bundle::BUNDLE.iter().find(|(c, _)| *c == code).map(|(_, t)| *t) }

/// Whether `code` is built into the program.
pub fn is_builtin(code: &str) -> bool { builtin_codes().any(|c| c == code) }

/// A language as the app shows it: the built-in catalog with the user's file over it, or the user's file alone.
pub fn catalog(code: &str) -> Option<Arc<Catalog>> {
    let user = read(|s| s.user.get(code).map(|(_, c)| c.clone()));
    let base = builtin(code);
    match user {
        None => base,
        Some(u) => Some(Arc::new(Catalog::layered(base.as_deref(), &u))),
    }
}

/// English first, then every built-in language and every language from the user's files, by code.
pub fn languages() -> Vec<LanguageInfo> {
    let keys = template_keys();
    // The list with its percentages reads every catalog; it is asked for only when the language page opens.
    let mut codes: Vec<String> = builtin_codes().map(String::from).collect();
    codes.extend(read(|s| s.user.keys().cloned().collect::<Vec<_>>()));
    codes.sort();
    codes.dedup();
    let mut out = vec![LanguageInfo { code: ENGLISH.into(), native_name: "English".into(), english_name: "English".into(), percent: 100, reviewed: true }];
    out.extend(codes.iter().filter_map(|code| catalog(code)).map(|c| {
        let done = keys.iter().filter(|(ctx, id)| c.get(ctx, id).is_some()).count();
        let percent = if keys.is_empty() { 100 } else { (done * 100 / keys.len()) as u8 };
        LanguageInfo { code: c.code.clone(), native_name: c.native_name.clone(), english_name: c.english_name.clone(), percent, reviewed: c.reviewed }
    }));
    out
}

/// Whether `code` is English, a built-in language or one from the user's files.
pub fn is_known(code: &str) -> bool { code == ENGLISH || is_builtin(code) || read(|s| s.user.contains_key(code)) }

/// The language on show and the user's translation files.
#[derive(Default)]
struct State {
    current: Option<Arc<Catalog>>,
    /// `<data>/translations/<code>.po` by code: its text and what it gives.
    user: BTreeMap<String, (String, Arc<Catalog>)>,
    folder: Option<PathBuf>,
    /// Bumped by every change to the text on show.
    revision: u64,
    /// User files that could not be read, for the operator to show.
    problems: Vec<String>,
}

static STATE: RwLock<State> = RwLock::new(State { current: None, user: BTreeMap::new(), folder: None, revision: 0, problems: Vec::new() });

#[cfg(test)]
thread_local! {
    /// Unit tests: each test thread has its own language and user files (English and none unless a test sets
    /// them), so a test that switches language never changes another test's text.
    static LOCAL: std::cell::RefCell<State> = std::cell::RefCell::default();
}

fn read<R>(f: impl FnOnce(&State) -> R) -> R {
    #[cfg(test)]
    return LOCAL.with(|l| f(&l.borrow()));
    #[cfg(not(test))]
    f(&STATE.read().unwrap_or_else(|e| e.into_inner()))
}

fn write<R>(f: impl FnOnce(&mut State) -> R) -> R {
    #[cfg(test)]
    { let _ = &STATE; LOCAL.with(|l| f(&mut l.borrow_mut())) }
    #[cfg(not(test))]
    f(&mut STATE.write().unwrap_or_else(|e| e.into_inner()))
}

fn active() -> Option<Arc<Catalog>> { read(|s| s.current.clone()) }

fn install(c: Option<Arc<Catalog>>) { write(|s| { s.current = c; s.revision += 1; }) }

/// Shows the app in `code` (`""` or `"en"` = English). Rust text changes at once; the Slint text on this thread
/// too, if it has windows (other threads' windows call `sync_slint`).
pub fn set_language(code: &str) -> Result<(), UnknownLanguage> {
    if code.is_empty() || code == ENGLISH { install(None) } else {
        install(Some(catalog(code).ok_or_else(|| UnknownLanguage(code.to_string()))?));
    }
    sync_slint();
    Ok(())
}

/// The language shown now, e.g. `"en"` or `"pt-BR"`.
pub fn current() -> String { active().map_or_else(|| ENGLISH.to_string(), |c| c.code.clone()) }

/// Changes with every change to the text on show (a language switch, a translation edited in the app).
pub fn revision() -> u64 { read(|s| s.revision) }

/// Gives this thread's Slint windows the current language and the user's edits. Call after making a window (Slint
/// picks the computer's language for the first one by itself) and after `revision` changes. Does nothing on a
/// thread with no Slint windows.
///
/// Slint only knows the built-in translations, so a language from a user file shows the bundle's English and the
/// patched Slint core asks `slint_text` first for every `@tr` (patches/i-slint-core/PATCHED.md).
pub fn sync_slint() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| i_slint_core::translations::set_translation_override(Some(slint_text)));
    let code = current();
    let bundled = code != ENGLISH && builtin(&code).is_some();
    let _ = slint::select_bundled_translation(if bundled { &code } else { "" });
    i_slint_core::translations::mark_all_translations_dirty();
}

/// The Slint hook: the user's translation of an `@tr` text (`.slint` texts have no context), the plural form for
/// `n` picked; `None` lets Slint use its bundle.
fn slint_text(en: &str, plural: Option<&str>, n: i32) -> Option<String> {
    let c = active()?;
    if c.builtin && !c.is_users("", en) { return None }
    let forms = c.get("", en)?;
    let form = if plural.is_some() { forms.get(c.plurals.index(n.max(0) as u64)).or(forms.last()) } else { forms.first() };
    form.cloned()
}

/// Reads the user's translation files, `<dir>/<code>.po` (the data folder's `translations`), over the built-in
/// ones, and shows the change. A file that cannot be read is left out; the sentences returned (and kept for
/// `take_problems`) say which.
pub fn load_user_folder(dir: &Path) -> Vec<String> {
    let mut found = BTreeMap::new();
    let mut problems = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect()).unwrap_or_default();
    files.sort();
    for path in files {
        let Some(code) = user::code_of(&path) else { continue };
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let read = std::fs::read(&path).map_err(|e| e.to_string())
            .and_then(|b| String::from_utf8(b).map_err(|_| tr!("it is not saved as UTF-8 text")))
            .map(|t| t.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(t))
            .and_then(|text| Catalog::from_po(&code, &text).map(|c| (text, c)).map_err(|e| e.to_string()));
        match read {
            Ok((text, c)) => { found.insert(code, (text, Arc::new(c))); }
            Err(reason) => problems.push(tr!("The translation file {file} could not be read ({reason}), so it is not used.", file = name, reason = reason)),
        }
    }
    let changed = write(|s| {
        s.folder = Some(dir.to_path_buf());
        s.problems.extend(problems.iter().cloned());
        let same = s.user.len() == found.len() && s.user.iter().zip(&found).all(|((a, (ta, _)), (b, (tb, _)))| a == b && ta == tb);
        if !same { s.user = found }
        !same
    });
    if changed { reinstall() }
    problems
}

/// Puts the text of one user file in place (`None` takes it away), after the editor saved it.
pub fn set_user_file(code: &str, text: Option<&str>) -> Result<(), po::ParseError> {
    let parsed = text.map(|t| Catalog::from_po(code, t).map(|c| (t.to_string(), Arc::new(c)))).transpose()?;
    write(|s| match parsed {
        Some(p) => { s.user.insert(code.to_string(), p); }
        None => { s.user.remove(code); }
    });
    reinstall();
    Ok(())
}

/// The folder `load_user_folder` last read.
pub fn user_folder() -> Option<PathBuf> { read(|s| s.folder.clone()) }

/// The text of the user's file for `code`, if there is one.
pub fn user_file(code: &str) -> Option<String> { read(|s| s.user.get(code).map(|(t, _)| t.clone())) }

/// The user files that could not be read since the last call.
pub fn take_problems() -> Vec<String> { write(|s| std::mem::take(&mut s.problems)) }

/// The language on show again, from its files as they are now (English if its only file went away).
fn reinstall() {
    let code = current();
    if code == ENGLISH { write(|s| s.revision += 1) } else { install(catalog(&code)) }
}

/// The computer's language, if the app has it.
pub fn system_language() -> Option<String> { sys_locale::get_locale().and_then(|l| system_language_from(&l)) }

/// `system_language` for a given locale such as `es-MX`, `pt_BR.UTF-8` or `zh-Hans-CN`.
pub fn system_language_from(locale: &str) -> Option<String> {
    let user: Vec<String> = read(|s| s.user.keys().cloned().collect());
    // Only the codes are needed to match the computer's language: no catalog is read for that.
    let mut codes: Vec<&str> = vec![ENGLISH];
    for code in builtin_codes() { codes.push(code) }
    codes.extend(user.iter().map(String::as_str));
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
        let before = LOCAL.with(|l| l.borrow_mut().current.replace(cat));
        let out = f();
        LOCAL.with(|l| l.borrow_mut().current = before);
        out
    }

    /// Forgets this thread's user files and goes back to English.
    pub(crate) fn reset() { LOCAL.with(|l| *l.borrow_mut() = State::default()) }
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
    fn the_slint_hook_gives_only_the_users_text() {
        testing::reset();
        let dir = crate::store::test_dir();
        std::fs::write(dir.join("es.po"), "msgid \"Language\"\nmsgstr \"Lengua\"\n\nmsgid \"{n} song\"\nmsgid_plural \"{n} songs\"\nmsgstr[0] \"{n} cántico\"\nmsgstr[1] \"{n} cánticos\"\n").unwrap();
        std::fs::write(dir.join("xx.po"), "msgid \"Cancel\"\nmsgstr \"Xancel\"\n").unwrap();
        assert!(load_user_folder(&dir).is_empty());
        assert_eq!(slint_text("Language", None, 0), None, "English: nothing to change");
        set_language("es").unwrap();
        assert_eq!(slint_text("Language", None, 0).as_deref(), Some("Lengua"));
        assert_eq!(slint_text("Settings saved", None, 0), None, "a built-in text is left to Slint's bundle");
        assert_eq!(slint_text("{n} song", Some("{n} songs"), 1).as_deref(), Some("{n} cántico"));
        assert_eq!(slint_text("{n} song", Some("{n} songs"), 4).as_deref(), Some("{n} cánticos"));
        set_language("xx").unwrap();
        assert_eq!(slint_text("Cancel", None, 0).as_deref(), Some("Xancel"), "a language Slint does not have");
        assert_eq!(slint_text("Language", None, 0), None, "untranslated: the bundle's English");
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
