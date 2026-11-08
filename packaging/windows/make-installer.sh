#!/usr/bin/env bash
# Makes RustyPresenter-Setup-<version>.exe with NSIS: per user, no admin rights, Start menu and desktop
# shortcuts, an uninstaller that keeps the data folder unless asked. make-installer.cmd runs this in Git Bash
# after building the release exe; see packaging/README.md, "Windows installer on a PC".
#
#   packaging/windows/make-installer.sh [--dry-run]
#
# --dry-run prints every step with its paths and pins, and changes nothing (no download, no folder).
#
# Inputs, from the environment (Windows or POSIX paths):
#   RP_EXE        the built rustypresenter.exe (required)
#   RP_TOOLS      the .tools folder; downloads and NSIS go to <RP_TOOLS>/installer, once
#   RP_OUT_DIR    where the setup file goes (make-installer.cmd passes the Desktop)
#   RP_MINGW_BIN  .tools/mingw64/bin: objdump, and the MinGW runtime DLLs the exe may need
#   RP_DEPS       %LOCALAPPDATA%\RustyPresenter\deps: its mpv\libmpv-2.dll is used while no libmpv release is pinned
#   RP_XFER       results for Claude to read (default <RP_TOOLS>/xfer/make-installer); a finished run writes the
#                 setup file's Windows path to <RP_XFER>/setup-path.txt
#   RP_LOG        a copy of everything printed goes here too (make-installer.cmd: <RP_XFER>/installer.txt)
#
# Every download is checked against packaging/pins.env. A pin that still says TODO is not trusted: the file is
# downloaded to installer/downloads/unverified/, its sha256 and sha1 are written to <RP_XFER>/hashes.txt, and the
# run stops before anything is built from it. Someone compares them with the publisher's page and pins them.
#
# Exit codes: 0 done, 1 error, 3 stopped for unpinned downloads.
set -euo pipefail

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=../pins.env
source "$SRC/packaging/pins.env"

DRY=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY=1 ;;
    *) echo "make-installer.sh: unknown option $arg (only --dry-run)" >&2; exit 1 ;;
  esac
done

# Git Bash: MSYS tools first (MinGW's bin is first on the PATH make-installer.cmd sets), and no rewriting of
# arguments passed to Windows programs. Those (curl, makensis, objdump, Windows' tar) get Windows paths from winpath.
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) PATH="/usr/bin:$PATH"; export MSYS2_ARG_CONV_EXCL='*' ;;
esac

die() { echo "make-installer: $*" >&2; exit 1; }
say() { echo "make-installer: $*"; }
STEP=0
step() { STEP=$((STEP + 1)); echo; echo "== Step $STEP: $*"; }

posix() { if [[ -n "$1" ]] && command -v cygpath >/dev/null 2>&1; then cygpath -u "$1"; else printf '%s\n' "$1"; fi; }
winpath() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s\n' "$1"; fi; }

[[ -n "${RP_TOOLS:-}" ]] || die "RP_TOOLS is not set (make-installer.cmd sets it to the .tools folder)"
TOOLS="$(posix "$RP_TOOLS")"
EXE="$(posix "${RP_EXE:-}")"
OUT_DIR="$(posix "${RP_OUT_DIR:-$HOME/Desktop}")"
MINGW_BIN="$(posix "${RP_MINGW_BIN:-$TOOLS/mingw64/bin}")"
DEPS="$(posix "${RP_DEPS:-}")"
XFER="$(posix "${RP_XFER:-$TOOLS/xfer/make-installer}")"

# The copy for RP_LOG: run again through tee, which a pipeline waits for (exec > >(tee) would not).
if [[ -n "${RP_LOG:-}" && $DRY == 0 && -z "${RP_LOGGING:-}" ]]; then
  LOG="$(posix "$RP_LOG")"
  mkdir -p "$(dirname "$LOG")"
  set +e
  RP_LOGGING=1 bash "${BASH_SOURCE[0]}" "$@" 2>&1 | tee "$LOG"
  exit "${PIPESTATUS[0]}"
fi

