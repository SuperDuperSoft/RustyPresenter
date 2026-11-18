//! The user's own translations, `<data>/translations/<code>.po`, and what the translation editor (Settings →
//! Translations…, `src/ui/dialogs/translation_editor.rs`) does with them: one row per English text, the checks while
//! typing, new languages, import and export. A user file holds only what the user changed; the app shows it over the
//! built-in translation (`super::catalog`).

use super::{ENGLISH, extract, languages, placeholders, plural::Plurals, po::{self, Entry, PoFile}, tr};
use std::{collections::HashSet, path::Path};

/// The language a user file is for: `es.po` → `es`. Other files, odd names and English give `None`.
pub fn code_of(path: &Path) -> Option<String> {
    if path.extension()? != "po" { return None }
    let stem = path.file_stem()?.to_str()?;
    valid_code(stem).then(|| stem.to_string())
}

/// A language code: 2–3 letters, then parts of 2–8 letters or digits after `-` (`es`, `pt-BR`, `zh-Hant`,
/// `es-419`). Not English, the language the app is written in.
pub fn valid_code(code: &str) -> bool {
    let mut parts = code.split('-');
    let first = parts.next().unwrap_or_default();
    (2..=3).contains(&first.len()) && first.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
        && !code.eq_ignore_ascii_case(ENGLISH)
}

/// One English text of the app, as the editor shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub ctx: String,
    pub id: String,
    pub plural: Option<String>,
    /// The note for translators from the code.
    pub note: String,
    /// Where it is used (`file:line`).
    pub places: Vec<String>,
    /// The built-in translation, one text per form; empty when there is none.
    pub builtin: Vec<String>,
    /// The built-in translation is only a guess (a native speaker should check it), so the app shows English.
    pub builtin_guess: bool,
    /// What the user typed, when they changed this text.
    pub mine: Option<Vec<String>>,
}

impl Row {
    /// The text in the boxes: the user's, else the built-in one.
    pub fn shown(&self) -> Vec<String> { self.mine.clone().unwrap_or_else(|| self.builtin.clone()) }

    /// What is wrong with the user's text, if anything (it is then not used).
    pub fn problem(&self) -> Option<String> { self.mine.as_ref().and_then(|m| check(&self.id, self.plural.as_deref(), m)) }

    /// The app shows this text translated.
    pub fn translated(&self) -> bool {
        match &self.mine {
            Some(m) => (complete(m) && self.problem().is_none()) || (!self.builtin_guess && complete(&self.builtin)),
            None => !self.builtin_guess && complete(&self.builtin),
        }
    }

    /// Needs a look: a guess from the built-in file, or the user's text has a problem.
    pub fn needs_review(&self) -> bool { (self.mine.is_none() && self.builtin_guess) || self.problem().is_some() }

    /// Where it is used, for people: `settings-language, operator`.
    pub fn where_used(&self) -> String {
        let mut seen = Vec::new();
        for p in self.places.iter().flat_map(|p| p.split_whitespace()) {
            let file = p.rsplit('/').next().unwrap_or(p).split(':').next().unwrap_or(p);
            let name = file.rsplit_once('.').map_or(file, |(n, _)| n).replace('_', "-");
            if !seen.contains(&name) { seen.push(name) }
        }
        seen.join(", ")
    }
}

fn complete(forms: &[String]) -> bool { !forms.is_empty() && forms.iter().all(|f| !f.is_empty()) }

/// What is wrong with a translation, for the red note under the box: a `{gap}` of the English left out or changed,
/// or a plural with some boxes empty. `None` when the app can use it. Empty boxes are not wrong: the English shows.
pub fn check(id: &str, plural: Option<&str>, forms: &[String]) -> Option<String> {
    if forms.iter().all(String::is_empty) { return None }
    if forms.iter().any(String::is_empty) { return Some(tr!("Fill in every box, or this text stays in English.")) }
    let mut source: Vec<String> = placeholders(id);
    if let Some(p) = plural { source.extend(placeholders(p)); source.sort(); source.dedup(); }
    let mut missing = Vec::new();
    let mut extra = Vec::new();
    for f in forms {
        let got = placeholders(f);
        for g in &source {
            // A plural form may leave out the number ("one song").
            if !got.contains(g) && !(plural.is_some() && g == "{n}") && !missing.contains(g) { missing.push(g.clone()) }
        }
        for g in got { if !source.contains(&g) && !extra.contains(&g) { extra.push(g) } }
    }
    if !missing.is_empty() { return Some(tr!("Keep {gaps} exactly as in the English: the app puts a name or number there.", gaps = missing.join(" "))) }
    if !extra.is_empty() { return Some(tr!("{gaps} is not in the English. Remove it, or copy the part in curly brackets exactly.", gaps = extra.join(" "))) }
    None
}

