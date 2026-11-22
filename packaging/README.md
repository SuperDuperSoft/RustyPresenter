# Packaging

RustyPresenter ships as a Windows NSIS installer, a macOS `.app` in a `.dmg` (Apple
silicon only), a Linux AppImage and a Linux `.deb`, plus a portable copy for each OS.
[cargo-packager] 0.11.8 builds the installers from the root `Packager.toml`.
`packaging/stage.sh` puts the native libraries where that file expects them first, and
`packaging/release-files.sh` names the files and makes the portable copies.

## Release files

Every release (`v*` tag, `.github/workflows/package.yml`) has these, `<version>` being `Packager.toml`'s:

| File | What it is |
|---|---|
| `RustyPresenter-<version>-windows-setup.exe` | Windows 10/11 installer, per user, no administrator password. Data in `%LOCALAPPDATA%\RustyPresenter` |
| `RustyPresenter-<version>-windows-portable.zip` | Windows, no install: a `RustyPresenter` folder with `rustypresenter.exe`, `libmpv-2.dll`, `pdfium.dll`, the licences, `portable.txt` and `HOW-TO-START.txt`. Unzip, double-click the exe |
| `RustyPresenter-<version>-macos.dmg` | Mac with Apple silicon, macOS 13 or later: drag RustyPresenter into Applications. Data in `~/Library/Application Support/RustyPresenter` |
| `RustyPresenter-<version>-macos-portable.zip` | Mac, no install: a `RustyPresenter` folder with `RustyPresenter.app`, `portable.txt` beside it, `HOW-TO-START.txt` and the licences (made with `ditto -c -k --keepParent`, so the signed bundle stays whole) |
| `RustyPresenter-<version>-linux.AppImage` | Linux (64-bit PC): one file, nothing to install (make it executable, double-click). Data in `~/.local/share/RustyPresenter`, or beside it with a `portable.txt` beside the AppImage |
| `RustyPresenter-<version>-linux.deb` | Ubuntu/Debian package (uses the distro's `libmpv2`) |
| `RustyPresenter-<version>-linux-portable.tar.gz` | Linux, no install: a `RustyPresenter` folder with `rustypresenter`, `libmpv.so.2` (our LGPL build), `libpdfium.so`, the launcher `RustyPresenter.sh` (puts the folder on `LD_LIBRARY_PATH`), `portable.txt`, `HOW-TO-START.txt` and the licences |
| `THIRD-PARTY-LICENSES.txt`, `libmpv-sources-r<N>.tar.xz` | the licences, and the libmpv/FFmpeg source (LGPL-2.1 §6(d)) |

**Portable copies.** A file `portable.txt` beside the program (beside `rustypresenter.exe`, beside
`RustyPresenter.app`, beside the AppImage) makes the app keep all its data (songs, services, settings,
`settings-window.json`, logs, backups, assets, downloaded models, FFmpeg and Bibles) in the folder
`RustyPresenter Data` beside it (`src/platform/data_folder.rs`, which every part of the app asks through
`server::data_dir`). `RUSTYPRESENTER_DATA` still wins. When that folder cannot be written to, when the app runs from
inside a zip (Windows' `Temp1_*.zip` folder), or when macOS runs a translocated copy (a downloaded app Finder has
not moved, run from `.../AppTranslocation/...`, where the folder beside it cannot be seen), the data goes to the
per-user folder for that run and a message box says why and what to do. Stream keys stay in each computer's
password store (Credential Manager, Keychain, Secret Service), and recordings go to the computer's Videos folder
unless Settings names another, as the `HOW-TO-START` texts say. The texts are in `packaging/portable/`.

`packaging/test-release-files.sh` tests `release-files.sh` with fake builds, `zip` and `ditto` (CI `packaging`
job). `packaging/windows/make-portable.cmd` makes the same Windows zip on a PC from try-latest's build
(`%LOCALAPPDATA%\RustyPresenter\build-check\release`) and the PC's own libmpv and pdfium in
`%LOCALAPPDATA%\RustyPresenter\deps`, onto the Desktop.

The packaged binary is `rustypresenter`.

## Files

| Path | What it is |
|---|---|
| `Packager.toml` | cargo-packager config, camelCase keys, version kept equal to `Cargo.toml` (CI checks it) |
| `packaging/pins.env` | every pinned version and checksum: cargo-packager, the libmpv release, pdfium |
| `packaging/stage.sh` | fetches, verifies and stages libmpv and pdfium for one OS |
| `packaging/icons/` | 32, 128, 256 and 512 PNGs and `icon.ico`, made from `icons/icon.ico` (its largest image is 256 px, so the 512 PNG is scaled up). `icon.icns` is generated on macOS by `stage.sh` and not committed |
| `packaging/libmpv/` | the LGPL libmpv build scripts (Plan 4 Task 16) |
| `packaging/windows/installer.nsi` | cargo-packager 0.11.8's NSIS template with the per-user install folder moved (see below) |
| `make-installer.cmd` (repo root) | builds the setup file on a Windows PC with MinGW, no cargo-packager (see "Windows installer on a PC") |
| `packaging/release-files.sh` | the release names of the packages, and the portable zip/tar.gz for one OS (see "Release files") |
| `packaging/portable/` | `portable.txt` (the marker), `HOW-TO-START-<os>.txt` and the Linux launcher `RustyPresenter.sh` |
| `packaging/windows/make-portable.cmd` | the Windows portable zip on a PC, from try-latest's build |
| `packaging/windows/make-installer.sh` | what `make-installer.cmd` runs in Git Bash: downloads, checks, licences, staging, makensis |
| `packaging/windows/rustypresenter.nsi` | the plain NSIS script it compiles |
| `packaging/windows/test-make-installer.sh`, `check-nsi.py` | tests of both that run without Windows (CI `packaging` job) |
| `packaging/macos/entitlements.plist` | disables library validation so the ad-hoc signed app can load its dylibs |
| `packaging/licenses/` | licence texts appended to `THIRD-PARTY-LICENSES.txt` (Plan 4 Task 14) |

Generated, never committed: `packaging/native/`, `packaging/native-macos/`,
`packaging/native-licenses/`, `packaging/out/` (downloads and unpacked archives),
`packaging/icons/icon.icns` and `dist/` (the packages).

## Staging layout

`stage.sh <os>` downloads the pinned libmpv artifact (`libmpv-r<N>` release of this repo)
and the pinned `bblanchon/pdfium-binaries` archive into `packaging/out/downloads/`, checks
both sha256 values from `pins.env`, and refuses a libmpv whose `BUILDINFO.txt` lacks
`-Dgpl=false`. It also checks that the committed `THIRD-PARTY-LICENSES.txt` holds the LGPL
text; it never regenerates it (the CI licence job keeps it current).

| OS | `packaging/native/` (resources) | elsewhere | Installed at |
|---|---|---|---|
| Windows | `libmpv-2.dll`, `pdfium.dll` | — | next to `rustypresenter.exe` |
| macOS | `NATIVE-LIBS.txt` only | `packaging/native-macos/libmpv.2.dylib`, `libpdfium.dylib` (Packager.toml `frameworks`) | `RustyPresenter.app/Contents/Frameworks/` |
| Linux `.deb` | `libpdfium.so` | libmpv comes from the distro package `libmpv2` | `/usr/lib/rustypresenter/` |
| Linux AppImage | `libpdfium.so` | `packaging/out/libmpv-linux/libmpv.so.2`, installed into `/usr/lib/x86_64-linux-gnu` on the build machine so the `appimage.libs` entry bundles it | `$APPDIR/usr/lib/rustypresenter/`, libmpv in `$APPDIR/usr/lib/x86_64-linux-gnu/` (see below) |

At run time `Mpv::candidates` and `pdfium_candidates` (Plan 4 Task 1) look in the resource
folder, `Contents/Frameworks`, next to the exe, `<data>/deps/` and the system, so every
layout works without a feature flag.

Every OS also gets `packaging/native-licenses/`: the notices from both archives
(libmpv's `COPYING.*`, `Copyright`, `BUILDINFO.txt` and `licenses/*`, pdfium's `LICENSE` and
`licenses/*`), prefixed `libmpv-` or `pdfium-` and installed under `licenses/` next to
`THIRD-PARTY-LICENSES.txt`. Both libraries statically contain other projects (dav1d,
FreeType, HarfBuzz, libass, winpthreads; abseil, ICU, libjpeg-turbo, ...) whose licences
require their notice in every binary copy. `stage.sh` stops if an expected notice is missing.

The AppImage's libmpv is found by name, not by path: cargo-packager copies it with
`cp --parents`, so it lands in `$APPDIR/usr/lib/x86_64-linux-gnu/`, a folder
`Mpv::candidates` does not list. The bare-name `dlopen("libmpv.so.2")` at the end of the
candidates finds it through the `LD_LIBRARY_PATH` that the AppImage's `AppRun` exports.

On Windows the installer is per user and needs no admin rights. cargo-packager's stock
template would install to `%LOCALAPPDATA%\RustyPresenter`, which is the app's data folder
(`data.json`, `assets`, `backups`, `deps`). `packaging/windows/installer.nsi` changes only
that default, to `%LOCALAPPDATA%\Programs\RustyPresenter`, so uninstalling or deleting the
program never touches the library. Re-copy the template when bumping cargo-packager.

Notes on the macOS layout:

- The dylibs are not resources, because code in `Contents/Resources` breaks
  `codesign --verify --strict`. `NATIVE-LIBS.txt` exists only so the `packaging/native/*`
  glob always matches a file.
- `dylibbundler -od -b -x libmpv.2.dylib -d packaging/native-macos -p @loader_path/` runs on
  a copy (`-od` empties the target folder first). If it bundles any extra dylib, `stage.sh`
  stops: add it to `macos.frameworks` and to the check in `stage.sh`.
- `stage.sh` checks with `otool` that both dylibs link only system libraries and each other,
  and that their minimum macOS is 12.0 or lower.
- cargo-packager signs with the hardened runtime, whose library validation refuses dylibs
  that lack the app's Team ID, and an ad-hoc signature has none. `entitlements.plist` turns
  library validation off; `mac-finish` signs the same way. Check on a Mac that Settings →
  About finds libmpv and pdfium.

## Building a package

From the repository root, after `cargo install cargo-packager --locked --version 0.11.8`:

```sh
packaging/stage.sh windows        # or macos, linux
cargo packager --release -c Packager.toml -f nsis          # Windows
cargo packager --release -c Packager.toml -f app,dmg       # macOS (MACOSX_DEPLOYMENT_TARGET=12.0)
sudo install -m 0644 packaging/out/libmpv-linux/libmpv.so.2 /usr/lib/x86_64-linux-gnu/ && sudo ldconfig
cargo packager --release -c Packager.toml -f appimage,deb  # Linux
packaging/linux/deb-recommends.sh dist/*.deb               # Linux: Recommends: pulseaudio-utils
```

- Always pass `-c Packager.toml`. Without it cargo-packager searches the whole tree for
  `packager.toml` files and silently skips one it cannot parse.
- `beforePackagingCommand` runs `cargo build --release --locked`, and the binary is read from
  `target/release`. cargo-packager 0.11.8 ignores `--binaries-dir`, so unset
  `CARGO_TARGET_DIR` while packaging (or the build lands somewhere it does not look).
- If cargo-packager's ad-hoc signing fails on macOS, package with `-f app` and run
  `packaging/stage.sh mac-finish` (ad-hoc `codesign --deep`, verify, `hdiutil` dmg).

CI does all of this in `.github/workflows/package.yml` on `v*` tags. The LGPL libmpv
itself comes from `.github/workflows/libmpv.yml`.

## Windows installer on a PC

`make-installer.cmd` makes `RustyPresenter-Setup-<version>.exe` on the Desktop of a PC that builds with the MinGW
toolchain in `.tools` (no MSVC, no cargo-packager). It builds the release exe exactly like `try-latest.cmd`
(the branch in `.tools\xfer\latest-branch.txt`, checked out in `%USERPROFILE%\rp-wt\windows-check`, built in
`%LOCALAPPDATA%\RustyPresenter\build-check`; run from any other checkout, it builds that checkout), then runs
`packaging/windows/make-installer.sh` in Git Bash:

1. Downloads into `.tools\installer\downloads`, once: the NSIS zip (`NSIS_URL`, SourceForge), pdfium
   (`PDFIUM_TAG`), and libmpv: the LGPL release `libmpv-r<N>` when `LIBMPV_SHA256_WINDOWS` is pinned, otherwise the
   PC's own `%LOCALAPPDATA%\RustyPresenter\deps\mpv\libmpv-2.dll` plus mpv's `Copyright` from `MPV_COPYRIGHT_URL`.
   No libmpv at all still makes an installer, without video. Every file is checked against `pins.env`.
2. A pin that says `TODO` is not trusted. The file goes to `downloads\unverified\`, its sha256 and sha1 to
   `.tools\xfer\make-installer\hashes.txt`, and the run stops (exit 3) before building. Compare them with the
   publisher (SourceForge lists sha1 for NSIS; for mpv's `Copyright`, compare the text), then pin the sha256.
3. Fills the placeholders `packaging/licenses/mpv-Copyright.txt` and `pdfium-LICENSE.txt` in the tree it built from
   with the real texts, and copies them to `.tools\xfer\make-installer\licenses\` to be committed (then run
   `scripts/licenses.sh`). `FFmpeg-COPYING.LGPLv2.1.txt` is committed already; the release zip's copy wins when there is one.
4. Stages `rustypresenter.exe`, `pdfium.dll`, `libmpv-2.dll`, every MinGW runtime DLL the exe imports (found with
   `objdump`; outside the build window `.tools\mingw64\bin` is not on the PATH), `LICENSE-*` and `licenses\`
   (`THIRD-PARTY-LICENSES.txt`, the real texts above, the libraries' own notices as `stage.sh` names them).
5. Writes the File and Delete lists and runs `makensis` on `rustypresenter.nsi`.

The installer is per user with no admin rights, in `%LOCALAPPDATA%\Programs\RustyPresenter`, with Start menu and
desktop shortcuts and an entry in Settings → Apps under the same uninstall key as cargo-packager's, so the two
builds upgrade each other. Running a newer one upgrades in place (it refuses while the app is open). The
uninstaller deletes only the files it installed; the data folder `%LOCALAPPDATA%\RustyPresenter` goes only when its
box, unticked by default, is ticked, and never in a silent (`/S`) uninstall. The program folder may not be the data
folder.

`make-installer.cmd --dry-run` and `make-installer.sh --dry-run` print every step and change nothing. Without
Windows: `packaging/windows/test-make-installer.sh` runs the script with fake downloads, `makensis` and `objdump`,
and `python3 packaging/windows/check-nsi.py` checks the `.nsi` statically (blocks, calls, `un.` functions, labels,
defines, variables). Only `makensis` on the PC compiles it for real.

## The Android remote

`apps/android/` is the remote as an Android app (Rust and Slint, `docs/superpowers/specs/2026-11-18-android-app-design.md`).
It is its own Cargo project with its own `Cargo.lock`: the desktop build never compiles it. Releases carry it as
`RustyPresenter-<version>-android-remote.apk` (the `android` job of `package.yml`); its version is the app's
(`apps/android/Cargo.toml`, checked by CI and by a test in `src/server.rs`).

- **Build:** `apps/android/build-apk.ps1 [-Out file.apk] [-Abis arm64-v8a,armeabi-v7a] [-Debug]` (Windows PowerShell
  or pwsh). It needs the Android SDK (`ANDROID_HOME`, else `.tools/android/sdk` of the main checkout) with
  `platforms;android-34`, `build-tools;34.0.0` and `ndk;27.2.12479018` (pinned in `pins.env`), a JDK (`keytool`, and
  `javac` for Slint's small Java helper), and the Rust targets `aarch64-linux-android` and `armv7-linux-androideabi`.
  No Gradle: `cargo build` per kind of phone with the NDK's clang as linker, then `aapt2 link`, the libraries added
  as `lib/<abi>/libremote.so`, `zipalign`, `apksigner`. On the PC it builds in
  `%LOCALAPPDATA%\RustyPresenter\build-android`.
- **Signing:** with `ANDROID_KEYSTORE`, `ANDROID_KEYSTORE_PASSWORD` and `ANDROID_KEY_ALIAS` when set (CI: the secrets
  `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`), else with a debug key made once in
  `%LOCALAPPDATA%\RustyPresenter\remote-debug.keystore`. A phone only updates an app signed with the same key, so
  releases need the secret set: without it each release is signed by a fresh key and must be uninstalled first.
- **Test:** `cargo test` in `apps/android` (everything that needs no phone), and
  `cargo run --example preview --features preview [-- <language>]` to draw every screen to PNGs. On a phone:
  `adb install -r RustyPresenter-…apk`.
- **Slint patch:** `apps/android/patches/i-slint-backend-android-activity` (see its `PATCHED.md`): drawn with Slint's
  software renderer instead of Skia (whose build downloads a prebuilt library from GitHub), and its Java helper
  buildable with JDK 21+ and build-tools 34. Recheck it when Slint is updated.

## Bumping a pin

- **pdfium:** pick a `chromium/<build>` tag of `bblanchon/pdfium-binaries`, then copy the
  sha256 digests of `pdfium-win-x64.tgz`, `pdfium-mac-arm64.tgz` and `pdfium-linux-x64.tgz`
  from the release (the GitHub API lists them as `digest`).
- **NSIS:** change `NSIS_VERSION` and `NSIS_URL`, set `NSIS_SHA256=TODO`, run `make-installer.cmd` once and pin the
  sha256 from `hashes.txt` after checking its sha1 on SourceForge.
- **libmpv:** bump `LIBMPV_REV` and the source versions, run `libmpv.yml`, publish its
  artifacts as release `libmpv-r<N>`, then fill in the three `LIBMPV_SHA256_*` values.

[cargo-packager]: https://github.com/crabnebula-dev/cargo-packager
