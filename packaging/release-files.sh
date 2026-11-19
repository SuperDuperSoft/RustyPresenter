#!/usr/bin/env bash
# The files of a release, with the same names on every OS (see packaging/README.md, "Release files").
#
#   packaging/release-files.sh rename windows|macos|linux     give cargo-packager's packages in dist/ their names
#   packaging/release-files.sh portable windows|macos|linux   make the portable package in dist/
#
# Run from anywhere, after packaging/stage.sh <os> and cargo packager (portable macos needs dist/RustyPresenter.app).
# Names: RustyPresenter-<version>-windows-setup.exe, -windows-portable.zip, -macos.dmg, -macos-portable.zip,
# -linux.AppImage, -linux.deb, -linux-portable.tar.gz. <version> is Packager.toml's.
#
# Settings for tests and the PC script (packaging/windows/make-portable.cmd):
#   RP_VERSION  the version (default: Packager.toml's)
#   RP_BIN      the built program (default: target/release/rustypresenter[.exe])
#   RP_DIST     where the packages go (default: dist)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

die() { echo "release-files.sh: $*" >&2; exit 1; }
say() { echo "release-files.sh: $*"; }

VERSION="${RP_VERSION:-$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Packager.toml | head -n 1)}"
[[ -n "$VERSION" ]] || die "no version in Packager.toml"
DIST="${RP_DIST:-dist}"
PORTABLE="packaging/portable"

# one <glob>: the single file that matches, or stop.
one() {
  local found=() f
  for f in $1; do [[ -e "$f" ]] && found+=("$f"); done
  [[ ${#found[@]} -eq 1 ]] || die "expected exactly one $1, found ${#found[@]}: ${found[*]:-none}"
  echo "${found[0]}"
}

# name <glob> <new name>: renames the one package matching the glob in $DIST.
name() {
  local from
  from="$(one "$DIST/$1")"
  [[ "$from" == "$DIST/$2" ]] || mv "$from" "$DIST/$2"
  say "$DIST/$2"
}

rename() {
  case "$1" in
    windows)
      # cargo-packager 0.11.8 calls it <productName>_<version>_x64-setup.exe; any one .exe will do.
      if compgen -G "$DIST/*-setup.exe" >/dev/null; then name "*-setup.exe" "RustyPresenter-$VERSION-windows-setup.exe"
      else name "*.exe" "RustyPresenter-$VERSION-windows-setup.exe"; fi
      ;;
    macos) name "*.dmg" "RustyPresenter-$VERSION-macos.dmg" ;;
    linux)
      name "*.AppImage" "RustyPresenter-$VERSION-linux.AppImage"
      name "*.deb" "RustyPresenter-$VERSION-linux.deb"
      ;;
    *) die "usage: release-files.sh rename windows|macos|linux" ;;
  esac
}

# licences <folder>: the app's own licences and every notice the packages carry, as in the installers.
licences() {
  mkdir -p "$1/licenses"
  cp LICENSE-MIT LICENSE-APACHE "$1/"
  cp THIRD-PARTY-LICENSES.txt "$1/licenses/"
  if [[ -d packaging/native-licenses ]]; then cp packaging/native-licenses/* "$1/licenses/"; fi
}

# The marker and the readme beside the app.
marker() {
  cp "$PORTABLE/portable.txt" "$1/portable.txt"
  cp "$PORTABLE/HOW-TO-START-$2.txt" "$1/HOW-TO-START.txt"
}

# zip_folder <folder> <zip>: the folder itself goes in the zip, so unzipping makes one RustyPresenter folder.
zip_folder() {
  local parent base out
  parent="$(dirname "$1")"; base="$(basename "$1")"
  out="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
  rm -f "$out"
  if command -v zip >/dev/null 2>&1; then
    (cd "$parent" && zip -qr -X "$out" "$base")
  elif command -v 7z >/dev/null 2>&1; then
    (cd "$parent" && 7z a -tzip -bd -bso0 "$out" "$base")
  elif command -v powershell.exe >/dev/null 2>&1; then
    powershell.exe -NoProfile -Command "Compress-Archive -Path '$(cygpath -w "$1")' -DestinationPath '$(cygpath -w "$out")' -Force"
  else
    die "no zip, 7z or PowerShell to make the zip"
  fi
}

portable() {
  local os="$1" stage="packaging/out/portable-$1" dir
  rm -rf "$stage"
  dir="$stage/RustyPresenter"
  mkdir -p "$dir" "$DIST"
  case "$os" in
    windows)
      local bin="${RP_BIN:-target/release/rustypresenter.exe}"
      [[ -f "$bin" ]] || die "$bin is not built"
      cp "$bin" "$dir/rustypresenter.exe"
      local lib
      for lib in libmpv-2.dll pdfium.dll; do
        if [[ -f "packaging/native/$lib" ]]; then cp "packaging/native/$lib" "$dir/"; else die "packaging/native/$lib is missing; run packaging/stage.sh windows"; fi
      done
      licences "$dir"
      marker "$dir" windows
      zip_folder "$dir" "$DIST/RustyPresenter-$VERSION-windows-portable.zip"
      say "$DIST/RustyPresenter-$VERSION-windows-portable.zip"
      ;;
    macos)
      local app="$DIST/RustyPresenter.app"
      [[ -d "$app" ]] || die "$app is not built; run cargo packager -c Packager.toml -f app first"
      # ditto keeps the bundle exactly as signed (links, permissions, extended attributes).
      ditto "$app" "$dir/RustyPresenter.app"
      cp LICENSE-MIT LICENSE-APACHE "$dir/"
      marker "$dir" macos
      ditto -c -k --keepParent "$dir" "$DIST/RustyPresenter-$VERSION-macos-portable.zip"
      say "$DIST/RustyPresenter-$VERSION-macos-portable.zip"
      ;;
    linux)
      local bin="${RP_BIN:-target/release/rustypresenter}"
      [[ -f "$bin" ]] || die "$bin is not built"
      # cp and chmod, not install: Git Bash's install adds ".exe" to a name without one.
      cp "$bin" "$dir/rustypresenter"
      chmod 0755 "$dir/rustypresenter"
      [[ -f packaging/native/libpdfium.so ]] || die "packaging/native/libpdfium.so is missing; run packaging/stage.sh linux"
      [[ -f packaging/out/libmpv-linux/libmpv.so.2 ]] || die "packaging/out/libmpv-linux/libmpv.so.2 is missing; run packaging/stage.sh linux"
      cp packaging/native/libpdfium.so packaging/out/libmpv-linux/libmpv.so.2 "$dir/"
      chmod 0644 "$dir/libpdfium.so" "$dir/libmpv.so.2"
      cp "$PORTABLE/RustyPresenter.sh" "$dir/RustyPresenter.sh"
      chmod 0755 "$dir/RustyPresenter.sh"
      licences "$dir"
      marker "$dir" linux
      tar -czf "$DIST/RustyPresenter-$VERSION-linux-portable.tar.gz" -C "$stage" RustyPresenter
      say "$DIST/RustyPresenter-$VERSION-linux-portable.tar.gz"
      ;;
    *) die "usage: release-files.sh portable windows|macos|linux" ;;
  esac
}

case "${1:-}" in
  rename) rename "${2:-}" ;;
  portable) portable "${2:-}" ;;
  *) die "usage: release-files.sh rename|portable windows|macos|linux" ;;
esac
