#!/usr/bin/env bash
# Tests for release-upload.sh, on Linux or in Git Bash, with no network: `gh` is a small stand-in that keeps a
# release as files in a folder.
#
#   packaging/test-release-upload.sh
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
FAILS=0
ok() { echo "ok   $*"; }
bad() { echo "FAIL $*"; FAILS=$((FAILS + 1)); }
check() { local what="$1"; shift; if "$@"; then ok "$what"; else bad "$what"; fi; }

# ---- the stand-in gh: $FAKE/assets.txt lists the release's files when the release exists
cat >"$T/gh" <<'EOF'
#!/usr/bin/env bash
[[ "$1" == release ]] || { echo "fake gh: $*" >&2; exit 2; }
case "$2" in
  view)
    [[ -f "$FAKE/assets.txt" ]] || exit 1
    if [[ " $* " == *" --json assets "* ]]; then cat "$FAKE/assets.txt"; fi ;;
  create)
    : >"$FAKE/assets.txt"
    echo "$*" >>"$FAKE/created.txt" ;;
  upload)
    shift 2
    for a in "$@"; do
      case "$a" in --repo|--clobber) ;; */*) basename "$a" >>"$FAKE/uploaded.txt"; basename "$a" >>"$FAKE/assets.txt" ;; esac
    done ;;
  *) echo "fake gh: $*" >&2; exit 2 ;;
esac
EOF
chmod +x "$T/gh"

V=9.9.9
BUILT=(RustyPresenter-$V-windows-setup.exe RustyPresenter-$V-windows-portable.zip RustyPresenter-$V-macos.dmg
  RustyPresenter-$V-linux.AppImage RustyPresenter-$V-android-remote.apk THIRD-PARTY-LICENSES.txt)

# new_case <name>: a folder of files built here, and no release yet
new_case() {
  C="$T/$1"
  mkdir -p "$C/release" "$C/gh"
  for f in "${BUILT[@]}"; do echo "$f" >"$C/release/$f"; done
}
run() { FAKE="$C/gh" GH="$T/gh" bash "$REPO/packaging/release-upload.sh" "v$V" owner/repo "$C/release" >"$C/out.txt" 2>&1; }
uploaded() { grep -qx -- "$1" "$C/gh/uploaded.txt"; }
not_uploaded() { ! grep -qx -- "$1" "$C/gh/uploaded.txt"; }
has() { grep -q -- "$2" "$1"; }

# ---- 1. the tag came first: no release yet. A draft is made and everything built here goes on it.
new_case missing
code=0; run || code=$?
check "no release: exits 0" test "$code" = 0
check "no release: a draft is created" has "$C/gh/created.txt" "--draft"
check "no release: the Windows installer built here is uploaded" uploaded "RustyPresenter-$V-windows-setup.exe"
check "no release: the Android app built here is uploaded" uploaded "RustyPresenter-$V-android-remote.apk"
check "no release: the Mac package is uploaded" uploaded "RustyPresenter-$V-macos.dmg"

# ---- 2. the release holds the PC-built installer and app: the ones built here are left out
new_case pc
printf '%s\n' "RustyPresenter-Setup-$V.exe" "RustyPresenter-Remote-$V.apk" >"$C/gh/assets.txt"
code=0; run || code=$?
check "PC files: exits 0" test "$code" = 0
check "PC files: no release is created" test ! -e "$C/gh/created.txt"
check "PC files: the Windows installer built here is left out" not_uploaded "RustyPresenter-$V-windows-setup.exe"
check "PC files: the Android app built here is left out" not_uploaded "RustyPresenter-$V-android-remote.apk"
check "PC files: the portable zip is still uploaded" uploaded "RustyPresenter-$V-windows-portable.zip"
check "PC files: the Linux package is still uploaded" uploaded "RustyPresenter-$V-linux.AppImage"

# ---- 3. the release exists but only the installer came from a PC: the Android app built here is uploaded
new_case half
printf '%s\n' "RustyPresenter-Setup-$V.exe" >"$C/gh/assets.txt"
run
check "half: the Windows installer built here is left out" not_uploaded "RustyPresenter-$V-windows-setup.exe"
check "half: the Android app built here is uploaded" uploaded "RustyPresenter-$V-android-remote.apk"

# ---- 4. an empty release and nothing built for Windows or Android: it says so, and still uploads the rest
new_case neither
: >"$C/gh/assets.txt"
rm "$C/release/RustyPresenter-$V-windows-setup.exe" "$C/release/RustyPresenter-$V-android-remote.apk"
code=0; run || code=$?
check "neither: exits 0" test "$code" = 0
check "neither: warns about the Windows installer" has "$C/out.txt" "::warning::The release will have no Windows installer"
check "neither: warns about the Android app" has "$C/out.txt" "::warning::The release will have no Android app"
check "neither: the rest is uploaded" uploaded "RustyPresenter-$V-macos.dmg"

# ---- 5. nothing at all to upload is an error
new_case empty
rm "$C/release"/*
code=0; run || code=$?
check "empty: exits 1" test "$code" = 1

echo
if ((FAILS)); then echo "$FAILS check(s) failed"; exit 1; fi
echo "all checks passed"
