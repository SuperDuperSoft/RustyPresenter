# i-slint-backend-android-activity 1.18.1, patched for RustyPresenter's Android remote

Two changes to the crate as published on crates.io (`.cargo-ok`, `Cargo.lock` and `Cargo.toml.orig` left out):

1. **Drawn with Slint's software renderer, not Skia.** The published backend draws only with Skia
   (`i-slint-renderer-skia`), whose build downloads a prebuilt Skia library from GitHub (rust-skia's releases), not
   from crates.io. `androidwindowadapter.rs` now keeps a `SoftwareRenderer` (`RepaintBufferType::NewBuffer`: each
   buffer Android hands out is drawn whole), asks the window for 8-bit RGBA buffers when it appears, and on each
   redraw locks the window's next buffer (`ANativeWindow_lock`) and draws into it (RGB565 too, if a phone insists).
   `Cargo.toml` depends on `i-slint-renderer-software` instead of `i-slint-renderer-skia` (the `unstable-wgpu-*`
   features do nothing now); `lib.rs` no longer imports `SkiaRendererExt`. No GPU or graphics driver is involved, so
   it draws the same on every phone; the remote's screens are simple enough for that to be quick.
2. **Its Java helper builds with JDK 21 and newer.** `build.rs` compiles `java/SlintAndroidJavaHelper.java` into a
   `classes.dex`. JDK 21+ `javac` writes a `MethodParameters` attribute with unnamed parameters for inner-class
   constructors, which the `d8` of build-tools 34 and older stops on ("Cannot invoke String.length() because
   <parameter1> is null", Slint issue 4973). `build.rs` takes those attributes out of the compiled classes before
   `d8` reads them (`strip_method_parameters`); the helper never reads its parameters' names.

Remove or redo this patch (the `[patch.crates-io]` line in `apps/android/Cargo.toml`) when moving to another Slint
version.
