//! Brings `translations/` in step with the code: writes `translations/rustypresenter.pot` from every `@tr(...)` in
//! `ui/` and every `tr!`/`trn!`/`trc!` in `src/`, then merges it into each language's `.po` (new texts untranslated,
//! old ones kept as `#~`). Run from the project folder: `cargo run --example i18n_extract` (or give another copy of
//! the project's folder as the first argument).
use rustypresenter::i18n::extract;
use std::path::PathBuf;

fn main() {
    let root = std::env::args_os().nth(1).map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    let root = root.as_path();
    let msgs = extract::collect(root).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1) });
    let pot = extract::pot(&msgs);
    let tr = root.join("translations");
    std::fs::write(tr.join("rustypresenter.pot"), &pot).expect("write the template");
    println!("translations/rustypresenter.pot: {} texts", msgs.len());
    let mut langs: Vec<_> = std::fs::read_dir(&tr).expect("translations folder").filter_map(Result::ok)
        .map(|e| e.path().join("LC_MESSAGES/rustypresenter.po")).filter(|p| p.is_file()).collect();
    langs.sort();
    for path in langs {
        let old = std::fs::read_to_string(&path).expect("read a translation");
        match extract::merge(&old, &pot) {
            Ok(new) => {
                if new != old { std::fs::write(&path, new).expect("write a translation") }
                println!("{}: up to date", path.strip_prefix(root).unwrap_or(&path).display());
            }
            Err(e) => { eprintln!("{}: {e}", path.display()); std::process::exit(1) }
        }
    }
}
