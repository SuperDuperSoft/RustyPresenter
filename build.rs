fn main() {
    i18n_bundle();
    // Element ids stay in the generated code so the testing backend can find elements by id (check C4).
    // `@tr` text is looked up in `translations/<lang>/LC_MESSAGES/rustypresenter.po`, with no context unless the
    // `.slint` gives one (I18N spec D2).
    let slint = slint_build::CompilerConfiguration::new()
        .with_debug_info(true)
        .with_bundled_translations("translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", slint).expect("slint build failed");
    #[cfg(windows)]
    windows_resources();
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
