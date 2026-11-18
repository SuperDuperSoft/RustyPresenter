# Portable versions on every platform (November 18, 2026)

Jake: "All releases should have one for each [Windows, Mac, Linux] and a portable version."

## What a portable copy is

A folder that runs without installing and keeps everything it saves inside itself, so it can live on a USB stick
and work the same on any computer of that kind.

- **The switch:** a file `portable.txt` beside the program: beside `rustypresenter.exe` (Windows), beside
  `RustyPresenter.app` (macOS; never inside the bundle, which would break its signature), beside the AppImage file
  (`$APPIMAGE`) or beside `rustypresenter` (Linux). A file, not a folder, so a person can read what it does and
  delete it to turn portable mode off. Its contents do not matter; it explains itself in plain words.
- **The data:** the folder `RustyPresenter Data` beside the program, made on first start. Everything the app keeps
  goes there, because every part of the app already asked `server::data_dir()`: `data.json`, `assets/`,
  `backups/`, `logs/`, `deps/` (FFmpeg, follow models, LibreOffice profile), `bibles/`, `translations/`,
  `settings-window.json`, `phones.json`, recovery state. That function now asks `platform::data_folder`, which
  works the answer out once per run. Order: `RUSTYPRESENTER_DATA` (tests, the graphics test), then the portable
  folder, then the per-user folder (`%LOCALAPPDATA%\RustyPresenter`, `~/Library/Application Support/RustyPresenter`,
  `~/.local/share/RustyPresenter`).
- **Songs folder** (Settings → Songs → "Keep my songs in a folder"): its default moves into
  `RustyPresenter Data/Songs` in a portable copy, so it travels too. A folder chosen by hand stays as chosen.
- **Stays per computer, on purpose:** stream keys (Credential Manager / Keychain / Secret Service: passwords never
  go in the data folder), recordings (the computer's Videos folder unless Settings names another: they are big and
  a stick fills up), macOS camera/microphone permission. The HOW-TO-START texts say so.
- **About** shows the data folder followed by "(a portable copy: the data is kept beside the app)"; the console
  banner says "(portable: beside the app)".

## When the folder beside it cannot be used

The app still starts, with the per-user folder for that run, and a message box (warning, OK) says why and what to
do. Checked in this order:

1. **macOS translocation:** a downloaded app that Finder has not moved runs from a hidden read-only copy under
   `.../AppTranslocation/...`, and the folder it came from cannot be seen, so `portable.txt` cannot be found.
   Detected from the program's path. Message: drag RustyPresenter out of its folder and back in (or into
   Applications), then open it again. Shown for the .dmg too, where the advice (Applications) is also right.
2. **Inside a zip:** the program's folder has `.zip` in its path (Windows' `Temp1_<name>.zip`, Linux file managers'
   `archive:...zip`). Message: unzip the whole file first (Extract All).
3. **Cannot be written:** the data folder cannot be made or a test file written in it (locked stick, CD, Program
   Files). Message: copy the whole folder somewhere that can be changed. Nothing on the stick is changed.

## Release files

`packaging/release-files.sh rename <os>` gives cargo-packager's output consistent names, and `portable <os>` makes
the portable package; `package.yml` runs both after packaging and uploads all of them, and the release job checks
that all seven files are there and the tag matches the version before publishing:
`RustyPresenter-<v>-windows-setup.exe`, `-windows-portable.zip`, `-macos.dmg`, `-macos-portable.zip` (made with
`ditto -c -k --keepParent`), `-linux.AppImage`, `-linux.deb`, `-linux-portable.tar.gz` (binary, `libmpv.so.2`,
`libpdfium.so`, launcher `RustyPresenter.sh` that puts the folder on `LD_LIBRARY_PATH`). Each portable package is a
single top folder `RustyPresenter/` with `portable.txt`, `HOW-TO-START.txt` and the licences.

`packaging/windows/make-portable.cmd` makes the Windows zip on the PC from try-latest's build.