INST="$TOOLS/installer"
DL="$INST/downloads"
UNVERIFIED="$DL/unverified"
WORK="$INST/work"
STAGE="$WORK/stage"
LICDIR="$SRC/packaging/licenses"
PLACEHOLDER="RUSTYPRESENTER-LICENCE-PLACEHOLDER"
HASHES="$XFER/hashes.txt"

VERSION="$(sed -n 's/^version = "\([^"]*\)".*/\1/p' "$SRC/Cargo.toml" | head -n 1)"
[[ -n "$VERSION" ]] || die "no version in $SRC/Cargo.toml"
# VIProductVersion needs four numbers: 0.2.0-rc1 -> 0.2.0.0.
IFS=. read -r v1 v2 v3 _ <<<"${VERSION%%[-+]*}"
VIVERSION="${v1:-0}.${v2:-0}.${v3:-0}.0"
SETUP_NAME="RustyPresenter-Setup-$VERSION.exe"
NSIS_DIR="$INST/nsis-$NSIS_VERSION"
MAKENSIS="$NSIS_DIR/makensis.exe"
LIBMPV_ZIP="libmpv-windows-x86_64-mpv$MPV_VERSION-ff$FFMPEG_VERSION-r$LIBMPV_REV.zip"
LIBMPV_URL="https://github.com/$LIBMPV_REPO/releases/download/libmpv-r$LIBMPV_REV/$LIBMPV_ZIP"
PDFIUM_TGZ="pdfium-win-x64.tgz"
PDFIUM_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_TAG//\//%2F}/$PDFIUM_TGZ"
NSIS_ZIP="nsis-$NSIS_VERSION.zip"

pinned() { [[ "$1" =~ ^[0-9a-f]{64}$ ]]; }
is_placeholder() { [[ -f "$1" ]] && grep -q "$PLACEHOLDER" "$1"; }

sha256_of() { sha256sum "$1" | cut -d' ' -f1; }
sha1_of() { sha1sum "$1" | cut -d' ' -f1; }

# Where libmpv comes from: the pinned LGPL release, else the build PC's own DLL, else none (no video).
if pinned "$LIBMPV_SHA256_WINDOWS"; then
  MPV_FROM=release
elif [[ -n "$DEPS" && -f "$DEPS/mpv/libmpv-2.dll" ]]; then
  MPV_FROM=local
else
  MPV_FROM=none
fi

UNPINNED=()
# fetch <url> <file name> <sha256 pin> <pin name>: a verified copy at $DL/<file name>, or, for a TODO pin, an
# unverified one at $UNVERIFIED/<file name> with its hashes in $HASHES (the run stops later).
fetch() {
  local url="$1" name="$2" want="$3" pin="$4"
  if [[ $DRY == 1 ]]; then
    if pinned "$want"; then
      say "would download $url"
      say "  to $DL/$name (once), and check sha256 $want ($pin)"
    else
      say "would download $url"
      say "  to $UNVERIFIED/$name: $pin is '$want', so its hashes go to $HASHES and the run stops"
    fi
    return 0
  fi
  if ! pinned "$want"; then
    mkdir -p "$UNVERIFIED" "$XFER"
    local file="$UNVERIFIED/$name"
    say "$pin is '$want', not a sha256: downloading $name only to show its hashes"
    curl -fL --retry 3 -o "$(winpath "$file.part")" "$url" || { rm -f "$file.part"; die "download failed: $url"; }
    mv "$file.part" "$file"
    {
      echo "$pin"
      echo "  url    $url"
      echo "  sha256 $(sha256_of "$file")"
      echo "  sha1   $(sha1_of "$file")"
      echo "  bytes  $(wc -c <"$file" | tr -d ' ')"
    } >>"$HASHES"
    UNPINNED+=("$pin")
    return 0
  fi
  local file="$DL/$name"
  mkdir -p "$DL"
  if [[ ! -f "$file" ]]; then
    say "downloading $url"
    curl -fL --retry 3 -o "$(winpath "$file.part")" "$url" || { rm -f "$file.part"; die "download failed: $url"; }
    mv "$file.part" "$file"
  fi
  local got
  got="$(sha256_of "$file")"
  if [[ "$got" != "$want" ]]; then
    rm -f "$file"
    die "sha256 mismatch for $name: expected $want ($pin in packaging/pins.env), got $got. The file was removed; run again to download it again."
  fi
  say "verified $name"
}

# unzip_to <zip> <folder>: Git Bash's unzip, else Windows' own tar (bsdtar reads zips).
unzip_to() {
  mkdir -p "$2"
  if command -v unzip >/dev/null 2>&1; then
    unzip -q -o "$1" -d "$2"
  elif [[ -x /c/Windows/System32/tar.exe ]]; then
    /c/Windows/System32/tar.exe -xf "$(winpath "$1")" -C "$(winpath "$2")"
  else
    die "no unzip and no Windows tar.exe to unpack $1"
  fi
}

fresh_dir() { rm -rf "$1"; mkdir -p "$1"; }

echo "RustyPresenter installer $VERSION"
echo "  code       $SRC"
echo "  app        ${EXE:-(not given)}"
echo "  tools      $INST"
echo "  setup file $OUT_DIR/$SETUP_NAME"
case "$MPV_FROM" in
  release) echo "  libmpv     the LGPL release libmpv-r$LIBMPV_REV" ;;
  local) echo "  libmpv     $DEPS/mpv/libmpv-2.dll (from this PC; libmpv-r$LIBMPV_REV is not pinned yet)" ;;
  none) echo "  libmpv     none found: the installed app will not play video or sound" ;;
