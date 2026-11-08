# Packaging

RustyPresenter ships as a Windows NSIS installer, a macOS `.app` in a `.dmg` (Apple
silicon only), a Linux AppImage and a Linux `.deb`. [cargo-packager] 0.11.8 builds all of
them from the root `Packager.toml`. `packaging/stage.sh` puts the native libraries where
that file expects them first.

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

## Bumping a pin

- **pdfium:** pick a `chromium/<build>` tag of `bblanchon/pdfium-binaries`, then copy the
  sha256 digests of `pdfium-win-x64.tgz`, `pdfium-mac-arm64.tgz` and `pdfium-linux-x64.tgz`
  from the release (the GitHub API lists them as `digest`).
- **NSIS:** change `NSIS_VERSION` and `NSIS_URL`, set `NSIS_SHA256=TODO`, run `make-installer.cmd` once and pin the
  sha256 from `hashes.txt` after checking its sha1 on SourceForge.
- **libmpv:** bump `LIBMPV_REV` and the source versions, run `libmpv.yml`, publish its
  artifacts as release `libmpv-r<N>`, then fill in the three `LIBMPV_SHA256_*` values.

[cargo-packager]: https://github.com/crabnebula-dev/cargo-packager
