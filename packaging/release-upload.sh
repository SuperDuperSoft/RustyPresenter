#!/usr/bin/env bash
# The last step of a tagged build (package.yml): puts the files in a folder on the tag's GitHub release.
#
#   packaging/release-upload.sh <tag> <owner/repo> [folder, default release]
#
# The release is usually there already, with the Windows installer and the Android app built and tried on a PC
# (RustyPresenter-Setup-<version>.exe, RustyPresenter-Remote-<version>.apk, the names the README gives). Those two
# are not replaced by the ones built here: the one built here is left out, but only when the release really holds
# the PC-built one. A tag pushed before the release exists gets a draft release with everything built here, so it
# is never published without a Windows installer or an Android app.
#
# GH names the GitHub CLI (the tests put a stand-in there).
set -euo pipefail

tag="${1:?usage: release-upload.sh <tag> <owner/repo> [folder]}"
repo="${2:?usage: release-upload.sh <tag> <owner/repo> [folder]}"
dir="${3:-release}"
GH="${GH:-gh}"

if ! "$GH" release view "$tag" --repo "$repo" >/dev/null 2>&1; then
  "$GH" release create "$tag" --repo "$repo" --draft --title "RustyPresenter ${tag#v}" --generate-notes
fi
assets="$("$GH" release view "$tag" --repo "$repo" --json assets --jq '.assets[].name')"
on_release() { grep -Eq "$1" <<<"$assets"; }

shopt -s nullglob

# keep_pc_built <what> <the PC-built file's name, a regex> <the file built here, a glob in the folder>
keep_pc_built() {
  local what="$1" pc="$2" built=("$dir"/$3)
  if on_release "$pc"; then
    if ((${#built[@]})); then rm -f "${built[@]}"; fi
    echo "The release already has the $what built on a PC; the one built here is left out."
  elif ((${#built[@]} == 0)); then
    echo "::warning::The release will have no $what: none was put there from a PC and none was built here."
  else
    echo "The release has no $what from a PC; the one built here is uploaded."
  fi
}
keep_pc_built "Windows installer" '^RustyPresenter-Setup-.*\.exe$' '*-windows-setup.exe'
keep_pc_built "Android app" '^RustyPresenter-Remote-.*\.apk$' '*-android-remote.apk'

files=("$dir"/*)
if ((${#files[@]} == 0)); then
  echo "::error::nothing to upload in $dir"
  exit 1
fi
"$GH" release upload "$tag" --repo "$repo" --clobber "${files[@]}"
