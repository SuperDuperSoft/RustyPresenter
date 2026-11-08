#!/usr/bin/env bash
# Tests for make-installer.sh, on Linux or in Git Bash, with no network: curl, makensis and objdump are fakes, and
# the downloads are small archives made here. The script runs on a copy of the repo's packaging files, so the
# placeholders it fills are the copy's.
#
#   packaging/windows/test-make-installer.sh
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
FAILS=0
ok() { echo "ok   $*"; }
bad() { echo "FAIL $*"; FAILS=$((FAILS + 1)); }
check() { local what="$1"; shift; if "$@"; then ok "$what"; else bad "$what"; fi; }
has() { grep -q -- "$2" "$1"; }

# ---- fixtures: what the publishers' archives look like, in miniature
F="$T/fixtures"
mkdir -p "$F/nsis/nsis-3.11" "$F/pdfium/bin" "$F/pdfium/licenses" "$F/libmpv/licenses" "$F/libmpv-gpl"
cat >"$F/nsis/nsis-3.11/makensis.exe" <<'EOF'
#!/usr/bin/env bash
# Fake makensis: records its arguments, checks the lists exist, writes OUTFILE.
printf '%s\n' "$@" >"$FAKE_LOG/makensis-args.txt"
for a in "$@"; do
  case "$a" in
    -DOUTFILE=*) out="${a#-DOUTFILE=}" ;;
    -DFILES_NSH=*) [[ -s "${a#-DFILES_NSH=}" ]] || { echo "no files.nsh"; exit 1; } ;;
    -DUNFILES_NSH=*) [[ -s "${a#-DUNFILES_NSH=}" ]] || { echo "no unfiles.nsh"; exit 1; } ;;
  esac
done
[[ -f "${!#}" ]] || { echo "no script ${!#}"; exit 1; }
echo "fake setup" >"$out"
EOF
chmod +x "$F/nsis/nsis-3.11/makensis.exe"
(cd "$F/nsis" && zip -qr "$F/nsis-3.11.zip" nsis-3.11)
echo "pdfium dll" >"$F/pdfium/bin/pdfium.dll"
echo "Copyright 2014 The PDFium Authors (test text)" >"$F/pdfium/LICENSE"
echo "abseil (test text)" >"$F/pdfium/licenses/abseil.txt"
tar -czf "$F/pdfium-win-x64.tgz" -C "$F/pdfium" .
echo "mpv Copyright (test text)" >"$F/Copyright"
echo "libmpv dll" >"$F/libmpv/libmpv-2.dll"
echo "meson setup ... -Dgpl=false -Dcplayer=false" >"$F/libmpv/BUILDINFO.txt"
echo "mpv Copyright from the zip (test text)" >"$F/libmpv/Copyright"
cp "$REPO/packaging/licenses/FFmpeg-COPYING.LGPLv2.1.txt" "$F/libmpv/COPYING.LGPLv2.1"
echo "LGPL 2.1 (test text)" >"$F/libmpv/COPYING.LGPL-2.1"
echo "dav1d (test text)" >"$F/libmpv/licenses/dav1d-COPYING.txt"
LIBMPV_ZIP_NAME="libmpv-windows-x86_64-mpv0.41.0-ff9.0.2-r1.zip"
(cd "$F/libmpv" && zip -qr "$F/$LIBMPV_ZIP_NAME" .)
cp -r "$F/libmpv/." "$F/libmpv-gpl/"
echo "meson setup ... -Dgpl=true" >"$F/libmpv-gpl/BUILDINFO.txt"
(cd "$F/libmpv-gpl" && zip -qr "$F/gpl-$LIBMPV_ZIP_NAME" .)

