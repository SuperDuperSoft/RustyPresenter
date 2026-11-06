# RustyPresenter v1 — Design Spec

**Date:** 2026-10-29 (revision 2)
**Author:** Claude, with the user
**Status:** Revised after the Codex review (`docs/superpowers/reviews/2026-10-29-rustypresenter-v1-codex-review.md`). Awaiting Codex re-review, then user approval of the written spec. No implementation plan yet.
**Authority:** This spec controls **v1 behavior**. `PROJECT_PLAN.md` holds long-term requirements, collaboration rules, and the work board.
**Revision 1 backup:** `2026-10-29-rustypresenter-v1-design.rev1.bak.md`

## 1. Purpose and context

- The church currently uses **EasyWorship**. Goal: replace it with our own app **to save money**. ProPresenter is only a behavior reference; the church does not use it.
- Hardware: one **Windows laptop**. Laptop screen = operator. HDMI = room display. A **foldback** monitor is planned but not yet bought, and the second video output is unconfirmed.
- No livestream today. It may be added later; v1 must not block it.
- The church has a SongSelect subscription and a CCLI license.

## 2. Scope decisions (user-approved)

| Topic | Decision |
| --- | --- |
| Platform | **Windows only for v1.** Keep code cross-platform-friendly. Mac/Linux later. |
| Architecture | **Tauri 2**: Rust core + WebView2 windows. Built-in LAN server for the phone remote only. |
| Phone remote | **Browser page** (Android + iPhone). No store app in v1. |
| Displays | Room output required. **Foldback optional**: v1 runs fine without it. |
| Backgrounds | Solid color, still image, looping muted video. |
| Media items | Foreground **video with audio**, full-screen **still image**, and **audio items** (walk-in music etc.). |
| Clear controls | Clear All / Clear Slide / Clear Media / Clear Audio / Logo. Behavior defined by our matrix in §6 (ProPresenter-inspired). No Blackout/Restore toggle. |
| Navigation | **Next on the last slide of a live item takes the next service item live** (ProPresenter/EasyWorship style). |
| Songs | No direct SongSelect login in v1. Import SongSelect `.txt` + one-time EasyWorship library (songs only). Direct login stays a long-term goal. |
| Bible | Six translations required (NIV, ESV, CSB, HCSB, NLT, KJV). **v1 ships only with all six Ready**, unless the user explicitly approves dropping or deferring one after seeing cost/terms (§8.5). |
| Decks | PowerPoint and Google Slides imported **as slide images**. PowerPoint export through the installed PowerPoint. |
| HCSB | Keep as distinct from CSB. |

## 3. Feasibility checks before dependent work

These run first in the build. Each records its evidence in `PROJECT_PLAN.md`. If a check fails, stop and bring options to the user before building on the assumption.

| ID | Check | Blocks |
| --- | --- | --- |
| F1 | Tauri 2 on Windows can give the **room output its own WebView2 environment** (separate user-data folder → separate browser, renderer, and GPU processes). We can receive `ProcessFailed` events and recreate the window. | §4 resilience design |
| F2 | **Audio routing and unattended playback** on the church laptop in the real Tauri origin: enumerate devices, route to HDMI and a USB interface with `setSinkId`, persist across restarts, autoplay from a command without a click in the output window, behavior on device unplug. | Media build (§6) |
| F3 | **Google desktop sign-in** via system browser + loopback redirect + PKCE, and where the Picker can run (loopback page in the system browser). Token lifetime for a personal-use Google Cloud project with the `drive.file` scope. | Google Slides import |
| F4 | **EasyWorship**: installed version, consistent read-only copy (SQLite backup API or verified quiescent source), schema and RTF format. | EasyWorship importer |
| F5 | **Bible sources** for all six translations: provider, exact edition, terms, attribution, cost, permitted local storage (= FEAS-02). | Bible release gate (§8.5) |
| F6 | **Monitor identity**: stable per-monitor identity on Windows (device path/EDID) via Tauri or Win32, surviving reboot and port changes. | Display roles (§10) |

## 4. Architecture and resilience

```
Rust core (state, command queue, persistence, importers, Bible, LAN server)
   │ Tauri IPC (trusted)                      │ HTTP/WebSocket (untrusted LAN)
   ├── Operator window   [WebView2 env A]     └── Phone remote pages
   ├── Foldback window   [WebView2 env A]
   └── Room output window [WebView2 env B — own data folder] + hidden layout view [env B]
```