/// For each plural form, the first numbers that use it ("1, 21, 31"), to label its box.
pub fn plural_examples(p: &Plurals) -> Vec<String> {
    (0..p.count).map(|form| {
        let ns: Vec<String> = (0..1000u64).filter(|n| p.index(*n) == form).take(3).map(|n| n.to_string()).collect();
        ns.join(", ")
    }).collect()
}

/// The user's file for one language, as the editor changes it. Only what the user changed is in it.
#[derive(Clone, Debug)]
pub struct Draft { pub code: String, file: PoFile }

impl Draft {
    /// The saved user file for `code`, or a new one. A new file for a language that is not built in gets the whole
    /// header from the list of languages; for a built-in one, the built-in header stands.
    pub fn open(code: &str) -> Draft {
        if let Some(file) = super::user_file(code).and_then(|t| po::parse(&t).ok()) { return Draft { code: code.into(), file } }
        let mut header = vec![("Language".into(), code.to_string()), ("Content-Type".into(), "text/plain; charset=UTF-8".into())];
        if !super::is_builtin(code) && let Some(l) = languages::find(code) {
            header.extend([("Plural-Forms".into(), l.plural_forms.to_string()), ("X-Native-Name".into(), l.native.to_string()),
                ("X-English-Name".into(), l.english.to_string()), ("X-Reviewed".into(), "no".to_string())]);
        }
        Draft { code: code.into(), file: PoFile { header, ..Default::default() } }
    }

    pub fn header(&self, key: &str) -> Option<&str> { self.file.header(key) }

    pub fn set_header(&mut self, key: &str, value: &str) {
        match self.file.header.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
            Some((_, v)) => *v = value.to_string(),
            None => self.file.header.push((key.to_string(), value.to_string())),
        }
    }

    /// How this language says "1 song, 2 songs": the user file's rule, else the built-in one, else English.
    pub fn plurals(&self) -> Plurals {
        match self.file.header("Plural-Forms") {
            Some(h) => Plurals::from_header(Some(h)),
            None => super::catalog(&self.code).map(|c| c.plurals.clone()).unwrap_or_default(),
        }
    }

    /// One row per English text of the app, in the template's order.
    pub fn rows(&self) -> Vec<Row> {
        let builtin = super::builtin_po(&self.code).and_then(|t| po::parse(t).ok()).unwrap_or_default();
        let count = self.plurals().count;
        super::template().iter().map(|t| {
            let key = t.key();
            let b = builtin.entries.iter().find(|e| e.key() == key);
            let usable = b.filter(|e| e.strs.iter().any(|s| !s.is_empty()));
            Row {
                ctx: key.0.clone(),
                id: t.id.clone(),
                plural: t.plural.clone(),
                note: t.extracted.join(" "),
                places: t.refs.clone(),
                builtin: usable.map(|e| e.strs.clone()).unwrap_or_default(),
                builtin_guess: usable.is_some_and(|e| e.fuzzy() || !super::gaps_kept(e)),
                mine: self.file.entries.iter().find(|e| e.key() == key).map(|e| {
                    let mut s = e.strs.clone();
                    if t.plural.is_some() { s.resize(count, String::new()) }
                    s
                }),
            }
        }).collect()
    }

    /// Sets the user's text for an English text (one per plural form). All boxes empty takes it out again.
    pub fn set(&mut self, ctx: &str, id: &str, forms: &[String]) {
        if forms.iter().all(String::is_empty) { return self.reset(ctx, id) }
        let key = (ctx.to_string(), id.to_string());
        let plural = super::template().iter().find(|e| e.key() == key).and_then(|e| e.plural.clone());
        let mut strs = forms.to_vec();
        if plural.is_some() { strs.resize(self.plurals().count, String::new()) } else { strs.truncate(1) }
        let entry = Entry { ctx: (!ctx.is_empty()).then(|| ctx.to_string()), id: id.to_string(), plural, strs, ..Default::default() };
        match self.file.entries.iter_mut().find(|e| e.key() == key) {
            Some(e) => *e = entry,
            None => self.file.entries.push(entry),
        }
    }

    /// Goes back to the built-in translation of an English text.
    pub fn reset(&mut self, ctx: &str, id: &str) {
        let key = (ctx.to_string(), id.to_string());
        self.file.entries.retain(|e| e.key() != key);
    }

    /// "Mark as reviewed": a native speaker read it all.
    pub fn mark_reviewed(&mut self) { self.set_header("X-Reviewed", "yes") }

    /// The file's text.
    pub fn text(&self) -> String { extract::write(&self.file) }

    /// Saves it as `<folder>/<code>.po` and shows the change in the app.
    pub fn save(&self, folder: &Path) -> Result<(), String> {
        let text = self.text();
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        crate::store::write_atomic(&folder.join(format!("{}.po", self.code)), text.as_bytes()).map_err(|e| e.to_string())?;
        super::set_user_file(&self.code, Some(&text)).map_err(|e| e.to_string())
    }
}