esac
if [[ $DRY == 1 ]]; then echo "  (dry run: nothing is downloaded, written or built)"; fi

if [[ $DRY == 0 ]]; then
  [[ -f "$EXE" ]] || die "the app is not built: $EXE is missing"
  mkdir -p "$XFER"
  : >"$HASHES"
fi

# ---------------------------------------------------------------------------------------------------------------
step "Download what the installer needs (into $INST, once)"
fetch "$NSIS_URL" "$NSIS_ZIP" "$NSIS_SHA256" NSIS_SHA256
fetch "$PDFIUM_URL" "$PDFIUM_TGZ" "$PDFIUM_SHA256_WINDOWS" PDFIUM_SHA256_WINDOWS
case "$MPV_FROM" in
  release) fetch "$LIBMPV_URL" "$LIBMPV_ZIP" "$LIBMPV_SHA256_WINDOWS" LIBMPV_SHA256_WINDOWS ;;
  # Without the release zip, mpv's own Copyright file comes from mpv's source at the pinned tag.
  local) fetch "$MPV_COPYRIGHT_URL" "mpv-$MPV_VERSION-Copyright" "$MPV_COPYRIGHT_SHA256" MPV_COPYRIGHT_SHA256 ;;
  none) say "no libmpv: skipping its download" ;;
esac