### 4.1 What v1 isolates (pending F1)

| Failure | Room output affected? | Detection | Recovery |
| --- | --- | --- | --- |
| Operator JS exception | No (separate JS context) | Window error handler | Error banner; operator keeps working. |
| Operator/foldback renderer hang or crash | No (env A ≠ env B) | `ProcessFailed` + heartbeat | Core recreates the window; state reloaded from core. |
| Env A browser process crash | No | `ProcessFailed` | Core recreates operator + foldback. |
| Room renderer hang/crash, env B crash | **Yes** | `ProcessFailed`, or no room heartbeat for 2 s | §4.2 room recovery. |
| Rust core panic | Yes (whole app) | Process exit | App restart path (§4.3). Mitigation: command handlers catch panics; core carries most tests. |
| GPU driver reset / Windows display failure | Possibly all | — | Not isolated. Not promised. |

If F1 fails (no separate environment possible), the user is told and v1 promises only the first row.

### 4.2 Room output recovery

1. Core marks the room "recovering". Operator shows a red **Room output recovering** banner.
2. Core recreates the room window on its assigned monitor. It starts **black**.
3. Core re-sends the **committed** LiveFrame. Static layers return: slide text, deck image, still image, color/image background. A looping muted background video restarts from its start.
4. **Audible media is not resumed.** Foreground video and audio items are cleared. The operator gets "Video/audio stopped by output recovery — click to replay."
5. Room acks the new commit (§5). The banner clears.

### 4.3 App restart

Room opens black (or logo). Last service reopens at the last position. **Nothing goes live automatically.**

## 5. Command and output lifecycle

### 5.1 Requested vs. committed

- The **room output window is the authority** on what the congregation sees and on media playback state.
- The core keeps two states: **requested** (last accepted command) and **committed** (last state the room confirmed).
- The operator **Live** box shows the **committed** frame. While requested ≠ committed it shows a "pending" marker.

### 5.2 Generations

- Every presentation-changing command gets a monotonic **generation** number (`u64`). Every message from the room carries the generation it applies to.
- Room messages: `Prepared(gen)`, `Committed(gen)`, `MediaState(gen, state, position, duration)`, `Failed(gen, reason)`.
- The core and the room **ignore any message or completion for a generation older than the current one**. A slow video load that finishes after Clear All cannot restart cleared media.

### 5.3 Transitions

- The room **preloads** a new frame (fonts, images, first video frame) off-screen, then swaps.
- Prepare timeout: 2 s for text/images, 5 s for video/audio. On failure or timeout, **the previous committed output stays**; the operator gets the error.
- Media that fails **while playing** (decode error, lost device): the room clears that media layer (to background/black for video, silence for audio), reports `Failed`, and does not retry automatically.

### 5.4 Ordering, targeting, retries

- One serialized command queue in the core. Sources: operator (IPC), phone (WebSocket).
- **Relative commands** (`Next`, `Prev`) carry the cue ID the sender currently sees. If it no longer matches the live cue, the command is dropped and the sender resyncs. A laggy double-tap cannot skip two slides.
- **Absolute commands** (`GoToCue`, `TakeLive`, clears, `Logo`) are idempotent and always apply.
- On reconnect, a phone receives a full snapshot. **Unsent commands are discarded**, never replayed.

### 5.5 Responsiveness

- The command queue and room messaging run on dedicated tasks. Imports, PowerPoint COM, PDF rendering, network lookups, and backups run on a bounded, cancellable worker pool. **Output and clear commands never wait behind them.**
- Target under 200 ms control-to-display on the local network; validate before adopting.

## 6. Layers, clears, and media

### 6.1 Layers (bottom → top)

1. **Background**: color, still image, or looping muted video. From the item or the theme.
2. **Media**: foreground video (with audio) or full-screen still image.
3. **Slide**: lyric/scripture text, or a deck slide image.
4. **Logo**: church logo image, or black if none is set.
5. **Audio** (not visual): audio items.

### 6.2 Clear matrix

| Content | Clear All `F1` | Clear Slide `F2` | Clear Media `F3` | Clear Audio `F4` | Logo `F5` |
| --- | --- | --- | --- | --- | --- |
| Lyric/scripture text | Removed | Removed | Stays | Stays | Removed |
| Deck slide image | Removed | Removed | Stays | Stays | Removed |
| Background (any) | Removed | Stays | **Removed** | Stays | Removed |
| Foreground video + its audio | Stopped | Stays | **Stopped** (audio too) | Stays | Stopped |
| Foreground still image | Removed | Stays | Removed | Stays | Removed |
| Audio item | Stopped | Stays | Stays | **Stopped** | **Stays** |
| Result on screen | Black | Background/media only | Text over black | Unchanged | Logo |

