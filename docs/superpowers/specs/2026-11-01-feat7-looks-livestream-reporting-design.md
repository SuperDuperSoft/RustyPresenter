# FEAT-7 — Screen Views, Livestream Output, CCLI Song Use Report, Planning Center Import

**Date:** 2026-11-01 (rev 2, after review) · **Status:** design, written while Jake was asleep; product defaults listed in §11 for review.
**Roadmap entry:** `PROJECT_PLAN.md` §10b FEAT-7 ("Livestream output with lower-thirds (uses FEAT-1 looks); CCLI usage reports; Planning Center").
**Builds on:** v1 spec (`2026-10-29-rustypresenter-v1-design.md`) §5 lifecycle, §6 layers and clear matrix, §7 navigation, §10 displays, §12.3 remote; native spec (`2026-10-29-native-slint-design.md`) §3–§7; Plan 3 (`2026-11-01-native-operator.md`) operator window, dialogs, `Theme` global, `Ops`.
**Depends on:** FEAT-1 (saved themes, overlay layer, `render_overlay`). Uses FEAT-2 (timer and message overlays, macros), FEAT-4 (per-item arrangement), FEAT-5 (parallel scripture) and FEAT-6 (practice mode) where present. §10 lists the exact contracts.

**Build phases** (each shippable on its own): **A** screen views · **B** livestream output · **C** song use report · **D** Planning Center import (behind the Cargo feature `planning-center`, built last, §5.6).

## 1. Purpose and who it's for

Four small-church needs that EasyWorship and ProPresenter charge for:

1. **Screen views.** Choose what each output shows. Example: the room shows everything, the livestream shows only the words in a lower third. Example: during a special song the room hides the words without clearing them, so the operator can bring them back with one click.
2. **Livestream output.** Many small churches stream with OBS on the same laptop or on a second computer. They need the words as a clean lower third with a transparent background (or a key colour for a hardware switcher), not a camera pointed at the projector screen.
3. **CCLI song use report.** A church with a CCLI Church Copyright License (and the Streaming License) must report which songs it used. Today volunteers count by hand. The app already knows every song it put on screen.
4. **Planning Center import.** Churches that plan in Planning Center Services want the plan's songs, readings and notes to arrive as a service, instead of retyping them.

Who: non-technical volunteers on any Windows, macOS or Linux laptop, including old ones. The church that started the project has no livestream today (v1 spec §1), so all four features stay out of the way until someone turns them on.

## 2. Scope

**In:**
- Screen views: named sets of visible layers (with overlays split by source: timers, messages, other overlays), deck rule and an optional text style, for the Room and Livestream outputs, switched live by an explicit command.
- Livestream output in two forms: (a) a **browser source** URL for OBS, Streamlabs or vMix, with real transparency; (b) a **stream window** (full screen on a chosen monitor or capture card, or a normal window) over a key colour.
- Song use log written by the core, report dialog with date range, CSV export and a printable page, "mark as reported".
- Planning Center Services import (phase D, Cargo feature): connect with OAuth + PKCE or a personal access token, pick a plan, preview, import as a service, re-import.

**Out (not in FEAT-7):**
- NDI output. §6.4 defines the `StreamSink` seam so it can be added later with the NDI runtime loaded at run time; nothing NDI is built or shipped.
- Video on the livestream output (§6.2). The stream computer captures video from its own sources.
- Stage outputs: FEAT-3 layouts decide what the stage shows, and **extra stage screens are not in FEAT-7** (they belong to a later FEAT-3 follow-up, §10).
- Uploading the report to CCLI (no public reporting API).
- Writing back to Planning Center, downloading its media files, syncing to Planning Center Live.
- Bible translation sources (another session owns `src/bible.rs` providers; this spec uses the abstract provider).
- Recording or encoding a stream; camera input.

## 3. User experience

### 3.1 Screen views

Built-in views. Their layer choices cannot be changed or deleted ("Duplicate" makes an editable copy). The look of the Lower third's words is the FEAT-1 theme **"Livestream lower third"**, which *is* editable in Settings → Look like any theme, with **Reset** (§10).

| View | Background | Videos & pictures | Words & slides | Other overlays (logo bug, clock) | Timers | Messages | Logo screen (F5) | Text style | Deck slides |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **Everything** | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | As the item | Shown |
| **Words only** | – | – | ✓ | ✓ | – | – | – | As the item | Shown |
| **Lower third** | – | – | ✓ | ✓ | – | – | – | "Livestream lower third" | Hidden |
| **No words** | ✓ | ✓ | – | ✓ | ✓ | ✓ | ✓ | – | Hidden |

Timers and messages are off on the stream views on purpose: a nursery pager ("Parent of child #12…") or a countdown must never reach the public livestream unless the church chooses it in a custom view.

Defaults: Livestream = **Lower third** (remembered between runs). Room = **Everything**, and the Room is **always Everything when the app starts** (§5.1): a view picked for a special song on Saturday never hides the words on Sunday.

Operator flow — choose a view for a screen:
1. Top bar **Screens** → the Screens dialog (Plan 3 Task 19). The Room screen card and the new **Livestream (optional)** card each have a **Shows** picker listing every view. Under the Room picker: "Goes back to Everything when RustyPresenter starts."
2. Picking a view sends `screen-view/set` at once (an explicit command; the dialog says "Changes the screen right away."). Toast: "Room screen now shows: No words".
3. When the Room view is not **Everything**, the live column shows an amber chip above the monitor: "Room shows: No words · Change". "Change" opens a small menu of views (same action). The live monitor always shows what the room shows, with the view applied.

Operator flow — make a custom view:
1. Screens dialog → **Edit screen views…** opens the Screen views dialog.
2. Left: list of views (built-ins marked "Built in"). Buttons **New**, **Duplicate**, **Rename**, **Delete** (custom only). Delete confirms "Delete the screen view “N”?"; when a FEAT-2 macro uses it, the confirm adds "The macro “Walk-in” uses it; that step will stop working."
3. Right, for the selected custom view: checkboxes "Show background", "Show videos and pictures", "Show words and slides", "Show the logo screen (F5)", and an **Overlays** group: "Logo bug, clock and other overlays", "Timers", "Messages". **Deck slides**: "Show" / "Hide". Under **More options** (collapsed): **Text style** — "Same as the song or item" (default) or any saved FEAT-1 theme, with the note "Replaces each song's own style on this screen." A 16:9 preview renders the sample slide with the view applied.
4. **Save**. If the edited view is on a screen right now: "“N” is on the room screen now. Update the screen now?" [Update now] [Later]. "Update now" sends `screen-view/set` again; "Later" leaves the screen as it is (an edit never changes output by itself).

Clear keys (F1–F5) behave exactly as today; a view only hides layers on its screen, it never clears them. Pressing **Logo F5** while the room view hides the logo shows black and toasts "The room's screen view hides the logo."

