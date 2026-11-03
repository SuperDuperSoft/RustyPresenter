# Shared by build-windows.sh, build-macos.sh and build-linux.sh. Sourced, never run.
#
# Builds an LGPL libmpv: static FFmpeg (no --enable-gpl/version3/nonfree), static dav1d,
# freetype, fribidi, harfbuzz, libass and libplacebo, then a shared libmpv with
# -Dgpl=false -Dcplayer=false. Every licence check that matters fails the build.
#
# Layout under $WORK (default packaging/out/libmpv):
#   src/<name>    pinned sources (git tags from packaging/pins.env)
#   build/<name>  build folders
#   prefix/       install prefix for everything
#   dist/         what goes in the zip: library, licences, BUILDINFO.txt
#   artifacts/    libmpv-<os>-<arch>-mpv<v>-ff<v>-r<N>.zip and libmpv-sources-r<N>.tar.xz
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=../pins.env
source "$ROOT/packaging/pins.env"

# BUILD_REV (the workflow's `rev` input) overrides the pinned revision for a new build.
REV="${BUILD_REV:-$LIBMPV_REV}"
WORK="${LIBMPV_WORK:-$ROOT/packaging/out/libmpv}"
# A Windows path (D:\a\...) breaks the MSYS2 tools: tar reads "D:" as a remote host, and
# FFmpeg's out-of-tree make splits its vpath on the colon. Use the /d/a/... form.
if command -v cygpath >/dev/null 2>&1; then
  WORK="$(cygpath -u "$WORK")"
fi
SRC="$WORK/src"
BUILD="$WORK/build"
PREFIX="$WORK/prefix"
DIST="$WORK/dist"
ARTIFACTS="$WORK/artifacts"
BUILDINFO="$DIST/BUILDINFO.txt"
JOBS="$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)"

die() { echo "libmpv: $*" >&2; exit 1; }
say() { echo "libmpv: $*"; }
note() { echo "$*" >> "$BUILDINFO"; }

start_build() {
  rm -rf "$BUILD" "$PREFIX" "$DIST"
  mkdir -p "$SRC" "$BUILD" "$PREFIX" "$DIST/licenses" "$DIST/patches" "$ARTIFACTS"
  : > "$BUILDINFO"
  note "LGPL libmpv build r$REV for RustyPresenter"
  note "mpv $MPV_VERSION (+ backport $MPV_BACKPORT), FFmpeg $FFMPEG_VERSION, libplacebo $LIBPLACEBO_VERSION"
  note "dav1d $DAV1D_VERSION, freetype $FREETYPE_VERSION, harfbuzz $HARFBUZZ_VERSION, fribidi $FRIBIDI_VERSION, libass $LIBASS_VERSION"
  note "built $(date -u '+%Y-%m-%dT%H:%M:%SZ') on $(uname -srm)"
  note "compiler: $(${CC:-cc} --version 2>/dev/null | head -n 1)"
  note "meson $(meson --version), ninja $(ninja --version)"
  note ""
  note "== sources (tag, commit)"
}

