//! `translations/` stays in step with the code (I18N spec D10): the template matches the texts in the code, and every
//! language's `.po` reads, uses only the template's texts, keeps their gaps and has the right number of plural forms.
use rustypresenter::i18n::{self, extract, plural::Plurals, po};
use std::{collections::HashSet, path::{Path, PathBuf}};

fn root() -> &'static Path { Path::new(env!("CARGO_MANIFEST_DIR")) }

fn read(p: &Path) -> String { std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display())) }

/// Every `translations/<code>/LC_MESSAGES/rustypresenter.po`, by code.
fn po_files() -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(root().join("translations")).unwrap().filter_map(Result::ok)
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().join("LC_MESSAGES/rustypresenter.po")))
        .filter(|(_, p)| p.is_file())
        .collect();
    out.sort();
    out
}

#[test]
fn i18n_pot_is_current() {
    let msgs = extract::collect(root()).unwrap();
    let want = extract::pot(&msgs);
    let have = read(&root().join("translations/rustypresenter.pot"));
    assert!(have == want, "translations/rustypresenter.pot is out of date: run `cargo run --example i18n_extract`");
}

#[test]
fn every_po_is_valid() {
    let pot = po::parse(&read(&root().join("translations/rustypresenter.pot"))).expect("the template reads");
    let keys: HashSet<(String, String)> = pot.entries.iter().map(po::Entry::key).collect();
    let files = po_files();
    assert!(files.iter().any(|(c, _)| c == "es"), "Spanish is there");
    for (code, path) in files {
        let text = read(&path);
        let f = po::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(f.header("Language"), Some(code.as_str()), "{code}: the Language header names the folder");
        for h in ["X-Native-Name", "X-English-Name", "X-Reviewed"] {
            assert!(f.header(h).is_some_and(|v| !v.is_empty()), "{code}: the header needs {h}");
        }
        let forms = f.header("Plural-Forms").unwrap_or_else(|| panic!("{code}: the header needs Plural-Forms"));
        let plurals = Plurals::parse(forms).unwrap_or_else(|e| panic!("{code}: Plural-Forms: {e}"));
        for e in &f.entries {
            let at = format!("{code} line {}", e.line);
            assert!(keys.contains(&e.key()), "{at}: {:?} is not in the template (run the extractor)", e.id);
            if e.plural.is_some() {
                assert_eq!(e.strs.len(), plurals.count, "{at}: {:?} needs {} plural forms", e.id, plurals.count);
            }
            if e.translated() {
                assert!(i18n::gaps_kept(e), "{at}: the translation of {:?} must keep its gaps {:?}", e.id, i18n::placeholders(&e.id));
            }
        }
    }
}

#[test]
fn slint_has_no_bare_user_text() {
    let allow = extract::Allow::parse(&read(&root().join("translations/allow-bare.txt")));
    let mut files = Vec::new();
    collect_slint(&root().join("ui"), &mut files);
    files.sort();
    let mut bad = Vec::new();
    for f in files {
        let rel = f.strip_prefix(root()).unwrap().to_string_lossy().replace('\\', "/");
        // TODO(i18n sweep): the dialogs are swept on their own branch; check them too once it is merged.
        if rel.starts_with("ui/dialogs/") { continue }
        for b in extract::bare_texts(&read(&f)) {
            if !allow.allows(&rel, &b.text) { bad.push(format!("{rel}:{} {}: \"{}\"", b.line, b.property, b.text)) }
        }
    }
    assert!(bad.is_empty(), "Use @tr(...) for these (or add symbols and names to translations/allow-bare.txt):\n{}", bad.join("\n"));
}

#[test]
fn allow_list_reads() {
    let allow = extract::Allow::parse(&read(&root().join("translations/allow-bare.txt")));
    assert!(allow.files.iter().all(|f| root().join(f).is_file()), "every file in allow-bare.txt exists");
}

fn collect_slint(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { collect_slint(&p, out) } else if p.extension().is_some_and(|x| x == "slint") { out.push(p) }
    }
}
