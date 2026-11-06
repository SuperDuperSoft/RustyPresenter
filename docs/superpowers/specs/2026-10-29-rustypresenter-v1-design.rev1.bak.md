# RustyPresenter v1 — Design Spec

**Date:** 2026-10-29
**Author:** Claude (brainstorming session with the user)
**Status:** Approved by the user in conversation. Awaiting Codex review. No implementation plan yet.
**Related:** `PROJECT_PLAN.md` (shared requirements and work board)

## 1. Purpose and context

- The church currently uses **EasyWorship**. Goal: replace it with our own app **to save money**. (The church does not use ProPresenter; ProPresenter is only a behavior reference.)
- Hardware: one **Windows laptop**. Laptop screen = operator. One HDMI output = room display. A second monitor output will be added for a **foldback** (stage/confidence) display.
- No livestream today. It may be added later; v1 must not block it.
- The church has a SongSelect subscription and a CCLI license.

## 2. Scope decisions made in this session

| Topic | Decision |
| --- | --- |
| Platform | **Windows only for v1.** Keep code cross-platform-friendly; Mac/Linux later. |
| Architecture | **Tauri app**: Rust core + WebView2 windows (operator, room output, foldback). Built-in local web server for the phone remote. Chosen as the best long-term option. |
| Phone remote | **Browser web page** (Android + iPhone). No store app in v1. |
| Displays | Room output + foldback, each a separate full-screen window on its own monitor. |
| Backgrounds | Solid color, still image, **and looping video** (muted). |
| Video items | Full video playback **with audio** as a service item. |
| Clear behavior | **Match ProPresenter**: Clear All / Clear Slide / Clear Media / Clear Audio, plus Logo (black or church logo). Replaces the earlier Blackout/Restore idea. |
| SongSelect | **No direct login in v1.** Import SongSelect `.txt` downloads + one-time EasyWorship library import. Direct login stays on the later list pending CCLI partner access. (User decision; see PROJECT_PLAN R4.) |
| EasyWorship migration | **Songs only** (lyrics, credits, CCLI #). No services or themes. |
| PowerPoint / Google Slides | **Slides as images** in v1. Animations/embedded video and editable text later. |
| PowerPoint renderer | PowerPoint is installed on the church laptop → use PowerPoint export. LibreOffice fallback later. |
| HCSB | Keep as a required, distinct translation. User accepts sourcing effort. |

## 3. Architecture

```
┌──────────────────── Tauri app (one process tree) ────────────────────┐
│  Rust core (single source of truth)                                  │
│   • Library (songs, Bible cache, media, decks)   • Service plan      │
│   • Preview cue / LiveFrame / layer + clear state                    │
│   • Command bus  • Persistence (SQLite)  • Importers  • Bible        │
│   • Local HTTP/WebSocket server (phone remote)                       │
│        │ events (state snapshots)          ▲ commands                │
│  ┌─────┴──────┐  ┌──────────────┐  ┌──────────────┐                  │
│  │ Operator   │  │ Room output  │  │ Foldback     │  WebView2        │
│  │ window     │  │ window       │  │ window       │  windows         │
│  └────────────┘  └──────────────┘  └──────────────┘                  │
└──────────────────────────────────────────────────────────────────────┘
          ▲ Wi-Fi (paired)
     Phone browser remote
```

- Each WebView2 window runs in its own renderer process: an operator UI fault must not take down the room output.
- The Rust core is the critical component and carries most of the tests.
- Room output is a web page, so a future livestream can capture it (e.g., OBS browser/window source) before any NDI work.

## 4. Core model and live state

- **Library:** songs, Bible text cache, media files (copied into the library folder), imported decks (slide images).
- **Service plan:** ordered items — song, scripture, deck, video, image, announcement.
- **Preview:** the item/slide the operator is looking at. Never shown to the room.
- **LiveFrame:** an immutable, fully resolved snapshot (text, style, background, media reference) created when content goes live. Edits, deletes, imports, and service loads never mutate it.
- **Output layers (bottom → top):** Background (color/image/video) → Slide (text or slide image) / Media item → Audio. Logo shown when everything is cleared (black or logo, per setting).

### Going live

1. Selecting a service item loads it into **Preview**. The room is untouched.
2. **Take Live** (`Enter`) sends it to the room.
3. Once an item is live, clicking any of its slides or pressing `→` / `←` / `Space` changes the live slide immediately (ProPresenter/EasyWorship feel).
4. The safety boundary is between items, not between slides within the live item.

### Clear controls (ProPresenter behavior)

| Control | Key | Effect |
| --- | --- | --- |
| Clear All | `F1` | Clears every layer, stops all media and audio → black or logo. |
| Clear Slide | `F2` | Removes text/slide layer only. Background keeps playing. |
| Clear Media | `F3` | **Stops** video/background media and its audio (stop, not pause). Re-trigger to play from the start. |
| Clear Audio | `F4` | Stops audio tracks only. |

- Configurable fade time (0 = instant).
- To bring content back, the operator clicks the slide/item again. There is no Restore button.
- The same controls are on the phone remote.
- **To verify during build:** exact ProPresenter 7 handling of a video's embedded audio on Clear Media (assumed: stops with the video).
- Custom clear groups: later.

### Command bus

- Operator keyboard/mouse, phone remote, and foldback (none in v1) all send the same typed commands: `TakeLive`, `Next`, `Prev`, `GoToSlide`, `ClearAll`, `ClearSlide`, `ClearMedia`, `ClearAudio`, `PlayPause`, `Seek`, etc.
- The core validates, updates state, and broadcasts a state snapshot to every window and paired remote.

### Video items

- Play on the room output **with audio**. Operator gets play/pause/seek/loop/volume and time remaining.
- The operator preview is **muted** (no double audio). Background videos are always muted.
- **Audio output device** selectable in settings (laptop speakers, HDMI, USB interface to the sound board) via WebView2 audio sink selection.
- Supported for sure: MP4 (H.264/AAC), WebM. Import probes the file and warns if WebView2 cannot play it. Auto-transcode (ffmpeg) later.

### Storage

- SQLite database in the local app data folder (**not** OneDrive).
- Media copied into a managed library folder.
- Autosave on every change; backup snapshot whenever a service is opened.
- Crash recovery: on restart, reopen the last service positioned at the last item. The output starts on the logo/black. It does not auto-resume live content.

## 5. Bible lookup

**Target flow:** `Ctrl+B` → type reference → pick translation → `Enter`/`Ctrl+Enter` → Take Live.

### Input

- Global shortcut `Ctrl+B` focuses the reference box.
- Parser accepts: `John 1:1-14`, `jn 1 1-14`, `1 Cor 13` (whole chapter), `John 1:1-2:3` (cross-chapter), `Ps 23:1,4-6` (lists). Common abbreviations and numbered books.
- Translation buttons NIV / ESV / CSB / HCSB / NLT / KJV. Remembers last choice. `Alt+1`…`Alt+6` switch.
- Invalid reference or unavailable translation → clear operator message. Live output untouched. **Never fall back to another translation silently.**

### Automatic splitting (driven by settings)

- Split mode (default configurable):
  - **Fit to screen** (default): as many whole verses as fit at the configured text size and theme.
  - **N verses per slide.**
- Verses are not split across slides unless a single verse cannot fit; then break at sentence, then clause/comma boundaries. Never drop words.
- Every slide shows a reference label, e.g. `John 1:1–3 (NIV)`, and the translation attribution. Verse numbers on/off.

### Easy tweaking (in the preview strip)

- Slider for text size or verses-per-slide; slides re-flow live.
- Click between two verses to add/remove a manual break. Manual breaks are pinned and survive re-flow.
- Tweaks apply to this item only; "Save as default" promotes them to settings.
- `Enter` = add to service. `Ctrl+Enter` = load into Preview, ready for Take Live.

### Sources

- One `BibleSource` trait. Each translation is a separate provider.
- Text cached locally where the provider's license permits, so live services make no network calls per slide.
- Translation status page: Ready / Needs setup / Offline-limited.
- KJV is local (public domain). NIV/ESV/CSB/HCSB/NLT depend on PROJECT_PLAN FEAS-02.

## 6. Songs and imports

### Song model

- Title, writers, CCLI song #, copyright, sections (Verse 1, Chorus, Bridge, Ending, …).
- **Arrangement:** ordered list of section references (e.g. `V1 C V2 C C End`), drag to reorder, repeats allowed.
- Lines per slide (configurable default), auto split, manual break editing like the Bible flow.
- Credits footer on first or last slide (setting): writers, ©, `CCLI Song #`, and the church `CCLI License #` from **settings** (stored once, not per song).
- Alternate words in parentheses (e.g. `(him)`) imported as-is; operator can edit.

### Import: SongSelect `.txt`

- Drag-and-drop or Import button.
- Format observed from a real export: title line; blank line; section blocks each headed by a label (`Verse 1`, `Chorus`, `Verse 2`, `Ending`); then a credits block: writers line, `CCLI Song #N`, `© …`, SongSelect terms line, `CCLI License #N`.
- Preview → Save. A duplicate CCLI # asks: update existing or keep both.
- Existing `src/songselect.rs` may be reused after review.

### Import: EasyWorship library (one-time)

- Target EasyWorship 6/7 (SQLite song DB, lyrics stored as RTF). **Confirm the installed version.**
- Open a **copy**, read-only. Never write to EasyWorship data.
- End report: imported count, skipped items with reasons.

### Import: PowerPoint → slide images

- Use the installed PowerPoint (COM automation) to export each slide as an image at output resolution.
- Never modify the source file. Store images locally (offline playback).
- LibreOffice fallback: later (also needed for Mac/Linux).

### Import: Google Slides → slide images

- Google sign-in once. **Google Picker** to choose a deck. Request only per-file access (`drive.file` scope), not whole-Drive access.
- Export as PDF → render each page to an image → store locally.
- "Re-import" refreshes the deck but never replaces a slide that is currently live.
- **To verify:** OAuth app publishing status and token lifetime for a personal-use Google Cloud project.

## 7. Operator screen, foldback, phone remote

### Operator (laptop screen)

- Left: Service plan + Library tabs (Songs, Bible, Media, Decks).
- Center: slide thumbnails of the selected item.
- Right: **Preview** above **Live**, always visible.
- Bottom: Clear All / Slide / Media / Audio, Logo, media transport.
- Top: Bible reference box.

### Foldback (second monitor)

- Large current slide text, smaller next slide text.
- Clock and video time remaining.
- Notes per item or per slide (operator-authored, e.g. "repeat chorus x2").
- Stage message: operator types a short message that appears on the foldback only.
- Layout choices and countdown timers: later.

### Phone remote (browser)

- Operator screen shows QR code + 4-digit PIN. Scan, enter PIN → paired session token.
- Phone shows current + next slide, service item list, Next/Prev, clear buttons, video play/pause.
- No editing, no settings, no account credentials exposed. Unpaired clients get nothing.
- Auto-reconnect on Wi-Fi loss. Remote loss never changes the room output.

## 8. Error handling

- All failures (import, lookup, network, bad media) surface on the operator screen only; live output is never altered by an error.
- Missing media is flagged on the item before the service.
- **Preflight** button: checks media files, translations, decks, and display assignments before a service.

## 9. Testing

- Rust core unit tests: preview/live separation, clear semantics, navigation at item boundaries, Bible reference parsing and splitting, song arrangement, TXT and EasyWorship parsers.
- Real fixtures: SongSelect `.txt` exports, a copy of the EasyWorship database, real church decks. Do not commit copyrighted song text or private church content. Keep fixtures local/ignored, or use synthetic equivalents in the repo.
- Final acceptance: full rehearsal service on the church laptop with projector, foldback monitor, and phone.
- Responsiveness target (from PROJECT_PLAN): under 200 ms control-to-display on the local network; validate before adopting.

## 10. Build order (v1, Windows)

1. **Setup:** git repo; move working copy out of OneDrive (OneDrive for backup only); Tauri shell; audit the early experiment (AUDIT-01).
2. **Live core:** service plan, preview/live, clears, three windows on chosen monitors, backgrounds, video + audio, audio device selection.
3. **Songs:** editor + arrangement, SongSelect `.txt` import, EasyWorship import.
4. **Bible:** parser, splitter, tweak UI; KJV first, then other translations as each source is approved.
5. **Phone remote + foldback.**
6. **Decks:** PowerPoint, then Google Slides.
7. **Rehearsal** on the real hardware.

## 11. Later list (explicitly out of v1)

SongSelect direct login (needs CCLI partner access) · livestream output / NDI · deck animations, embedded deck video, editable imported text · LibreOffice conversion · macOS and Linux · native store phone app · custom clear groups · foldback layouts and timers · ffmpeg auto-transcode · EasyWorship services/themes import · CCLI usage reporting.

## 12. Open items (do not block this design)

- Bible source, terms, and cost for NIV, ESV, CSB, HCSB, NLT (FEAS-02).
- CCLI partner access reply (FEAS-01).
- EasyWorship version installed on the church laptop.
- Laptop's second video output for the foldback (USB-C video or a USB display adapter; an HDMI splitter only mirrors).
- ProPresenter Clear Media audio behavior (verify).
- Google OAuth publishing/token behavior for personal use.
