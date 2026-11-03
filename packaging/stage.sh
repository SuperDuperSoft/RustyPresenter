#!/usr/bin/env bash
# Stages the native libraries cargo-packager bundles (see packaging/README.md).
#
#   packaging/stage.sh windows|macos|linux   fetch, verify and stage libmpv + pdfium
#   packaging/stage.sh mac-finish            fallback: ad-hoc sign dist/RustyPresenter.app, build the dmg
#
# Runs in Git Bash on Windows too. Archives are downloaded into STAGE_DOWNLOADS
# (default packaging/out/downloads); one already there is reused once its sha256
# matches packaging/pins.env. Nothing here regenerates the licences.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
# shellcheck source=pins.env
source packaging/pins.env

OUT="packaging/out"
DOWNLOADS="${STAGE_DOWNLOADS:-$OUT/downloads}"
NATIVE="packaging/native"
NATIVE_MAC="packaging/native-macos"

die() { echo "stage.sh: $*" >&2; exit 1; }
say() { echo "stage.sh: $*"; }

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# fetch <url> <file name> <sha256>: leaves a verified copy at $DOWNLOADS/<file name>.
fetch() {
  local url="$1" name="$2" want="$3" file="$DOWNLOADS/$2"
  [[ "$want" =~ ^[0-9a-f]{64}$ ]] || die "no sha256 pinned for $name in packaging/pins.env (found '$want')"
  mkdir -p "$DOWNLOADS"
  if [[ ! -f "$file" ]]; then
    say "downloading $url"
    curl -fsSL --retry 3 -o "$file.part" "$url" || { rm -f "$file.part"; die "download failed: $url"; }
    mv "$file.part" "$file"
  fi
  local got
  got="$(sha256_of "$file")"
  if [[ "$got" != "$want" ]]; then
    rm -f "$file"
    die "sha256 mismatch for $name: expected $want, got $got (file removed)"
  fi
  say "verified $name"
}

# The committed root licence file (Plan 4 Task 14) ships in every package.
check_licenses() {
  [[ -f THIRD-PARTY-LICENSES.txt ]] || die "THIRD-PARTY-LICENSES.txt is missing; run scripts/licenses.sh"
  grep -q "GNU LESSER GENERAL PUBLIC LICENSE" THIRD-PARTY-LICENSES.txt \
    || die "THIRD-PARTY-LICENSES.txt has no LGPL text; run scripts/licenses.sh"
}

# unpack <os> <libmpv arch> <pdfium archive suffix> <libmpv sha256> <pdfium sha256>
# Fetches both pinned archives and unpacks them into $STAGE ($OUT/stage-<os>/).
unpack() {
  local os="$1" arch="$2" pdfium_suffix="$3" mpv_sha="$4" pdfium_sha="$5"
  local mpv_zip="libmpv-$os-$arch-mpv$MPV_VERSION-ff$FFMPEG_VERSION-r$LIBMPV_REV.zip"
  local pdfium_tgz="pdfium-$pdfium_suffix.tgz"
  fetch "https://github.com/$LIBMPV_REPO/releases/download/libmpv-r$LIBMPV_REV/$mpv_zip" "$mpv_zip" "$mpv_sha"
  fetch "https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_TAG//\//%2F}/$pdfium_tgz" \
    "$pdfium_tgz" "$pdfium_sha"

  STAGE="$OUT/stage-$os"
  rm -rf "$STAGE"
  mkdir -p "$STAGE/libmpv" "$STAGE/pdfium"
  unzip -q "$DOWNLOADS/$mpv_zip" -d "$STAGE/libmpv"
  # Read from stdin: GNU tar takes a "C:/..." file name for a remote host.
  tar -xzf - -C "$STAGE/pdfium" < "$DOWNLOADS/$pdfium_tgz"
  # The workflow writes the meson line into BUILDINFO.txt; a GPL build must never get this far.
  grep -q -- "-Dgpl=false" "$STAGE/libmpv/BUILDINFO.txt" \
    || die "$mpv_zip is not an LGPL build (no -Dgpl=false in BUILDINFO.txt)"
}

fresh_dir() { rm -rf "$1"; mkdir -p "$1"; }

stage_windows() {
  unpack windows x86_64 win-x64 "$LIBMPV_SHA256_WINDOWS" "$PDFIUM_SHA256_WINDOWS"
  fresh_dir "$NATIVE"
  # NSIS puts resources next to the exe, where Mpv::candidates and pdfium_candidates look.
  cp "$STAGE/libmpv/libmpv-2.dll" "$STAGE/pdfium/bin/pdfium.dll" "$NATIVE/"
}

stage_linux() {
  unpack linux x86_64 linux-x64 "$LIBMPV_SHA256_LINUX" "$PDFIUM_SHA256_LINUX"
  fresh_dir "$NATIVE"
  # The .deb puts resources in /usr/lib/rustypresenter, the AppImage in $APPDIR/usr/lib/rustypresenter.
  cp "$STAGE/pdfium/lib/libpdfium.so" "$NATIVE/"
  # libmpv is not a resource: the .deb depends on the distro libmpv2, and the AppImage's
  # `libs` entry picks our LGPL build up from /usr/lib* once CI installs it there.
  mkdir -p "$OUT/libmpv-linux"
  cp "$STAGE/libmpv/libmpv.so.2" "$OUT/libmpv-linux/"
  say "for the AppImage: sudo install -m 0644 $OUT/libmpv-linux/libmpv.so.2 /usr/lib/x86_64-linux-gnu/ && sudo ldconfig"
}