- **Clear Media stops a video's embedded audio.** This is our product decision.
- **Logo** = clear all visual layers, then show the logo. Audio items keep playing (walk-in music over the logo). Any trigger replaces the logo.
- `F1`–`F5` work **even while typing** in a text field.

### 6.3 Triggers and retriggers

- **Selecting** an item in the service plan only loads Preview. It never changes the room.
- **Take Live** or **clicking a slide thumbnail of the live item** applies that slide's full layer set: its slide content plus its background. If the same background is already playing it continues; if it was cleared it starts again.
- A foreground video/audio item retriggered **plays from the start**.
- **A different item going live:** replaces the slide layer and background. A text item going live **stops foreground video**. **Audio items continue across items** until Clear Audio, Clear All, or another audio item goes live.
- Video items auto-play when they go live (per-item setting, default on).
- **Video end** (per-item setting): Stop → media layer cleared (default), Loop, or Hold last frame.
- **Fades:** each transition/clear has a generation. A new command cancels an in-progress fade and starts its own; Clear All during a fade always ends fully cleared.

### 6.4 Media files and audio

- Supported: MP4 (H.264 + AAC), WebM (VP8/VP9 + Opus/Vorbis), MP3/AAC/WAV audio, PNG/JPEG images. Import probes each file in the output environment and warns if it cannot play.
- **Audio output device** selected in settings (pending F2). Operator preview is muted; background video is always muted. Exactly one audible player: the room window.
- **Selected device missing or changed:** pause audible media, alert the operator, and **do not silently fall back to laptop speakers**. The operator picks a device.
- If F2 shows in-app routing cannot work, the user decides the fallback (e.g., route via Windows default device).

## 7. Navigation and input

### 7.1 Cues

The service plan flattens into a **cue list**. Each cue has a stable ID: `(service item ID, arrangement occurrence, slide index)`. Repeated song sections get **distinct cues** even with identical lyrics.

### 7.2 Transition table

| Situation | Next | Prev |
| --- | --- | --- |
| Fresh service, nothing has been live | First slide of the first item goes live | Nothing |
| Mid-item | Next slide of the live item | Previous slide |
| Last slide of live item | **First slide of the next item in service order goes live** | — |
| First slide of live item | — | **Last slide of the previous item goes live** |
| Last slide of the last item | Nothing; "End of service" shown | — |
| First slide of the first item | — | Nothing |
| Another item selected in Preview | **Ignored by Next/Prev.** Order comes from the service plan, never the Preview selection. | Same |
| After Clear Slide / Clear All / Logo | Position kept; Next shows the next cue (its layers return) | Same |
| Live item edited in library | LiveFrame and live navigation use the **snapshot taken at Take Live**. Operator sees "Live item changed — Take Live to update." | Same |
| Live item removed from service | Live output unchanged; navigation continues in the snapshot; at its end, Next goes to the item now at the removed item's former position | Same |
| Re-taking an edited live item | Stays on the same source position (same section occurrence or same first verse); falls back to slide 1 | — |

Foldback "next" and phone "next" show the cue that Next would produce under this table.

### 7.3 Keyboard focus

- When focus is in an editable field (Bible box, search, notes, editors, dialogs), **that field gets all keys**, except the global keys below.
- Global keys: `F1`–`F5` (clears/logo), `Ctrl+B` (Bible box), `Esc` (leave field, focus Preview).
- Presentation keys (`→`, `←`, `Space`, `Enter` = Take Live) work only when focus is not in an editable field.

## 8. Bible

### 8.1 Flow

`Ctrl+B` → type reference → pick translation → **`Enter` = add to service (after the live item, or at the end) and load into Preview; focus moves to Preview** → `Enter` again = Take Live. Exactly one Bible action per Enter.

### 8.2 Input

- Accepts `John 1:1-14`, `jn 1 1-14`, `1 Cor 13`, `John 1:1-2:3`, `Ps 23:1,4-6`. Common abbreviations, numbered books.
- Translation buttons NIV / ESV / CSB / HCSB / NLT / KJV. Remembers last choice. `Alt+1`…`Alt+6` switch (inside the Bible box).
- Invalid reference or unavailable translation → clear message; live output untouched; **never silently substitute another translation**.
- A verse absent from a translation (e.g., textual variants) is skipped, the label reflects the verses actually shown, and the operator sees a note.

