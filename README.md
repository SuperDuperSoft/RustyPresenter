# RustyPresenter

Church presentation app: song lyrics, Bible verses, PowerPoint slides, videos, pictures, music, a stage display, and a phone remote.

## Start it

Double-click **`RustyPresenter.cmd`**. The first start builds the app (a few minutes); after that it opens in a few seconds. Closing the main window quits RustyPresenter and turns off the room and stage screens.

The window has four parts:

| Where | What |
| --- | --- |
| **Left** | **Service**: everything for this Sunday, in order |
| **Middle** | **Slides** of the item you picked. Click a slide to show it. Songs have coloured sections and buttons (V1, C, B…) to jump to one |
| **Bottom** | **Media library**: every video, picture and song file, in folders. Double-click one to make it the background of the item you picked, or click **Add to service** |
| **Right** | **Live**: exactly what the room sees, with Next/Back, Up next, a message box for the stage screen and the Clear buttons |

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
| `C` `B` `P` `E` | Jump to the chorus, bridge, pre-chorus or ending of the live song |
| `1`–`9` | Jump to that verse of the live song |
| `Ctrl + V` | Add a YouTube link you copied |

## Songs from EasyWorship

**Settings → Songs → Import from EasyWorship** copies every song (words, writers, copyright and CCLI number) from EasyWorship on the same computer, once. EasyWorship itself is not changed. Songs already here are skipped.

## More than one Bible on a slide

**Settings → Bible → Also show with every new passage**: pick up to two more Bibles (for example English and Spanish). Each new passage then shows the same verses from every Bible, one under the other.

## Video

MP4 and WebM play right away. For **any other format** (MOV, MKV, AVI, WMV, HEVC, ProRes, MPEG, phone videos…), install the free FFmpeg converter once:

```
winget install --id Gyan.FFmpeg -e
```

After that, RustyPresenter converts each unusual video **once when you add it**, so it plays smoothly on Sunday.

## Where your data lives

`%LOCALAPPDATA%\RustyPresenter` (not OneDrive). A full backup (songs, services, settings, and every video, picture and slide) is made each time the app starts and once a day. The last 10 are kept. Restore from **Settings → Backups**.

## Bible translations

KJV, WEB and BSB are built in and work offline. Many more can be added from **Settings → Bible** (search by language or name). NIV, ESV, CSB, HCSB and NLT need the publisher's free key, or HCSB can be imported from a licensed file.

## For collaborators

- The app is native: a Rust core with a local web server for the phone remote, and Slint windows for the operator, room and stage. Video and audio play through libmpv.
- Building needs `.tools\cargo` + `.tools\rustup` (the `stable-x86_64-pc-windows-gnu` toolchain) and `.tools\mingw64` (WinLibs MinGW-w64 GCC 16.2.0 UCRT, from github.com/brechtsanders/winlibs_mingw). The scripts put MinGW first on PATH and use its `dlltool`.
- `dev-build.cmd` builds the app and runs the tests, writing everything to `build.log`.
- `dev-watch.cmd` waits for `build.request`, `run.request` or `stop.request` files and acts on them. It lets a remote collaborator build on this PC. Close the window to stop it.
- The design is in `docs/superpowers/specs/`. The work board is `PROJECT_PLAN.md`.
