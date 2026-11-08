//! Compiles `ui/app.slint` (at the top of the repository) into Rust for `rp_ui`.
fn main() {
    // Slint names the translation domain (and the .po file it bundles) after the package: keep the app's name.
    // SAFETY: the build script is one thread; nothing else reads the environment at the same time.
    unsafe { std::env::set_var("CARGO_PKG_NAME", "rustypresenter") };
    // Element ids stay in the generated code so the testing backend can find elements by id (check C4).
    // `@tr` text is looked up in `translations/<lang>/LC_MESSAGES/rustypresenter.po`, with no context unless the
    // `.slint` gives one (I18N spec D2).
    let slint = slint_build::CompilerConfiguration::new()
        .with_debug_info(true)
        .with_bundled_translations("../../translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("../../ui/app.slint", slint).expect("slint build failed");
}