### 8.3 Layout contract

- Fit is measured by a **hidden layout view in the room's WebView2 environment**, sized to the room output's actual resolution and scaling, with the same theme CSS and fonts. If no room monitor is present, use the configured output resolution (default 1920×1080).
- The reference/attribution area is reserved first; text may never overlap it.
- Split modes: **Fit to screen** (default) or **N verses per slide**.
- Fallback order for a verse that will not fit: shrink to the theme's **minimum readable size** → split at sentence end → split at clause (`; : ,`) → split at a word boundary. Words are never dropped or reordered.
- Continuation slides are labeled, e.g. `John 3:16 (cont.)`.

### 8.4 Identity and labels

- Each slide stores its source segments structurally: `(book, chapter, verse, word range)`.
- **Manual breaks are pinned to verse boundaries** (between verse X and X+1). They survive text size changes and reflow. On a translation change, a break on a missing verse moves to the nearest existing boundary.
- Labels reflect exactly what is on the slide. Discontiguous selections keep exact references: `Psalm 23:1, 4–6 (NIV)`, never `23:1–6`.
- **Reflow and tweaks change only the item and Preview.** The live frame never changes. Live navigation uses the Take Live snapshot (§7.2) until the operator re-takes the item, which maps to the slide containing the same first verse.

### 8.5 Translation readiness and release gate

| Status | Meaning |
| --- | --- |
| **Ready – Full offline** | Any passage can be looked up without internet. |
| **Ready – Prepared offline** | Passages prepared while online work offline; new lookups need internet and fail with a clear message. |
| Needs setup | Source known; account/key/download not done. |
| Unavailable | No authorized source found, or terms/cost not accepted. |

- **v1 release requires all six at Ready (Full or Prepared).** If any is Unavailable, report source, terms, and cost to the user. Only the user can approve dropping or deferring it; record the decision in `PROJECT_PLAN.md`. An unavailable translation never counts as meeting R6.
- Development may go KJV-first.
- The readiness table in `PROJECT_PLAN.md` records, per translation: provider, exact edition, availability, attribution text, permitted local storage, cost, and lookup/preparation test results.

## 9. Songs and imports

### 9.1 Song model

- Title, writers, CCLI song #, copyright, sections.
- **Arrangement**: ordered section occurrences (e.g. `V1 C V2 C C End`); drag to reorder; repeats allowed; each occurrence is a distinct cue.
- Lines per slide default (setting), auto split, manual break editing.
- Credits footer on first or last slide (setting). Church `CCLI License #` stored once in settings.
- Alternate words in parentheses (e.g. `(him)`) imported as-is.

### 9.2 SongSelect `.txt`

- Observed format: title; blank line; section blocks headed by a label (`Verse 1`, `Chorus`, `Verse 2`, `Ending`); credits block in this order: **writers line, then `CCLI Song #N`**, `© …`, SongSelect terms line, `CCLI License #N`.
- The parser must find the writers line **before** `CCLI Song #`. The early `src/songselect.rs` must be checked against this order before reuse.
- Preview → Save. A duplicate CCLI # asks: update existing or keep both.

### 9.3 EasyWorship library (one-time, pending F4)

- Work from a **consistent read-only copy** (SQLite backup API, or a verified quiescent copy including WAL). Never write to EasyWorship data.
- Whole import is one transaction; failure rolls back.
- Duplicate handling as for TXT. End report: imported count, skipped items with reasons.
- Fixtures cover writers/copyright/CCLI extraction and RTF edge cases.

### 9.4 PowerPoint → images

- Uses its **own** PowerPoint automation session. Never closes or touches presentations the user has open. Source file opened read-only, never modified.
- Runs on the worker pool; cancellable; time-bounded.
- Output staged to a temp folder, then **published atomically** (§11). An interrupted import publishes nothing.

### 9.5 Google Slides → images (pending F3)

- Authorization in the **system browser** (not a WebView), loopback redirect with PKCE and `state` validation.
- Scope: `drive.file`. Picker runs on a loopback page in the system browser and returns the chosen file ID to the app.
- Refresh token stored in **Windows Credential Manager**. Never exposed to the phone, foldback, or output pages.
- Disconnect button revokes the token. If refresh fails, prompt to sign in again; **previously imported slides stay usable**.
- Export to PDF → render pages to images → atomic publish. Re-import creates a new asset generation (§11) and never alters a live slide.