# clone <name> <url> <tag> [submodule paths...]
clone() {
  local name="$1" url="$2" tag="$3"
  shift 3
  rm -rf "${SRC:?}/$name"
  git -c advice.detachedHead=false clone -q --depth 1 --branch "$tag" "$url" "$SRC/$name"
  if (($#)); then
    git -C "$SRC/$name" submodule -q update --init --depth 1 -- "$@"
  fi
  note "$name $tag $(git -C "$SRC/$name" rev-parse HEAD)"
}

fetch_sources() {
  clone ffmpeg https://github.com/FFmpeg/FFmpeg.git "n$FFMPEG_VERSION"
  clone mpv https://github.com/mpv-player/mpv.git "v$MPV_VERSION"
  clone libplacebo https://code.videolan.org/videolan/libplacebo.git "v$LIBPLACEBO_VERSION" \
    3rdparty/glad 3rdparty/jinja 3rdparty/markupsafe 3rdparty/fast_float
  clone dav1d https://code.videolan.org/videolan/dav1d.git "$DAV1D_VERSION"
  clone freetype https://gitlab.freedesktop.org/freetype/freetype.git "VER-${FREETYPE_VERSION//./-}"
  clone fribidi https://github.com/fribidi/fribidi.git "v$FRIBIDI_VERSION"
  clone harfbuzz https://github.com/harfbuzz/harfbuzz.git "$HARFBUZZ_VERSION"
  clone libass https://github.com/libass/libass.git "$LIBASS_VERSION"

  # The one backport onto mpv v0.41.0, kept in the artifact and the sources tarball.
  local patch="$DIST/patches/mpv-$MPV_BACKPORT.patch"
  curl -fsSL --retry 3 -o "$patch" "https://github.com/mpv-player/mpv/commit/$MPV_BACKPORT.patch"
  git -C "$SRC/mpv" apply --verbose "$patch"
  note "patch: mpv $MPV_BACKPORT applied (patches/mpv-$MPV_BACKPORT.patch)"
  note ""
}

# meson_build <name> [meson options...]: static library into $PREFIX.
meson_build() {
  local name="$1"
  shift
  local args=(--prefix "$PREFIX" --libdir lib --buildtype release --default-library static
    -Db_ndebug=true -Dwrap_mode=nodownload "$@")
  say "building $name"
  meson setup "$BUILD/$name" "$SRC/$name" "${args[@]}"
  note "$name: meson setup ${args[*]}"
  meson compile -C "$BUILD/$name"
  meson install -C "$BUILD/$name"
}

build_deps() {
  meson_build dav1d -Denable_tools=false -Denable_tests=false -Denable_examples=false -Denable_docs=false
  meson_build freetype -Dbrotli=disabled -Dbzip2=disabled -Dharfbuzz=disabled -Dpng=disabled \
    -Dzlib=disabled -Dtests=disabled
  meson_build fribidi -Ddocs=false -Dbin=false -Dtests=false
  meson_build harfbuzz -Dfreetype=enabled -Dglib=disabled -Dgobject=disabled -Dcairo=disabled \
    -Dchafa=disabled -Dicu=disabled -Dgraphite=disabled -Dpng=disabled -Dzlib=disabled \
    -Dtests=disabled -Dintrospection=disabled -Ddocs=disabled -Dutilities=disabled -Dbenchmark=disabled
  # libass picks its font provider per OS (DirectWrite, CoreText, fontconfig).
  meson_build libass -Dtest=disabled -Dcompare=disabled -Dprofile=disabled -Dfuzz=disabled \
    -Dcheckasm=disabled -Dlibunibreak=disabled
  # OpenGL only: the app draws through mpv's render API on a GL context.
  meson_build libplacebo -Dvulkan=disabled -Dopengl=enabled -Dd3d11=disabled -Dglslang=disabled \
    -Dshaderc=disabled -Dlcms=disabled -Dlibdovi=disabled -Dunwind=disabled -Dxxhash=disabled \
    -Ddemos=false -Dtests=false -Dbench=false -Dfuzz=false
}

# build_ffmpeg [configure options for this OS...]
# --disable-autodetect means nothing is picked up by accident; every library and the
# threading model is enabled by name.
build_ffmpeg() {
  local arg
  for arg in "$@"; do
    case "$arg" in
      --enable-gpl | --enable-version3 | --enable-nonfree) die "FFmpeg must not be built with $arg" ;;
    esac
  done
  local args=(--prefix="$PREFIX" --enable-static --disable-shared --enable-pic
    --disable-programs --disable-doc --disable-avdevice --disable-debug
    --disable-autodetect --pkg-config-flags=--static --enable-libdav1d --enable-zlib "$@")
  say "building FFmpeg"
  mkdir -p "$BUILD/ffmpeg"
  (cd "$BUILD/ffmpeg" && "$SRC/ffmpeg/configure" "${args[@]}") | tee "$BUILD/ffmpeg-configure.txt"
  note "ffmpeg: configure ${args[*]}"

  grep -q "License: LGPL version 2.1 or later" "$BUILD/ffmpeg-configure.txt" \
    || die "FFmpeg configure did not print 'License: LGPL version 2.1 or later'"
  grep -q "^#define CONFIG_GPL 0" "$BUILD/ffmpeg/config.h" || die "FFmpeg config.h lacks '#define CONFIG_GPL 0'"
  grep -q "^#define CONFIG_VERSION3 0" "$BUILD/ffmpeg/config.h" || die "FFmpeg config.h lacks '#define CONFIG_VERSION3 0'"
  grep -q "^#define CONFIG_NONFREE 0" "$BUILD/ffmpeg/config.h" || die "FFmpeg config.h lacks '#define CONFIG_NONFREE 0'"
  # Autodetect also switches threads off; a single-threaded decoder would stutter on 4K.
  if grep -Eq "^threading support +no" "$BUILD/ffmpeg-configure.txt"; then
    die "FFmpeg was configured without threads"
  fi
  note "ffmpeg: License: LGPL version 2.1 or later; CONFIG_GPL 0, CONFIG_VERSION3 0, CONFIG_NONFREE 0"

  make -C "$BUILD/ffmpeg" -j"$JOBS"
  make -C "$BUILD/ffmpeg" install
}

# Our libraries are static only, so their .pc files must hand out their private
# dependencies too. Merging Requires.private/Libs.private into Requires/Libs does that
# without meson's prefer_static, which would also try to link the system's X11 or ALSA
# statically.
flatten_pc() {
  local pc
  for pc in "$PREFIX"/lib/pkgconfig/*.pc; do
    awk '
      /^Requires(\.private)?:/ { sub(/^Requires(\.private)?: */, ""); req = req " " $0; next }
      /^Libs(\.private)?:/ { sub(/^Libs(\.private)?: */, ""); libs = libs " " $0; next }
      { print }
      END { print "Requires:" req; print "Libs:" libs }' "$pc" > "$pc.tmp"
    mv "$pc.tmp" "$pc"
  done
  note "pkg-config: Requires.private and Libs.private merged into Requires and Libs for static linking"
}

