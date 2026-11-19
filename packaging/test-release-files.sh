#!/usr/bin/env bash
# Tests for release-files.sh, on Linux or in Git Bash, with no network and no real build: the program, the
# libraries and cargo-packager's packages are small fakes, and so are `zip` and macOS's `ditto` (Python's zipfile
# does their work). The script runs on a copy of the repo's packaging files.
#
#   packaging/test-release-files.sh
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
FAILS=0
ok() { echo "ok   $*"; }
bad() { echo "FAIL $*"; FAILS=$((FAILS + 1)); }
check() { local what="$1"; shift; if "$@"; then ok "$what"; else bad "$what"; fi; }
PY="$(command -v python3 || command -v python)"

# ---- a copy of the repo's packaging files
R="$T/repo"
mkdir -p "$R/packaging" "$R/target/release" "$R/packaging/native" "$R/packaging/native-licenses" "$R/packaging/out/libmpv-linux"
cp "$REPO/packaging/release-files.sh" "$R/packaging/"
cp -R "$REPO/packaging/portable" "$R/packaging/"
cp "$REPO/Packager.toml" "$REPO/LICENSE-MIT" "$REPO/LICENSE-APACHE" "$REPO/THIRD-PARTY-LICENSES.txt" "$R/"
VERSION="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$REPO/Packager.toml" | head -n 1)"
echo "pdfium notice" >"$R/packaging/native-licenses/pdfium-LICENSE"

# ---- fake tools: zip -qr -X <out> <folder>, ditto <from> <to>, ditto -c -k --keepParent <folder> <zip>
B="$T/bin"
mkdir -p "$B"
cat >"$B/zipit.py" <<'EOF'
import os, sys, zipfile
out, folder = sys.argv[1], sys.argv[2]
base = os.path.dirname(os.path.abspath(folder))
with zipfile.ZipFile(out, "w") as z:
    for top, _, files in os.walk(folder):
        for f in files:
            p = os.path.join(top, f)
            z.write(p, os.path.relpath(p, base).replace(os.sep, "/"))
EOF
cat >"$B/zip" <<EOF
#!/usr/bin/env bash
# -qr -X <out> <folder>
"$PY" "$B/zipit.py" "\$3" "\$4"
EOF
cat >"$B/ditto" <<EOF
#!/usr/bin/env bash
if [[ "\$1" == "-c" ]]; then "$PY" "$B/zipit.py" "\$5" "\$4"; else cp -R "\$1" "\$2"; fi
EOF
chmod +x "$B/zip" "$B/ditto"
export PATH="$B:$PATH"

zip_names() { "$PY" -c 'import sys, zipfile; print("\n".join(zipfile.ZipFile(sys.argv[1]).namelist()))' "$1"; }
listed() { grep -qx -- "$2" <<<"$1"; }

# ---- rename: cargo-packager's names become the release names
mkdir -p "$R/dist"
touch "$R/dist/RustyPresenter_${VERSION}_x64-setup.exe" "$R/dist/RustyPresenter_${VERSION}_aarch64.dmg" \
  "$R/dist/rustypresenter_${VERSION}_x86_64.AppImage" "$R/dist/rustypresenter_${VERSION}_amd64.deb"
"$R/packaging/release-files.sh" rename windows >/dev/null
"$R/packaging/release-files.sh" rename macos >/dev/null
"$R/packaging/release-files.sh" rename linux >/dev/null
for f in windows-setup.exe macos.dmg linux.AppImage linux.deb; do
  check "rename gives RustyPresenter-$VERSION-$f" test -f "$R/dist/RustyPresenter-$VERSION-$f"
done
check "rename leaves no old name" test "$(ls "$R/dist" | grep -c '_')" -eq 0
check "rename twice changes nothing" "$R/packaging/release-files.sh" rename linux
touch "$R/dist/second.deb"
if "$R/packaging/release-files.sh" rename linux >/dev/null 2>&1; then bad "two .deb files stop the rename"; else ok "two .deb files stop the rename"; fi
rm "$R/dist/second.deb"

# ---- portable windows
if "$R/packaging/release-files.sh" portable windows >/dev/null 2>&1; then bad "no exe stops it"; else ok "no exe stops it"; fi
echo "exe" >"$R/target/release/rustypresenter.exe"
if "$R/packaging/release-files.sh" portable windows >/dev/null 2>&1; then bad "no libmpv stops it"; else ok "no libmpv stops it"; fi
echo "mpv" >"$R/packaging/native/libmpv-2.dll"
echo "pdf" >"$R/packaging/native/pdfium.dll"
"$R/packaging/release-files.sh" portable windows >/dev/null
Z="$R/dist/RustyPresenter-$VERSION-windows-portable.zip"
check "windows portable zip is made" test -f "$Z"
N="$(zip_names "$Z")"
for f in rustypresenter.exe libmpv-2.dll pdfium.dll portable.txt HOW-TO-START.txt LICENSE-MIT LICENSE-APACHE \
  licenses/THIRD-PARTY-LICENSES.txt licenses/pdfium-LICENSE; do
  check "windows zip has RustyPresenter/$f" listed "$N" "RustyPresenter/$f"