# ---- fakes on the PATH: curl serves fixtures by the URL's last part and logs each call
mkdir -p "$T/bin" "$T/log"
cat >"$T/bin/curl" <<EOF
#!/usr/bin/env bash
out=""; url=""
while ((\$#)); do case "\$1" in -o) out="\$2"; shift 2 ;; -*) shift ;; *) url="\$1"; shift ;; esac; done
echo "\$url" >>"$T/log/curl.txt"
name="\${url##*/}"
[[ "\$name" == Copyright ]] && name=Copyright
src="$F/\${FAKE_PREFIX:-}\$name"
[[ -f "\$src" ]] || src="$F/\$name"
[[ -f "\$src" ]] || { echo "fake curl: no fixture for \$url" >&2; exit 22; }
cp "\$src" "\$out"
EOF
chmod +x "$T/bin/curl"
export PATH="$T/bin:$PATH" FAKE_LOG="$T/log"

sha() { sha256sum "$1" | cut -d' ' -f1; }

# new_case <name>: a fresh repo copy, tools folder (with a fake MinGW), Desktop and data folder.
new_case() {
  C="$T/$1"
  mkdir -p "$C/repo/packaging/windows" "$C/tools/mingw64/bin" "$C/desktop" "$C/deps/mpv" "$C/build"
  cp "$REPO/Cargo.toml" "$REPO/LICENSE-MIT" "$REPO/LICENSE-APACHE" "$REPO/THIRD-PARTY-LICENSES.txt" "$C/repo/"
  cp -r "$REPO/packaging/licenses" "$REPO/packaging/icons" "$C/repo/packaging/"
  cp "$REPO/packaging/pins.env" "$C/repo/packaging/"
  cp "$REPO/packaging/windows/make-installer.sh" "$REPO/packaging/windows/rustypresenter.nsi" "$C/repo/packaging/windows/"
  echo "exe" >"$C/build/rustypresenter.exe"
  # The exe imports libstdc++, which imports libgcc_s and winpthread; KERNEL32 is Windows' own.
  cat >"$C/tools/mingw64/bin/objdump.exe" <<'EOF'
#!/usr/bin/env bash
case "$(basename "$2")" in
  rustypresenter.exe) printf '\tDLL Name: KERNEL32.dll\r\n\tDLL Name: libstdc++-6.dll\r\n' ;;
  libstdc++-6.dll) printf '\tDLL Name: libgcc_s_seh-1.dll\n\tDLL Name: libwinpthread-1.dll\n\tDLL Name: msvcrt.dll\n' ;;
  libgcc_s_seh-1.dll) printf '\tDLL Name: libwinpthread-1.dll\n' ;;
