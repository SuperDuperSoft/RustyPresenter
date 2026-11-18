//! For the phone (`--features phone`, as `build-apk.ps1` builds it): the screens (`ui/app.slint`) with their words
//! in every language (`translations/`), and the phone's camera library. Without it only the parts that need no
//! phone are built, so `cargo test` on any computer stays small.
fn main() {
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=translations");
    let android = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android");
    // The screens alone, drawn on this computer (`examples/preview.rs`).
    #[cfg(feature = "preview")]
    if !android { screens() }
    #[cfg(feature = "phone")]
    if android {
        screens();
        println!("cargo:rustc-link-lib=camera2ndk");
    }
    #[cfg(not(feature = "phone"))]
    if android { panic!("build the phone app with --features phone (build-apk.ps1 does)") }
}

/// The screens, their words looked up by the English text alone (as the desktop app's catalogs are), so the same
/// text has one translation wherever it is used.
#[cfg(any(feature = "phone", feature = "preview"))]
fn screens() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = slint_build::CompilerConfiguration::new()
        .with_style("material-dark".into())
        .with_bundled_translations(dir.join("translations"))
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config).expect("the screens");
}