if ((${#UNPINNED[@]})); then
  echo
  say "STOPPED before building: ${UNPINNED[*]} not pinned in packaging/pins.env yet."
  say "The hashes of what was downloaded are in $(winpath "$HASHES")"
  say "Send that file to Claude. Once the hashes are checked against the publishers' pages and pinned, run this again."
  exit 3
fi

# ---------------------------------------------------------------------------------------------------------------
step "Unpack NSIS $NSIS_VERSION and the libraries"
if [[ $DRY == 1 ]]; then
  say "would unpack $NSIS_ZIP to $NSIS_DIR (once; makensis.exe runs from there, nothing is installed)"
  say "would unpack $PDFIUM_TGZ to $WORK/pdfium and take bin/pdfium.dll and its licences"
  case "$MPV_FROM" in
    release) say "would unpack $LIBMPV_ZIP to $WORK/libmpv, check BUILDINFO.txt says -Dgpl=false, take libmpv-2.dll" ;;
    local) say "would take $DEPS/mpv/libmpv-2.dll" ;;
  esac
else
  # A stamp holds the zip's sha256, so a changed pin unpacks again.
  if [[ ! -f "$MAKENSIS" || "$(cat "$NSIS_DIR/.rp-sha256" 2>/dev/null)" != "$NSIS_SHA256" ]]; then
    rm -rf "$NSIS_DIR" "$INST/nsis-unpack"
    unzip_to "$DL/$NSIS_ZIP" "$INST/nsis-unpack"
    # The zip holds one folder, nsis-<version>/.
    found="$(find "$INST/nsis-unpack" -maxdepth 2 -iname makensis.exe | head -n 1)"
    [[ -n "$found" ]] || die "$NSIS_ZIP has no makensis.exe"
    mv "$(dirname "$found")" "$NSIS_DIR"
    rm -rf "$INST/nsis-unpack"
    echo "$NSIS_SHA256" >"$NSIS_DIR/.rp-sha256"
  fi
  [[ -f "$MAKENSIS" ]] || die "$MAKENSIS is missing"
  say "NSIS: $MAKENSIS"

  fresh_dir "$WORK"
  mkdir -p "$WORK/pdfium"
  # Read from stdin: GNU tar takes a "C:/..." file name for a remote host.
  tar -xzf - -C "$WORK/pdfium" <"$DL/$PDFIUM_TGZ"
  [[ -f "$WORK/pdfium/bin/pdfium.dll" ]] || die "$PDFIUM_TGZ has no bin/pdfium.dll"
  [[ -s "$WORK/pdfium/LICENSE" ]] || die "$PDFIUM_TGZ has no LICENSE"
  if [[ $MPV_FROM == release ]]; then
    unzip_to "$DL/$LIBMPV_ZIP" "$WORK/libmpv"
    grep -q -- "-Dgpl=false" "$WORK/libmpv/BUILDINFO.txt" \
      || die "$LIBMPV_ZIP is not an LGPL build (no -Dgpl=false in BUILDINFO.txt)"
    [[ -f "$WORK/libmpv/libmpv-2.dll" ]] || die "$LIBMPV_ZIP has no libmpv-2.dll"
  fi
fi

# ---------------------------------------------------------------------------------------------------------------
step "Licence texts into packaging/licenses/ (and a copy for Claude in $XFER/licenses)"
# fill <source file> <name in packaging/licenses>: replaces a placeholder with the real text. A real text that
# differs is left in the repo (the shipped library's own copy goes into the installer) and reported.
SHIP_LICENCES=()
fill() {
  local from="$1" name="$2" to="$LICDIR/$2"
  if [[ $DRY == 1 ]]; then
    say "would copy $from to $to (and to $XFER/licenses/$name)"
    return 0
  fi
  [[ -s "$from" ]] || die "licence source $from is missing or empty"
  mkdir -p "$XFER/licenses"
  cp "$from" "$XFER/licenses/$name"
  if [[ ! -f "$to" ]] || is_placeholder "$to"; then
    cp "$from" "$to"
    say "filled $to"
  elif [[ "$(sha256_of "$from")" != "$(sha256_of "$to")" ]]; then
    say "NOTE: $name in the repo differs from the one shipped with the library; the library's copy is installed"
  fi
  SHIP_LICENCES+=("$XFER/licenses/$name")
}
fill "$WORK/pdfium/LICENSE" pdfium-LICENSE.txt
case "$MPV_FROM" in
  release)
    fill "$WORK/libmpv/Copyright" mpv-Copyright.txt
    fill "$WORK/libmpv/COPYING.LGPLv2.1" FFmpeg-COPYING.LGPLv2.1.txt
    ;;
  local)
    fill "$DL/mpv-$MPV_VERSION-Copyright" mpv-Copyright.txt
    # FFmpeg's COPYING.LGPLv2.1 is committed; there is no libmpv zip to take it from.
    if [[ $DRY == 0 ]] && is_placeholder "$LICDIR/FFmpeg-COPYING.LGPLv2.1.txt"; then
      die "$LICDIR/FFmpeg-COPYING.LGPLv2.1.txt is a placeholder"
    fi
    ;;
esac