# build_mpv [meson options for this OS...]
build_mpv() {
  flatten_pc
  local args=(--prefix "$PREFIX" --libdir lib --buildtype release --default-library shared
    -Dwrap_mode=nodownload -Db_ndebug=true
    -Dgpl=false -Dcplayer=false -Dlibmpv=true -Dbuild-date=false
    -Dlua=disabled -Djavascript=disabled -Dlibarchive=disabled -Dlibbluray=disabled
    -Ddvdnav=disabled -Dcdda=disabled -Drubberband=disabled -Dvapoursynth=disabled
    -Dlibavdevice=disabled -Dmanpage-build=disabled -Dhtml-build=disabled -Dpdf-build=disabled
    "$@")
  say "building mpv"
  meson setup "$BUILD/mpv" "$SRC/mpv" "${args[@]}"
  note "mpv: meson setup ${args[*]}"
  # mpv's meson writes every feature as HAVE_<NAME> (set10), so -Dgpl=false is HAVE_GPL 0.
  if ! grep -q "^#define HAVE_GPL 0" "$BUILD/mpv/config.h"; then
    grep -n "GPL" "$BUILD/mpv/config.h" >&2 || true
    die "mpv config.h lacks '#define HAVE_GPL 0'"
  fi
  note "mpv: config.h has #define HAVE_GPL 0"
  meson compile -C "$BUILD/mpv" 2>&1 | tee "$BUILD/mpv-compile.txt"
  meson install -C "$BUILD/mpv"
}

# check_strings <library>: no GPL, version3 or nonfree FFmpeg configuration inside.
check_strings() {
  local lib="$1" flag count
  for flag in --enable-gpl --enable-version3 --enable-nonfree; do
    count="$(strings -a "$lib" | grep -c -- "$flag" || true)"
    [[ "$count" == 0 ]] || die "$(basename "$lib") contains '$flag' ($count times)"
  done
  note "strings -a $(basename "$lib") | grep -c -- '--enable-gpl': 0 (also version3, nonfree)"
}

# make_artifact <os> <arch> <library file> [extra licence files...]
make_artifact() {
  local os="$1" arch="$2" lib="$3"
  shift 3
  cp "$lib" "$DIST/"
  cp "$SRC/mpv/LICENSE.LGPL" "$DIST/COPYING.LGPL-2.1"
  cp "$SRC/mpv/Copyright" "$DIST/Copyright"
  cp "$SRC/ffmpeg/COPYING.LGPLv2.1" "$DIST/COPYING.LGPLv2.1"
  cp "$SRC/dav1d/COPYING" "$DIST/licenses/dav1d-COPYING.txt"
  cp "$SRC/freetype/LICENSE.TXT" "$DIST/licenses/freetype-LICENSE.txt"
  cp "$SRC/freetype/docs/FTL.TXT" "$DIST/licenses/freetype-FTL.txt"
  cp "$SRC/fribidi/COPYING" "$DIST/licenses/fribidi-COPYING.txt"
  cp "$SRC/harfbuzz/COPYING" "$DIST/licenses/harfbuzz-COPYING.txt"
  cp "$SRC/libass/COPYING" "$DIST/licenses/libass-COPYING.txt"
  cp "$SRC/libplacebo/LICENSE" "$DIST/licenses/libplacebo-LICENSE.txt"
  local extra
  for extra in "$@"; do cp "$extra" "$DIST/licenses/"; done

  local name="libmpv-$os-$arch-mpv$MPV_VERSION-ff$FFMPEG_VERSION-r$REV.zip"
  rm -f "$ARTIFACTS/$name"
  (cd "$DIST" && zip -q -r -X "$ARTIFACTS/$name" .)
  say "artifact $name"
  (cd "$ARTIFACTS" && { sha256sum "$name" 2>/dev/null || shasum -a 256 "$name"; })
}

# libmpv-sources-r<N>.tar.xz: the exact sources, patch and build scripts (LGPL-2.1 §6(d)).
make_sources() {
  local name="libmpv-sources-r$REV.tar.xz"
  rm -f "$ARTIFACTS/$name"
  tar -cJf "$ARTIFACTS/$name" --exclude=.git \
    -C "$WORK" src dist/patches dist/BUILDINFO.txt \
    -C "$ROOT" packaging/libmpv packaging/pins.env
  say "sources $name"
}