/// The whole language as one `.po` file, built-in and user text together, to share or send to the project.
pub fn export(code: &str) -> String {
    let d = Draft::open(code);
    let c = super::catalog(code);
    let name = |f: fn(&super::Catalog) -> String| c.as_deref().map(f).unwrap_or_else(|| code.to_string());
    let plural_forms = d.header("Plural-Forms").map(str::to_string)
        .or_else(|| super::builtin_po(code).and_then(|t| po::parse(t).ok()).and_then(|f| f.header("Plural-Forms").map(str::to_string)))
        .unwrap_or_else(|| "nplurals=2; plural=(n != 1);".into());
    let header = vec![
        ("Project-Id-Version".into(), "RustyPresenter".into()),
        ("Content-Type".into(), "text/plain; charset=UTF-8".into()),
        ("Language".into(), code.to_string()),
        ("Plural-Forms".into(), plural_forms),
        ("X-Native-Name".into(), name(|c| c.native_name.clone())),
        ("X-English-Name".into(), name(|c| c.english_name.clone())),
        ("X-Reviewed".into(), if c.as_deref().is_some_and(|c| c.reviewed) { "yes" } else { "no" }.into()),
    ];
    let count = d.plurals().count;
    let entries = d.rows().into_iter().zip(super::template()).map(|(r, t)| {
        let mut strs = r.shown();
        if strs.is_empty() { strs = vec![String::new(); if r.plural.is_some() { count } else { 1 }] }
        let guess = r.mine.is_none() && r.builtin_guess;
        Entry { strs, flags: if guess { vec!["fuzzy".into()] } else { vec![] }, ..t.clone() }
    }).collect();
    extract::write(&PoFile { header, header_comments: vec![format!("{} for RustyPresenter.", name(|c| c.english_name.clone()))], entries, obsolete: vec![] })
}

/// Reads a shared `.po` into the user's files: its language comes from its header (`Language:`), else `code`.
/// Only texts that differ from the built-in ones are kept. Gives the language's code.
pub fn import(folder: &Path, text: &str, code: Option<&str>) -> Result<String, String> {
    let file = po::parse(text.trim_start_matches('\u{feff}')).map_err(|e| tr!("This file could not be read ({reason}).", reason = e))?;
    let code = file.header("Language").map(|c| c.replace('_', "-")).filter(|c| valid_code(c)).or(code.map(str::to_string))
        .ok_or_else(|| tr!("This file does not say which language it is."))?;
    let mut d = Draft::open(&code);
    for key in ["Plural-Forms", "X-Native-Name", "X-English-Name", "X-Reviewed"] {
        if let Some(v) = file.header(key).filter(|v| !v.is_empty()) { d.set_header(key, v) }
    }
    let rows = d.rows();
    let known: HashSet<(String, String)> = rows.iter().map(|r| (r.ctx.clone(), r.id.clone())).collect();
    for e in file.entries.iter().filter(|e| e.translated() && known.contains(&e.key())) {
        let row = rows.iter().find(|r| r.ctx == e.key().0 && r.id == e.id).expect("known");
        if row.builtin == e.strs && !row.builtin_guess { d.reset(&row.ctx, &row.id) } else { d.set(&row.ctx, &row.id, &e.strs) }
    }
    d.save(folder)?;
    Ok(code)
}

