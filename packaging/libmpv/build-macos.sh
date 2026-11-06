#!/usr/bin/env bash
# LGPL libmpv.2.dylib for macOS arm64 (Apple silicon only; Intel Macs are not supported).
# Needs Xcode command line tools plus Homebrew meson, ninja, nasm and pkgconf.
# Never install Homebrew's ffmpeg or mpv: everything below is built from pinned sources.

# Before anything is built: without this a libmpv built on macOS 15 does not load on 12-14.
export MACOSX_DEPLOYMENT_TARGET=12.0
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || die "build-macos.sh builds on Apple silicon macOS only"
if command -v brew >/dev/null 2>&1; then
  for formula in ffmpeg mpv; do
    if brew list --formula "$formula" >/dev/null 2>&1; then die "Homebrew's $formula is installed; remove it"; fi
  done
fi

# For build systems that ignore MACOSX_DEPLOYMENT_TARGET (meson reads these at setup).
MINOS_FLAG="-mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET"
export CFLAGS="$MINOS_FLAG" CXXFLAGS="$MINOS_FLAG" OBJCFLAGS="$MINOS_FLAG" LDFLAGS="$MINOS_FLAG"
# Only our own prefix is visible to pkg-config, so no Homebrew library can slip in.
export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig"
unset PKG_CONFIG_PATH

start_build
note "MACOSX_DEPLOYMENT_TARGET=$MACOSX_DEPLOYMENT_TARGET, Xcode $(xcodebuild -version 2>/dev/null | head -n 1)"
fetch_sources
build_deps

# FFmpeg 9 may no longer offer SecureTransport; without a TLS backend, no network at all.
tls=(--disable-network)
ffmpeg_help="$("$SRC/ffmpeg/configure" --help)"
if [[ "$ffmpeg_help" == *securetransport* ]]; then
  tls=(--enable-securetransport)
fi
note "ffmpeg TLS on macOS: ${tls[*]}"
build_ffmpeg --enable-pthreads --enable-videotoolbox --enable-audiotoolbox \
  --extra-cflags="$MINOS_FLAG" --extra-ldflags="$MINOS_FLAG" "${tls[@]}"

# mpv 0.41 only offers videotoolbox-gl through gl-cocoa, which needs the cocoa feature
# (the Cocoa/IOKit/QuartzCore frameworks); -Dcocoa=disabled would fail at setup. With cocoa,
# mpv 0.41 always compiles player/clipboard/clipboard-mac.m, which includes the header the
# Swift build generates, so Swift is on; cocoa-cb (mpv's own window) stays off.
build_mpv -Dcocoa=enabled -Dgl-cocoa=enabled -Dswift-build=enabled -Dmacos-cocoa-cb=disabled \
  -Dvideotoolbox-gl=enabled

if grep -q "built for newer macOS version" "$BUILD/mpv-compile.txt"; then
  die "an object was built for a macOS newer than $MACOSX_DEPLOYMENT_TARGET (see mpv-compile.txt)"
fi

[[ -f "$PREFIX/lib/libmpv.2.dylib" ]] || die "libmpv.2.dylib was not built"
# A copy (cp follows a symlink) whose install name becomes @rpath/libmpv.2.dylib.
cp "$PREFIX/lib/libmpv.2.dylib" "$BUILD/libmpv.2.dylib"
LIB="$BUILD/libmpv.2.dylib"
chmod u+w "$LIB"
install_name_tool -id @rpath/libmpv.2.dylib "$LIB"
check_strings "$LIB"

# Only system libraries and frameworks.
otool -L "$LIB" | tee "$BUILD/otool-L.txt"
note ""
note "== otool -L libmpv.2.dylib"
cat "$BUILD/otool-L.txt" >> "$BUILDINFO"
outside="$(tail -n +2 "$BUILD/otool-L.txt" | awk '{ print $1 }' \
  | grep -v -e '^/usr/lib/' -e '^/System/Library/' -e '^@rpath/libmpv.2.dylib$' || true)"
[[ -z "$outside" ]] || die "libmpv.2.dylib links non-system libraries: $outside"

# Loads on macOS 12: the library's LC_BUILD_VERSION says minos 12.0.
minos="$(otool -l "$LIB" | awk '/LC_BUILD_VERSION/ { f = 1 } f && !done && $1 == "minos" { print $2; done = 1 }')"
note "otool -l libmpv.2.dylib: minos $minos"
[[ "$minos" == "$MACOSX_DEPLOYMENT_TARGET" ]] || die "libmpv.2.dylib has minos '$minos', expected $MACOSX_DEPLOYMENT_TARGET"

make_artifact macos arm64 "$LIB"
make_sources
