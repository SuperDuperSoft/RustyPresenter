#!/usr/bin/env bash
# LGPL libmpv-2.dll for Windows x86_64. Runs in an MSYS2 UCRT64 shell with the toolchain
# from .github/workflows/libmpv.yml (gcc, meson, ninja, nasm, pkgconf, zlib, binutils).
# Never install MSYS2's ffmpeg or mpv packages: everything below is built from pinned sources.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

[[ "${MSYSTEM:-}" == UCRT64 ]] || die "run this in an MSYS2 UCRT64 shell (MSYSTEM=$MSYSTEM)"
if pacman -Qq mingw-w64-ucrt-x86_64-ffmpeg >/dev/null 2>&1 || pacman -Qq mingw-w64-ucrt-x86_64-mpv >/dev/null 2>&1; then
  die "MSYS2's ffmpeg or mpv package is installed; remove it"
fi

# Only our own prefix is visible to pkg-config, so no MSYS2 package can slip in.
export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig"
unset PKG_CONFIG_PATH

start_build
fetch_sources
build_deps
# SChannel for https; D3D11VA/DXVA2 hardware decoding; native Windows threads.
build_ffmpeg --enable-w32threads --enable-schannel --enable-d3d11va --enable-dxva2
# Only static zlib and iconv may link: with MSYS2's import libraries also on the search path, -lz picked
# libz.dll.a and -static then pulled in libz.a too (every inflate* defined twice), and -liconv made
# libmpv-2.dll need libiconv-2.dll. Set aside for the build, put back afterwards.
mingw_lib="$(cygpath -u "${MINGW_PREFIX:-/ucrt64}")/lib"
aside=()
for import_lib in "$mingw_lib/libz.dll.a" "$mingw_lib/libiconv.dll.a"; do
  if [[ -f "$import_lib" ]]; then mv "$import_lib" "$import_lib.rp-aside"; aside+=("$import_lib"); fi
done
trap 'for f in "${aside[@]}"; do mv "$f.rp-aside" "$f" 2>/dev/null || true; done' EXIT
# -static links libgcc, libstdc++ and winpthreads into the DLL, so it needs only system DLLs.
build_mpv -Dc_link_args=-static -Dcpp_link_args=-static

LIB="$PREFIX/bin/libmpv-2.dll"
[[ -f "$LIB" ]] || die "$LIB was not built"
check_strings "$LIB"

# Every DLL libmpv loads must come from Windows itself.
ldd "$LIB" | tee "$BUILD/ldd.txt"
note ""
note "== ldd libmpv-2.dll"
cat "$BUILD/ldd.txt" >> "$BUILDINFO"
outside="$(grep -v "libmpv-2.dll" "$BUILD/ldd.txt" | grep -vi "=> /c/windows/" || true)"
[[ -z "$outside" ]] || die "libmpv-2.dll needs DLLs from outside C:\\Windows: $outside"

note ""
note "== pacman -Q"
pacman -Q >> "$BUILDINFO"

# The static runtime libraries linked into the DLL, each recorded in BUILDINFO.txt.
note ""
note "== static runtime licences"
extra=()
for f in /ucrt64/share/licenses/mingw-w64-libraries/COPYING \
  /ucrt64/share/licenses/gcc-libs/RUNTIME.LIBRARY.EXCEPTION /ucrt64/share/licenses/zlib/LICENSE; do
  if [[ -f "$f" ]]; then
    cp "$f" "$BUILD/$(basename "$(dirname "$f")")-$(basename "$f").txt"
    extra+=("$BUILD/$(basename "$(dirname "$f")")-$(basename "$f").txt")
    note "found $f"
  else
    note "not found $f"
  fi
done
# -static always links winpthreads, so its notice is required. MSYS2 has shipped it under
# more than one package name; fall back to whichever package now owns it.
winpthreads=""
for f in /ucrt64/share/licenses/winpthreads/COPYING /ucrt64/share/licenses/libwinpthread/COPYING \
  /ucrt64/share/licenses/mingw-w64-winpthreads/COPYING; do
  if [[ -f "$f" ]]; then
    winpthreads="$f"
    break
  fi
done
if [[ -z "$winpthreads" ]]; then
  winpthreads="$(find /ucrt64/share/licenses -ipath '*winpthread*' -type f -iname 'COPYING*' 2>/dev/null | head -n 1)"
fi
[[ -n "$winpthreads" ]] || die "no winpthreads licence under /ucrt64/share/licenses, but -static links winpthreads into libmpv-2.dll"
# stage.sh requires this exact name in the zip's licenses/.
cp "$winpthreads" "$BUILD/winpthreads-COPYING.txt"
extra+=("$BUILD/winpthreads-COPYING.txt")
note "found $winpthreads (shipped as licenses/winpthreads-COPYING.txt)"

make_artifact windows x86_64 "$LIB" "${extra[@]}"
make_sources
