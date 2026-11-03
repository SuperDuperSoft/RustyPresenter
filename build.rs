fn main() {
    // Element ids stay in the generated code so the testing backend can find elements by id (check C4).
    let slint = slint_build::CompilerConfiguration::new().with_debug_info(true);
    slint_build::compile_with_config("ui/app.slint", slint).expect("slint build failed");
    #[cfg(windows)]
    windows_resources();
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
