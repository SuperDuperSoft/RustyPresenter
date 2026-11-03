#!/usr/bin/env bash
# LGPL libmpv.so.2 for Linux x86_64, built on Ubuntu 22.04 so the AppImage runs on older
# glibc. Needs the apt packages from .github/workflows/libmpv.yml and meson >= 1.3 from pip.
# Never install the distro's ffmpeg or mpv development packages: FFmpeg is built here.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || die "build-linux.sh builds on x86_64 Linux only"
for pkg in libavcodec-dev libavformat-dev libmpv-dev; do
  if dpkg -s "$pkg" >/dev/null 2>&1; then die "$pkg is installed; remove it so the system FFmpeg cannot be linked"; fi
done

# Our prefix first; the system's .pc files are still needed for X11, ALSA, PulseAudio and libva.
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

start_build
note "glibc $(ldd --version | head -n 1)"
fetch_sources
build_deps
build_ffmpeg --enable-pthreads --enable-vaapi
build_mpv

LIB="$PREFIX/lib/libmpv.so.2"
[[ -e "$LIB" ]] || die "libmpv.so.2 was not built"
# The real file, named by its soname, for the zip.
cp -L "$LIB" "$BUILD/libmpv.so.2"
LIB="$BUILD/libmpv.so.2"
check_strings "$LIB"

# Only system libraries: nothing missing, nothing from our prefix, and no shared FFmpeg
# (that would be the distro's build, possibly GPL).
ldd "$LIB" | tee "$BUILD/ldd.txt"
note ""
note "== ldd libmpv.so.2"
cat "$BUILD/ldd.txt" >> "$BUILDINFO"
if grep -q "not found" "$BUILD/ldd.txt"; then die "libmpv.so.2 has unresolved libraries"; fi
if grep -qF "$WORK" "$BUILD/ldd.txt"; then die "libmpv.so.2 links a library from $WORK"; fi
if grep -Eq "lib(avcodec|avformat|avutil|avfilter|swscale|swresample|postproc|dav1d|ass|placebo)\.so" "$BUILD/ldd.txt"; then
  die "libmpv.so.2 links a shared FFmpeg or dependency instead of the static one"
fi

make_artifact linux x86_64 "$LIB"
make_sources