## 10. Displays

- **Roles** (Room, Foldback) are bound to a stable monitor identity (pending F6), not to a monitor index.
- **Startup:** if the room monitor is missing, no room window opens and the operator shows a red alert. If the foldback is missing, it is simply off.
- **Hot-plug / reorder / scaling change:** each output window stays only if its assigned monitor is still present. Otherwise it closes and the operator is alerted. **Roles are never reassigned automatically.** Foldback content can never move to the room on its own.
- Output windows never open on the operator's laptop screen unless the user explicitly assigns it (test mode).
- **Displays** dialog: "Identify" shows big ROOM / FOLDBACK labels; the user assigns roles.
- Preflight checks role assignments.

## 11. Storage, assets, backups

- SQLite database in the local app data folder (**not** OneDrive).
- **Content-addressed assets:** `assets/<sha256>.<ext>`, immutable once written. Library entries reference asset hashes.
- Re-import creates new assets; it never overwrites. Deleting a library entry removes the reference only.
- **Asset retention:** an asset is kept while referenced by any library item, service, retained backup, or the current LiveFrame/live snapshot. Cleanup runs only at startup, when nothing is live.
- **Imports are staged** in a temp folder and published in one DB transaction plus move. An interrupted import publishes nothing.
- **Backups:** on service open and daily. Made with the SQLite backup API plus a manifest of referenced asset hashes. The last 10 are kept. A backup restored into a clean data folder must present its media and decks (same assets, not just same filenames).
- **Service bundle export** (zip with assets) for moving a service to another machine.
- Autosave on every change.

## 12. Operator, foldback, phone remote

### 12.1 Operator (laptop screen)

- Left: Service plan + Library tabs (Songs, Bible, Media, Decks).
- Center: slide thumbnails of the selected item.
- Right: **Preview** above **Live** (committed frame, with pending marker).
- Bottom: Clear All / Slide / Media / Audio, Logo, media transport with reported time.
- Top: Bible reference box. Status: room output, foldback, translations, paired phones.

### 12.2 Foldback (optional)

- Current slide text large, next cue text smaller.
- Clock, plus time remaining of foreground video/audio (reported by the room).
- Notes per item or per slide. Stage message box (operator → foldback only).
- Layouts and timers: later.

### 12.3 Phone remote and security

- **Assumption:** trusted church LAN. Plain HTTP; the connection is **not encrypted** by the PIN. No credentials or tokens for other services ever travel over it.
- The LAN server exposes **only** remote pages and the remote WebSocket. All operator, settings, import, and edit actions use Tauri IPC and do not exist on the LAN server.
- **Pairing:** the operator clicks **Pair phone**. This opens a **2-minute window** with a fresh random **6-digit PIN**. The QR code holds only the URL, no token or PIN.
- **Limits:** 5 wrong PINs close the window. At most 1 attempt per second per client address.
- **Session:** on success, the server issues a random 256-bit session token in an `HttpOnly`, `SameSite=Strict` cookie and stores only its hash. Sessions expire after 30 days unused. Max 5 paired phones.
- **Revoke:** the operator's Paired phones list has Revoke; revocation closes the socket at once.
- **WebSocket:** origin must match the server's own host. Every message is authorized against the session server-side.
- **Remote allowlist:** `Next`, `Prev`, `GoToCue` (within the service), `TakeLive` of a service item (tap + confirm), Clear All/Slide/Media/Audio, Logo, media Play/Pause. **Nothing else.**
- Phone shows current + next cue, service item list, buttons. Auto-reconnect with snapshot resync (§5.4).

## 13. Errors and preflight

- Preparation failures keep the previous committed output (§5.3). Failures of media already playing follow §5.3/§6.4. Process failures follow §4.
- Missing media, unavailable translations, and display problems are flagged **before** the service.
- **Preflight** button: media and asset integrity, translation readiness for passages in the service, deck assets, display roles, audio device present.

## 14. Testing and acceptance

### 14.1 Core unit tests

Navigation table (§7.2), clear matrix (§6.2), generations (§5.2), command ordering and stale-relative-command drops, Bible parse/split/labels/manual-break pinning, song arrangement cues, TXT and EasyWorship parsers, asset retention, pairing limits.

### 14.2 Runtime acceptance matrix (on the church laptop)

