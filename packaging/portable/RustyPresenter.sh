#!/bin/sh
# Starts the portable RustyPresenter from this folder, with the video and PDF libraries beside it.
here="$(cd "$(dirname "$0")" && pwd)"
LD_LIBRARY_PATH="$here${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LD_LIBRARY_PATH
exec "$here/rustypresenter" "$@"
