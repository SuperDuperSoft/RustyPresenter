#!/usr/bin/env bash
# Adds "Recommends:" to the .deb cargo-packager built (it has no such setting, Packager.toml [deb]).
#
#   packaging/linux/deb-recommends.sh dist/*.deb
#
# pulseaudio-utils gives `parec` and `pactl`: the app reads sound inputs with them when the FFmpeg it downloaded was
# built without PulseAudio, lists the inputs with `pactl`, and plays "Listen to the stream" with `pacat`. They work
# with PipeWire's PulseAudio layer too. Recommended, not required: apt installs it unless told not to.
set -euo pipefail

RECOMMENDS="pulseaudio-utils"

for deb in "$@"; do
  work="$(mktemp -d)"
  dpkg-deb -R "$deb" "$work"
  control="$work/DEBIAN/control"
  if grep -q '^Recommends:' "$control"; then
    sed -i "s/^Recommends:.*/Recommends: $RECOMMENDS/" "$control"
  else
    # After Depends (or at the end), as dpkg writes it.
    if grep -q '^Depends:' "$control"; then
      sed -i "/^Depends:/a Recommends: $RECOMMENDS" "$control"
    else
      printf 'Recommends: %s\n' "$RECOMMENDS" >> "$control"
    fi
  fi
  dpkg-deb --root-owner-group -b "$work" "$deb" >/dev/null
  rm -rf "$work"
  echo "$deb: Recommends: $RECOMMENDS"
done