| Area | Check |
| --- | --- |
| Isolation | Inject an operator JS exception, renderer hang, and renderer crash separately. Room stays up. Then crash the room renderer: it recovers per §4.2 and audible media does **not** resume. |
| Generations | Delay a video load, Clear All, then let the load finish: nothing restarts. Reject a play/seek: operator shows the real state. |
| Responsiveness | Advance and clear during a PowerPoint import and an EasyWorship import; stay within the agreed latency. |
| Navigation | Song A live, B previewed: arrows never take B. Next at last slide takes the next service item. Enter in the Bible box performs exactly one action. |
| Clears | For lyrics over a looping background, foreground video with audio, a deck image, a still image, and an audio item: each clear, then slide/item retrigger, matches §6. Clear All mid-fade ends cleared. |
| Bible | Short and very long verses, discontiguous and cross-chapter ranges, repeated reflow, missing fonts, target resolutions/scaling: no clipping, no lost or reordered words, exact labels, no live change on reflow. All six translations per §8.5. |
| Remote | Wrong PINs throttled; revoked phone fails; a paired phone cannot call settings/import/edit by building requests; reconnect resyncs and replays nothing; simultaneous phone + operator actions resolve per §5.4. |
| Audio (F2) | Route to HDMI and the sound-board interface; restart; change the Windows default; unplug/replug; trigger play from the phone with no click on the output. One audible player, truthful UI. |
| Displays | Three-display setup; restart with one absent; unplug/replug each; change scaling/primary. Roles hold; foldback content never reaches the room. |
| Assets/backups | Re-import and delete a library entry while it is live: output stays valid. Restore a backup into a clean folder and present its media. Interrupted import publishes nothing. |
| Google (F3) | Authorize, pick and export a private deck, restart and refresh, revoke, re-authorize. No token reaches remote/output pages. |
| Rehearsal | Full service on real gear including an unplanned Bible lookup. |

## 15. Build order (v1, Windows)

0. **Setup + feasibility:** git repo; move the working copy out of OneDrive (OneDrive for backup only); Tauri shell; audit the early experiment; run **F1, F2, F6** (F3–F5 start in parallel).
1. **Live core:** cue list, command queue, generations, preview/committed, clear matrix, room + foldback windows, display roles, recovery, backgrounds, media + audio items, assets/backups.
2. **Songs:** editor + arrangement, SongSelect `.txt`, EasyWorship import (after F4).
3. **Bible:** parser, layout view, splitter, tweak UI; KJV first, then each translation as F5 clears; release gate §8.5.
4. **Phone remote + pairing security.**
5. **Decks:** PowerPoint, then Google Slides (after F3).
6. **Acceptance matrix + rehearsal.**

## 16. Later list (out of v1)

SongSelect direct login (needs CCLI partner access) · livestream output / NDI · deck animations, embedded deck video, editable imported text · LibreOffice conversion · macOS and Linux · store phone app · custom clear groups · foldback layouts and timers · ffmpeg auto-transcode · EasyWorship services/themes · CCLI usage reporting · TLS for the remote · foldback over the network.

## 17. Open items

- F1–F6 results.
- CCLI partner access reply (long-term direct login only; does not gate v1).
- Laptop's second video output for the foldback (USB-C video or USB display adapter; an HDMI splitter only mirrors).

## 18. Response to the Codex review

| Finding | Where addressed |
| --- | --- |
| A01 renderer isolation | §3 F1, §4 |
| A02 requested vs. actual output | §5, §6.3 fades |
| A03 navigation/focus/reflow | §7, §8.1, §8.4 |
| A04 clear/retrigger contract | §6.2, §6.3 |
| A05 Bible release gate | §2, §8.5, §3 F5 |
| A06 pairing/session security | §12.3 |
| A07 PROJECT_PLAN contradictions | `PROJECT_PLAN.md` revised |
| A08 audio routing | §3 F2, §6.4 |
| A09 Google desktop auth | §3 F3, §9.5 |
| A10 asset immutability/backups | §11 |
| A11 display hot-plug | §3 F6, §10 |
| A12 Bible layout/identity | §8.3, §8.4 |
| Additional: import isolation | §5.5, §9.4 |
| Additional: EasyWorship evidence / SQLite copies | §3 F4, §9.3, §11 |
| Additional: acceptance matrix | §14.2 |

User decisions taken in this revision: Next at last slide → next item; Bible ships only with all six unless the user approves an exception; audio items added and Clear Audio kept; foldback optional.