esac
EOF
  chmod +x "$C/tools/mingw64/bin/objdump.exe"
  for d in libstdc++-6.dll libgcc_s_seh-1.dll libwinpthread-1.dll libunused-1.dll; do echo "$d" >"$C/tools/mingw64/bin/$d"; done
  : >"$T/log/curl.txt"
}
pin() { sed -i "s|^$1=.*|$1=$2|" "$C/repo/packaging/pins.env"; }
# The cases start from the first-run state, whatever the real pins.env holds now.
unpin_all() { pin NSIS_SHA256 TODO; pin MPV_COPYRIGHT_SHA256 TODO; }
pin_all() {
  pin NSIS_SHA256 "$(sha "$F/nsis-3.11.zip")"
  pin PDFIUM_SHA256_WINDOWS "$(sha "$F/pdfium-win-x64.tgz")"
  pin MPV_COPYRIGHT_SHA256 "$(sha "$F/Copyright")"
}
run() {
  RP_EXE="$C/build/rustypresenter.exe" RP_TOOLS="$C/tools" RP_OUT_DIR="$C/desktop" \
    RP_MINGW_BIN="$C/tools/mingw64/bin" RP_DEPS="$C/deps" RP_XFER="$C/tools/xfer/make-installer" \
    RP_LOG="$C/tools/xfer/make-installer/installer.txt" \
    bash "$C/repo/packaging/windows/make-installer.sh" "$@" >"$C/out.txt" 2>&1
}
SETUP="RustyPresenter-Setup-$(sed -n 's/^version = "\([^"]*\)".*/\1/p' "$REPO/Cargo.toml" | head -n 1).exe"

# ---- 1. dry run: prints every step, touches nothing
new_case dry
unpin_all
echo dll >"$C/deps/mpv/libmpv-2.dll"
(cd "$C" && find . | sort) >"$T/before.txt"
code=0; run --dry-run || code=$?
(cd "$C" && find . -path ./out.txt -prune -o -print | sort) >"$T/after.txt"
check "dry run exits 0" test "$code" = 0
check "dry run changes no file" cmp -s "$T/before.txt" "$T/after.txt"
check "dry run downloads nothing" test ! -s "$T/log/curl.txt"
for n in 1 2 3 4 5; do check "dry run prints step $n" has "$C/out.txt" "== Step $n:"; done
check "dry run names the NSIS pin" has "$C/out.txt" "NSIS_SHA256 is 'TODO'"
check "dry run names the setup file" has "$C/out.txt" "$SETUP"
check "dry run says it changed nothing" has "$C/out.txt" "dry run finished; nothing was changed"

# ---- 2. TODO pins: downloads them only to show their hashes, then stops before building
new_case unpinned
unpin_all
echo dll >"$C/deps/mpv/libmpv-2.dll"
pin PDFIUM_SHA256_WINDOWS "$(sha "$F/pdfium-win-x64.tgz")"
code=0; run || code=$?
H="$C/tools/xfer/make-installer/hashes.txt"
check "unpinned run exits 3" test "$code" = 3
check "hashes.txt has the NSIS sha256" has "$H" "sha256 $(sha "$F/nsis-3.11.zip")"
check "hashes.txt has the NSIS sha1" has "$H" "sha1   $(sha1sum "$F/nsis-3.11.zip" | cut -d' ' -f1)"
check "hashes.txt has the mpv Copyright" has "$H" "MPV_COPYRIGHT_SHA256"
check "pinned pdfium is not listed" bash -c "! grep -q PDFIUM '$H'"
check "no setup file" test ! -e "$C/desktop/$SETUP"
check "unverified download kept apart" test -f "$C/tools/installer/downloads/unverified/nsis-3.11.zip"
check "nothing unpacked from it" test ! -e "$C/tools/installer/nsis-3.11"
check "placeholder left alone" grep -q RUSTYPRESENTER-LICENCE-PLACEHOLDER "$C/repo/packaging/licenses/mpv-Copyright.txt"
check "the log has the output" has "$C/tools/xfer/make-installer/installer.txt" "STOPPED before building"

# ---- 3. all pinned, libmpv from this PC: a setup file, with the MinGW runtime and the real licence texts
new_case local
echo dll >"$C/deps/mpv/libmpv-2.dll"
pin_all
code=0; run || { code=$?; cat "$C/out.txt"; }
S="$C/tools/installer/work/stage"
check "local run exits 0" test "$code" = 0
check "setup file on the Desktop" test -s "$C/desktop/$SETUP"
check "setup-path.txt names it" has "$C/tools/xfer/make-installer/setup-path.txt" "$SETUP"
for f in rustypresenter.exe pdfium.dll libmpv-2.dll libstdc++-6.dll libgcc_s_seh-1.dll libwinpthread-1.dll \
  LICENSE-MIT LICENSE-APACHE licenses/THIRD-PARTY-LICENSES.txt licenses/pdfium-LICENSE.txt licenses/mpv-Copyright.txt \
  licenses/FFmpeg-COPYING.LGPLv2.1.txt licenses/pdfium-abseil.txt licenses/libmpv-NOTE.txt; do
  check "staged $f" test -s "$S/$f"
done
check "an unneeded MinGW DLL is left out" test ! -e "$S/libunused-1.dll"
check "Windows' own DLLs are not copied" test ! -e "$S/KERNEL32.dll"
check "native.txt is not staged" test ! -e "$S/licenses/native.txt"
check "mpv-Copyright.txt filled in the repo" cmp -s "$F/Copyright" "$C/repo/packaging/licenses/mpv-Copyright.txt"
check "pdfium-LICENSE.txt filled in the repo" cmp -s "$F/pdfium/LICENSE" "$C/repo/packaging/licenses/pdfium-LICENSE.txt"
check "copies for Claude" test -s "$C/tools/xfer/make-installer/licenses/pdfium-LICENSE.txt"
N="$C/tools/installer/work/files.nsh"; U="$C/tools/installer/work/unfiles.nsh"
check "files.nsh starts in INSTDIR" has "$N" 'SetOutPath "$INSTDIR"'
check "files.nsh has the licences folder" has "$N" 'SetOutPath "$INSTDIR\\licenses"'
check "files.nsh has the exe" has "$N" "File \"$S/rustypresenter.exe\""
check "unfiles.nsh deletes the exe" has "$U" 'Delete "$INSTDIR\\rustypresenter.exe"'
check "unfiles.nsh deletes a licence" has "$U" 'Delete "$INSTDIR\\licenses\\pdfium-LICENSE.txt"'
check "unfiles.nsh removes the empty folder" has "$U" 'RMDir "$INSTDIR\\licenses"'
check "unfiles.nsh never removes a folder recursively" bash -c "! grep -q 'RMDir /r' '$U'"
check "same number of File and Delete lines" test "$(grep -c '^File ' "$N")" = "$(grep -c '^Delete ' "$U")"
A="$T/log/makensis-args.txt"
check "makensis gets the version" has "$A" "-DVERSION=0.2.0"
check "makensis gets a four-part version" has "$A" "-DVIVERSION=0.2.0.0"
check "makensis builds rustypresenter.nsi" has "$A" "rustypresenter.nsi"
# Again: everything is reused, nothing downloaded.
: >"$T/log/curl.txt"
code=0; run || code=$?
check "second run exits 0" test "$code" = 0
check "second run downloads nothing" test ! -s "$T/log/curl.txt"

# ---- 4. a changed download: refused and removed
new_case mismatch
pin_all
pin PDFIUM_SHA256_WINDOWS "$(printf '0%.0s' {1..64})"
code=0; run || code=$?
check "mismatch exits 1" test "$code" = 1
check "mismatch is reported" has "$C/out.txt" "sha256 mismatch for pdfium-win-x64.tgz"
check "mismatched file removed" test ! -e "$C/tools/installer/downloads/pdfium-win-x64.tgz"
check "no setup file after a mismatch" test ! -e "$C/desktop/$SETUP"

# ---- 5. the pinned LGPL release: its notices are shipped, and a GPL build is refused
new_case release
pin_all
pin LIBMPV_SHA256_WINDOWS "$(sha "$F/$LIBMPV_ZIP_NAME")"
code=0; run || { code=$?; cat "$C/out.txt"; }
S="$C/tools/installer/work/stage"
check "release run exits 0" test "$code" = 0
check "release libmpv staged" cmp -s "$F/libmpv/libmpv-2.dll" "$S/libmpv-2.dll"
check "mpv Copyright from the zip" cmp -s "$F/libmpv/Copyright" "$S/licenses/mpv-Copyright.txt"
check "libmpv notices staged" test -s "$S/licenses/libmpv-dav1d-COPYING.txt"
check "BUILDINFO staged" test -s "$S/licenses/libmpv-BUILDINFO.txt"
check "no local-libmpv note" test ! -e "$S/licenses/libmpv-NOTE.txt"
check "mpv Copyright not downloaded on its own" bash -c "! grep -q raw.githubusercontent '$T/log/curl.txt'"

new_case gpl
pin_all
pin LIBMPV_SHA256_WINDOWS "$(sha "$F/gpl-$LIBMPV_ZIP_NAME")"
code=0; FAKE_PREFIX=gpl- run || code=$?
check "GPL libmpv refused" test "$code" = 1
check "GPL refusal says why" has "$C/out.txt" "is not an LGPL build"

# ---- 6. no libmpv anywhere: the installer is still made, without video
new_case none
pin_all
code=0; run || code=$?
check "no-libmpv run exits 0" test "$code" = 0
check "no-libmpv run warns" has "$C/out.txt" "will not play video"
check "no libmpv staged" test ! -e "$C/tools/installer/work/stage/libmpv-2.dll"
check "no mpv Copyright staged" test ! -e "$C/tools/installer/work/stage/licenses/mpv-Copyright.txt"

# ---- 7. the app is not built
new_case nobuild
pin_all
rm "$C/build/rustypresenter.exe"
code=0; run || code=$?
check "unbuilt app exits 1" test "$code" = 1
check "unbuilt app is reported" has "$C/out.txt" "the app is not built"

echo
if ((FAILS)); then echo "$FAILS check(s) failed"; exit 1; fi
echo "all checks passed"
