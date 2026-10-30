# RustyPresenter

Church presentation app: song lyrics, Bible verses, PowerPoint slides, videos, pictures, music, a stage display, and a phone remote.

## Start it

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

MP4 and WebM play right away. For **any other format** (MOV, MKV, AVI, WMV, HEVC, ProRes, MPEG, phone videos…), install the free FFmpeg converter once:

```
winget install --id Gyan.FFmpeg -e
```

After that, RustyPresenter converts each unusual video **once when you add it**, so it plays smoothly on Sunday.

## Where your data lives

`%LOCALAPPDATA%\RustyPresenter` (not OneDrive). A full backup (songs, services, settings, and every video, picture and slide) is made each time the app starts and once a day. The last 10 are kept. Restore from **Settings → Backups**.

## Bible translations

KJV is built in and works offline. NIV, ESV, CSB, HCSB and NLT need a licensed text source before they can be turned on (tracked as F5 in `PROJECT_PLAN.md`).

## For collaborators

- The app is Tauri 2: a Rust core with a local web server (it also serves the phone remote), and WebView2 windows for the operator, room and stage. Each window has its own WebView2 data folder, so each runs in its own browser process.
- Building needs `.tools\cargo` + `.tools\rustup` (the `stable-x86_64-pc-windows-gnu` toolchain) and `.tools\mingw64` (WinLibs MinGW-w64 GCC 16.2.0 UCRT, from github.com/brechtsanders/winlibs_mingw). The scripts put MinGW first on PATH and use its `dlltool`.
- `dev-build.cmd` builds and runs the tests, writing everything to `build.log`.
- `dev-watch.cmd` waits for `build.request`, `run.request` or `stop.request` files and acts on them. It lets a remote collaborator build on this PC. Close the window to stop it.
- The design is in `docs/superpowers/specs/`. The work board is `PROJECT_PLAN.md`.