Phone remote: the phone shows "Room shows: No words" under the live slide when the room view is not Everything. It cannot change views (allowlist unchanged, §5.7). A FEAT-2 macro containing a view step runs from the phone only if FEAT-2's rules allow that macro there.

### 3.2 Livestream output

Turning it on (Screens dialog → Livestream card, text "Words for your livestream, as a lower third or full screen. Off until you turn it on."):
1. **How does your stream get it?** segmented:
   - **OBS / streaming app (browser source)** (default). The card shows `http://<this-computer>:<port>/stream/<key>` in a `CopyText` with **Copy**, plus steps: "1. In OBS, add a Browser source. 2. Paste this address. 3. Set width 1920 and height 1080." A second address with `127.0.0.1` has the note "Use this one when OBS runs on this computer." **New address** (confirm "The old address stops working. Continue?") makes a new key.
   - **A screen or capture card**: monitor picker with **Show on this screen** / **Turn off** / **Test in a window**, like the Room card. **Background**: Black (default, for luma key) or Green (for chroma key). The picker never offers the room's or stage's monitor as a choice to apply: picking one shows "That screen is used by the room. Pick another." (or "…by the stage."). The operator's own monitor is listed but never chosen automatically.
2. **Shows** picker (default Lower third).
3. Status chip: browser source "Connected" (an OBS page is open, §5.3) / "Waiting for OBS"; screen "Showing" / "Starting…" / "Off".
4. Note under the card, always: "Videos aren't sent to the livestream output. Send video to your stream from your streaming app."

What the stream shows: the room's committed picture (§5.3), filtered through the stream's view, built from the same layer pictures the room uses (§6.2). Words appear on the stream when they appear in the room, crossfading over 400 ms. When the stream is in screen mode and its monitor is saved, the stream window reopens by itself at startup, like the room.

### 3.3 Song use report (CCLI)

Logging is on by default and needs nothing from the operator (§5.4). Settings → Songs (Plan 3 Task 20) gets a checkbox **"Keep a song use log for CCLI reporting"** (default on). The report opens from a new **⋯ More** menu button on the top bar (next to Settings; FEAT-7 adds it, other features may add items) → **Song use report…** (and from a link in Settings → Songs that first asks "Save your settings changes and open the report?" [Save and open] [Cancel] when Settings has unsaved changes; with no changes it opens directly).

