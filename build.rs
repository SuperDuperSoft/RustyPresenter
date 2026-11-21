fn main() {
    i18n_bundle();
    youversion_key();
    // The screens (`ui/*.slint`) are compiled by `crates/rp-ui`.
    #[cfg(windows)]
    windows_resources();
}

/// The YouVersion key that comes with the app (`bible::sources::BUILT_IN_YOUVERSION`): from `RP_YOUVERSION_KEY`, else
/// from `youversion.key` beside `Cargo.toml`. Neither is in the repo. Without one the app is built with no key.
fn youversion_key() {
    println!("cargo:rerun-if-env-changed=RP_YOUVERSION_KEY");
    println!("cargo:rerun-if-changed=youversion.key");
    let file = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("youversion.key");
    let key = std::env::var("RP_YOUVERSION_KEY").ok().or_else(|| std::fs::read_to_string(file).ok()).map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
    match key {
        Some(k) => println!("cargo:rustc-env=RP_YOUVERSION_KEY={k}"),
        None if std::env::var("PROFILE").as_deref() == Ok("release") => println!("cargo:warning=no YouVersion key (RP_YOUVERSION_KEY or youversion.key): this build asks each church for its own"),
        None => {}
    }
}

/// `$OUT_DIR/i18n_bundle.rs`: every `translations/<code>/LC_MESSAGES/rustypresenter.po` and the template, for
/// `crate::i18n` (the Rust side reads the same files as Slint).
fn i18n_bundle() {
    use std::{fmt::Write, path::PathBuf};
    println!("cargo:rerun-if-changed=translations");
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("translations");
    let mut langs: Vec<(String, PathBuf)> = std::fs::read_dir(&root).expect("translations folder").filter_map(Result::ok)
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().join("LC_MESSAGES").join("rustypresenter.po")))
        .filter(|(_, p)| p.is_file())
        .collect();
    langs.sort();
    let mut out = String::from("pub static BUNDLE: &[(&str, &str)] = &[\n");
    for (code, path) in &langs {
        println!("cargo:rerun-if-changed={}", path.display());
        writeln!(out, "    ({code:?}, include_str!({:?})),", path.display().to_string()).unwrap();
    }
    let pot = root.join("rustypresenter.pot");
    println!("cargo:rerun-if-changed={}", pot.display());
    writeln!(out, "];\npub static POT: &str = include_str!({:?});", pot.display().to_string()).unwrap();
    std::fs::write(PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("i18n_bundle.rs"), out).unwrap();
}

/// The icon and the manifest. Common Controls v6 is required (rfd's dialogs import TaskDialogIndirect), so this goes
/// into every program this package links: a test binary without it does not start in a debug build. Rust's std on
/// windows-gnu embeds its own manifest too; the linker keeps this one (and warns ".rsrc merge failure: multiple
/// non-default manifests"), so this one repeats std's settings.
#[cfg(windows)]
fn windows_resources() {
    println!("cargo:rerun-if-changed=packaging/windows/rustypresenter.manifest");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    embed_resource::compile_for_everything("packaging/windows/rustypresenter.rc", embed_resource::NONE).manifest_required().expect("windows resources failed");
}