# ---------------------------------------------------------------------------------------------------------------
step "Put the program files together in $STAGE"
# mingw_imports <dll or exe>: the DLL names it imports.
mingw_imports() { "$OBJDUMP" -p "$(winpath "$1")" 2>/dev/null | sed -n 's/^[[:space:]]*DLL Name:[[:space:]]*//p' | tr -d '\r'; }
# Copies every DLL the exe needs from MinGW's bin (libstdc++, libgcc_s, libwinpthread...), and what those need.
# Outside the build window that folder is not on the PATH, so the app would not start without them.
# The build keeps debug info (for crash backtraces on the build PC); the copies that go into the setup file
# don't need it: it is most of the exe's size. --strip-debug keeps the symbol names. Only the staged copies change.
strip_debug() {
  local tool="$MINGW_BIN/strip.exe" f
  [[ -x "$tool" ]] || tool="$MINGW_BIN/strip"
  if [[ ! -x "$tool" ]]; then
    say "NOTE: no strip in $MINGW_BIN, so the setup file keeps the debug info (it works, it is just bigger)"
    return 0
  fi
  # A release libmpv stays byte-for-byte what was verified; the build PC's own copy may carry debug info.
  local files=("$STAGE/rustypresenter.exe")
  [[ $MPV_FROM == local ]] && files+=("$STAGE/libmpv-2.dll")
  for f in "${files[@]}"; do
    [[ -f "$f" ]] || continue
    "$tool" --strip-debug "$(winpath "$f")" || die "strip failed on $f"
    say "stripped debug info: $(basename "$f") is now $(( $(wc -c <"$f") / 1048576 )) MB"
  done
}

