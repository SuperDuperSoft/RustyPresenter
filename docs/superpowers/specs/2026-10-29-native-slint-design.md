# RustyPresenter — Native App Design (Slint)

Date: October 29, 2026 · Status: written; user asked Claude to decide all technical choices
Supersedes the platform parts of `2026-10-29-rustypresenter-v1-design.md` (§4 windows and isolation, §10 displays, §12.1–12.2 screens, §15 build order, §19 Tauri notes). Everything else in that spec still applies: the live engine rules (§5–§7), Bible (§8), songs and imports (§9), storage and backups (§11), phone remote security (§12.3).

## 1. What the user asked for

Said by the user:
- A native app, not running in a browser. Native app on **Windows, macOS and Linux**.
- Phones and tablets are remotes. The remote stays a web page the phone opens (QR + PIN), no installed phone app for now.
- PowerPoint import must work on all three platforms: use PowerPoint on Windows when installed, otherwise LibreOffice.
- Slint for every screen. The user holds a **Slint Royalty-Free License (v2.0), accepted October 30, 2026 02:10 UTC**.
- Same layout and features as today's screens, rebuilt natively.
- "Do what is best long term, works well cross-platform, and runs well on any computer. Just write good code." Technical choices are Claude's to make.
- Nobody at the church writes code; the user and Claude maintain the app.

Assumed (not stated by the user):
- The app's own source license is **MIT OR Apache-2.0** (the Rust convention). It works with the Slint royalty-free license and keeps NDI possible later. The user can change this before the first public release.
- "Any computer" means any machine from about the last 10 years running a supported OS (Windows 10+, macOS 12+, a current Linux desktop with X11 or Wayland), including ones with weak or no GPU acceleration.

Success means: a volunteer on any of the three platforms installs one package, opens the app, and runs a service (songs, scripture, slides, video with sound, clears, phone remote) with no browser involved, and the app recovers by itself from a crashed video or an unplugged screen.

## 2. Scope

In: native operator, room output and stage screens; video and audio through libmpv; cross-platform monitors and fullscreen; cross-platform PowerPoint import; installers for all three platforms; crash recovery.

Out (later list): native phone apps, NDI/livestream output, redesigning the operator layout, Google Slides import changes beyond §9.5 of the v1 spec, plugins.

Removed: Tauri, WebView2, the `web/operator.*`, `web/output.*`, `web/render.js` and `web/stage.html` pages, the FFmpeg conversion step, Win32-only code paths for monitors and error boxes.

## 3. Architecture

One process for the app (plus a small supervisor process, §8.3). Modules:

| Module | Purpose | Depends on |
|---|---|---|
| `core` — today's `model`, `live`, `bible`, `songselect`, `slides`, `store` | Data, live engine, persistence. Unchanged. | nothing UI |
| `app` (moved out of `server.rs`) | `App` state, the action dispatcher (`do_action`), broadcasts, jobs, backups | core |
| `render` | Slide compositor: frame + theme → RGBA image (§4) | core, cosmic-text, tiny-skia |
| `video` | libmpv wrapper: players, events, GL drawing (§5) | libmpv loaded at runtime |
| `platform` | Monitors, fullscreen, native dialogs, tool discovery (LibreOffice, PowerPoint), single instance | Slint/winit, rfd |
| `import` | PowerPoint/LibreOffice → slide images, media probing and thumbnails | platform, video, pdfium |
| `ui` — `.slint` files + Rust glue | Operator, room output, stage, dialogs | app, render, video, platform |
| `server` (slimmed) | HTTP for the phone remote only | app |

Rules:
- **One command surface.** Every operator action goes through `App::do_action(action, args)`, the same dispatcher the HTTP `/api/do/*` route uses today. The native UI calls it in-process through typed wrappers; the phone remote calls it over HTTP. The live engine's versions, command IDs, `base` checks and playback instances (v1 spec §5.2–§5.4) apply to both unchanged.
- **One update stream.** `App` publishes typed `Update` values (live changed, data changed, job progress, notice) on a `tokio::sync::broadcast` channel. The UI glue subscribes and applies them on the Slint event loop with `slint::invoke_from_event_loop`. The phone remote's SSE stream serializes the same updates to JSON.
- **No blocking on the UI thread.** File I/O, imports, slide rendering and Bible search run on the tokio runtime; results come back through `invoke_from_event_loop`.
- Each `.slint` component and each Rust glue file covers one screen or one dialog, so no file grows past what can be read in one go.

