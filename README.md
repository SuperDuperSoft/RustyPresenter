# RustyPresenter

Church presentation app: song lyrics, Bible verses, PowerPoint slides, videos, pictures, music, a stage display, and a phone remote.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

## Install

Download the installer for your computer from the [releases page](https://github.com/SuperDuperSoft/RustyPresenter/releases). The releases are not signed yet, so the first start shows a warning.

**Windows:** run the installer; it needs no administrator password.

- If SmartScreen appears, click **More info**, then **Run anyway**. If Smart App Control is on, Windows blocks the app outright and these buttons do not appear.
- On the first start Windows asks whether RustyPresenter may use the network. Tick **Private networks** and click **Allow**, or phones cannot connect. Do this before pairing a phone.

**macOS** (Apple silicon): open the `.dmg` and drag the app to Applications. On first start, go to **System Settings → Privacy & Security** and click **Open Anyway**, or run `xattr -dr com.apple.quarantine /Applications/RustyPresenter.app`.

**Linux:** either run `sudo apt install ./rustypresenter_*.deb`, or make the AppImage runnable with `chmod +x` and start it. The AppImage needs `libfuse2`.

To import PowerPoint and Keynote files without PowerPoint, install LibreOffice (free) from [libreoffice.org](https://www.libreoffice.org/download/download/):

| Computer | Command |
| --- | --- |
| Windows | `winget install TheDocumentFoundation.LibreOffice` |
| macOS | `brew install --cask libreoffice` |
| Linux | `sudo apt install libreoffice-impress` |

On Linux, the Snap package of LibreOffice cannot be used.

## Start it from the source code

Double-click **`RustyPresenter.cmd`**. The first start builds the app (a few minutes); after that it opens in a few seconds. Closing the main window quits RustyPresenter and turns off the room and stage screens.

A window opens with three columns:

| Left | Middle | Right |
| --- | --- | --- |
| **Service**: everything for this Sunday, in order | **Slides** of the item you picked. Click a slide to show it | **Live**: exactly what the room sees, with Next/Back and Clear buttons |

## First time

1. **Screens** (top right): pick the projector/TV and click *Show on this screen*. Windows must be set to *Extend* (press `Win + P`).
2. **Add to service**: songs, Bible, videos/pictures/music, slides, announcements. You can also **drag files** onto the window.
3. **Bible box** at the top: type `John 3:16` and press **Enter** to add it. Press **Enter** again to show it.
4. During the service, press **→** or click the big **Next** button. Next keeps going into the next item.

## Keys

| Key | Does |
| --- | --- |
| `→` `Space` `Page Down` | Next |
| `←` `Page Up` | Back |
| `Enter` | Show the selected item |
| `Ctrl + B` | Bible box |
| `F1` | Clear all (black) |
| `F2` | Clear text |
| `F3` | Clear media and background |
| `F4` | Stop audio |
| `F5` | Logo |

## Video

Any video or audio format plays right away: MP4, MOV, MKV, AVI, WMV, HEVC, ProRes, MPEG, phone videos and more. There is nothing to install or convert.

## Where your data lives

`%LOCALAPPDATA%\RustyPresenter` (not OneDrive). A full backup (songs, services, settings, and every video, picture and slide) is made each time the app starts and once a day. The last 10 are kept. Restore from **Settings → Backups**.

## Bible translations

KJV is built in and works offline. NIV, ESV, CSB, HCSB and NLT need a licensed text source before they can be turned on (tracked as F5 in `PROJECT_PLAN.md`).

## Licenses

RustyPresenter's own code is under the MIT license or the Apache License 2.0, whichever you prefer (`LICENSE-MIT`, `LICENSE-APACHE`).

RustyPresenter uses libmpv and FFmpeg under the GNU Lesser General Public License. **Settings → About → Show licenses** has every third-party license; the installers include the same text. The exact sources of the shipped library and the scripts that built it are on the [releases page](https://github.com/SuperDuperSoft/RustyPresenter/releases), next to each release.

You may use your own build of libmpv: set `RP_LIBMPV` to its path, or replace the file next to the program (on macOS, in `Contents/Frameworks`).

## For collaborators

- The app is native: a Rust core with a local server for the phone remote, and Slint windows for the operator, room and stage, with video and sound through libmpv.
- Building needs `.tools\cargo` + `.tools\rustup` (the `stable-x86_64-pc-windows-gnu` toolchain) and `.tools\mingw64` (WinLibs MinGW-w64 GCC 16.2.0 UCRT, from github.com/brechtsanders/winlibs_mingw). The scripts put MinGW first on PATH and use its `dlltool`.
- `dev-build.cmd` builds the app and runs the tests, writing everything to `build.log`.
- The app needs `libmpv-2.dll` next to the exe, in `%LOCALAPPDATA%\RustyPresenter\deps\mpv\`, or named by `RP_LIBMPV`; without it everything but video and audio works. `RP_FORCE_SOFTWARE=1` draws without the GPU. Crashes are written to `logs\native-YYYYMMDD.log` in the data folder.
- `dev-watch.cmd` waits for `build.request`, `run.request` or `stop.request` files and acts on them. It lets a remote collaborator build on this PC. Close the window to stop it.
- `scripts/licenses.sh` remakes `THIRD-PARTY-LICENSES.txt`. Run it after changing `Cargo.lock` or `packaging/licenses/`.
- The design is in `docs/superpowers/specs/`. The work board is `PROJECT_PLAN.md`.