Report dialog ("Song use report"):
1. **From** / **To** date fields. Default From = the day after "reported through" if set, else 3 months ago; To = today. Quick buttons: "Since last report", "Last 3 months", "Last 6 months", "This year".
2. Checkbox "Include songs without a CCLI number" (default off; note "Public domain songs don't need reporting.").
3. Table, one row per song, sorted by title: Song · CCLI # · Times shown · Times streamed · Last used. Header: "CCLI license #: <church_ccli>" or "Add your church's CCLI license number in Settings → Songs."
4. Buttons: **Save as spreadsheet (CSV)…** (native save dialog, default name `song-use-<from>-to-<to>.csv`), **Open printable page** (HTML in the data folder's `reports/`, opened in the system browser to print or save as PDF), **Mark as reported through <To>** (confirm "Next time, the report starts the day after <To>.").
5. **Details** expander: every log entry in range (date, time, service, song, "streamed") with **Remove** (confirm "Remove this entry? Use this for rehearsals that shouldn't count."). Toast "Entry removed".
6. Footer: "Enter these numbers at CCLI's reporting site: projected uses go under “Digital”, streamed uses under your Streaming License. “Streamed” means a livestream page or window was open, not that you were on air."

### 3.4 Planning Center import (phase D)

Connect (Settings → new tab **Planning Center**, present only in builds with the feature):
1. Text: "Bring songs, readings and notes from a Planning Center Services plan into a service here. Optional." Button **Connect to Planning Center** (shown only when OAuth is available, §5.6).
2. The system browser opens Planning Center's sign-in and consent page. After approval it shows "RustyPresenter is connected. You can close this tab." and the tab shows "Connected to <organization name>" with **Disconnect**.
3. **"Use a personal access token instead"** shows "Application ID" and "Secret" fields with a link to Planning Center's personal access token page. **Save** verifies them with one API call.

Import (Services dialog, **Import from Planning Center…**; when not connected it opens the Planning Center tab):
1. Pick **Service type** (remembered) and **Plan** (the next 8 upcoming plus the 4 most recent, with date and title).
2. Preview list, one row per plan item, each with a checkbox, the planned action and a warning icon where needed:
   - Song matched in your library → "Uses your song “Amazing Grace” (order from Planning Center)" (checked).
   - Song not in your library, with lyrics → "Adds new song from Planning Center" (checked).
   - Song without lyrics → "Adds new song — lyrics missing, paste them in the song editor" (checked, ⚠).
   - Item that names a Bible passage (book and chapter, §5.6) → "Scripture: John 3:16-18 (KJV)" (checked); if the translation can't show it → ⚠ "KJV can't show this passage" (unchecked).
   - Other items (Welcome, Prayer, Sermon) → "Announcement slide with the title" (unchecked).
   - Headers → "Skipped (heading)"; media items → "Skipped — add the file yourself" (unchecked, disabled).
3. **Import**. A new service "<plan date> <plan title or service type>" is created. If nothing is live it opens; otherwise toast "Imported “…”. Open it from Services when you're ready."
4. Re-importing a plan imported before asks: "This plan was imported before as “<service>”." [**Import as a new service**] (primary) [Update that service…]. "Update that service…" shows a second confirm listing what happens: "6 items from Planning Center will be replaced. Changes you made to them here are lost. 2 items you added yourself stay: Welcome video, Announcements." [Update] [Cancel].

Item notes from Planning Center become the service item's notes (shown on stage per FEAT-3).

## 4. Data model

All new fields have serde defaults; a `data.json` from before FEAT-7 loads unchanged. No format bump (FEAT-7 is additive on whatever format FEAT-1 sets).

```rust
// src/screen_view.rs
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum OutputRole { Room, Stream }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(default)]
pub struct OverlayMask { pub props: bool, pub timers: bool, pub messages: bool }
impl Default for OverlayMask { fn default() -> Self { Self { props: true, timers: true, messages: true } } }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(default)]
pub struct LayerMask { pub background: bool, pub media: bool, pub slide: bool, pub logo: bool, pub overlays: OverlayMask }
impl Default for LayerMask { fn default() -> Self { Self { background: true, media: true, slide: true, logo: true, overlays: OverlayMask::default() } } }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeckSlides { #[default] Show, Hide }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ScreenView {
    pub id: Uuid,
    pub name: String,
    #[serde(default)] pub layers: LayerMask,
    /// None = the theme the item resolves to (FEAT-1 theme::resolve). Some = a FEAT-1 SavedTheme id.
    #[serde(default)] pub text_theme: Option<Uuid>,
    #[serde(default)] pub deck_slides: DeckSlides,
}

/// What the live engine holds: a copy taken when the view was set (like LiveItem snapshots).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ActiveView {
    pub id: Uuid, pub name: String, pub layers: LayerMask, pub deck_slides: DeckSlides,
    pub text_style: Option<Theme>,   // copy of the SavedTheme's style at set time; None = item's own
}

/// Which overlay switch governs a FEAT-1 overlay. Unknown future kinds count as Props.
pub enum OverlaySource { Props, Timer, Message }
pub fn source_of(c: &OverlayContent) -> OverlaySource; // Timer{..}→Timer, Message{..}→Message, else Props

pub const EVERYTHING: Uuid  = uuid!("00000000-0000-4000-8000-000000000701");
pub const WORDS_ONLY: Uuid  = uuid!("00000000-0000-4000-8000-000000000702");
pub const LOWER_THIRD: Uuid = uuid!("00000000-0000-4000-8000-000000000703");
pub const NO_WORDS: Uuid    = uuid!("00000000-0000-4000-8000-000000000704");
/// FEAT-1 SavedTheme seeded into Data.themes: position "lower", size 60, scripture 46, shadow, band on (opacity 55).
pub const STREAM_LOWER_THIRD_THEME: Uuid = uuid!("00000000-0000-4000-8000-000000000711");

pub fn builtins() -> [ScreenView; 4];                                     // the rows of §3.1
pub fn all(data: &Data) -> Vec<ScreenView>;                               // builtins then Data.screen_views
pub fn find(data: &Data, id: Uuid) -> Option<ScreenView>;
pub fn activate(data: &Data, v: &ScreenView) -> ActiveView;               // a missing text_theme → None (item's own)
pub fn everything() -> ActiveView;
pub fn apply(frame: &Frame, v: &ActiveView, slide_is_deck: bool) -> Frame; // drops hidden layers and overlays
```

```rust
// Data (src/model.rs) — kept in Data, not Settings, so the Settings dialog's whole-struct save can't overwrite them
pub struct Data {
    // … existing + FEAT-1 fields …
    #[serde(default)] pub screen_views: Vec<ScreenView>,   // custom views only
    #[serde(default)] pub usage: UsageLog,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct UsageLog { pub entries: Vec<UsageEntry>, pub reported_through: Option<String> /* "YYYY-MM-DD" */ }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UsageEntry {
    pub id: Uuid,
    pub at: u64,                 // UTC seconds
    pub local_date: String,      // "YYYY-MM-DD" on this computer when shown
    pub song_id: Uuid,
    pub title: String, pub ccli: String, pub writers: String, pub copyright: String, // copies: the song may be deleted later
    pub service_id: Uuid, pub service_name: String,
    #[serde(default)] pub streamed: bool,
}

// Settings
pub struct Settings {
    // … existing fields …
    #[serde(default)] pub stream: StreamSettings,
    #[serde(default)] pub usage: UsageSettings,
    #[serde(default)] pub planning_center: PcoSettings,
}
pub struct StreamSettings {                 // #[serde(default)], explicit Default below
    pub enabled: bool, pub mode: StreamMode, pub key: String, pub key_color: KeyColor,
    pub size: (u32, u32), pub view: Uuid,
}
impl Default for StreamSettings {
    fn default() -> Self { Self { enabled: false, mode: StreamMode::BrowserSource, key: String::new(),
        key_color: KeyColor::Black, size: (1920, 1080), view: LOWER_THIRD } }
}
pub enum StreamMode { #[default] BrowserSource, Screen }
pub enum KeyColor { #[default] Black, Green }   // #000000, #00b140
pub struct UsageSettings { pub enabled: bool }   // #[serde(default)]; impl Default → enabled: true
pub struct DisplayRoles { pub room: Option<String>, pub stage: Option<String>, #[serde(default)] pub stream: Option<String> }

#[serde(default)] pub struct PcoSettings {        // derived Default: all None
    pub auth: PcoAuth,                            // None | OAuth | PersonalToken; secrets never here (§5.6)
    pub organization: Option<String>,
    pub service_type_id: Option<String>,
}

// Service / ServiceItem
pub struct Service { /* … */ #[serde(default, skip_serializing_if = "Option::is_none")] pub planning_center: Option<PcoLink> }
pub struct PcoLink { pub service_type_id: String, pub plan_id: String, pub imported_at: u64 }
pub struct ServiceItem { /* … */ #[serde(default, skip_serializing_if = "Option::is_none")] pub pco_item_id: Option<String> }
```

There is no saved room assignment: the room view lives only in the live engine and is Everything at every start.

Live engine (`src/live.rs`):
- `Live.room_view: ActiveView`, part of each `Snapshot` and of `version` (a room view change goes through prepare/commit like any other room change).
- `Live.stream_view: ActiveView` and `Live.stream_rev: u64` — **outside** `version`, snapshots and history. Changing it never makes a phone's `base` stale and is never reverted by a room failure.
- `LiveView` gains `room_view`, `stream_view`, `stream_rev`.
- `LiveItem` gains `song: Option<SongRef>` (`id, title, ccli, writers, copyright`), filled by `DataPlan::build` for song items, so the usage log reads the Take-Live snapshot (v1 §7.2), never the library.
- `Command::SetScreenView { screen: OutputRole, view: Uuid }`.
- `Plan` gains `fn view(&self, id: Uuid) -> Option<ActiveView>` (DataPlan: `find` + `activate`).

Load normalisation, `Data::normalize_on_load` (store load path): drop custom views whose id equals a built-in id; seed the `STREAM_LOWER_THIRD_THEME` SavedTheme if missing (FEAT-1 contract, §10); prune usage entries older than 3 years. `Settings::normalize` (after load and every settings save): `stream.view` unknown → `LOWER_THIRD`; make `stream.key` (32 hex chars from `random_token`) when enabled and empty.

## 5. Core behaviour

### 5.1 Setting a view

`SetScreenView` is an absolute operator command: no `base`, fresh command id, same queue (v1 §5.4). The engine:
1. `Plan::view(id)`; `None` → command error "That screen view no longer exists." (nothing changes).
2. **Room:** equal to `room_view` → `ok`, no bump. Else set `room_view`, bump `version`, remember the snapshot; frame, cursor and players unchanged. The room re-plans from the masked frame (§6.1): if no visible layer key changes, nothing is prepared and the room commits at once; if a revealed layer fails to prepare, the request reverts to the committed snapshot (old room view) and the operator sees the room's error.
3. **Stream:** equal → `ok`. Else set `stream_view`, bump `stream_rev`, broadcast `Update::Live` (version unchanged). App then saves `settings.stream.view` through `edit` (off the command path). A stream view change cannot fail (the stream has no prepare step that can reject it).

Startup: `App::new` sets `room_view = everything()` and `stream_view` from `settings.stream.view` (fallback Lower third) before anything is live.

### 5.2 Interaction with the clear matrix, navigation and media

- Views never change the `Frame`. Clears, logo, overlays, triggers and the incoming-item policy (v1 §6.2–§6.3, FEAT-1/FEAT-2 overlay rules) act on the frame exactly as today. `screen_view::apply` runs after, per screen: hidden layers are dropped; `Frame.overlays` entries are filtered by `source_of` against the `OverlayMask`; the slide layer is dropped when it is a deck image and `deck_slides == Hide`.
- Navigation (v1 §7) is untouched; Next with the words hidden still advances the cursor. FEAT-2 auto-advance is unaffected by views and, since a stream view change does not bump `version`, never races with it.
- **Hidden media keeps playing.** The room stays the playback authority: a video on a hidden layer keeps its player (sound, position reports, end action); only drawing is skipped. A still on a hidden layer is not decoded. Audio items are never affected.
- The operator's live monitor (Plan 3 `src/ui/preview.rs`) applies the room view. Slide thumbnails are unaffected.
- The phone's `remote_view` gains `"room_view": <name or null when Everything>`.

### 5.3 Stream output state

The stream follows the **real** live engine only: `StreamOutput` subscribes to `App::subscribe()` and reads `Update::Live` (never FEAT-6's practice engine updates, exactly as the room does). Input for each render:
- Room window connected: the committed frame (`LiveView` when `!pending`; while pending the stream keeps its last picture).
- No room window (stream-only setups): the requested frame.

Then `apply(frame, stream_view)`, then **always** drop the media layer and any `Background::Video`. Text style: `stream_view.text_style` if set, else each slide's own FEAT-1 theme. When the text style's `position == "lower"`, a FEAT-5 parallel scripture slide is drawn with its primary translation only (`second` ignored).

`StreamOutput` (`src/stream/mod.rs`) computes a `StreamInput` (masked layers, theme, overlay texts) and renders only when (a) the input changed, (b) at most 10 times a second, and (c) the stream is **active**: at least one browser-source SSE connection open (`stream_conns > 0`) or the stream window open. When nothing is active it renders nothing; on the first connection it renders the current picture.

Renders run on a dedicated **`stream-render`** thread, never on the room's `Renderer` worker, and the thread yields to the room: before each render it waits while `room_preparing` (an `AtomicBool` set by the room around prepare) is true, up to 2 s. The latest two PNG frames (and the decoded RGBA for the window) are kept in memory (`Arc<[u8]>`, numbered).

**Connected** (browser source) = `stream_conns > 0`, counted like `room_conns` on `/stream/<key>/events`. It does not depend on new frames, so a 30-minute sermon on one slide stays "Connected". OBS keeps browser sources open even when not on air, so "connected" means "an OBS page is open", nothing more.

### 5.4 Song use log

A use is logged when **all** hold, checked in `App::room_event` after a `Committed(v)` is accepted (and, with no room connected, after a command is applied):
1. `settings.usage.enabled`.
2. The committed live item has `song: Some(..)` and the committed frame has a text slide layer.
3. The words are visible somewhere: `room_view` shows the slide layer, or the stream is enabled, active and its view shows the slide layer.
4. `usage::should_log(&log.entries, song_id, service_id, now)`: the song is not already in the current **usage session** of that service.

Practice mode needs no rule: FEAT-6 practice runs a separate engine, and the real engine never commits practice slides.

**Usage session** (pure, computed from the log so restarts and crashes don't change it): take this service's entries, newest first; the session is the run of entries in which each entry is within **45 minutes** of the next newer one, and the newest within 45 minutes of `now`. A song already in that run is not logged again. Effect: going back to a chorus, Refresh and FEAT-4 jumps never log twice; a Service object reused for 9:00 and 11:00 logs twice (the last 9:00 song and the first 11:00 song are normally more than 45 minutes apart); a closing reprise after a sermon longer than 45 minutes with no song in between logs again (the Details list can remove it).

`streamed` = stream enabled, active (§5.3) and its view shows the slide layer. It means a livestream page or window was open, not that the church was on air.

Writing never blocks the command path or the Slint thread: `room_event` sends a `UsageHit` on an `std::sync::mpsc` channel to the `usage-log` thread started by `App::new`, which re-checks `should_log` under the data lock and appends through `App::edit` (one save, one `Update::Data`). Report and exports are pure:

```rust
pub fn report(log: &[UsageEntry], from: &str, to: &str, include_no_ccli: bool) -> Vec<ReportRow>; // grouped by ccli, else song_id
pub struct ReportRow { pub title: String, pub ccli: String, pub writers: String, pub shown: u32, pub streamed: u32, pub last_used: String }
pub fn to_csv(rows: &[ReportRow], license: &str) -> String;  // Title,CCLI Song #,Writers,Times shown,Times streamed,Last used
pub fn to_html(rows: &[ReportRow], license: &str, range: (&str, &str)) -> String;
```

### 5.5 Operator actions (`App::do_action`; none on the LAN server)

| Action | Args | Result | Errors (text from the area's `text` module) |
| --- | --- | --- | --- |
| `screen-view/set` | `{screen: "room"\|"stream", view}` | `{ok}` after the command is applied (room: as any command; the toast waits for nothing) | unknown id "That screen view no longer exists." |
| `screen-view/save` | `{view: ScreenView}` (new id when absent) | `{id, on_screens: ["room"\|"stream"]}` (the dialog uses this for the "Update now?" ask) | built-in id "Built-in views can't be changed. Duplicate it to make your own."; empty name "Give the view a name."; duplicate name "There's already a view called “N”." |
| `screen-view/delete` | `{id}` | `{ok}` | built-in (as above); on a screen "That view is on a screen. Pick another view for that screen first." |
| `stream/set` | `{enabled?, mode?, key_color?, size?}` | `{stream: StreamSettings}` | — (enabling makes the key if missing) |
| `stream/new-key` | `{}` | `{key}`; open SSE connections are closed | stream off "Turn on the livestream output first." |
| `display/open`, `display/close` | `{role: "stream", name?}` (existing actions, new role) | as today | monitor used by room/stage "That screen is used by the room. Pick another." / "…by the stage." |
| `usage/report` | `{from, to, include_no_ccli}` | `{rows, license}` | bad date "Pick a From date before the To date." |
| `usage/remove` | `{id}` | `{ok}` | — (unknown id is ok) |
| `usage/mark-reported` | `{through}` | `{ok}` | — |
| `usage/export` | `{from, to, include_no_ccli, path?, format: "csv"\|"html"}` (html without path → `reports/`, opened) | `{path}` (written on the blocking pool) | "Couldn't save the report: <reason>." |
| `pco/*`, `service/import-pco` | §5.6 (feature `planning-center`) | | §8 |

Settings dialog Save (Plan 3 Task 20 rule "keep fields edited elsewhere from the latest snapshot") keeps: `stream` (whole), `displays.stream`, `planning_center` (whole), in addition to Plan 3's `translation` and `displays`. Screen views and the usage log live in `Data`, so Save cannot touch them.

### 5.6 Planning Center (phase D, Cargo feature `planning-center`)

The feature is off in default builds until phase D is finished and **feasibility check P1** passes; official release builds then turn it on. With the feature off there is no tab, no button and none of its crates. With it on, nothing runs (no threads, no network) until the church connects.

**Check P1** (before building, on all three platforms): does Planning Center allow authorization-code + PKCE for a public client **without a client secret**, with redirect `http://127.0.0.1:4318/pco/callback`? Outcomes, both fully specified: (a) yes → OAuth and personal tokens; (b) no → **personal access tokens only**, the Connect button is not built. The app never ships a client secret.

OAuth (`src/pco/auth.rs`):
1. `pco/connect` creates `state` and a PKCE verifier/challenge (S256, `sha2` + base64url), registers a one-shot waiter in `App` and opens the system browser (`open` crate) at Planning Center's authorize URL (`response_type=code&scope=services&state=…&code_challenge=…&code_challenge_method=S256&redirect_uri=http://127.0.0.1:4318/pco/callback`).
2. The **existing app server** serves `GET /pco/callback` behind `local_ok` (loopback only); a matching `state` completes the waiter and answers the "connected" page, others get 400. If the server is not on 4318 (port taken, `RUSTYPRESENTER_PORT`), Connect shows "Planning Center sign-in needs port 4318, which is busy. Use a personal access token instead." Timeout 5 minutes → "Planning Center sign-in timed out. Try again."
3. Tokens are stored with the `keyring` crate (service `RustyPresenter`, account `planning-center`); with no OS keyring, `secrets/planning-center.json` in the data folder with owner-only permissions ("Saved on this computer only.").
4. Access tokens refresh within 5 minutes of expiry; a failed refresh sets `auth = None`: "Planning Center signed you out. Connect again."
5. **Disconnect** revokes (errors ignored), deletes the secret, sets `auth = None`.

Personal tokens (`pco/token {app_id, secret}`): verified with `GET /services/v2` and stored like OAuth tokens.

Secrets are never in `data.json`, backups, service bundles, logs, the LAN server, the phone remote, or crash reports.

API client (`src/pco/api.rs`): `ureq` (blocking, rustls, small) on the blocking pool, behind a `PcoApi` trait for tests. `service_types()`, `plans(st)` (`filter=future` ×8, `filter=past&order=-sort_date` ×4), `plan_items(st, plan)` with `include=song,arrangement`. Follows `links.next`; HTTP 429 waits `Retry-After` (max 3); 20 s timeout; the dialog's `PcoImportSession` cancels on close.

Actions: `pco/connect`, `pco/token`, `pco/disconnect`, `pco/service-types`, `pco/plans {service_type_id}`, `pco/preview {service_type_id, plan_id}` → rows, `service/import-pco {service_type_id, plan_id, rows: [(pco_item_id, checked)], target: "new" | {"update": service_id}}`.

Mapping (`src/pco/map.rs`, pure): `pub fn preview(items: &[PcoItem], lib: &[Song], bible: &dyn TranslationProvider, translation: &str) -> Vec<PreviewRow>`, `PreviewRow { pco_item_id, title, action: Planned, checked, warning: Option<String> }`, `enum Planned { UseSong { id, arrangement: Option<Vec<String>> }, NewSong { song: Song, arrangement: Option<Vec<String>> }, Scripture { reference, translation }, Text { title }, Skip(String) }`.
- Song match: CCLI number (digits only, non-empty) → exact title, case- and punctuation-insensitive → new song. A new song takes title, author → writers, copyright, CCLI number and the arrangement's lyrics; its library `arrangement` stays empty (written order).
- **Order:** the Planning Center arrangement `sequence` becomes the imported **service item's** FEAT-4 `ItemKind::Song.arrangement`, for new and existing songs alike. Each entry maps to a section label present in the song's lyrics: exact label (case-insensitive), else same `SectionKind::from_label` kind and number. Unmatched entries are dropped; if none match, `None` (the song's usual order). **The library song is never modified.**
- Scripture: the title, then the description, goes through the Bible reference parser; a match counts only if it has a book **and** a chapter number (verses optional) **and** the provider confirms the passage exists. "Acts of Service", "Mark's welcome" and "Job fair" are not scripture. Verses are fetched at import time in `settings.translation`.
- Every imported item gets `pco_item_id`.

Import applies the dialog's preview in one `edit` (new songs + service). **Update** (re-import) keeps the service id and name, replaces every item that has a `pco_item_id`, and keeps items without one: each stays after the imported item it followed (matched by `pco_item_id`), at the top if it preceded all, at the end if its anchor is gone. A live item that disappears follows v1 §7.2. Live output is never touched.

### 5.7 Phone remote allowlist

Unchanged. `SetScreenView`, stream, usage and Planning Center actions are not added. The phone only reads `room_view`. The stream routes (§6.3) are a separate read-only surface, not part of the remote.

### 5.8 Edge cases

| Case | Behaviour |
| --- | --- |
| Custom view deleted while on a screen | Refused (§5.5). |
| Custom view deleted while a FEAT-2 macro uses it | Allowed after the confirm in §3.1; the macro step is kept and fails with "That screen view no longer exists." (FEAT-2's rule for unknown ids). |
| A view's FEAT-1 theme deleted | The `ActiveView` on screen keeps its style copy until the view is set again; `activate` then uses the item's own theme; the Screen views dialog shows "Theme was deleted — using the item's theme". |
| A view's FEAT-1 theme edited while that view is on a screen | FEAT-1's theme Save reports it; the operator gets "“N” uses this theme and is on the livestream now. Update the screen now?" [Update now] [Later] (same as §3.1 step 4). |
| `STREAM_LOWER_THIRD_THEME` | Editable, Reset restores the seeded values, Delete refused by FEAT-1 ("The Lower third screen view uses this theme."). |
| Room view hides everything | Allowed; the chip uses the stronger `notice` colours. |
| Stream key leaked | **New address** invalidates it; old URLs return 404, open SSE streams close. |
| Two OBS instances | Both get frames; `stream_conns` = 2. |
| Port changes between runs | The card always shows the current address. |
| Stream monitor = room or stage monitor | Refused (§5.5). A saved `displays.stream` that now equals the room's is ignored at startup with the notice "The livestream screen is the same as the room screen. Pick another in Screens." |
| Only the operator's monitor available | The stream window is not opened automatically; the card says "Pick a screen". The operator may pick it by hand or use "Test in a window". |
| Startup with `stream.enabled`, mode Screen, `displays.stream` present and connected | The stream window reopens, like the room. |
| Stream window monitor unplugged | As the room (native spec §7): closes, notice, reopens when it returns; never moved. |
| Deck slide of a song deck | Not logged: only song items with text slides count. |
| Song edited after it was logged | The entry keeps its copies; the report groups by CCLI number when present. |
| Clock / time zone changes | `local_date` recorded at log time; session gaps use UTC seconds. |
| Planning Center plan has 0 items | "This plan has no items." Import disabled. |
| Offline during import | "Can't reach Planning Center. Check the internet connection." Nothing imported. |

## 6. Rendering

### 6.1 Room

`ui/room.rs` builds `RoomState::plan` from `screen_view::apply(&view.frame, &view.room_view, …)`. `room::Layers::of` includes the room view's mask and text style in its keys, so a view change re-plans only layers whose visibility or style changed; FEAT-1's `Kind::Overlays` key is computed from the filtered overlay list. Hidden media players keep running; `RoomWindow` gets `media-visible: bool`, which hides the mpv underlay with the base black (GL) or hides `video-frame` (software).

### 6.2 Stream compositor

`src/render/compose.rs`: `pub fn compose(input: &StreamInput, canvas: (u32, u32), fonts: &Fonts, images: &ImageCache) -> Rendered`. It does **not** lay anything out itself: it calls the same per-layer functions the room's prepare step and the Preview use — background (`render::background_picture`, the same cover fit and dim), still image (same fit), slide (`render_text` with the slide's theme, or the deck image), logo, overlays (FEAT-1 `render_overlay` placed by `Placement`) — and alpha-blends them bottom to top in the room's order: **background, still, slide, logo, overlays**. Pixels outside drawn content stay transparent. `ImageCache` decodes assets once at canvas size (LRU, 64 MB). PNG encoding uses `image`'s encoder with fast compression (target under 60 ms per 1080p frame on the oldest test laptop). Crossfades are done by the consumer (browser page, stream window), 400 ms like the room.

Why no video: drawing video into the stream would need a second mpv player kept in sync with the room, or per-frame PNGs; both are too heavy for old laptops and risk the room's playback (decision 8).

The seeded **Livestream lower third** theme is an ordinary FEAT-1 theme (position lower, size 60, scripture 46, shadow, band). Text that does not fit shrinks by the existing fit steps down to 26 px; words are never dropped.

### 6.3 Browser source page

`server.rs` serves, outside `local_ok` but only when `stream.enabled` and the path key matches (constant-time compare):
- `GET /stream/<key>` → `web/stream.html` (embedded, under 60 lines): transparent `body`, two `<img>` crossfading 400 ms, an `EventSource` on `/stream/<key>/events`.
- `GET /stream/<key>/events` → SSE `{"n": <frame number>}` per new frame, a comment ping every 15 s; increments `stream_conns` while open (a guard decrements on drop).
- `GET /stream/<key>/frame/<n>.png` → the PNG (`Cache-Control: no-store`); unknown `n` → newest.
Read-only; wrong key or stream off → 404.

### 6.4 Stream window and the sink seam

`ui/stream.slint` `StreamWindow`: background `key-color`, two `Image` faces with the room's 400 ms crossfade, no cursor. `src/ui/stream.rs` sets each new frame from the RGBA kept with the PNG. Opened and closed by `NativeScreens` under the role `"stream"`; the window counts as active while open.

```rust
pub trait StreamSink: Send + Sync { fn frame(&self, n: u64, rgba: &Rendered, png: &Arc<[u8]>); fn active(&self) -> bool; }
```
`BrowserSourceSink` and `WindowSink` implement it. A later NDI sink would load the NDI runtime at run time with `libloading` (never bundled or linked), keeping MIT/Apache and the LGPL libmpv arrangement intact.

## 7. UI (Plan 3 files extended; new files)

Extended:
- `ui/dialogs/screens.slint`, `src/ui/dialogs/screens.rs`: **Shows** pickers; Livestream card in `ui/dialogs/screens-stream.slint`; **Edit screen views…**; `default_pick` for the stream excludes the room's, stage's and operator's monitors.
- `ui/operator/live.slint`, `src/ui/operator/live.rs`: "Room shows: … · Change" chip and menu.
- `ui/operator/topbar.slint`: new `more-btn` (**⋯ More**, `callback more()`) opening a menu with **Song use report…**; element id `more-btn`.
- `src/ui/preview.rs`: applies the room view.
- `ui/dialogs/settings.slint`, `src/ui/dialogs/settings.rs`: Songs tab checkbox and report link (save-first confirm); Save keep-list (§5.5); **Planning Center** tab (`ui/dialogs/settings-pco.slint`, `src/ui/dialogs/settings_pco.rs`, `#[cfg(feature = "planning-center")]`).
- `ui/dialogs/services.slint`, `src/ui/dialogs/services.rs`: **Import from Planning Center…** (feature only).
- `src/ui/dialogs/mod.rs`: `DialogKind::{ScreenViews, Usage, PcoImport}`.
- `src/ui/operator/view.rs`: `pub fn room_view_chip(view: &LiveView) -> Option<String>`.
- `web/remote.js`: shows `room_view`.

New: `ui/dialogs/screen-views.slint` + `src/ui/dialogs/screen_views.rs`; `ui/dialogs/usage.slint` + `src/ui/dialogs/usage.rs`; `ui/dialogs/pco-import.slint` + `src/ui/dialogs/pco_import.rs`; `ui/stream.slint` + `src/ui/stream.rs`; core `src/screen_view.rs`, `src/usage.rs`, `src/stream/{mod,sink,text}.rs`, `src/render/compose.rs`, `src/pco/{mod,auth,api,map,text}.rs`, `web/stream.html`.

Colours come from the `Theme` global (new tokens `chip-warn-bg`, `chip-warn-text`). Every control a test touches has an element id (`ScreensDialog::stream-copy`, `UsageDialog::export-csv`, `ScreenViewsDialog::more-options`, …).

**Strings:** one module per area, in the plain tone of `web/operator.js`: `screen_view::text`, `stream::text`, `usage::text`, `pco::text` (`pub const`s and small `fn`s for strings with values). Every message in §3, §5.5 and §8 lives there; `.slint` files receive FEAT-7 labels as properties.

**New dependencies:** phase A–C: `csv`, `uuid` macro feature. Phase D only (feature `planning-center`): `ureq` (rustls), `keyring`, `open`, `base64`. All MIT/Apache-compatible; Plan 4 adds them to `THIRD-PARTY-LICENSES`.

## 8. Errors

| Situation | Message |
| --- | --- |
| Unknown view id (command, macro step) | "That screen view no longer exists." |
| Room fails to show a revealed layer | The room's prepare error; the previous room view stays. |
| Stream port blocked | Card note when no OBS page has connected 2 minutes after enabling: "OBS on another computer can't connect? Allow RustyPresenter through this computer's firewall." |
| Stream render fails | Logged; the stream keeps its last frame; toast "The livestream picture couldn't update." at most once a minute. |
| Export write fails | "Couldn't save the report: <reason>." |
| Usage log save fails | Logged; the hit stays in memory and is retried on the next save; never interrupts the service. |
| Planning Center offline | "Can't reach Planning Center. Check the internet connection." |
| 401 after refresh | "Planning Center signed you out. Connect again." |
| 403 | "This Planning Center account can't see Services plans. Ask your Planning Center admin for access." |
| Personal token wrong | "Planning Center didn't accept that Application ID and Secret." |
| Callback port busy | "Planning Center sign-in needs port 4318, which is busy. Use a personal access token instead." |
| No keyring and file write fails | "Couldn't save the Planning Center sign-in on this computer." |

## 9. Testing

**Core unit tests** (names fixed):
- `screen_view`: `builtins_have_fixed_ids_and_cannot_be_saved_as_custom`, `find_unknown_id_is_none`, `apply_hides_masked_layers`, `apply_filters_overlays_by_source`, `lower_third_hides_messages_and_timers_by_default`, `everything_shows_all_overlays`, `apply_hides_deck_image_when_deck_slides_hide`, `apply_keeps_text_slide_when_deck_slides_hide`, `activate_with_deleted_theme_uses_item_theme`, `active_view_with_deleted_theme_keeps_its_copy`, `empty_json_deserialises_to_stream_settings_default` (`{}` → view `LOWER_THIRD`, size 1920×1080, black), `old_data_json_loads_without_screen_views_or_usage`, `normalize_seeds_stream_lower_third_theme`.
- `live`: `room_view_change_bumps_version_without_touching_frame_or_cursor`, `stream_view_change_does_not_stale_phone_base`, `stream_view_change_bumps_stream_rev_only`, `same_view_twice_is_ok_without_bump`, `unknown_view_id_fails_with_message`, `failed_reveal_reverts_room_view_with_frame`, `room_failure_does_not_revert_stream_view`, `room_view_resets_to_everything_on_startup`, `stream_view_restored_from_settings_on_startup`, `clear_all_with_words_hidden_still_clears_frame`, `next_with_words_hidden_advances_cursor`, `set_screen_view_has_no_base_and_dedupes_by_id`.
- `room` (`src/room.rs`): `stream_view_change_prepares_nothing_in_room`, `room_view_hiding_media_keeps_player_and_reports`, `room_view_reveal_prepares_only_revealed_layer`, `room_prepare_not_delayed_by_stream_render` (a slow fake stream render is running; a room prepare starts at once on its own worker and the stream thread waits on `room_preparing`).
- `server`: `remote_cannot_send_set_screen_view`, `stream_routes_404_when_disabled_or_wrong_key`, `stream_frame_returns_newest_for_unknown_number`, `stream_routes_accept_no_commands`, `connected_while_sse_open_without_new_frames`, `new_key_closes_open_streams`, `pco_callback_rejects_non_loopback_and_wrong_state`.
- `stream`: `stream_follows_committed_not_requested_when_room_connected`, `stream_uses_requested_when_no_room`, `stream_ignores_practice_updates`, `stream_never_contains_media_or_video_background`, `stream_renders_only_on_input_change`, `stream_render_rate_capped_at_ten_per_second`, `stream_renders_nothing_while_inactive`, `lower_third_stream_drops_second_translation`.
- `render::compose` golden images: `compose_lower_third_transparent`, `compose_full_view_with_image_background`, `compose_words_only_over_transparent`, `compose_message_over_logo`, `compose_matches_room_layers_for_same_frame` (blending the room prepare step's layer pictures by hand equals `compose`).
- `usage`: `logs_once_per_song_per_session`, `refresh_and_jumps_do_not_log_twice`, `same_service_reused_after_gap_logs_again` (9:40 and 11:20), `reprise_within_session_not_logged`, `session_survives_restart` (pure over the log), `does_not_log_when_words_hidden_everywhere`, `does_not_log_non_song_items`, `streamed_flag_only_when_stream_active_and_showing_words`, `report_groups_by_ccli_and_excludes_no_ccli_by_default`, `csv_quotes_commas_and_quotes`, `mark_reported_moves_default_from_date`, `prune_keeps_three_years`, `usage_write_happens_off_the_command_path`.
- `screens` (`src/ui/dialogs/screens.rs` pure parts): `stream_refuses_room_monitor`, `stream_refuses_stage_monitor`, `stream_default_pick_skips_operator_monitor`, `stream_window_reopens_at_startup_when_enabled`, `saved_stream_monitor_equal_to_room_is_ignored`.
- `pco::map`: `matches_song_by_ccli_then_title`, `new_song_takes_lyrics`, `existing_song_gets_item_arrangement_not_library_change`, `sequence_labels_matched_by_from_label`, `unmatched_sequence_gives_none`, `song_without_lyrics_warns`, `bible_reference_item_becomes_scripture`, `book_name_without_chapter_is_not_scripture`, `headers_and_media_skipped`.
- `pco::auth`: `pkce_challenge_matches_rfc7636_example`, `callback_with_wrong_state_is_rejected`, `secrets_never_serialized_into_data_json`.
- `app`: `import_pco_creates_service_and_songs_in_one_save`, `update_import_keeps_service_id_and_hand_added_items`, `hand_added_item_keeps_its_place_after_update`, `import_while_live_keeps_current_service`, `settings_save_keeps_stream_and_views_changed_meanwhile`, `screen_view_delete_refused_while_on_screen`.

**UI tests** (Slint testing backend, `tests/ui_operator_flows.rs` style, `Ops::inline`): `screens_dialog_picks_room_view_and_chip_appears`, `chip_change_menu_sets_everything_and_chip_hides`, `editing_active_view_asks_before_updating_screen`, `text_style_hidden_under_more_options`, `livestream_card_copy_address_and_new_address_confirm`, `livestream_card_refuses_room_monitor`, `usage_dialog_defaults_since_last_report`, `usage_remove_entry_asks_then_removes`, `settings_edits_survive_opening_usage_report` (type a CCLI number, open the report, choose Save and open: the report header shows it), `reimport_primary_button_is_new_service`, `pco_import_preview_checkboxes_follow_defaults` (fake `PcoApi`).

**Manual checks** (per platform): OBS browser source on the same computer and over Wi-Fi shows transparent lower thirds; "Connected" stays during a 30-minute single slide; ATEM Mini via HDMI with luma key on black; unplug the stream monitor; room to No words during a video (sound continues); a FEAT-2 message shows in the room and not on the stream; a real Sunday with two services logs each song twice; CSV opens in Excel/LibreOffice; Planning Center connect, import, update, disconnect; a room change on the oldest laptop while OBS is connected and a timer ticks feels as quick as without OBS; PNG render time under 60 ms per 1080p frame.

## 10. Dependencies on other FEAT specs

**FEAT-1 contracts FEAT-7 relies on** (FEAT-1 to confirm or rename): `model::Theme` (`Eq + Hash`, `position` incl. `"lower"`, `band`); `theme::SavedTheme { id: Uuid, name, style, background }` in `Data.themes`; `theme::resolve` (item > service > church default) and `LiveItem.theme` / `SlideLayer::Text.theme`; `live::Frame.overlays: Vec<OverlayOn>` with `OverlayContent` (`Text`, `Image`, `Clock`, FEAT-2 `Timer`, `Message`); `render::render_overlay` and `overlay::bound_text`; room `Kind::Overlays` drawn above the logo. **New asks:** (1) seeded built-in themes with fixed ids and a **Reset** button (`theme::seeded(id) -> Option<SavedTheme>`), used for `STREAM_LOWER_THIRD_THEME`; (2) theme delete calls `screen_view::theme_in_use(data, id) -> Option<String>` and refuses with that reason; (3) theme Save returns the ids of screen views using it so the operator can be asked "Update the screen now?"; (4) background/still fit exposed as `render::background_picture` for compose.

- **FEAT-2:** timers and messages are FEAT-1 overlays; per screen they follow `OverlayMask.timers` / `.messages` (defaults: room yes; livestream messages no, timers no — matches FEAT-2 §10). Add a macro step `MacroStep::ScreenView { screen: OutputRole, view: Uuid }` → `SetScreenView`; unknown ids follow FEAT-2's "kept and reported" rule. Phone access follows FEAT-2's macro allowlist. A stream view change never bumps `version`, so AutoNext is not disturbed.
- **FEAT-3:** Stage is not a screen-view target. **Extra stage screens are not in FEAT-7**; FEAT-3 decision #16/#20 and its §2 "Out" line should point to a later FEAT-3 follow-up instead.
- **FEAT-4:** Planning Center order goes into `ItemKind::Song.arrangement` using `SectionKind::from_label`. FEAT-4's line "counts a song once per service" should read "once per usage session (FEAT-7 §5.4)".
- **FEAT-5:** the import uses the abstract translation provider and reference parser; the stream draws only the primary translation of a parallel slide when its text style is a lower third.
- **FEAT-6:** practice mode is a separate engine; the stream and the usage log follow only the real engine (`stream_ignores_practice_updates`). Printable pages share `src/report_page.rs` (built by whichever lands first).
- **Plan 3:** dialogs, `Ops`, Screens/Services/Settings, the top bar and the live column are extended, not replaced.

**Naming.** The roadmap's "Looks" (which layers each output shows) is called **screen view** in the app, because FEAT-1 and Plan 3's Settings "Look" tab already use "Look" for text styles. References to rename to "screen views": FEAT-1 header line 4, §2 Out, §10 FEAT-7 line; FEAT-2 §10 FEAT-7 line; FEAT-3 §2 Out, §10 FEAT-7 line and decision 20; FEAT-5 §2 Out; `PROJECT_PLAN.md` §10b FEAT-7 wording.

## 11. Decisions for Jake to review

1. **User-facing name "screen view"** for what each output shows. Alternative: "Look" (ProPresenter's word), which clashes with FEAT-1's themes and the Settings "Look" tab.
2. **Livestream defaults to Lower third; the room is always Everything at startup** (the room's view is not remembered). Alternative: remember the room's view and show a blocking "The room screen is set to hide words. Keep it?" at startup.
3. **Four built-in views** whose layer choices are fixed; the Lower third's text style is an editable FEAT-1 theme with Reset. Alternative: fully editable built-in views.
4. **Timers and messages never go to the livestream in built-in stream views** (Words only, Lower third); logo bug and clock do. Alternative: show timers on the stream too.
5. **Changing a screen's view takes effect immediately**, from the Screens dialog or the live-column chip; not on the phone except through a phone-enabled FEAT-2 macro. Alternative: allow it on the phone.
6. **Hidden media keeps playing** (sound and timing continue). Alternative: hiding media in the room also pauses it.
7. **Editing a view or its theme while it is on a screen asks before updating the screen.** Alternative: always update immediately.
8. **Videos are never sent to the livestream output.** Alternative: a muted second player in the stream window (heavier, sync risk).
9. **Deck slides hidden in the Lower third view.** Alternative: show them full screen on the stream.
10. **Text style override hidden under "More options"** in custom views (default "Same as the song or item"). Alternative: always visible.
11. **Browser source (OBS) is the default stream connection**, reachable on the church network with a secret address. Alternative: this computer only (127.0.0.1), which breaks the "second streaming PC" setup.
12. **Stream window key colour defaults to black** (luma key). Alternative: green.
13. **Stream follows the real room picture, including during practice mode** (practice never reaches the stream). Alternative: blank the stream during practice.
14. **"Connected" means an OBS page is open**; "streamed" in the report means the same, not "on air". Alternative: ask OBS (obs-websocket) whether it is live — more setup for volunteers.
15. **Song use logging on by default.** Alternative: off until the church enters a CCLI number.
16. **A song counts once per usage session**: a run of that service's song uses with no gap over 45 minutes. A reused service at 9:00 and 11:00 counts twice; a reprise after a sermon over 45 minutes may count twice (removable in Details). Alternative: once per service per calendar day (under-counts two services from one Service).
17. **Songs count when words are visible on the room or an active stream**, not when only selected. Alternative: count every Take Live.
18. **Report excludes songs without a CCLI number by default.** Alternative: include all.
19. **Report default range: since last report, else last 3 months.** Alternative: last 6 months.
20. **Log kept 3 years**, then pruned. Alternative: keep forever.
21. **Export as CSV plus a printable page** (no built-in PDF writer). Alternative: native PDF export.
22. **Song use report opens from the top bar's More menu** (Settings has a link that saves first). Alternative: only inside Settings.
23. **Planning Center is a later phase behind a build feature**, on in official builds once done; OAuth only if Planning Center allows PKCE without a secret, otherwise personal access tokens only; no secret is ever shipped. Alternative: personal tokens only from the start (no developer-app registration).
24. **Planning Center sign-in stored in the OS keychain**, never in data or backups (a restored backup needs reconnecting). Alternative: include it in backups.
25. **Import defaults:** songs and Bible references checked; other items unchecked; headers and media skipped. Alternative: import every item as an announcement slide.
26. **Planning Center's song order goes on the imported service item; library songs are never changed**, and new songs keep the written order in the library. Alternative: also set a new song's usual order from Planning Center.
27. **Re-import: "Import as a new service" is the primary button**; "Update that service…" replaces only imported items, keeps hand-added ones and lists what changes. Alternative: always a new service, no update.
28. **After import, the new service opens only when nothing is live.** Alternative: always open it.
29. **NDI is not built now**; only the seam exists, loading the runtime the user installs. Alternative: build NDI in FEAT-7.

## Review notes

- *Persist room assignment only after `Committed(v)`* (P2): superseded. The room view is never persisted (decision 2), so a failed reveal cannot leave a saved value; `room_view_resets_to_everything_on_startup` covers it instead of `failed_reveal_does_not_persist_assignment`.
- *Render-time lookup of a deleted theme falls back to the item theme* (P2): `ActiveView` keeps a copy of the style (like `LiveItem` snapshots), so a deletion cannot change output mid-service; the fallback to the item's theme happens when the view is next activated (`activate_with_deleted_theme_uses_item_theme`, `active_view_with_deleted_theme_keeps_its_copy`).
- *Refuse delete of a view a macro uses* (P3): chose FEAT-2's existing "kept and reported" rule with a warning in the delete confirm, so the two specs behave the same.
