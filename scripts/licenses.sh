#!/usr/bin/env bash
# Makes THIRD-PARTY-LICENSES.txt: the crates' licences from cargo-about, then packaging/licenses/ in a fixed order.
# It is the only licence text: About embeds it, and the installers ship it.
# Needs: cargo install cargo-about --version 0.9.2 --locked --features cli
set -euo pipefail
cd "$(dirname "$0")/.."

OUT=THIRD-PARTY-LICENSES.txt
DIR=packaging/licenses
# native.txt is the short summary, so it comes first and has no heading.
FILES=(
  NOTICE-LGPL.txt
  LGPL-2.1.txt
  FFmpeg-COPYING.LGPLv2.1.txt
  mpv-Copyright.txt
  pdfium-LICENSE.txt
  NotoSans-OFL.txt
)

# A file nobody listed must not be left out quietly.
for f in "$DIR"/*; do
  name=$(basename "$f")
  [[ "$name" == native.txt || " ${FILES[*]} " == *" $name "* ]] || { echo "licenses.sh: $name is not in FILES" >&2; exit 1; }
done

cargo about generate about.hbs -o "$OUT"
cat "$DIR/native.txt" >> "$OUT"

missing=0
for name in "${FILES[@]}"; do
  if [[ ! -f "$DIR/$name" ]]; then
    echo "licenses.sh: $DIR/$name is missing" >&2
    missing=1
    continue
  fi
  { printf '\n=== %s ===\n\n' "$name"; cat "$DIR/$name"; } >> "$OUT"
done

# Allowed on a developer's machine, so the rest can be looked at; a release must not ship without them.
if [[ $missing == 1 && "${CI:-}" == true ]]; then exit 1; fi