/// "Add a language…": a new user file for `code`, empty (the English shows) or a copy of the language `copy` as the
/// app shows it.
pub fn add_language(folder: &Path, code: &str, native: &str, english: &str, plural_forms: &str, copy: Option<&str>) -> Result<(), String> {
    if !valid_code(code) { return Err(tr!("“{code}” is not a language code. Use letters such as “es”, or “pt-BR” with a country.", code = code)) }
    if super::is_known(code) { return Err(tr!("{language} is already in the list.", language = native)) }
    let plurals = Plurals::parse(plural_forms).map_err(|_| tr!("The plural rule is not right. Copy it from the gettext plural list."))?;
    let mut d = Draft { code: code.into(), file: PoFile::default() };
    for (k, v) in [("Language", code), ("Content-Type", "text/plain; charset=UTF-8"), ("Plural-Forms", plural_forms),
        ("X-Native-Name", native), ("X-English-Name", english), ("X-Reviewed", "no")] { d.set_header(k, v) }
    if let Some(from) = copy.and_then(super::catalog) {
        for t in super::template() {
            let Some(forms) = from.forms(&t.key().0, &t.id) else { continue };
            let mut forms = forms.to_vec();
            if t.plural.is_some() && forms.len() != plurals.count {
                let last = forms.last().cloned().unwrap_or_default();
                forms.resize(plurals.count, last);
            }
            d.set(&t.key().0, &t.id, &forms);
        }
    }
    d.save(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{i18n::{self, testing::reset}, trn};
    use std::path::PathBuf;

    fn folder(files: &[(&str, &str)]) -> PathBuf {
        let dir = crate::store::test_dir().join("translations");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in files { std::fs::write(dir.join(name), text).unwrap() }
        dir
    }

    const ES_MINE: &str = "msgid \"\"\nmsgstr \"Language: es\\n\"\n\nmsgid \"Cancel\"\nmsgstr \"Anular\"\n\nmsgid \"{n} song\"\nmsgid_plural \"{n} songs\"\nmsgstr[0] \"{n} cántico\"\nmsgstr[1] \"{n} cánticos\"\n";
    const XX: &str = "msgid \"\"\nmsgstr \"\"\n\"Language: xx\\n\"\n\"Plural-Forms: nplurals=1; plural=0;\\n\"\n\"X-Native-Name: Ixxa\\n\"\n\"X-English-Name: Testish\\n\"\n\nmsgid \"Cancel\"\nmsgstr \"Xancel\"\n";

    #[test]
    fn user_files_win_over_the_built_in_ones() {
        reset();
        let dir = folder(&[("es.po", ES_MINE)]);
        assert!(i18n::load_user_folder(&dir).is_empty());
        i18n::set_language("es").unwrap();
        assert_eq!(tr!("Cancel"), "Anular", "the user's text wins");
        assert_eq!(tr!("Settings saved"), "Configuración guardada", "the rest stays built in");
        assert_eq!(trn!("{n} song", "{n} songs", 3), "3 cánticos");
        let es = i18n::languages().into_iter().find(|l| l.code == "es").unwrap();
        assert_eq!((es.native_name.as_str(), es.english_name.as_str()), ("Español", "Spanish"), "the built-in header stands");
        let c = i18n::catalog("es").unwrap();
        assert!(c.is_users("", "Cancel") && !c.is_users("", "Settings saved"));
        // A change on disk shows once the folder is read again.
        let before = i18n::revision();
        std::fs::write(dir.join("es.po"), ES_MINE.replace("Anular", "Dejar")).unwrap();
        i18n::load_user_folder(&dir);
        assert_eq!(tr!("Cancel"), "Dejar");
        assert!(i18n::revision() > before);
        let same = i18n::revision();
        i18n::load_user_folder(&dir);
        assert_eq!(i18n::revision(), same, "reading the same files again changes nothing");
    }

    #[test]
    fn a_user_file_for_a_new_code_is_a_new_language() {
        reset();
        assert!(!i18n::is_known("xx"));
        i18n::load_user_folder(&folder(&[("xx.po", XX), ("notes.txt", "hello"), ("English.po", XX)]));
        let xx = i18n::languages().into_iter().find(|l| l.code == "xx").expect("listed");
        assert_eq!((xx.native_name.as_str(), xx.english_name.as_str(), xx.reviewed), ("Ixxa", "Testish", false));
        assert_eq!(xx.percent, 0, "one text of many");
        assert!(i18n::is_known("xx") && i18n::resolve("xx") == "xx");
        i18n::set_language("xx").unwrap();
        assert_eq!(tr!("Cancel"), "Xancel");
        assert_eq!(tr!("Settings saved"), "Settings saved", "untranslated text is English");
        assert_eq!(i18n::system_language_from("xx-YY").as_deref(), Some("xx"));
    }

    #[test]
    fn a_broken_file_is_skipped_with_a_note() {
        reset();
        let dir = folder(&[("de.po", "msgid \"a\"\nmsgid \"b\"\n"), ("xx.po", XX), ("fr.po", "\u{feff}\u{0}")]);
        std::fs::write(dir.join("ko.po"), [0xffu8, 0xfe, 0x00]).unwrap();
        let problems = i18n::load_user_folder(&dir);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems.iter().any(|p| p.starts_with("The translation file de.po could not be read (line 2")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("ko.po") && p.contains("UTF-8")), "{problems:?}");
        assert_eq!(i18n::take_problems(), problems, "kept for the window to show");
        assert!(i18n::take_problems().is_empty());
        assert!(i18n::is_known("xx"), "the good file is read");
        i18n::set_language("de").unwrap();
        assert_ne!(tr!("Cancel"), "Cancel", "German stays built in");
    }

    #[test]
    fn placeholders_are_checked() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(check("Delete “{name}”?", None, &s(&["¿Borrar “{name}”?"])), None);
        assert_eq!(check("Delete “{name}”?", None, &s(&[""])), None, "empty shows the English");
        assert_eq!(check("Delete “{name}”?", None, &s(&["¿Borrar?"])).unwrap(), "Keep {name} exactly as in the English: the app puts a name or number there.");
        assert_eq!(check("Delete “{name}”?", None, &s(&["¿Borrar “{nombre}”?"])).unwrap(), "Keep {name} exactly as in the English: the app puts a name or number there.");
        assert_eq!(check("Saved", None, &s(&["Guardado {x}"])).unwrap(), "{x} is not in the English. Remove it, or copy the part in curly brackets exactly.");
        assert_eq!(check("{n} song", Some("{n} songs"), &s(&["una canción", "{n} canciones"])), None, "a plural form may leave out the number");
        assert_eq!(check("{n} song", Some("{n} songs"), &s(&["una canción", ""])).unwrap(), "Fill in every box, or this text stays in English.");
        assert_eq!(check("Two\nlines", None, &s(&["Dos\nlíneas"])), None);
    }

    #[test]
    fn plural_boxes_are_labelled_with_numbers() {
        let p = |h: &str| Plurals::parse(h).unwrap();
        assert_eq!(plural_examples(&Plurals::default()), ["1", "0, 2, 3"]);
        assert_eq!(plural_examples(&p(languages::find("ru").unwrap().plural_forms)), ["1, 21, 31", "2, 3, 4", "0, 5, 6"]);
        assert_eq!(plural_examples(&p(languages::find("fr").unwrap().plural_forms)), ["0, 1", "2, 3, 4"]);
        assert_eq!(plural_examples(&p(languages::find("ja").unwrap().plural_forms)), ["0, 1, 2"]);
        assert_eq!(plural_examples(&p(languages::find("ar").unwrap().plural_forms)).len(), 6);
    }

    #[test]
    fn drafts_change_save_and_reset_entries() {
        reset();
        let dir = folder(&[]);
        i18n::load_user_folder(&dir);
        i18n::set_language("ru").unwrap();
        let mut d = Draft::open("ru");
        let rows = d.rows();
        assert_eq!(rows.len(), i18n::template().len());
        let song = rows.iter().find(|r| r.id == "{n} song").unwrap();
        assert_eq!(song.builtin.len(), 3, "Russian has three forms");
        assert!(song.mine.is_none() && song.translated());
        d.set("", "{n} song", &["{n} гимн".into(), "{n} гимна".into(), "{n} гимнов".into()]);
        d.set("", "Cancel", &["Отбой\nсейчас".into()]);
        d.save(&dir).unwrap();
        assert_eq!(trn!("{n} song", "{n} songs", 5), "5 гимнов", "shown at once");
        assert_eq!(tr!("Cancel"), "Отбой\nсейчас", "line breaks kept");
        let saved = std::fs::read_to_string(dir.join("ru.po")).unwrap();
        assert!(saved.contains("msgstr[2] \"{n} гимнов\"") && !saved.contains("Configuración"), "{saved}");
        // Read back from disk, as at the next start.
        reset();
        i18n::load_user_folder(&dir);
        i18n::set_language("ru").unwrap();
        assert_eq!(tr!("Cancel"), "Отбой\nсейчас");
        let mut d = Draft::open("ru");
        assert_eq!(d.rows().iter().find(|r| r.id == "Cancel").unwrap().mine.as_deref(), Some(&["Отбой\nсейчас".to_string()][..]));
        // A broken text is saved but not used; reset goes back to the built-in one.
        d.set("", "Cancel", &["Отбой {x}".into()]);
        d.save(&dir).unwrap();
        let row = d.rows().into_iter().find(|r| r.id == "Cancel").unwrap();
        assert!(row.problem().is_some() && row.needs_review());
        let builtin = row.builtin[0].clone();
        assert_eq!(tr!("Cancel"), builtin, "the broken one is not used");
        d.reset("", "Cancel");
        d.mark_reviewed();
        d.save(&dir).unwrap();
        assert!(d.rows().iter().find(|r| r.id == "Cancel").unwrap().mine.is_none());
        assert!(i18n::languages().iter().find(|l| l.code == "ru").unwrap().reviewed, "X-Reviewed: yes");
    }

    #[test]
    fn export_and_import_round_trip() {
        reset();
        let dir = folder(&[("es.po", ES_MINE)]);
        i18n::load_user_folder(&dir);
        let mut d = Draft::open("es");
        d.set("", "Settings saved", &["Todo guardado".into()]);
        d.save(&dir).unwrap();
        let text = export("es");
        let file = po::parse(&text).unwrap();
        assert_eq!((file.header("Language"), file.header("X-Native-Name")), (Some("es"), Some("Español")));
        assert_eq!(file.entries.len(), i18n::template().len(), "every text, for the project");
        let shown = |code: &str| { let c = i18n::catalog(code).unwrap(); i18n::template().iter().map(|t| c.forms(&t.key().0, &t.id).map(<[String]>::to_vec)).collect::<Vec<_>>() };
        let before = shown("es");

        // Into a fresh data folder: the same text shows, and only the changes are in the user file.
        reset();
        let other = folder(&[]);
        i18n::load_user_folder(&other);
        assert_eq!(import(&other, &text, None).unwrap(), "es");
        assert_eq!(shown("es"), before);
        let mine = po::parse(&std::fs::read_to_string(other.join("es.po")).unwrap()).unwrap();
        let mut ids: Vec<&str> = mine.entries.iter().map(|e| e.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["Cancel", "Settings saved", "{n} song"]);

        // A new language with no Language line takes the code it is given.
        let bare = XX.replace("\"Language: xx\\n\"\n", "");
        assert_eq!(import(&other, &bare, None).unwrap_err(), "This file does not say which language it is.");
        assert_eq!(import(&other, &bare, Some("yy")).unwrap(), "yy");
        assert_eq!(i18n::catalog("yy").unwrap().forms("", "Cancel"), Some(&["Xancel".to_string()][..]));
        assert!(import(&other, "msgid \"a\"\nmsgid \"b\"", Some("yy")).unwrap_err().starts_with("This file could not be read (line 2"));
    }

    #[test]
    fn add_a_language_empty_or_as_a_copy() {
        reset();
        let dir = folder(&[]);
        i18n::load_user_folder(&dir);
        let pl = languages::find("pl").unwrap();
        add_language(&dir, pl.code, pl.native, pl.english, pl.plural_forms, None).unwrap();
        let info = i18n::languages().into_iter().find(|l| l.code == "pl").expect("listed");
        assert_eq!((info.native_name.as_str(), info.percent), ("Polski", 0));
        assert_eq!(add_language(&dir, "pl", "Polski", "Polish", pl.plural_forms, None).unwrap_err(), "Polski is already in the list.");
        assert!(add_language(&dir, "not a code", "x", "x", pl.plural_forms, None).is_err());
        assert!(add_language(&dir, "qq", "x", "x", "nplurals=; plural=", None).is_err());
        // A copy of Russian (three forms) for Belarusian (three forms).
        let be = languages::find("be").unwrap();
        add_language(&dir, be.code, be.native, be.english, be.plural_forms, Some("ru")).unwrap();
        let (ru, be) = (i18n::catalog("ru").unwrap(), i18n::catalog("be").unwrap());
        assert_eq!(be.forms("", "{n} song"), ru.forms("", "{n} song"));
        assert!(be.translated() > 1000, "{}", be.translated());
        assert_eq!(be.native_name, "Беларуская");
    }
}