## 4. Slide rendering

All slides (room output, live preview, operator thumbnails) come from one Rust compositor (the stage screen shows plain text, as today), so the preview matches the projector exactly on every platform.

- Input: the frame's slide layer (text, reference, footer, scripture flag), the theme (font, sizes, color, shadow, uppercase, position), and the target size.
- Text layout: **cosmic-text**, with the same rules as today's `render.js`: 1920-wide logical canvas, padding 110 px, lyrics centered at weight 600 / line height 1.22, scripture left-aligned at weight 500 / line height 1.32, reference bottom-right at 44 px, footer bottom-center at 30 px, optional lower-third position with its gradient.
- Auto-fit: start at the theme size and step down (4 px above 60, else 2 px) until the text fits, never below 26 px, never dropping words. Same as today.
- Shadow: rasterize the text's alpha, blur it (two-pass box blur approximating `0 3px 14px` and `0 0 3px`), draw it under the text. Drawing with **tiny-skia**.
- Output: a premultiplied RGBA buffer handed to Slint as an `Image`, transparent where there's no text.
- Fonts: bundle an open-licensed default (Noto Sans, OFL) so text looks the same everywhere; system fonts are also available through cosmic-text's font database. A theme naming a font that isn't installed falls back to the bundled one, and Settings says so.
- Caching: rendered images are cached in memory by (slide content, theme, size), least-recently-used with a size cap. Operator thumbnails are rendered at thumbnail size, not scaled down from 1080p. Rendering runs off the UI thread; a thumbnail shows a placeholder until ready.
- Slide images, stills and image backgrounds are decoded once at display size (`image` crate), off the UI thread.

## 5. Video and audio (libmpv)

- **libmpv is loaded at runtime** (`libloading`) with a small hand-written binding for the ~20 functions needed. If it's missing, the app still runs; video and audio items say "The video player is missing" with a reinstall hint. Dynamic loading also keeps the LGPL build of libmpv separate.
- One mpv player per playing layer: background video (muted, looping), foreground video (with sound), audio item. Each play gets the live engine's playback instance ID; mpv's events (`playback-restart`, `pause`, `end-file`, errors, `time-pos` at 4 Hz) become `RoomEvent` reports, so the existing stale-report rules apply.
- Settings: `hwdec=auto-safe` (hardware decoding where it is known to work, software otherwise), `keep-open` per the item's end action, `loop-file` for loops, audio device from Settings.
- **Drawing in the room window:** Slint's rendering notifier (`Window::set_rendering_notifier`, `GraphicsAPI::NativeOpenGL`) gives the OpenGL context; mpv's render API draws the active video into it during `BeforeRendering`, then Slint draws its scene on top. The app asks Slint for the OpenGL-based renderer (FemtoVG) on the room window. While a video is live, the room window requests a redraw per mpv frame (`mpv_render_context_set_update_callback` → `Window::request_redraw`).
- **Layer order** follows v1 spec §6.1 (background, media, slide, logo). Because video always draws underneath the Slint scene: when a foreground video is live, Slint doesn't draw the background layer (the video fills the screen with black bars, as today). Text and logo layers draw over video normally.
- **Transitions:** crossfades between Slint-drawn layers use Slint opacity animations. A change that starts or stops a video fades through a black Slint rectangle over the video.
- **Fallback for computers without working OpenGL:** if the GL context or mpv's GL renderer fails, the room window uses Slint's software renderer and mpv's software render API (`MPV_RENDER_API_TYPE_SW`) into a buffer shown as a Slint `Image`. Slower, but it plays. The choice is logged and shown in Settings → About.
- **Operator live preview of video:** the preview shows the video's current frame, refreshed 4 times a second with mpv's `screenshot-raw` at preview size. Live preview never plays a second copy of the video.
- **Thumbnails and probing:** when media is imported, a headless mpv instance reads its duration and grabs a frame at 1 s for the thumbnail. FFmpeg is no longer used.

