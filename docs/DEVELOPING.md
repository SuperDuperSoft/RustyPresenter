# Developing RustyPresenter

This page is for people who build RustyPresenter or change its code. To use the app, start with the [README](../README.md).

## Start it from the source code

Double-click **`RustyPresenter.cmd`**. The first start builds the app (a few minutes); after that it opens in a few seconds. Closing the main window quits RustyPresenter and turns off the room and stage screens.

## Make the Windows installer yourself

On the PC that builds RustyPresenter, double-click **`make-installer.cmd`** in the RustyPresenter folder (the one with `.tools` in it). It builds the newest version the same way as `try-latest.cmd`, downloads the installer maker (NSIS) and the video and PDF libraries into `.tools\installer` the first time, checks every download against `packaging\pins.env`, and puts `RustyPresenter-Setup-<version>.exe` on your Desktop. It needs Git for Windows and the tools in `.tools`; no Visual Studio. The first run takes 10 to 15 minutes, later runs a few.

- If it stops with "The installer was NOT made yet", a download is new and its fingerprint is not in the code yet: send `.tools\xfer\make-installer\hashes.txt` to whoever maintains the code, and run it again once they have added it.
- Everything it did is written to `.tools\xfer\make-installer\installer.txt` (and the build to `build.txt` next to it).
- `make-installer.cmd --dry-run` (typed in a Command Prompt in that folder) only lists the steps.

## The YouVersion key

The released app comes with a YouVersion key, so NIV and the YouVersion Bibles work for every church with no sign-up. The key is not in the source code. `build.rs` takes it, when the app is built, from the environment variable `RP_YOUVERSION_KEY`, or from a file named `youversion.key` beside `Cargo.toml` (git ignores it). The release workflow passes the repository secret of the same name.

A build without the key works; YouVersion then asks each church for its own free key in Settings → Bible, like the other services.

## How it is built

- The app is native: a Rust core with a local server for the phone remote, and Slint windows for the operator, room and stage, with video and sound through libmpv.
- Building needs `.tools\cargo` + `.tools\rustup` (the `stable-x86_64-pc-windows-gnu` toolchain) and `.tools\mingw64` (WinLibs MinGW-w64 GCC 16.2.0 UCRT, from github.com/brechtsanders/winlibs_mingw). The scripts put MinGW first on PATH and use its `dlltool`.
- `dev-build.cmd` builds the app and runs the tests, writing everything to `build.log`.
- The app needs `libmpv-2.dll` next to the exe, in `%LOCALAPPDATA%\RustyPresenter\deps\mpv\`, or named by `RP_LIBMPV`; without it everything but video and audio works. `RP_FORCE_SOFTWARE=1` draws without the GPU, and `RP_RENDERER` names any Slint renderer; both beat Settings → Video → Graphics. Vulkan, Direct3D (12) and Metal are offered only in builds with the `gpu-apis` feature (FemtoVG on wgpu, pure Rust, builds with MinGW; not on by default yet) or `renderer-skia`. Crashes are written to `logs\native-YYYYMMDD.log` in the data folder.
- `dev-watch.cmd` waits for `build.request`, `run.request` or `stop.request` files and acts on them. It lets a remote collaborator build on this PC. Close the window to stop it.
- `scripts/licenses.sh` remakes `THIRD-PARTY-LICENSES.txt`. Run it after changing `Cargo.lock` or `packaging/licenses/`.
- To add or fix a language (no programming needed), see [docs/TRANSLATING.md](docs/TRANSLATING.md).
- The design is in `docs/superpowers/specs/`. The work board is `PROJECT_PLAN.md`.

## libmpv

You may use your own build of libmpv: set `RP_LIBMPV` to its path, or replace the file next to the program (on macOS, in `Contents/Frameworks`).
