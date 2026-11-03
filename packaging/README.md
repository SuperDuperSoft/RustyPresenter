# Packaging

RustyPresenter ships as a Windows NSIS installer, a macOS `.app` in a `.dmg` (Apple
silicon only), a Linux AppImage and a Linux `.deb`. [cargo-packager] 0.11.8 builds all of
them from the root `Packager.toml`. `packaging/stage.sh` puts the native libraries where
that file expects them first.

The packaged binary is `rustypresenter`. Until Plan 4 Task 12 that name still belongs to
the Tauri app (the native app is `rustypresenter-native`), so packages built before Task 12
are not the native app.

## Files

| Path | What it is |
|---|---|
| `Packager.toml` | cargo-packager config, camelCase keys, version kept equal to `Cargo.toml` (CI checks it) |
| `packaging/pins.env` | every pinned version and checksum: cargo-packager, the libmpv release, pdfium |
| `packaging/stage.sh` | fetches, verifies and stages libmpv and pdfium for one OS |
| `packaging/icons/` | 32, 128, 256 and 512 PNGs and `icon.ico`, made from `icons/icon.ico` (its largest image is 256 px, so the 512 PNG is scaled up). `icon.icns` is generated on macOS by `stage.sh` and not committed |
| `packaging/libmpv/` | the LGPL libmpv build scripts (Plan 4 Task 16) |
| `packaging/licenses/` | licence texts appended to `THIRD-PARTY-LICENSES.txt` (Plan 4 Task 14) |

Generated, never committed: `packaging/native/`, `packaging/native-macos/`,
`packaging/out/` (downloads and unpacked archives), `packaging/icons/icon.icns` and
`dist/` (the packages).

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
| Linux AppImage | `libpdfium.so` | `packaging/out/libmpv-linux/libmpv.so.2`, installed into `/usr/lib/x86_64-linux-gnu` on the build machine so the `appimage.libs` entry bundles it | `$APPDIR/usr/lib/rustypresenter/`, libmpv in `$APPDIR/usr/lib/` |

At run time `Mpv::candidates` and `pdfium_candidates` (Plan 4 Task 1) look in the resource
folder, `Contents/Frameworks`, next to the exe, `<data>/deps/` and the system, so every
layout works without a feature flag.

Notes on the macOS layout:

- The dylibs are not resources, because code in `Contents/Resources` breaks
  `codesign --verify --strict`. `NATIVE-LIBS.txt` exists only so the `packaging/native/*`
  glob always matches a file.
- `dylibbundler -od -b -x libmpv.2.dylib -d packaging/native-macos -p @loader_path/` runs on
  a copy (`-od` empties the target folder first). If it bundles any extra dylib, `stage.sh`
  stops: add it to `macos.frameworks` and to the check in `stage.sh`.
- `stage.sh` checks with `otool` that both dylibs link only system libraries and each other,
  and that their minimum macOS is 12.0 or lower.

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

## Bumping a pin

- **pdfium:** pick a `chromium/<build>` tag of `bblanchon/pdfium-binaries`, then copy the
  sha256 digests of `pdfium-win-x64.tgz`, `pdfium-mac-arm64.tgz` and `pdfium-linux-x64.tgz`
  from the release (the GitHub API lists them as `digest`).
- **libmpv:** bump `LIBMPV_REV` and the source versions, run `libmpv.yml`, publish its
  artifacts as release `libmpv-r<N>`, then fill in the three `LIBMPV_SHA256_*` values.

[cargo-packager]: https://github.com/crabnebula-dev/cargo-packager