## 6. Screens

Same layout and features as today (v1 spec §12.1–§12.2 and the current `web/` pages):

- **Operator window:** top bar (service picker, Bible box with Ctrl+B and translation buttons, status, Screens, Phone, Settings), Service column (drag to reorder, Add to service), Slides grid, Live column (preview, transport, Back/Next, Clear All F1, Text F2, Media F3, Audio F4, Logo F5, Up next), all dialogs (add song/scripture/media/deck/text, song editor, item edit, background picker, services, screens, phone pairing with QR + PIN, settings), import progress, notices, and every keyboard shortcut.
- **Room output:** fullscreen on the chosen monitor, no cursor.
- **Stage:** fullscreen; title, clock, remaining media time, current slide text, notes, next slide text — same as `stage.html`.
- **Settings → About** shows the `AboutSlint` widget (license condition, §10), the version, the data folder, and whether video uses GPU or software drawing.
- Adding files: **Add to service** uses native file dialogs (`rfd`). Dragging files from the desktop onto the operator window also adds them (Slint's window file-drop events, or the winit window events if Slint doesn't surface them — see §12 check C3).
- Look: the current dark theme, colors and spacing carried over into a shared Slint `Theme` global.

## 7. Monitors and fullscreen

- Monitor list, names, positions and sizes come from winit through Slint's winit backend accessor. Fullscreen uses winit's borderless fullscreen on the chosen monitor (no exclusive mode changes).
- A monitor's stable ID is its name + size + position, matching how today's saved display settings work, so saved room/stage choices survive restarts.
- Monitors are re-checked every 2 seconds and on winit's monitor events. If the room or stage monitor is unplugged, its window closes and the operator shows a notice. When the same monitor returns, the window reopens on it. The room is never placed on the operator's monitor automatically.
- macOS: fullscreen windows use borderless fullscreen on the target screen, not macOS's separate fullscreen Space, so the operator window stays usable.

## 8. Failures and recovery

### 8.1 Video and audio
- mpv reports an error → the live engine records `PlayState::Error` for that instance; the operator shows "This video couldn't play" with the file name; the rest of the frame stays up. Clear Media works as normal.
- libmpv missing or failing to load → see §5.

### 8.2 Screens
- Unplugged monitor → §7.
- GPU/OpenGL failure → software fallback, §5.

