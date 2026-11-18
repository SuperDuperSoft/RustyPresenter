# The Android remote (November 18, 2026)

Jake: "I want a custom APK written in Rust for this. I think the browser works great — don't remove it. But an APK
would work a little cleaner."

## What it is

An Android app, `apps/android/`, written in Rust with Slint, that does the phone pages' jobs: the remote, the stage
screen, the camera's on-air light, the phone microphone and the phone camera. It talks to the computer exactly as the
browser pages do (same requests, same pairing, same passwords), so the browser pages stay and keep working, and a
church can mix phones with the app and phones with the browser.

Why it is cleaner than the browser: the computer is found on the Wi-Fi (no address to type), the phone stays paired
and remembers the computer, the screen stays on by itself on the Stage and On air tabs, and the camera and
microphone need no "connection is not private" step (the browser only lets a secure page use them; an app may use
them over the church's plain network once Android's own permission is given).

## Screens

- **Choosing the computer.** On opening, the app looks for RustyPresenter on the Wi-Fi and lists each computer it
  finds by name ("SANCTUARY-PC · 192.168.1.20"). Or the QR code of Phone → Pair a phone or tablet is scanned with
  the app's camera (it carries a one-time secret: no code to type), or the address is typed. The last computer is
  remembered and opened straight away next time.
- **Pairing.** The 6-digit code, and the remote's password when the church set one. Sent as the browser sends it.
- **Remote** (tab 1): on screen now (the slide's words, or its picture for PowerPoint, PDF and designed slides), the
  live song's sections to jump to, presenter notes, big Back and Next, Pause/Play video, Up next, Clear all with Text,
  Media, Audio and Logo, and the service list: a tap on an item lists its slides; a tap on a slide shows it. Drawing,
  macros, messages, walk-in music and "stage screen only" stay in the browser remote.
- **Stage** (tab 2): the stage layout as the stage window draws it, including the chord sheet (chords over their own
  letters, lines wrapping between words). A tap hides the tabs and the phone's bars for the whole screen. The screen
  stays on.
- **On air** (tab 3): the chosen camera's light, red on air, green next, grey off, filling the screen; − and + choose
  the camera. The screen stays on.
- **Microphone** (tab 4): Start sending sound / Stop, a level bar and the computer's answer ("Sending, but the stream
  is not using this phone yet…"). The screen stays on while it sends.
- **Camera** (tab 5): the camera's name on the computer, back or front camera, Start the camera / Stop, a preview with
  the on-air light round it.
- Stage, On air and Microphone use the stage screen on phones (Settings → Remote and network), as their pages do: when
  it is off, the computer's own words say so; when it has a password, the tab asks for it once.
- The camera and microphone are asked for only when Start (or Scan the QR code) is tapped, after one sentence saying
  why ("Android will ask you next"). When Android says no, the app says where to allow it.

All words are plain English, translated into the desktop's 13 languages (the desktop's own translations where the
text is the same), in the phone's language.

## How it talks to the computer

Plain HTTP on the phone server's port (4318), one request a connection, with the browser's cookies (`rp_session`
from pairing, `rp_stage` from the stage password). Live updates are the same Server-Sent Events (`/remote/events`,
`/stage-view/events`); the On air light is polled twice a second (`/stage-view/tally.json`); sound goes as 16-bit
mono a tenth of a second at a time (`/stage-view/mic/sound`); camera pictures go as JPEG over the same WebSocket
(`/camera/ws`), each answered with the light, with one picture a request (`/camera/frame`) when the WebSocket cannot
connect. The app sends no `Origin`, which the server's same-page checks allow (they stop other web pages, not apps).

**One small server addition**, backward compatible: `/remote/status` also says `"app": "RustyPresenter"`, the
computer's name and the version. Finding works without it (older computers are found and listed by address).

**Finding the computer**: every address of the phone's own network (1–254) is tried on port 4318, 48 at a time,
each that answers `/remote/status` like RustyPresenter is listed (a few seconds). Nothing to turn on at the computer,
no new port for the firewall. The phone's Wi-Fi address is used even when its internet goes over mobile data.

## How it is built

- `apps/android/` is its own Cargo project (its own `[workspace]`), not part of the desktop's build; the desktop's
  `Cargo.toml` and `Cargo.lock` are unchanged and its build needs no network. Its crates come from crates.io and are
  pinned in `apps/android/Cargo.lock`.
- Everything that needs no phone is plain Rust tested with `cargo test` on any computer: the HTTP client and events,
  the WebSocket, the JSON of every page, pairing, addresses and QR links, finding, what is remembered, sizing the
  stage's words and chord rows, turning camera pictures upright, the phone's language and that every word is
  translated. `cargo run --example preview --features preview` draws every screen to PNGs on a computer.
- On the phone: Slint's Android backend in a `NativeActivity` (no Java of our own), drawn with Slint's software
  renderer (`patches/`, see below), Android's AAudio for the microphone, the NDK's Camera2 for the camera (YUV
  pictures, turned upright in Rust, made JPEG by Android's own `YuvImage`), JNI for permissions, the phone's name and
  how the screen is turned, `rqrr` for the QR code.
- `build-apk.ps1` makes the APK with the Android SDK's own tools: the Rust library for arm64 and 32-bit arm (linked
  with the NDK's clang), `aapt2` for the manifest and icon, `zipalign`, `apksigner`. Android 8 (API 26) or newer.
- CI: the `android` job of `package.yml` builds, tests and checks the APK, and the release carries it as
  `RustyPresenter-<version>-android-remote.apk`.

**The patched Slint Android backend** (`apps/android/patches/i-slint-backend-android-activity`, `PATCHED.md`): Slint
1.18's Android backend draws only with Skia, whose build downloads a prebuilt Skia library from GitHub (not crates.io);
the patch draws with Slint's software renderer into the window's own buffers instead, which works on every phone
without its graphics driver. It also lets JDK 21+ with build-tools 34 build the backend's small Java helper.

## Not verified by a person

No phone was connected when this was built and no emulator is installed: the APK was built, signed and checked
(`aapt2 dump badging`, `apksigner verify`), and the screens were drawn on the computer, but nothing ran on a phone.
To check on a phone: install, find and pair the computer (found list, QR code, typed address, password), Remote
taps (Next, Back, a slide from the list, a section, clears, video), the Stage tab with a chord sheet and a picture
slide, On air with a live stream, the microphone into the stream, the camera into the stream (back and front,
upright held both ways), Back button, and the words in another language.