done
check "windows readme is the Windows one" grep -q "rustypresenter.exe" "$R/packaging/out/portable-windows/RustyPresenter/HOW-TO-START.txt"

# ---- portable linux
# Out of target/release: Git Bash treats "rustypresenter" as the "rustypresenter.exe" made above.
mkdir -p "$T/linux-build"
# Starts like a script so Git Bash also sees it as a program (Linux goes by the chmod).
printf '#!/bin/sh\n# elf\n' >"$T/linux-build/rustypresenter"
export RP_BIN="$T/linux-build/rustypresenter"
echo "pdf" >"$R/packaging/native/libpdfium.so"
if "$R/packaging/release-files.sh" portable linux >/dev/null 2>&1; then bad "no libmpv.so.2 stops it"; else ok "no libmpv.so.2 stops it"; fi
echo "mpv" >"$R/packaging/out/libmpv-linux/libmpv.so.2"
"$R/packaging/release-files.sh" portable linux >/dev/null
G="$R/dist/RustyPresenter-$VERSION-linux-portable.tar.gz"
check "linux portable tar.gz is made" test -f "$G"
L="$(tar -tzf "$G")"
for f in rustypresenter RustyPresenter.sh libmpv.so.2 libpdfium.so portable.txt HOW-TO-START.txt licenses/THIRD-PARTY-LICENSES.txt; do
  check "linux tar has RustyPresenter/$f" listed "$L" "RustyPresenter/$f"
done
V="$(tar -tvzf "$G")"
check "the launcher can be run" grep -Eq '^-rwx.* RustyPresenter/RustyPresenter.sh$' <<<"$V"
check "the program can be run" grep -Eq '^-rwx.* RustyPresenter/rustypresenter$' <<<"$V"
check "the launcher finds the libraries beside it" grep -q 'LD_LIBRARY_PATH="$here' "$R/packaging/portable/RustyPresenter.sh"

# The launcher runs the program beside it with the folder on LD_LIBRARY_PATH (a fake program prints it).
X="$T/launch"
mkdir -p "$X"
cp "$R/packaging/portable/RustyPresenter.sh" "$X/"
printf '#!/bin/sh\necho "$LD_LIBRARY_PATH|$1"\n' >"$X/rustypresenter"
chmod +x "$X/RustyPresenter.sh" "$X/rustypresenter"
out="$(LD_LIBRARY_PATH=/old "$X/RustyPresenter.sh" --restore)"
check "the launcher passes the libraries' folder and the arguments" test "$out" = "$(cd "$X" && pwd):/old|--restore"

unset RP_BIN

# ---- portable macos
if "$R/packaging/release-files.sh" portable macos >/dev/null 2>&1; then bad "no .app stops it"; else ok "no .app stops it"; fi
mkdir -p "$R/dist/RustyPresenter.app/Contents/MacOS"
echo "mach-o" >"$R/dist/RustyPresenter.app/Contents/MacOS/rustypresenter"
"$R/packaging/release-files.sh" portable macos >/dev/null
M="$R/dist/RustyPresenter-$VERSION-macos-portable.zip"
check "macos portable zip is made" test -f "$M"
N="$(zip_names "$M")"
for f in RustyPresenter.app/Contents/MacOS/rustypresenter portable.txt HOW-TO-START.txt LICENSE-MIT; do
  check "macos zip has RustyPresenter/$f" listed "$N" "RustyPresenter/$f"
done
check "the marker is beside the .app, not in it" bash -c "! grep -q 'RustyPresenter.app/.*portable.txt' <<<'$N'"

# ---- the marker's name is the app's (src/platform/data_folder.rs)
check "the app looks for portable.txt" grep -q 'pub const MARKER: &str = "portable.txt";' "$REPO/src/platform/data_folder.rs"
check "the app's data folder is the one the readmes name" grep -q 'pub const FOLDER: &str = "RustyPresenter Data";' "$REPO/src/platform/data_folder.rs"
for os in windows macos linux; do
  check "the $os readme names the data folder" grep -q '"RustyPresenter Data"' "$REPO/packaging/portable/HOW-TO-START-$os.txt"
done

echo
if [[ $FAILS -eq 0 ]]; then echo "all release-files tests passed"; else echo "$FAILS release-files test(s) failed"; exit 1; fi