### 8.3 Crash recovery (replaces the separate WebView2 process)
All windows now share one process, so a crash in video decoding could take everything down. Recovery:
- The executable starts in **supervisor mode** by default: it launches itself again with `--app` and waits. The supervisor has no UI and does nothing else.
- On every committed live change, the app writes the committed frame (and, for playing media, asset + position, refreshed every 5 s) to `live-state.json` atomically.
- If the app exits with a crash (non-zero code that isn't a normal quit), the supervisor relaunches it with `--restore` within 2 seconds. The restored app reopens room and stage on their monitors, puts the last committed frame back on screen, and resumes media at the saved position. The operator shows "RustyPresenter restarted after a problem."
- After 3 crashes within 60 seconds, the supervisor stops and shows a native error dialog with the log file path.
- Panics are logged with a backtrace to the data folder's `logs/` before exit.

### 8.4 Single instance
Kept as today: the app binds its port; a second launch that finds the port in use asks the running app to bring its operator window forward, then exits.

## 9. Imports (cross-platform)

- **PowerPoint:** on Windows with PowerPoint installed, keep today's PowerPoint export (best fidelity). Otherwise, use LibreOffice: `soffice --headless --convert-to pdf` into a temp folder, then render each PDF page to PNG at 1920 px wide with **pdfium** (`pdfium-render`, pdfium library bundled and loaded at runtime). LibreOffice is found on PATH and in the standard install locations per platform. If neither is available, the dialog says how to install LibreOffice.
- Images, video and audio import: copied into the content-addressed asset store as today, probed and thumbnailed with mpv (§5).
- SongSelect `.txt` and EasyWorship imports: unchanged.

## 10. Licensing

- **Slint Royalty-Free License v2.0.** Conditions met by: (a) the `AboutSlint` widget in Settings → About, reachable from the operator's top bar; and also (b) the Slint attribution badge on the project's download page/README. The app does not expose Slint's APIs (no plugin system). Slint's source license notices are left intact.
- **libmpv:** an LGPL build (`-Dgpl=false`), dynamically loaded, shipped with its license and source location.
- **pdfium:** BSD/Apache; **Noto Sans:** OFL; both with license files in the package.
- A `THIRD-PARTY-LICENSES` file is generated at build time (`cargo about`) and shown in Settings → About.

## 11. Packaging and builds

- **cargo-packager** produces: Windows `.exe` (NSIS) installer; macOS `.app` in a `.dmg`; Linux AppImage and `.deb`.
- Bundled with each: libmpv (LGPL build), pdfium, fonts. The Linux `.deb` depends on the distro's `libmpv2` instead of bundling it.
- **GitHub Actions** builds and tests all three platforms on every push. A separate cached job builds the LGPL libmpv for Windows and macOS.
- Code signing: Windows and macOS packages are unsigned at first, so first launch shows an OS warning; the README explains how to open it. Signing (Apple Developer account, Windows certificate) is a later step.
- The project moves into git and off OneDrive first (already step 0 in `PROJECT_PLAN.md`).
- Performance rules for "any computer": Slint redraws only on change (except while a video is live); slides render once and are cached; images decode at display size; hardware video decoding when safe; release builds use `lto = "thin"` as today.

## 12. Checks before dependent work

Each is a small throwaway test on all three platforms (via CI where possible, otherwise on real machines):

| ID | Check | If it fails |
|---|---|---|
| C1 | mpv draws video under a Slint scene via the rendering notifier, with Slint text on top, 1080p at full frame rate | Use mpv software render into a Slint `Image` for all platforms and measure CPU cost |
| C2 | Winit monitor list + borderless fullscreen on a chosen monitor through Slint's winit accessor, including macOS | Use `platform`-specific calls behind the same interface |
| C3 | Files dropped onto a Slint window arrive in the app | Native file dialog only; drag-and-drop later |
| C4 | Slint's testing backend can drive the operator (click, type, read text) in CI | UI tests limited to glue-level tests; manual checklist covers the rest |
| C5 | LibreOffice → PDF → pdfium images on all three platforms | PowerPoint import Windows-only until fixed |

## 13. Testing

- **Kept:** all current unit tests (live engine, store, Bible, imports), unchanged.
- **Compositor:** golden-image tests: render fixed slides (long lyrics, scripture with reference, lower third, uppercase) and compare to stored PNGs with a small tolerance.
- **Video:** integration tests behind an environment flag that play short sample files (H.264, HEVC, VP9, ProRes, WMV, MP3, AAC) through the `video` module and check the reported events and instance IDs.
- **UI:** Slint testing-backend tests for the main flows (select item, Next/Back, Clear All, Bible lookup, add song) if C4 passes.
- **Recovery:** a test that kills the `--app` process and checks that the supervisor restores the saved frame.
- **Per-platform manual checklist** (Windows, macOS, Linux on real hardware): room on a second monitor, unplug and replug it, a set of video formats with sound, phone pairing from a real phone, PowerPoint via PowerPoint (Windows) and via LibreOffice (all three), software fallback forced with an environment variable.

## 14. Build order

The Tauri app keeps working until the native app reaches parity; the native app is built as a second binary (`rustypresenter-native`) in the same crate, then becomes the only one.

0. Git, move off OneDrive, CI skeleton for three platforms.
1. Checks C1–C5.
2. Move `App` and the dispatcher out of `server.rs` into `app`; typed `Update` stream. (Tauri app still works on top of it.)
3. `render` compositor with golden tests.
4. `video` module with integration tests.
5. Room output window (render + video + transitions + monitors).
6. Stage window.
7. Operator window, then dialogs.
8. Cross-platform imports (LibreOffice + pdfium).
9. Supervisor and crash recovery.
10. Packaging for all three platforms.
11. Remove Tauri, WebView2 code, the old web screens and FFmpeg; update `PROJECT_PLAN.md` and the v1 spec's status section.