# Minimum macOS a dylib was built for: LC_BUILD_VERSION minos, or LC_VERSION_MIN_MACOSX version.
dylib_minos() {
  # awk reads to the end (no `exit`), so otool never dies of SIGPIPE under pipefail.
  otool -l "$1" | awk '
    /LC_BUILD_VERSION|LC_VERSION_MIN_MACOSX/ { f = 1 }
    f && !done && ($1 == "minos" || $1 == "version") { print $2; done = 1 }'
}

make_icns() {
  local set="$OUT/icon.iconset" icons="packaging/icons"
  fresh_dir "$set"
  sips -z 16 16 "$icons/32x32.png" --out "$set/icon_16x16.png" >/dev/null
  cp "$icons/32x32.png" "$set/icon_16x16@2x.png"
  cp "$icons/32x32.png" "$set/icon_32x32.png"
  sips -z 64 64 "$icons/128x128.png" --out "$set/icon_32x32@2x.png" >/dev/null
  cp "$icons/128x128.png" "$set/icon_128x128.png"
  cp "$icons/256x256.png" "$set/icon_128x128@2x.png"
  cp "$icons/256x256.png" "$set/icon_256x256.png"
  cp "$icons/512x512.png" "$set/icon_256x256@2x.png"
  cp "$icons/512x512.png" "$set/icon_512x512.png"
  iconutil -c icns "$set" -o "$icons/icon.icns"
}

stage_macos() {
  command -v dylibbundler >/dev/null 2>&1 || die "dylibbundler is not installed (brew install dylibbundler)"
  unpack macos arm64 mac-arm64 "$LIBMPV_SHA256_MACOS" "$PDFIUM_SHA256_MACOS"

  # dylibbundler -od empties the -d folder first, so it fixes a copy outside it and
  # the libraries are copied in afterwards.
  local fixed="$OUT/macos-fixed"
  fresh_dir "$fixed"
  cp "$STAGE/libmpv/libmpv.2.dylib" "$fixed/"
  chmod u+w "$fixed/libmpv.2.dylib"
  dylibbundler -od -b -x "$fixed/libmpv.2.dylib" -d "$NATIVE_MAC" -p @loader_path/
  cp "$fixed/libmpv.2.dylib" "$STAGE/pdfium/lib/libpdfium.dylib" "$NATIVE_MAC/"

  # Packager.toml lists exactly these two frameworks. Anything else dylibbundler
  # pulled in would be left out of the .app, so stop here instead.
  local f extra=""
  for f in "$NATIVE_MAC"/*; do
    case "$(basename "$f")" in
      libmpv.2.dylib | libpdfium.dylib) ;;
      *) extra="$extra $(basename "$f")" ;;
    esac
  done
  [[ -z "$extra" ]] || die "libmpv needs extra dylibs:$extra. Add them to Packager.toml macos.frameworks and this check."

  local lib dep id minos
  for lib in "$NATIVE_MAC/libmpv.2.dylib" "$NATIVE_MAC/libpdfium.dylib"; do
    say "otool -L $lib"
    otool -L "$lib"
    id="$(otool -D "$lib" | tail -n +2)"
    # Skip the first line (the file name) and the library's own install name.
    while read -r dep _; do
      [[ "$dep" == "$id" ]] && continue
      case "$dep" in
        /usr/lib/* | /System/Library/*) ;;
        @loader_path/libmpv.2.dylib | @loader_path/libpdfium.dylib) ;;
        *) die "$(basename "$lib") links $dep, which is neither a system library nor bundled" ;;
      esac
    done < <(otool -L "$lib" | tail -n +2)
    minos="$(dylib_minos "$lib")"
    [[ -n "$minos" ]] || die "no minimum macOS version recorded in $(basename "$lib")"
    # Must load on macOS 12 (Packager.toml minimumSystemVersion).
    [[ "$(printf '%s\n%s\n' "$minos" 12.0 | sort -V | tail -n 1)" == "12.0" ]] \
      || die "$(basename "$lib") needs macOS $minos; it must load on 12.0"
  done

  # Dylibs live in Contents/Frameworks (code in Resources breaks codesign --strict), but
  # the packaging/native/* resource glob must still match a file.
  fresh_dir "$NATIVE"
  echo "The video and PDF libraries are in Contents/Frameworks." > "$NATIVE/NATIVE-LIBS.txt"
  make_icns
}

# Fallback when cargo-packager's ad-hoc signing fails (Plan 4 Task 15 Step 3):
# package with `-f app`, then run this.
mac_finish() {
  local app="dist/RustyPresenter.app"
  [[ -d "$app" ]] || die "$app not found; run cargo packager -c Packager.toml -f app first"
  codesign --force --deep -s - "$app"
  codesign --verify --deep --strict -v "$app"
  hdiutil create -volname RustyPresenter -srcfolder "$app" -ov -format UDZO dist/RustyPresenter.dmg
}

case "${1:-}" in
  windows) check_licenses; stage_windows ;;
  macos) check_licenses; stage_macos ;;
  linux) check_licenses; stage_linux ;;
  mac-finish) mac_finish; exit 0 ;;
  *) die "usage: packaging/stage.sh windows|macos|linux|mac-finish" ;;
esac

say "staged:"
ls -l "$NATIVE"
if [[ -d "$NATIVE_MAC" && "$1" == macos ]]; then ls -l "$NATIVE_MAC"; fi