copy_mingw_runtime() {
  OBJDUMP="$MINGW_BIN/objdump.exe"
  [[ -x "$OBJDUMP" ]] || OBJDUMP="$MINGW_BIN/objdump"
  if [[ ! -x "$OBJDUMP" ]]; then
    say "NOTE: no objdump in $MINGW_BIN, so the MinGW runtime DLLs were not checked"
    return 0
  fi
  local queue=("$STAGE/rustypresenter.exe") seen=" " file dll src
  while ((${#queue[@]})); do
    file="${queue[0]}"
    queue=("${queue[@]:1}")
    while read -r dll; do
      [[ -n "$dll" ]] || continue
      src="$(find "$MINGW_BIN" -maxdepth 1 -iname "$dll" | head -n 1)"
      [[ -n "$src" ]] || continue
      case "$seen" in *" ${dll,,} "*) continue ;; esac
      seen="$seen${dll,,} "
      cp "$src" "$STAGE/"
      say "added $(basename "$src") from MinGW (the app needs it)"
      queue+=("$STAGE/$(basename "$src")")
    done < <(mingw_imports "$file")
  done
}

if [[ $DRY == 1 ]]; then
  say "would copy $EXE"
  say "would copy pdfium.dll$([[ $MPV_FROM != none ]] && echo ", libmpv-2.dll")"
  say "would add any MinGW runtime DLL the exe imports, from $MINGW_BIN (checked with objdump)"
  say "would strip the debug info from the copied rustypresenter.exe$([[ $MPV_FROM == local ]] && echo " and libmpv-2.dll") (strip --strip-debug)"
  say "would copy LICENSE-MIT, LICENSE-APACHE, THIRD-PARTY-LICENSES.txt, the real texts in packaging/licenses/ and the libraries' own notices into licenses/"
else
  mkdir -p "$STAGE/licenses"
  cp "$EXE" "$STAGE/rustypresenter.exe"
  cp "$WORK/pdfium/bin/pdfium.dll" "$STAGE/"
  case "$MPV_FROM" in
    release) cp "$WORK/libmpv/libmpv-2.dll" "$STAGE/" ;;
    local) cp "$DEPS/mpv/libmpv-2.dll" "$STAGE/" ;;
  esac
  strip_debug
  copy_mingw_runtime
  cp "$SRC/LICENSE-MIT" "$SRC/LICENSE-APACHE" "$STAGE/"
  cp "$SRC/THIRD-PARTY-LICENSES.txt" "$STAGE/licenses/"
  for f in "$LICDIR"/*; do
    if [[ "$(basename "$f")" == native.txt ]] || is_placeholder "$f"; then continue; fi
    cp "$f" "$STAGE/licenses/"
  done
  # The library's own copy wins over the repo's (fill above).
  for f in "${SHIP_LICENCES[@]}"; do cp "$f" "$STAGE/licenses/"; done
  # Notices of the projects built into each library, as stage.sh ships them.
  for f in "$WORK/pdfium/licenses"/*; do
    if [[ -f "$f" ]]; then cp "$f" "$STAGE/licenses/pdfium-$(basename "$f")"; fi
  done
  if [[ $MPV_FROM == release ]]; then
    for f in "$WORK"/libmpv/COPYING* "$WORK/libmpv/BUILDINFO.txt" "$WORK"/libmpv/licenses/*; do
      if [[ -f "$f" ]]; then cp "$f" "$STAGE/licenses/libmpv-$(basename "$f")"; fi
    done
  elif [[ $MPV_FROM == local ]]; then
    cat >"$STAGE/licenses/libmpv-NOTE.txt" <<EOF
libmpv-2.dll in this install was copied from the PC that made the installer, not from RustyPresenter's own
LGPL build (release libmpv-r$LIBMPV_REV). Settings > About says whether it is an LGPL build. If it is not, it is
under the GNU GPL, and its source is wherever that copy came from.
EOF
  fi
  say "staged:"
  (cd "$STAGE" && find . -type f | sort | sed 's|^\./|  |')
fi

# ---------------------------------------------------------------------------------------------------------------
step "Build $SETUP_NAME with NSIS"
FILES_NSH="$WORK/files.nsh"
UNFILES_NSH="$WORK/unfiles.nsh"
# write_lists: the installer's File lines and the uninstaller's matching Delete lines, from the stage folder.
write_lists() {
  local rel dir
  : >"$FILES_NSH"
  : >"$UNFILES_NSH"
  while read -r dir; do
    rel="${dir#.}"
    rel="${rel#/}"
    printf 'SetOutPath "$INSTDIR%s"\n' "${rel:+\\${rel//\//\\}}" >>"$FILES_NSH"
    while read -r f; do
      printf 'File "%s"\n' "$(winpath "$STAGE/${f#./}")" >>"$FILES_NSH"
      f="${f#./}"
      printf 'Delete "$INSTDIR\\%s"\n' "${f//\//\\}" >>"$UNFILES_NSH"
    done < <(cd "$STAGE" && find "$dir" -mindepth 1 -maxdepth 1 -type f | sort)
  done < <(cd "$STAGE" && find . -type d | sort)
  # Folders deepest first; RMDir without /r only removes empty ones.
  while read -r dir; do
    printf 'RMDir "$INSTDIR\\%s"\n' "${dir//\//\\}" >>"$UNFILES_NSH"
  done < <(cd "$STAGE" && find . -mindepth 1 -type d | sed 's|^\./||' | sort -r)
}
NSI="$SRC/packaging/windows/rustypresenter.nsi"
ICON="$SRC/packaging/icons/icon.ico"
OUT_TMP="$WORK/$SETUP_NAME"
if [[ $DRY == 1 ]]; then
  say "would list the staged files in $FILES_NSH and $UNFILES_NSH"
  say "would run $MAKENSIS -DVERSION=$VERSION -DVIVERSION=$VIVERSION ... $NSI"
  say "would copy the result to $OUT_DIR/$SETUP_NAME"
  echo
  say "dry run finished; nothing was changed"
  exit 0
fi
write_lists
"$MAKENSIS" -V3 -INPUTCHARSET UTF8 \
  "-DVERSION=$VERSION" "-DVIVERSION=$VIVERSION" \
  "-DFILES_NSH=$(winpath "$FILES_NSH")" "-DUNFILES_NSH=$(winpath "$UNFILES_NSH")" \
  "-DOUTFILE=$(winpath "$OUT_TMP")" "-DICON=$(winpath "$ICON")" \
  "$(winpath "$NSI")" || die "makensis failed (the messages above say why)"
[[ -s "$OUT_TMP" ]] || die "makensis made no $OUT_TMP"
mkdir -p "$OUT_DIR"
cp "$OUT_TMP" "$OUT_DIR/$SETUP_NAME"
winpath "$OUT_DIR/$SETUP_NAME" >"$XFER/setup-path.txt"
echo
say "done: $(winpath "$OUT_DIR/$SETUP_NAME")"
