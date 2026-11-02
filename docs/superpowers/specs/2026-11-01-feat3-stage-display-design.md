# FEAT-3 — Stage Display: Layouts, Timers, Notes, Messages — Design Spec

**Date:** 2026-11-01 (rev 3, cross-spec alignment) · **Author:** Claude (Jake asleep; product defaults chosen, see §11) · **Status:** design, not built
**Roadmap:** `PROJECT_PLAN.md` §10b FEAT-3 (depends on FEAT-2). Requirement R10 long-term column ("Layouts, timers").
**Builds on:** v1 spec §5.4 (command ids), §6 (layers, clear matrix), §7.2 ("next"), §12.2 (foldback), §12.3 (phone remote); native spec §3, §4 (compositor), §6 (stage screen), §7 (monitors), §8.3 (crash state); Plan 3 (`docs/superpowers/plans/2026-11-01-native-operator.md`: `Theme`, `Ops`, dialog overlays, `ask` prompts, Screens dialog, Live column, `keys.rs`); FEAT-2 spec (`TimersView`, `TimerSnapshot`, `TimerState`, `timers::text`, macros, `crate::lenient::Lenient`, phone allowlist §5.11); FEAT-1 §7 (Settings tab table, key table, layout budget §7.2); FEAT-4 (`Slide::new`, `SectionRef`, live-item reconcile); FEAT-5 (`VerseId`, `SlideSource`); FEAT-6 (`App::display_view()`, `Update::Practice`).
**Today's stage** (`src/ui/stage.rs` `stage_text`/`fit_size`, `ui/stage.slint`; the browser page `web/stage.html` at the local-only `/stage` route does the same): one fixed screen — item title, media time left, clock, the current slide's text (auto-fit 9 % → 3 % of height in 0.4 % steps; **empty and dimmed after Clear Slide**; "(slide picture)", "(video)" etc. for non-text slides), the current slide's `Slide.notes` (which `slides::build_item` fills from the item's notes when the slide has none, `src/slides.rs:234`), and "NEXT" text. No layouts, no timers, no stage message.
**User-visible name:** always **"Stage layout"** (never "theme" or "screen view" for the stage; "Look" is not used anywhere). The string lives in `src/strings/stage.rs`.

## 1. Purpose and who it's for

The stage (foldback) screen is the monitor the people at the front look at: singers and the band want the words now and next and which section is coming; the preacher wants a clock, a countdown to the end of the sermon, their notes and the slide the room sees; everyone wants to be told "wrap up" or "last chorus" quietly from the back. Volunteers at a small church set this up once and rarely touch it, so:

- **It works with zero setup.** Old data opens on "Classic", which reproduces today's stage (§3.1 lists the only additions).
- **Picking beats building.** Five ready-made stage layouts cover most churches. One click switches the stage now; a service item can switch it by itself (the sermon item shows "Speaker").
- **Messages are one line.** Type, Enter, it's on stage; Hide removes it. From the laptop or a paired phone.
- **Building is possible without training** (Phase C): an editor with drag-and-resize boxes on a picture of the screen.

Who: the operator (laptop), anyone with a paired phone (stage messages only), and the people on the platform (read only).

## 2. Scope and phases

Three phases, each shippable and tested on its own (§9 tags tests by phase).

**Phase A — presets, picker, messages, timers (the simple wins):**
1. Stage drawn with the Rust compositor per box (same fonts and fitting everywhere, cheap on old laptops).
2. Five built-in stage layouts (read-only) with these box kinds: current slide, next slide, section name, item title, next item title, service position, notes, clock, timer (FEAT-2 timer by id or "current timer"), media time left, stage message, fixed text.
3. Stage strip in the Live column: layout picker ("for now" switch, plus "Set as the stage default"), message box, quick messages, Show/Hide, "On stage" chip with elapsed time.
4. Stage messages from the phone remote ("Stage" tab).
5. Stage actions as FEAT-2 macro steps (four step kinds, §5.5).

**Phase B — per-item layout and slide notes:**
6. Per-service-item stage layout.
7. Notes per slide, stored on the item, merged into `Slide.notes` by `build_item`.

**Phase C — the editor and the tablet view:**
8. The church's own stage layouts (copies of presets or new) and the layout editor dialog (drag/resize, snap, nudge, undo).
9. Read-only tablet stage view on a view-only pairing.

**Out (not in FEAT-3):** more than one native stage window (a later FEAT-3 follow-up; not FEAT-7); chord charts, music-stand PDFs, click/multitrack; room overlays (FEAT-1/FEAT-2); timer creation and logic (FEAT-2); custom layouts on the tablet view (one fixed layout); stage as a second native app over the network; live video picture on stage (video shows a label and time left). The local `/stage` browser page stays as it is (it belongs to the web reference app and is retired with it; it is not extended).

## 3. User experience

### 3.1 First run after the update

The stage shows "Classic": the same boxes, sizes and fitting as today, the same empty-and-dimmed current box after Clear Slide, the same "(slide picture)" label for picture slides. The only visible additions: a short section label line ("Chorus", "Verse 2") in the section's colour beside the item title, for songs; a stage message, when one is sent, appears in the notes area; a FEAT-2 timer marked "Show on stage" takes the media-time spot when no media plays (FEAT-3 picks the most recently started one, §5.3; FEAT-2's interim line did this until FEAT-3 lands). The Live column gets a **Stage** strip (§7.2). Help lists the new shortcuts.

### 3.2 Switch the stage layout during a service

1. In the Stage strip, click the layout button ("Stage: Classic ▾").
2. A menu lists every stage layout (built-ins first, then the church's), each with a tiny preview; the one on stage has a check. Below a separator: "Set “Classic” as the stage default…", "Back to automatic" (only while a for-now choice is active), "Edit stage layouts…" (Phase C).
3. Click "Speaker". The stage switches immediately (no fade); the room does not change. The button now reads "Stage: Speaker (for now)". Toast: "Stage: Speaker until the next item."
4. When a different item goes live, the for-now choice ends and the stage follows the item's layout or the default again.

Resolution order (§5.1): **for now > the live item's layout > the stage default > Classic**. Picking in the strip never changes the church-wide default; "Set … as the stage default…" asks "Use “Speaker” as the stage layout for every service?" and also clears the for-now choice. Screens → stage card has the same default picker.

### 3.3 Send a stage message

1. Press **Ctrl+J** (Cmd+J on macOS) or click the message box in the Stage strip.
2. Type "Last chorus" and press **Enter** (or click **Show**). The stage shows it in its Message box, flashing its border three times, then steady. The strip shows a yellow chip "On stage · just now", which counts up ("On stage · 12 min").
3. To remove it: click **Hide**, or press **Ctrl+Shift+J** (Cmd+Shift+J) anywhere in the operator window while a message is up, or send a new message (replaces it). Esc only leaves the text field, as everywhere in Plan 3.
4. The **▾** beside the box lists quick messages (default: "Last chorus", "Repeat the chorus", "Go to the bridge", "Please wrap up", "5 minutes left", "Check your microphone"). Clicking one shows it at once. "Edit quick messages…" opens a small list editor (add, remove, drag to reorder, max 12).
5. Typing stops at 120 characters, with the counter turning amber.
6. A message is hidden automatically when the operator opens a different service, and after the time set in Settings → Stage "Hide stage messages after" (Off / 1 / 5 / 10 minutes; default Off).

If the stage window is off, Show still works and the toast says "The stage display is off. The message will show when it's turned on." Messages never reach the room. F1–F6 and Ctrl/Cmd+Shift+J keep working while the box has focus (the capture set in FEAT-1 §7's key table); arrows and Space go to the text field.

### 3.4 Make the sermon item switch the stage (Phase B)

1. Open the item (Item edit dialog, Plan 3 Task 18; its **Stage** section, FEAT-1 §7.2). New field **Stage layout**: "Use the stage default" (default) or any layout.
2. Pick "Speaker", Save. While that item is live the stage shows "Speaker" (unless a for-now choice is active); when another item goes live, it goes back.
A small stage icon marks the item's row in the service column (tooltip "Stage layout: Speaker").

### 3.5 Notes (Phase B)

- **Item notes** stay where they are (Item edit → "Stage notes"); as today they fill every slide that has no note of its own.
- **Slide notes (new):** right-click a slide thumbnail (or select it and press **Alt+N**; bare N stays free) → "Stage note…" → Plan 3 prompt "Note for this slide (stage only)". Thumbnails with a note show a small mark. Clearing the text removes the note.
- The Notes box shows the slide's note if it has one, otherwise the item's note (one value: `Slide.notes`).
- Editing a note of the live item updates the stage at once (notes never reach the room; §5.7).
- Notes stay on "their" slide when the song is re-split (Settings → lines per slide), the arrangement is reordered or the Bible passage is shown in another translation (§4.4). A note that has nowhere to go is listed in Item edit under "Notes for slides that changed" with Delete buttons.

### 3.6 Edit or create a stage layout (Phase C)

1. **Screens** → stage card → **Stage layouts…** (or the strip menu "Edit stage layouts…").
2. Left: the list. Built-ins have a lock and say "Built-in — make a copy to change it". Buttons: **New**, **Copy**, **Rename**, **Delete**, **Use on stage now**.
3. Middle: a picture of the stage screen (the stage monitor's aspect, or 16:9), showing what is live now or sample words. Every box shows its name in a corner tab.
4. Click a box to select it (accent outline, eight handles). Drag to move, drag a handle to resize. Boxes snap to a 2.5 % grid and to edges and centre lines of other boxes (thin accent guides). Hold Alt to move freely.
5. Right: the selected box's settings: what it shows, text colour, biggest text size, alignment, caption on/off and its words, background, and per-kind options (which timer, 24-hour clock, seconds, show pictures, keep the words after Clear).
6. **Add ▾** lists every kind with a one-line description ("Clock — the time of day"). A new box appears centred at 30 % × 15 %, selected.
7. Keys: arrows move one grid step, Shift+arrows resize, Delete removes, Ctrl/Cmd+Z / Ctrl/Cmd+Y undo/redo (50 steps), Ctrl/Cmd+D duplicates, Tab selects the next box. Right-click: Bring to front, Send to back, Duplicate, Delete.
8. A box made by a newer RustyPresenter shows as a grey hatched box "Needs a newer RustyPresenter"; it can be deleted but not moved or changed, and its content is saved back unchanged (same meaning; formatting may differ, §4.1).
9. Deleting a layout that something uses asks, counting every use (§5.6): "Delete “Band”? It is used by 3 service items, the stage default and 1 macro. Items and the default will use Classic; the macro won't run until it is edited."
10. Footer: **Cancel** (asks "Discard your changes to the stage layouts?" only if something changed) and **Save**. Save sends every change as one all-or-nothing action (§5.5); on an error nothing is saved and the dialog stays open showing the error. Saving a layout that is on stage updates the stage at once.

### 3.7 Phone remote

A tab "Stage" on `/remote` beside the Control tab (whose order FEAT-2 owns): message box, quick messages as buttons, **Show**, **Hide**, the "On stage: … (from Jake's phone)" line, and the stage's current timer (§5.3). Phones cannot switch layouts or edit notes or layouts.

**Tablet stage view (Phase C).** In the operator, Screens → stage card → **Pair a tablet as a stage screen…** shows a pairing code like phone pairing, but the session it creates is view-only (§5.8). The tablet opens `/remote/stage-view`: full-screen, read-only, no controls and no way to reach the remote controls. It counts against the 5 paired devices.

### 3.8 Effect on stage and room

Stage only. Nothing in FEAT-3 changes the room output, the output version, command ids or anything the clear matrix covers. Clears change only what the current-slide box shows (§5.2); they never clear timers, notes, the clock or the stage message.

## 4. Data model

New core module `src/stage/` (no UI): `mod.rs` (types, `StageState`), `presets.rs`, `view.rs`, `notes.rs`, `geom.rs`, `refs.rs`. User-facing text lives in `src/strings/stage.rs` (§7.4).

### 4.1 Forward-compatible storage (`crate::lenient::Lenient`)

`App::edit` autosaves the whole `Data`, so anything an older build cannot parse must still be written back with the same meaning. Rule: **every new stored enum sits inside a `Lenient<T>`**. FEAT-3 uses the one shared type that FEAT-2 owns, unchanged; it does not define its own:

```rust
use crate::lenient::Lenient;   // src/lenient.rs: pub enum Lenient<T> { Known(T), Unknown(serde_json::Value) }
```

`Lenient` round-trips **meaning, not bytes**: an `Unknown` value is written back as the same JSON value (key order and whitespace may differ). The `raw_value` feature is not used. Lenient containers: each `StageElement` in a layout, each `SlideNote`. Inside them, `ElementKind`, `TimerRef`, `Align`, `VAlign`, `NoteKey` need no `#[serde(other)]`: any unknown variant, tag or field type anywhere in the element makes the whole element `Unknown`. Stored settings enums outside elements (none in FEAT-3 besides those) would get `#[serde(other)]` + a default. Runtime-only `MsgSource` (persisted only in the crash state file) has `#[serde(other)] Unknown`. An `Unknown` element draws nothing; an `Unknown` note is kept, never shown, and not counted as an orphan.

### 4.2 Stage layouts (Phase A types; church layouts stored from Phase C)

```rust
/// Built-ins are made in code (`presets::builtins()`) and never stored.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageLayout {
    pub id: Uuid,
    pub name: String,                                  // 1..=60 chars after trim
    #[serde(default = "default_stage_bg")] pub background: String,   // "#07080a"
    #[serde(default)] pub font: String,                // "" = bundled Noto Sans
    #[serde(default)] pub elements: Vec<Lenient<StageElement>>,      // drawn in order, later on top; max 24
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageElement { pub id: Uuid, pub kind: ElementKind, pub rect: PctRect, #[serde(default)] pub style: ElementStyle }

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct PctRect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }   // % of the window, 0..=100

/// None = the kind's default from `presets::kind_defaults(kind)`, resolved in `view::build`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct ElementStyle {
    #[serde(skip_serializing_if = "Option::is_none")] pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub max_size: Option<f32>,  // biggest text, % of window height
    #[serde(skip_serializing_if = "Option::is_none")] pub min_size: Option<f32>,  // floor; kind default 3.0 (today's)
    #[serde(skip_serializing_if = "Option::is_none")] pub align: Option<Align>,   // Left | Center | Right
    #[serde(skip_serializing_if = "Option::is_none")] pub valign: Option<VAlign>, // Top | Middle | Bottom
    #[serde(skip_serializing_if = "Option::is_none")] pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub caption: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub caption_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub fill: Option<String>,    // None = transparent
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ElementKind {
    CurrentSlide { #[serde(default)] pictures: bool, #[serde(default)] keep_after_clear: bool },
    NextSlide { #[serde(default)] pictures: bool },
    SectionName, ItemTitle, NextItemTitle,
    ServicePosition,                       // "Item 3 of 8"
    Notes,
    Clock { #[serde(default)] seconds: bool, #[serde(default)] h24: bool },
    Timer { timer: TimerRef },
    MediaTimeLeft, StageMessage,
    Text { text: String },                 // max 200 chars
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum TimerRef { Id { id: Uuid }, Current }
```

`presets::kind_defaults(kind) -> ResolvedStyle` gives every `None` its value: colour `#ffffff`, align Center, valign Middle, bold true, min 3, caption off; max size 9 (CurrentSlide), 6 (NextSlide), 4 (titles, section, position, notes, media), 8 (Clock), 10 (Timer), 6 (Message), 5 (Text); caption on for NextSlide ("NEXT"), NextItemTitle ("NEXT ITEM"), Timer (the timer's name).

### 4.3 Built-in presets (`presets.rs`, Phase A)

Fixed ids (`Uuid::from_u128` constants). All: background `#07080a`, bundled font. Rects in %.

| Name | Boxes (x, y, w, h) |
|---|---|
| **Classic** (default) | ItemTitle (3,2,44,6) `#9aa3b5` left · SectionName (48,2,15,6) section colour left · MediaTimeLeft (64,2,13,6) `#ff6b6b` right · Timer Current (64,2,13,6) `#ff6b6b` right · Clock (78,1,19,8) right · CurrentSlide (3,10,94,52) max 9 min 3, pictures off, keep off · Notes (3,63,94,11) `#cfe3ff` fill `#12151b` left · StageMessage (3,63,94,11) · NextSlide (3,76,94,22) `#f5c66b` caption "NEXT" left top |
| **Lyrics only** | CurrentSlide (3,3,94,62) max 12 · NextSlide (3,68,94,29) `#f5c66b` · StageMessage (3,3,94,14) |
| **Band** | SectionName (3,2,40,8) section colour · Clock (70,2,27,8) right · CurrentSlide (3,11,94,48) · NextSlide (3,61,62,24) · MediaTimeLeft (68,61,29,12) · Timer Current (68,74,29,11) · StageMessage (3,87,94,11) |
| **Speaker** | Clock (3,2,30,14) left · Timer Current (60,2,37,20) right max 18 · CurrentSlide pictures on (3,24,45,40) · Notes (51,24,46,56) left top max 6 · NextItemTitle (3,66,45,14) · StageMessage (3,82,94,16) |
| **Clock and countdown** | Clock (5,8,90,40) max 30 · Timer Current (5,52,90,30) max 22 · StageMessage (5,84,90,13) |

Classic reproduces today, plus the section label line (the item title box is narrower to make room; the slide, notes and next regions are unchanged): shared rects draw later boxes on top only when they have content (an empty Message or an `Empty` Timer draws nothing), and MediaTimeLeft is listed before Timer, so while media plays the view builder blanks the Timer cell in that rect (`view::shared_rect_priority`: the first non-empty box in a shared rect wins; media keeps priority). Which timer Classic shows follows FEAT-3's latched "most recently started on_stage timer" rule (§5.3), not FEAT-2's interim "first on_stage not Stopped" line, which applies only until FEAT-3 lands. "Section colour" means `sections::color_for_label(label)` (FEAT-4), falling back to `#9aa3b5` for labels with no colour.

### 4.4 Settings, items and notes

```rust
// model.rs (all serde defaults, so old data.json loads)
pub struct Settings { …, #[serde(default)] pub stage: StageSettings }
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct StageSettings {
    pub layout: Uuid,                 // the stage default; default presets::CLASSIC
    pub quick_messages: Vec<String>,  // default: the six in §3.3; max 12, each ≤ 120 chars
    pub flash: bool,                  // default true
    pub auto_hide_mins: u8,           // 0 = off (default); allowed 0, 1, 5, 10
}
pub struct Data { …, #[serde(default)] pub stage_layouts: Vec<StageLayout> }   // Phase C; the church's own only
pub struct ServiceItem { …,                                                     // Phase B
    #[serde(default, skip_serializing_if = "Option::is_none")] pub stage_layout: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]   pub slide_notes: Vec<Lenient<SlideNote>>,
}
// Slide is unchanged by FEAT-3. The one listing (built with Slide::new):
// Slide { label, content, notes, section: Option<SectionRef> /* FEAT-4 */, source: Option<SlideSource> /* FEAT-5 */ }
// FEAT-3 adds no field to it; note keys are derived (below).

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SlideNote { pub key: NoteKey, pub text: String }        // text ≤ 1000 chars

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(tag = "on", rename_all = "snake_case")]
pub enum NoteKey {
    /// Songs: section label (SectionRef.label, lowercased); nth time that label is played (SectionRef.ordinal + 1);
    /// page within that occurrence (SectionRef.page + 1); pages in that occurrence when saved (SectionRef.pages).
    Song { section: String, occurrence: u16, page: u16, pages: u16 },
    /// Scripture: FEAT-5's translation-independent VerseId (0-based book index, chapter, verse).
    Verse(VerseId),
    /// Everything else (decks, media, custom text): the slide index.
    Index { slide: u16 },
}
```

Keys are **derived, never stored on the slide**: `notes::key_of(&Slide) -> Option<NoteKey>` returns `Song` from `slide.section` (FEAT-4's `SectionRef`), `Verse(source.first)` from `slide.source` (FEAT-5's `SlideSource`; the slide's range is `source.first..=source.last`), and `None` otherwise; `notes::key_at(slides, i)` = `key_of(&slides[i])` or `Index { slide: i }`. Two-translation slides key on the primary translation's verses (FEAT-5 sets `source` from it).

**One source for notes.** `slides::build_item` resolves each slide's note with `notes::resolve(item, slides)` and writes it into `Slide.notes`; the slide note wins over the item note (today's line 234 then fills only slides still empty). Stage, phone and FEAT-6 print all read `Slide.notes`.

**Matching (`notes::resolve`), per note:**
1. `Song`: the slide with the same section and occurrence. If that occurrence now has the same `pages`, the slide with the same page. If the page count changed (lines-per-slide change or a lyric edit), the occurrence's **first page**. Several notes landing on one slide are joined with a line break in page order.
2. `Verse`: the slide whose `source` range contains the `VerseId` (works across translations and re-broken passages; for a verse split across slides, the slide with `part` 0).
3. `Index`: that index, if it exists.
4. Otherwise the note is an **orphan**: kept, listed in Item edit "Notes for slides that changed" with Delete buttons.
Arrangement reorders keep occurrence numbers per section (moving the bridge does not renumber choruses); removing the 2nd chorus orphans its note.

`pub fn resolve(item: &ServiceItem, slides: &mut [Slide])`, `pub fn orphans(item, slides) -> Vec<&SlideNote>`, `pub fn key_of(slide: &Slide) -> Option<NoteKey>`, `pub fn key_at(slides, index) -> NoteKey` (used by the note prompt).

**Saving these fields.** `Settings.stage`, `ServiceItem.stage_layout` and `ServiceItem.slide_notes` rely on FEAT-1's merge rule for `settings/save` and `service/update`: a missing key keeps the stored value, an explicit `null` clears an optional field. The Settings dialog sends only the fields its tabs edit, so stage settings changed from the strip or Screens are never overwritten by an unrelated Settings save, and Item edit's Save never drops slide notes it did not send.

### 4.5 Runtime state (not in data.json)

```rust
pub struct StageState {
    pub message: Option<StageMessage>,
    pub manual: Option<ManualLayout>,          // the "for now" choice
    pub current_timer: Option<Uuid>,           // latched "current timer" (§5.3)
    pub seen: VecDeque<(Uuid, StageOutcome)>,  // phone duplicate memory, 256 ids
}
pub struct ManualLayout { pub layout: Uuid, pub item_id: Option<Uuid> /* live item of the displayed engine when chosen */ }
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageMessage { pub seq: u64, pub text: String, pub from: MsgSource, pub shown_at: chrono::DateTime<chrono::Utc> }
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum MsgSource { Operator, Phone(String), Macro, #[serde(other)] Unknown }
```

`App.stage: Mutex<StageState>`. `seq` increments on every Show (a re-sent identical text still flashes). `shown_at` serializes as RFC 3339 (chrono is the only time crate). The per-phone rate limit is FEAT-2's shared `remote::RateLimit` (§5.8), not a field here.

**Migration:** none. The data format number is unchanged by FEAT-3 (all new fields are serde defaults; whatever `store.rs` `SUPPORTED_FORMAT` is when FEAT-3 lands). No `Data::after_load` fix-up is needed. Old `data.json` → Classic, default quick messages, no overrides, no notes. A newer file keeps unknown boxes and notes as `Lenient::Unknown` and writes them back with the same meaning.

## 5. Core behaviour

### 5.1 Which layout is on stage

`stage::active_layout(data, state, live_item_id) -> &StageLayout` over built-ins + `stage_layouts`:
1. `state.manual`, if its layout exists.
2. The live item's `stage_layout`, if set and the layout exists. The item is found by id in **every** service (current service first), so switching `current_service` mid-sermon does not drop it.
3. `settings.stage.layout`, if it exists. 4. Classic.

"Live item" = the cursor's item (kept after Clear Slide/Logo, v1 §7.2) of the **displayed engine**: the stage reads FEAT-6's `App::display_view()`, which is the practice engine while practice mode is on and the live engine otherwise. `manual` is cleared when that engine's cursor item id differs from `manual.item_id` (checked on every `Update::Live` and `Update::Practice`; `item_id: None` = chosen with nothing live, cleared when anything goes live), on "Back to automatic", and on "Set as the stage default".

### 5.2 Stage view (pure, `view.rs`)

```rust
pub struct StageInputs<'a> { live: &'a LiveView, data: &'a Data, state: &'a StageState,
                              timers: &'a TimersView, now: chrono::NaiveTime, service_pos: Option<(usize, usize)> }
// `live` is the displayed engine's view from FEAT-6's App::display_view() (§5.1).
pub struct StageView { pub layout_id: Uuid, pub background: String, pub font: String,
                       pub cells: Vec<Cell>, pub message_fallback: Option<StageMessage> }
pub struct Cell { pub element: Uuid, pub rect: PctRect, pub style: ResolvedStyle, pub content: CellContent }
pub enum CellContent { Empty, Text { text: String, dim: bool, caption: Option<String>, red: bool },
                       Picture { asset: AssetId, caption: Option<String> }, Message { text: String, seq: u64 } }
pub fn build(inp: &StageInputs, layout: &StageLayout) -> StageView
```

| Kind | Content (strings from `src/strings/stage.rs`) |
|---|---|
| CurrentSlide | On-air text slide → its text. Image/Still with `pictures` → `Picture`; without → "(slide picture)" / "(picture)"; video "(video)", audio "(audio)"; logo → "(logo)". **Clear Slide** (frame has no slide but still has a background or media): `keep_after_clear` off → empty, `dim: true` (today); on → the cursor slide's text, `dim: true`. **Clear All** (empty frame) or end of service → always empty, dimmed. |
| NextSlide | `LiveView.next` (v1 §7.2). Crossing into another item, caption "NEXT · <item title>". None → "End of service". `pictures` as above. |
| SectionName | Live slide's `section.label` (FEAT-4 `SectionRef`; no " (n/m)") when the slide has one; else empty. Default colour (style colour `None`) = `sections::color_for_label(label)`, else `#9aa3b5` |
| ItemTitle / NextItemTitle | Live item title / next item in service order ("End of service"); nothing live → "Nothing live" / first item's title |
| ServicePosition | "Item 3 of 8"; nothing live → empty |
| Notes | The snapshot slide's `notes` (kept current by FEAT-4's notes-only adoption, §5.7); empty → `Empty` |
| Clock | "9:05 AM" / "9:05:30 AM" / "09:05" / "09:05:30" |
| Timer | §5.3. Id not found → "Timer removed", dimmed |
| MediaTimeLeft | Today's rule: video, else audio, duration > 0 → "−m:ss" (never below 0); else `Empty` |
| StageMessage | `Message` when a message is up, else `Empty` |
| Text | Its fixed text |
| `Unknown` element | not drawn |

Shared rects: §4.3 `shared_rect_priority`. `message_fallback` is set when a message is up and the layout has no StageMessage box: the window shows it as a banner over the top 16 %, so a message is never lost because of a layout choice.

### 5.3 Timers (contract with FEAT-2)

FEAT-3 reads, never writes, timers. It uses FEAT-2's types unchanged:

```rust
pub struct TimersView { pub timers: Vec<TimerSnapshot>, pub auto: /* FEAT-2 */ … }
pub struct TimerSnapshot { pub id, pub name, pub kind, pub state: TimerState, pub value_ms, pub overrun: bool, pub on_stage: bool, pub on_remote: bool }
pub enum TimerState { Stopped, Running, Paused, Finished }
```

`App::timers() -> Arc<TimersView>` returns the same `Arc` that `Update::Timers` carries. FEAT-2 publishes `Update::Timers` when any displayed second, state or auto readout changes, at most every 250 ms and at least once a second while a timer runs (FEAT-2 §5.8). Text always comes from `timers::text(value_ms)`; FEAT-3's view adds only colours and dimming.

**Current timer** (`TimerRef::Current`), tracked in core by `stage::track_timers(&mut StageState, &TimersView)`, called by `App` where FEAT-2 publishes `Update::Timers`. This latched rule is the one the stage uses once FEAT-3 lands; it replaces FEAT-2's interim "first on_stage timer not Stopped" line.
- Candidates: timers with `on_stage` true.
- When a candidate goes from Stopped/Finished to Running, it becomes `current_timer` (most recently started wins).
- It stays latched while its state is Running (including overrun), Paused or Finished, and is released only when it returns to Stopped (reset) or another candidate starts.
- With nothing latched (e.g. after restart), the first candidate in FEAT-2 order whose state is not Stopped; else `Empty`.

**Display** (`view::timer_cell`): text = `timers::text(snapshot.value_ms)` (FEAT-2's formatting, including the overrun sign). `overrun` → `red: true` (`#ff4f55` regardless of box colour). Finished → its text, red. Paused → `dim: true`, caption "<name> · paused". Stopped (only reachable by an Id reference) → its text (the start value). Caption = the timer's name when the caption is on.

### 5.4 Updates and ticking

- New `Update::Stage` (message shown/hidden, layout used, layouts saved, notes changed). `server.rs` ignores it except for the phone stream, which re-sends `remote_view`.
- The stage follower (`follow` in `src/ui/stage.rs`) reads FEAT-6's `App::display_view()` and listens to `Update::Live`, `Update::Practice`, `Update::Data`, `Update::Stage`, `Update::Timers`, keeps the newest of each and rebuilds the `StageView` on the event loop (one queued redraw at a time, as today). It also sets the window's `practice` property (§7.1) while practice mode is on.
- The clock ticks on the window's own 1 s `slint::Timer` and rebuilds only clock cells. Timer arithmetic is never done on the stage.
- Auto-hide: the operator's 1 s strip timer calls `stage/message/hide` with `{if_seq}` when `auto_hide_mins > 0` and the message is older; `if_seq` makes it a no-op if a newer message replaced it.
- Cells compare by `(element, content, rect, style)`; only changed cells re-render (§6).

### 5.5 Actions (`App::do_action`, one command surface)

| Action | Phase | Body → result | Rules / errors |
|---|---|---|---|
| `stage/layout/use` | A | `{id}` → `{}` | Sets `state.manual` (for now). Unknown id → "That stage layout is gone." |
| `stage/layout/auto` | A | `{}` → `{}` | Clears `state.manual`. |
| `stage/layout/default` | A | `{id}` → `{}` | Sets `settings.stage.layout`, clears `manual`. Unknown id → same error. |
| `stage/message/show` | A | `{text}` → `{seq}` | Trimmed; empty → "Type a message first."; >120 → "Messages can be at most 120 characters." Replaces any message. Thin wrapper over `App::stage_message_show(text, MsgSource::Operator)`. |
| `stage/message/hide` | A | `{if_seq?}` → `{}` | Always succeeds; with `if_seq`, only hides that message. |
| `stage/quick/save` | A | `{messages}` → `{}` | Trims, drops empties, max 12, each ≤ 120. |
| `stage/settings` | A | `{flash, auto_hide_mins}` → `{}` | `auto_hide_mins` not in {0,1,5,10} → "Choose Off, 1, 5 or 10 minutes." |
| `stage/note` | B | `{item_id, key, text}` → `{}` | Empty text removes. Unknown item → "That item is gone." >1000 → "Notes can be at most 1000 characters." |
| `service/update` (existing) | B | gains `stage_layout` | Unknown layout id → "That stage layout is gone." |
| `stage/layout/refs` | C | `{id}` → `{refs: [LayoutRef]}` | Read-only; feeds the delete confirm text. |
| `stage/layouts/replace` | C | `{layouts: [StageLayout], deleted: [Uuid]}` → `{cleared}` | The only way to delete a stage layout (there is no `stage/layout/delete`). Validated as a whole, applied in one `App::edit`: (1) clear references to `deleted` (§5.6), (2) remove them, (3) upsert `layouts`. Checks: built-in id in either list → "Built-in stage layouts can't be changed. Make a copy first."; empty name → "Give the stage layout a name."; duplicate name in the **resulting** set (case-insensitive) → "There is already a stage layout called “X”."; >24 boxes → "A stage layout can have at most 24 boxes."; rects sanitised (§5.9). Any error → nothing changes. `Unknown` boxes are kept as sent. |

None of these touch `live.rs`, the output version, command ids or `base`. Data-changing ones go through `App::edit` (autosave) and publish `Update::Data` + `Update::Stage`; runtime ones publish `Update::Stage`. A service switch (`current_service` change) hides the message in the same call.

**FEAT-2 macros:** exactly four stage step kinds, which FEAT-2 owns: `StageMessageShow { text }`, `StageMessageHide`, `StageLayout { layout }` and `StageLayoutAuto`. They map to `App::stage_message_show(text, MsgSource::Macro)` (called directly, not through the public `stage/message/show` action), `stage/message/hide`, `stage/layout/use` and `stage/layout/auto`. FEAT-3 exposes `pub fn stage_message_show(&self, text: &str, from: MsgSource) -> Result<u64, String>` on `App` (same validation as the action; returns `seq`). Missing references follow FEAT-2's rule: every reference is resolved before a macro runs, so a `StageLayout` step whose layout was deleted means **nothing runs**, with the notice "Couldn't run “<macro>”: step N's stage layout was deleted. Edit the macro." A step that fails at run time after resolution does not stop later steps (FEAT-2's toast "Ran “<macro>”, but step N didn't work: …").

### 5.6 Layout references (`refs.rs`)

```rust
pub enum LayoutRef { StageDefault, ServiceItem { service: Uuid, item: Uuid, title: String },
                     MacroStep { macro_id: Uuid, name: String } }
pub fn layout_refs(data: &Data, id: Uuid) -> Vec<LayoutRef>
```

**Contract for other FEATs:** any FEAT that stores a stage layout id adds its variant and arm here when it lands; FEAT-3 does not pre-declare them. FEAT-2 macro steps are `MacroStep`. **Extension note:** FEAT-6 adds `Template { template: Uuid, name: String }` for service templates; template refs are kept and reported, and FEAT-6 gives a new service's item `stage_layout: None` when the id is gone. FEAT-7 screen views store none (§10). Deletes go only through `stage/layouts/replace`. On delete, `StageDefault` and `ServiceItem` refs are cleared (→ Classic / stage default); `MacroStep` refs are kept and reported (the macro refuses to run until edited, §5.5). The confirm text counts every ref by kind.

### 5.7 Interaction with live rules

- **Clear matrix (v1 §6.2):** only `LiveView.frame` changes → CurrentSlide per §5.2; timers, clock, notes, message, section name stay. Logo → "(logo)".
- **Navigation (v1 §7.2):** NextSlide = `LiveView.next` exactly.
- **Live item edited:** CurrentSlide, NextSlide and the slide index come from the Take Live snapshot (as today). Notes come from the snapshot's `Slide.notes`. FEAT-3 has no reconcile of its own: FEAT-4 is the one mechanism for every item kind (run after `App::edit` and on `Update::PlansReady`). When `stage/note` or an item-notes edit touches the live item and only notes differ, FEAT-4 adopts the change into the snapshot with `Change::None` (no output-version bump), matching slides by their derived keys (`notes::key_of`), never by an index into rebuilt slides. So a lyric edit that adds a slide, saved but not re-taken, cannot shift notes; any non-notes difference that would change the room screen raises FEAT-4's change banner instead. The stage just reads the snapshot.
- **Pending transitions:** the stage follows the displayed `LiveView` as today.
- **FEAT-6 practice mode:** room suppressed; the stage follows the practice engine through `App::display_view()` (§5.1, §5.4) and shows a PRACTICE tag (§7.1).

### 5.8 Phone remote and tablet view

- `remote_view` gains `"stage": { "message": Option<{seq,text,from}>, "quick": [String], "notes": String, "clock_h24": bool, "timer": Option<{name,text,over}>, "media_left": String }` (`timer` = current timer).
- New route `POST /remote/stage`: `{ "id": <uuid>, "op": "show", "text": … }` or `{ "id", "op": "hide" }`. Same origin, JSON and session checks as `/remote/command`. Order: (1) session/role check, (2) duplicate check against `StageState.seen` (256 ids; a duplicate returns the original result, nothing re-flashes), (3) rate limit via the shared `remote::RateLimit` (one `show` per second per phone → `"Too fast — try again."`; the same limiter FEAT-2's room ShowMessage uses); `hide` is never rate-limited, (4) `App::stage_message_show(text, MsgSource::Phone(name))` or `stage/message/hide`. Any other `op` → "Not allowed from the phone.". **Allowlist:** this route is FEAT-3's row in FEAT-2 §5.11's single phone allowlist table (separate route `/remote/stage`, show and hide, rate-limited); FEAT-3 adds nothing to `/remote/command`.
- The phone stream resends `remote_view` (including this block) on `Update::Stage`, as for Live, Data and Practice; `Update::Timers` goes to the phone only as `{type:'timers'}` (FEAT-2), and the Stage tab's timer line reads the current timer from it.
- **View-only pairing (Phase C).** `Phone` gains `#[serde(default)] role: PhoneRole { #[default] Remote, StageView }` (old `phones.json` → Remote). A StageView session is accepted only by `/remote/stage-view` and `/remote/events`; `/remote/command`, `/remote/stage` and `/remote` return 403. Its pairing code comes from Screens (§3.7). The page `web/stage-view.html` has one fixed layout (clock and timer top, current slide large with the 0.4 vh step fit of `web/stage.html`, notes, next, message banner) and no links or gestures back to controls.

### 5.9 Edge cases

- Stage window closed: state still changes; the strip says "Stage display is off". Opening it shows the current state.
- Layout deleted while on stage: next rebuild falls back by §5.1; toast "The stage layout was deleted; showing Classic." (or the default).
- Two boxes of the same kind: allowed.
- Text too big for its box: shrinks to `min_size` (default 3 %), then clips at the box edge; the editor marks "Text may not fit".
- Picture asset missing: "(slide picture)".
- Monitor aspect differs from the editor's: percentages scale; text refits.
- Phone message while the operator types: the typed text is kept; the chip shows the phone's message "from <phone name>".
- Lines-per-slide change, translation switch, arrangement reorder: notes follow §4.4.
- Service switched while an item from another service is live: the item override still applies (§5.1); the message is hidden (§3.3).

### 5.10 Sanitising and editor geometry (`geom.rs`)

On load and save: NaN/inf → defaults; `w, h` → `[2, 100]`; `x` → `[0, 100 − w]`, `y` → `[0, 100 − h]`; `max_size`/`min_size` → `[2, 40]` with `min ≤ max`; colours that don't parse (`render::parse_color`) → `None`; strings truncated to their limits. `Unknown` elements untouched.
Editor (pure): `snap(v, 2.5)`, `snap_to_guides(rect, others, 1.0) -> (PctRect, Vec<Guide>)`, `move_by`, `resize(rect, handle, dx, dy, min=5.0)`, `hit_test(point, elements) -> Option<Uuid>` (topmost, `Unknown` boxes hit-testable for delete), `nudge(rect, key, shift)`, `UndoStack<Vec<StageLayout>>` (cap 50).

## 6. Rendering

- New `src/render/stage.rs`: `pub fn render_cell(fonts: &mut Fonts, cell: &CellContent, style: &ResolvedStyle, font: &str, px: (u32, u32), window_h: u32) -> Rendered`.
  - Text: cosmic-text in the box (padding 1.2 % of window height), line height 1.15, weight 650 bold / 500. Fit: start at `max_size`, step down 0.4 % at a time to `min_size` (exactly today's `fit_size` loop for Classic's 9 → 3), never dropping words; clip at the floor.
  - Caption above the text at 2.2 % of height, letter-spaced 0.12 em, `#8b93a7`, uppercase; text area shrinks by its height.
  - Dim alpha 0.35 (today's). Red text `#ff4f55`. Fill: rounded rect, radius 1 % of height.
  - Picture: decoded at box size (existing decode-at-display-size path), letterboxed, 2 px `#2a303b` border.
  - Message: fill `#f5c66b`, text `#1a1405`, bold.
- Fonts: the layout's font, falling back to bundled Noto Sans; the editor shows the same "isn't installed" note as FEAT-1's Theme editor.
- Cache: the `Renderer` LRU is keyed by FEAT-1's `CacheKey { Slide(RenderKey), Overlay(OverlayKey), Stage(StageKey) }`; FEAT-3 defines `pub struct StageKey { content: u64 /* hash */, style: u64 /* hash */, font: String, px: (u32, u32) }` and uses `CacheKey::Stage(StageKey)`. `RenderKey` stays FEAT-1's slide struct. Cells render off the UI thread. Rich text is flattened to plain text on stage (the `styles` spans are ignored).
- **Swaps.** Within one layout, a changed cell keeps its previous image until its new one is ready. On a **layout switch or window open**, the whole old frame stays until every new cell has rendered once, or 300 ms pass; then all cells swap at once. CurrentSlide is rendered first, and the swap never happens without it while a slide is on air (at 300 ms the window swaps as soon as CurrentSlide is ready; other late cells appear when ready). On open there is no old frame: background only until CurrentSlide is ready.
- Cost: each cell re-renders only on content change; clock and timer cells (~400×100 px) once per second. Target on an old dual-core laptop with the software renderer: under 5 % CPU with clock + running timer (manual check).
- Flash: Slint animates the message cell's 4 px accent border (3 pulses, 1.5 s) on a new `seq` when `flash` is on. No re-render.

## 7. UI

### 7.1 Stage window (Phase A; Plan 2 files, modified)

- `ui/stage.slint` rewritten: `in property <color> bg; in property <[StageCellVm]> cells; in property <string> banner; in property <int> banner-seq; in property <bool> connected; in property <bool> practice;`. `StageCellVm { x, y, w, h: float, image: image, flash-seq: int, is-message: bool }`, placed at `x * root.width / 100` etc. Banner and a grey "Not connected" corner dot on top. While `practice` is true a **PRACTICE** tag is drawn top-right in every layout; it is window chrome, not a layout box (it is not in `StageLayout`, the editor or `StageView`). Cursor hidden. Literal colours stay (Plan 3 exempts `ui/stage.slint`).
- `src/ui/stage.rs`: `stage_text`/`fit_size` replaced by `stage::view::build` + `render_cell`; `Stage::open`/`follow` keep their shape and add the new update kinds; resize re-renders all cells (debounced 100 ms) using the §6 swap rule.

### 7.2 Operator (Plan 3 files)

- **New (A)** `ui/operator/stage-strip.slint` + `src/ui/operator/stage_strip.rs`, in `ui/operator/live.slint` at its place in FEAT-1 §7.2's Live-column budget: below FEAT-2's automation strip and directly above "Up next". It is **one 32 px row**: layout button + menu (thumbnails via `render_cell` at 120 px, cached per layout), `LineEdit` `stage-msg`, quick-message button, `stage-show-btn`, `stage-hide-btn`, `stage-chip` (text + elapsed minutes, updated by a 1 s timer that also drives auto-hide). At **1250 px wide or less** the row collapses to a single **Stage…** button (`stage-strip-btn`, with the chip's dot when a message is up) that opens a popover holding the same controls; Ctrl/Cmd+J opens the popover and focuses its message box. One new `Theme` token `stage-chip-bg: #f5c66b26`.
- `keys.rs`: all bindings are rows in FEAT-1 §7's single key table. (A) **Ctrl/Cmd+J** = focus the message box (global like Ctrl/Cmd+B; not over dialogs). **Ctrl/Cmd+Shift+J** = hide the message (capture set: works while typing and over dialogs; consumed only while a message is up). (B) **Alt+N** = stage note for the selected slide (presentation scope: not in text fields, no dialog; bare N stays free). `keys.rs` gains `const OS_RESERVED: &[KeyCombo]` (macOS Cmd+M/H/Q/W/Tab/Space/`,`; Windows Alt+F4, Alt+Tab, Win+any; Linux Super+any), against which every binding in the table is checked; Help rows for all three.
- `ui/dialogs/screens.slint` / `screens.rs` (A): stage card shows "Stage layout: Classic" with a default picker; (C) **Stage layouts…** and **Pair a tablet as a stage screen…**.
- `ui/dialogs/settings.slint` (A): **Stage** tab (FEAT-3's row in FEAT-1 §7's Settings tab table) — "Flash new messages", "Hide stage messages after". The dialog sends only these fields (merge by key, §4.4).
- **New (A)** `ui/dialogs/quick-messages.slint` + `src/ui/dialogs/quick_messages.rs`.
- (B) `ui/operator/slides.slint`: note mark, context item "Stage note…". `ui/operator/service.slint`: override icon. `ui/dialogs/item-edit.slint` / `item_edit.rs`: in the scrollable dialog's **Stage** section (FEAT-1 §7.2; sections Words, Theme, Automation, Stage, Bible), "Stage layout" `ComboBox` and the orphaned-notes list.
- **New (C)** `ui/dialogs/stage-layouts.slint` (shell, list, footer), `stage-canvas.slint` (boxes, handles, guides; pointer → callbacks in %), `stage-props.slint`, glue `src/ui/dialogs/stage_layouts.rs` (edited clone of all church layouts + deleted ids + `UndoStack`; Save = one `stage/layouts/replace`). Each file under ~400 lines. Not rebuilt by `dialog_refresh`; the live preview refreshes on `Update::Live`.

### 7.3 Phone (`web/remote.html`, `web/remote.js`; new `web/stage-view.html`)

Tabs "Control" (order per FEAT-2) and "Stage" (A). Strings in a `STR` object at the top of `remote.js`. `server.rs`: `/remote/stage` (A), `/remote/stage-view` and pairing roles (C), `remote_view` additions (A).

### 7.4 Strings

All new Rust strings in `src/strings/stage.rs` (FEAT-4 §7 convention; captions, kind names and descriptions, errors, toasts, sample words, "Stage layout"). Tone as `web/operator.js`: short and plain ("box", not "element").

## 8. Errors

| Situation | What the user sees | System |
|---|---|---|
| Action validation fails | Error toast with the core's message (Plan 3 `guard()`) | Nothing saved |
| `stage/layouts/replace` fails | Error under the editor footer; dialog stays open | Nothing saved (one `edit`) |
| Cell render fails | That box keeps its previous image, or stays empty | Logged once per key |
| Picture asset missing | "(slide picture)" | Preflight lists it as today |
| Timer id not found | "Timer removed", dimmed | — |
| Box/note from a newer version | Box draws nothing; editor shows a locked hatched box | Kept with the same meaning (`Lenient::Unknown`) |
| Macro step with a deleted layout | "Couldn't run “<macro>”: step N's stage layout was deleted. Edit the macro." Nothing runs | Ref kept |
| Phone request without session / wrong origin / StageView role | 401 / 403 | — |
| Stage monitor unplugged | As native spec §7 | Message, for-now layout kept |

Nothing in FEAT-3 can blank, change or delay the room output.

## 9. Testing

### 9.1 Unit tests (no window). Phase in brackets.

`src/stage/mod.rs` [A] (stage types inside the shared `crate::lenient::Lenient`): `unknown_element_kind_kept_as_unknown_and_written_back`, `unknown_align_or_timer_ref_makes_element_unknown_not_load_error`, `future_box_survives_unrelated_edit_with_same_meaning` (load a data.json holding a layout with `{"type":"hologram",…}` and a note with `{"on":"bar",…}`, run `App::edit` renaming a service, read data.json back: the two elements parse to equal `serde_json::Value`s), `known_value_round_trips`.
`src/stage/presets.rs` [A]: `builtin_ids_are_stable`, `builtin_names_are_unique`, `every_builtin_passes_sanitise_unchanged`, `classic_matches_todays_stage_regions` (slide, notes and next regions as today; header split into title and section label), `classic_fit_sizes_equal_old_fit_size` (for 20 sample texts and box sizes, the render fit equals the old `fit_size` result).
`src/stage/view.rs` [A]: `active_layout_manual_then_item_then_default_then_classic`, `manual_layout_ends_when_live_item_changes`, `stage_follows_practice_engine_via_display_view`, `manual_layout_ends_on_practice_item_change`, `manual_with_nothing_live_ends_when_something_goes_live`, `deleted_layout_falls_back`, `classic_current_slide_empty_and_dim_after_clear_slide` (renamed port of `stage_text_dims_when_cleared`, same assertions), `keep_after_clear_shows_dim_words_after_clear_slide_only`, `clear_all_always_empties_current_slide`, `current_slide_shows_logo_label`, `classic_picture_slide_shows_label`, `pictures_option_shows_picture`, `next_slide_matches_live_next_and_names_next_item`, `end_of_service_text`, `section_name_from_section_ref_songs_only`, `section_name_uses_color_for_label`, `classic_shows_section_label_line`, `service_position_counts_items`, `clock_formats_12h_24h_seconds`, `current_timer_latches_most_recent_start`, `current_timer_stays_when_paused`, `current_timer_shows_overrun_red`, `current_timer_shows_finished_red`, `current_timer_released_on_reset_to_stopped`, `current_timer_ignores_not_on_stage`, `timer_text_comes_from_timers_text`, `timer_paused_is_dim_with_caption`, `missing_timer_says_removed`, `classic_media_wins_shared_rect_over_timer`, `media_time_left_matches_todays_countdown` (port of `stage_text_timer_counts_down`), `message_fills_message_cell`, `message_without_message_box_sets_fallback`, `clears_never_touch_message_timer_clock_notes`, `unknown_element_draws_nothing`.
`src/stage/view.rs` [B]: `notes_prefer_slide_note_over_item_note`, `override_survives_switching_current_service`, `live_note_edit_reaches_stage_via_snapshot_after_unretaken_edit` (Take Live a song; add a verse before the chorus in the editor and save; edit the chorus note; after FEAT-4's notes-only adoption the stage still shows it on the snapshot's chorus slide, and the output version is unchanged).
`src/stage/notes.rs` [B]: `key_of_derives_song_key_from_section_ref`, `key_of_derives_verse_key_from_source`, `key_at_falls_back_to_index`, `note_exact_match`, `note_survives_lines_per_slide_change` (2 → 4 lines: note goes to the occurrence's first page; back to 2: still on page 1), `note_stays_on_its_chorus_occurrence` (note on chorus #3 does not move to chorus #1), `note_follows_arrangement_reorder`, `removed_occurrence_orphans_note`, `verse_note_survives_translation_switch_and_rebreak`, `index_note_for_decks`, `merged_notes_join_in_page_order`, `build_item_puts_slide_note_into_slide_notes`.
`src/stage/geom.rs` [C]: `sanitise_clamps_and_fixes_nan`, `snap_to_grid`, `snap_to_other_box_edges_and_centres_returns_guides`, `resize_keeps_minimum_size_and_inside_window`, `hit_test_returns_topmost`, `nudge_and_shift_nudge`, `undo_redo_cap_50`.
`src/stage/refs.rs` [C]: `deleting_layout_reports_macro_uses`, `layout_refs_counts_items_in_every_service`.
`src/app.rs` [A]: `stage_layout_use_is_for_now_and_default_is_separate`, `stage_layout_use_unknown_is_error`, `stage_message_show_trims_limits_and_bumps_seq`, `stage_message_hide_is_idempotent_and_if_seq_guards`, `service_switch_hides_stage_message`, `stage_actions_do_not_change_output_version` (every stage action), `macro_with_deleted_layout_refuses_to_run`, `stage_message_show_from_macro_records_source`. [B] `stage_note_set_and_clear`, `service_update_without_stage_fields_keeps_them` (merge by key). [C] `layouts_replace_is_all_or_nothing`, `rename_into_deleted_name_succeeds`, `layouts_replace_clears_item_and_default_refs`, `layouts_replace_rejects_builtin_empty_duplicate_and_too_many`.
`src/server.rs` [A]: `remote_stage_show_requires_pairing_and_same_origin`, `remote_stage_duplicate_id_does_not_reflash`, `retry_within_rate_window_returns_duplicate_not_too_fast`, `hide_never_rate_limited`, `remote_stage_rate_limited_per_phone` (via `remote::RateLimit`), `remote_stage_rejects_other_ops` ("Not allowed from the phone."), `remote_command_with_stage_body_is_bad_command` ("Bad command."), `remote_view_includes_stage_block`. [C] `stage_view_session_cannot_send_commands` (403 on `/remote/command`, `/remote/stage`, `/remote`), `old_phones_json_loads_as_remote_role`.
`src/ui/operator/keys.rs` [A]: `stage_shortcuts_avoid_os_reserved` (every binding in FEAT-1 §7's table checked against `OS_RESERVED` on all three platforms), `ctrl_shift_j_is_in_capture_set`, `help_lists_every_bound_key` (existing, now with the new rows).
`src/render/stage.rs` [A]: goldens (native spec §13 tolerance) `golden_classic_lyric`, `golden_speaker_with_timer_and_notes`, `golden_message_banner`; `stage_cells_cached_under_cache_key_stage`; `long_text_shrinks_to_floor_and_clips`, `caption_reduces_text_area`.
**Gated on Plan 4** (supervisor + `live-state.json`): `stage_state_restored_from_live_state` and `stage_state_written_on_message_show_hide`; they live in Plan 4's checklist and are `#[ignore]` until it lands.

### 9.2 UI tests (Slint testing backend, `ui_test_init()`, element ids)

- [A] `stage_window_places_cells_from_layout` (Classic → its cells at the preset %), `layout_switch_never_shows_empty_current_slide` (mock time; switch Classic → Speaker with a lyric on air; every frame the window shows has a CurrentSlide image), `strip_pick_is_for_now_until_next_item`, `strip_set_default_asks_then_sets`, `strip_show_message_reaches_stage`, `strip_hide_removes_message`, `ctrl_j_focuses_message_box`, `ctrl_shift_j_hides_only_when_message_up`, `strip_is_one_row_and_collapses_to_stage_button_at_1250`, `stage_window_shows_practice_tag_in_practice_mode`, `chip_shows_elapsed_minutes`, `auto_hide_after_setting`, `f1_in_message_box_clears_and_types_nothing`, `quick_message_click_shows_it`, `stage_off_toast_when_showing_without_window`. Existing `now_text_element_shows_the_live_lyric` ported to the cell model.
- [B] `stage_window_switches_layout_on_item_override`, `item_edit_stage_layout_round_trips`, `slide_note_prompt_saves_and_marks_thumbnail`, `alt_n_opens_note_prompt_only_outside_text_fields`.
- [C] `editor_drag_moves_box_snapped`, `editor_resize_handle_respects_minimum`, `editor_cancel_discards`, `editor_save_updates_stage_live`, `editor_save_error_keeps_dialog_and_changes`, `editor_builtin_is_read_only_copy_works`, `editor_delete_in_use_counts_all_refs`, `editor_unknown_box_locked_but_deletable`.

### 9.3 Manual checks (per platform, real hardware)

1. [A] Upgrade with an existing data.json: stage identical to before (side-by-side photo with the old build), including Clear Slide and a PowerPoint slide.
2. [A] Readability from 5 m on a 32" TV for each preset.
3. [A] Old laptop, software renderer: CPU under 5 % with clock + running timer + a lyric change every 5 s; layout switch shows no blank lyrics.
4. [A] Phone: "Last chorus" from an iPhone and an Android phone; flash visible; quick retry does not say "Too fast"; hide works.
5. [A] Unplug/replug the stage monitor mid-message: message back when the window reopens.
6. [B] Change lines per slide with notes on a repeated chorus; notes stay on their occurrence.
7. [C] Editor with mouse and trackpad: drag, resize, guides, undo. Tablet paired as a stage screen fills the screen in landscape, keeps up with Next, and has no way to the controls.
8. [Plan 4] Kill the app: stage comes back with the same message and for-now layout.

## 10. Dependencies on other FEAT specs

- **FEAT-2 (required first):** `TimersView`, `TimerSnapshot`, `TimerState`, `timers::text`, `App::timers()`, `Update::Timers` used as FEAT-2 defines them, with FEAT-2 §5.8's publish rule (§5.3); `on_stage` picks candidates for "current timer"; FEAT-3's latched rule replaces FEAT-2's interim stage line once FEAT-3 lands. FEAT-2 owns the four stage macro steps (`StageMessageShow`, `StageMessageHide`, `StageLayout`, `StageLayoutAuto`), resolves every reference before a macro runs and refuses to run one whose stage layout is gone; `StageLayout` steps register in `layout_refs`. Macro messages call `App::stage_message_show(text, MsgSource::Macro)`. FEAT-2 owns `crate::lenient::Lenient`, the phone allowlist table (§5.11), the Control tab order and `remote::RateLimit`. chrono is the only time crate. FEAT-2 room messages are separate.
- **Plan 4 (supervisor, `live-state.json`; native spec §8.3):** crash restore. `live-state.json` gains a separate `stage` field `{ message, manual }` (current timer is recomputed from FEAT-2's restored timers). It is written atomically (write + rename) on every message show/hide and for-now change, not only on live commits, and read only on `--restore`. Until Plan 4 lands, stage state is not restored after a crash (the restore tests are gated, §9.1).
- **FEAT-1:** no feature required, but FEAT-1 owns shared conventions FEAT-3 follows: the Settings tab table (FEAT-3 adds the **Stage** tab), the single key table with its capture set and presentation scope, the Live-column layout budget at 1366×768 (§7.2), merge-by-key for `settings/save` and `service/update`, the data format number and `Data::after_load` (FEAT-3 needs neither a bump nor a fix-up), and `CacheKey` (FEAT-3 adds `Stage(StageKey)`). Stage does not use the room overlay layer, themes or the background library; rich text (`styles`) shows flattened as plain text.
- **FEAT-4:** `Slide::new` and `SectionRef` (song keys are derived from it, §4.4); SectionName and Classic's section label line use `sections::color_for_label(label)`. FEAT-4 is the only live-item reconcile: notes-only edits are adopted into the snapshot with `Change::None`, which is how note edits reach the stage (§5.7). `src/strings/<area>.rs` convention.
- **FEAT-5:** `VerseId` (0-based book index) and `SlideSource` on scripture slides, so `Verse` keys and ranges work across translations; two-translation slides key on the primary translation's verses.
- **FEAT-6:** the stage follows `App::display_view()` and `Update::Practice` (§5.1, §5.4) and shows the PRACTICE tag (§7.1). Templates/copy-last-week copy `stage_layout` and `slide_notes`; FEAT-6 adds `LayoutRef::Template` when it lands (§5.6). Print/export reads `Slide.notes`.
- **FEAT-7:** the stage output's **screen view** shows the **stage layout name** and links to the same picker; it does not add a second selector. More stage outputs are not FEAT-7's: they are a later FEAT-3 follow-up, where each output would get its own `layout: Uuid` and `StageSettings.layout` stays the first output's default.
- **Plan 3:** merged first (Theme, Ops, dialogs, Live column, Screens, Item edit, keys).

## 11. Decisions for Jake to review

| # | Decision (default chosen) | Main alternative |
|---|---|---|
| 1 | Existing churches stay on **Classic**, which reproduces today's stage (empty box after Clear Slide, "(slide picture)" label); the only additions are a section label line in the section colour, the stage message and a "Show on stage" timer in the media-time spot. *(Changed Nov 1: section label line added.)* | Switch everyone to a nicer new default layout. |
| 2 | **Five built-ins**: Classic, Lyrics only, Band, Speaker, Clock and countdown; read-only, copy to change. | Editable built-ins. |
| 3 | Built in **three phases**: A presets + picker + messages + timers; B per-item layout + slide notes; C the drag-and-drop editor + tablet view. | Ship everything at once. |
| 4 | The editor uses **free boxes on a grid** (drag/resize, 2.5 % snap). | A simpler "template with slots" editor. |
| 5 | Picking a layout in the Live column is **"for now"** (until the next item); the church default changes only through "Set as the stage default" or Screens. | Picking always changes the default. |
| 6 | A service item can **switch the stage layout** while it's live. | Manual switching only. |
| 7 | A stage message **stays until hidden** or replaced, with a 3× flash; the chip shows how long it has been up; it hides itself when another service is opened; "Hide after" (1/5/10 min) is available but **Off** by default. | Auto-hide after a fixed time by default. |
| 8 | **120-character** messages and six default quick messages ("Last chorus", "Repeat the chorus", "Go to the bridge", "Please wrap up", "5 minutes left", "Check your microphone"), editable, max 12. | No quick messages, or a different list. |
| 9 | **Phones may show and hide stage messages**; they cannot change layouts or notes. | Laptop only. |
| 10 | **Tablet stage view** (Phase C) uses a **view-only pairing** made from Screens; it can never send commands. The local `/stage` browser page is kept unchanged, not extended. | Leave the tablet view out. |
| 11 | If a layout has no Message box, a message shows as a **banner across the top**. | Messages silently don't show. |
| 12 | Notes box shows **the slide note if set, else the item note**. | Show both. |
| 13 | After Clear Slide the current box is **empty** (today's behaviour) in every built-in; "Keep the words after Clear" is a per-box option, off by default. After Clear All it is always empty. | Keep dimmed words in band layouts by default. |
| 14 | Classic shows the **"(slide picture)" label**; Speaker shows **the picture**. Video never shows its moving picture on stage (old laptops). | Pictures on in every layout; or live video on stage. |
| 15 | Clock **12-hour without seconds** by default (per box). | 24-hour, or follow the computer's locale. |
| 16 | Shortcuts **Ctrl/Cmd+J** (message box), **Ctrl/Cmd+Shift+J** (hide message, works while typing and over dialogs), **Alt+N** (slide note; bare N stays free). No Esc-to-hide; Ctrl+M avoided (Cmd+M minimises on macOS). *(Changed Nov 1: N → Alt+N.)* | No new shortcuts. |
| 17 | Editing a note of the live item **updates the stage at once** (room never affected). | Notes wait for the next Take Live. |
| 18 | Notes follow the **song section and its occurrence**; when a section's page count changes, its notes move to the section's first page; scripture notes follow the **verse**. | Notes keyed by slide number (simpler, but jump when songs re-split). |
| 19 | Presets show the **"current timer"**: the most recently started "Show on stage" timer, kept while paused or over time, released on reset. | Presets ship without timers. |
| 20 | One native stage window (as today); more stage screens are a later FEAT-3 follow-up. *(Changed Nov 1: not FEAT-7.)* | Two stage screens now. |
| 21 | Deleting a stage layout moves items and the default to Classic/default; **macros keep the step and refuse to run** ("Couldn't run “<macro>”: step N's stage layout was deleted. Edit the macro.") until edited. *(Changed Nov 1: was "skip the step and continue".)* | Remove the step from macros automatically. |
| 22 | In practice mode the stage follows the practice engine and shows a **PRACTICE** tag top-right. *(New Nov 1.)* | Stage keeps showing the live engine during practice. |
| 23 | The Stage strip is **one row**; at 1250 px wide or less it becomes a single **Stage…** button. *(New Nov 1.)* | Two-row strip. |

## Review notes

All P1, P2 and P3 findings from the 2026-11-01 review were applied; none rejected. One adjustment: for the macro reference on delete, the spec follows FEAT-2's existing rule (keep unknown ids; the macro refuses to run until edited) rather than clearing the step, so a later undo or re-import can still find it.

## Cross-spec alignment (Nov 1)

Applied the cross-spec rulings; what changed in this spec:

- **Timers (§5.3, §4.3, §10):** `TimerReadout`/`Phase`/`text`/`secs` → FEAT-2's `TimerSnapshot`, `TimerState { Stopped, Running, Paused, Finished }` + `overrun`; text from `timers::text(value_ms)`, FEAT-3 adds colours only; `App::timers()` is the same `Arc` as `Update::Timers`; publish rule per FEAT-2 §5.8. Classic uses FEAT-3's latched "most recently started on_stage timer"; FEAT-2's interim line applies only until FEAT-3 lands.
- **Time (§4.5, §5.2, §10):** chrono only. `shown_at: chrono::DateTime<Utc>` (RFC 3339), `now: chrono::NaiveTime`; jiff removed.
- **Lenient (§4.1, §4, §9):** uses `crate::lenient::Lenient { Known, Unknown(Value) }`; `src/stage/lenient.rs`, `raw_value` and the byte-for-byte promise dropped (meaning round-trips). `Raw` → `Unknown` throughout; tests renamed.
- **Macros (§5.5, §5.6, §3.6, §8, decision 21):** four FEAT-2 step kinds `StageMessageShow{text}`, `StageMessageHide`, `StageLayout{layout}`, `StageLayoutAuto`; a deleted layout makes the macro refuse to run (not skip); `App::stage_message_show(text, MsgSource)` exposed; test → `macro_with_deleted_layout_refuses_to_run`.
- **Practice (§5.1, §5.4, §5.7, §7.1, decision 22):** follower reads `App::display_view()` and `Update::Practice`; for-now layout clears on the displayed engine's item change; `practice` property draws a PRACTICE tag as window chrome.
- **Data format (§4.5):** "format unchanged by FEAT-3"; no `after_load` fix-up.
- **UI words / Settings (§6, §7.2, §10, header):** "Look" removed; Theme editor's not-installed note; FEAT-7 "screen view"; Settings **Stage** tab per FEAT-1 §7.
- **Merge semantics (§4.4, §7.2, §9):** `Settings.stage`, `stage_layout`, `slide_notes` rely on merge-by-key; test added.
- **Layout refs (§5.6, §5.5, §10):** `LayoutRef::Template` removed (FEAT-6 adds it); deletes only via `stage/layouts/replace`.
- **Extra stage outputs (§2, §10, decision 20):** a later FEAT-3 follow-up, not FEAT-7.
- **Slide / notes (§4.4, §5.2, §4.3, §3.1, decision 1):** `Slide.key` dropped; `notes::key_of(&Slide)` derives `Song` from `SectionRef` and `Verse(VerseId)` from `SlideSource`; `sections::color_for_label`; Classic gains a section label line (header split; slide/notes/next regions unchanged).
- **Live item edits (§4.5, §5.2, §5.7, §9):** `live_notes` dropped; notes reach the stage through FEAT-4's notes-only adoption (`Change::None`, no version bump).
- **Render cache (§6):** `RenderKey::Stage` → `CacheKey::Stage(StageKey)`.
- **Strings (§4, §5.2, §7.4):** `src/stage/strings.rs` → `src/strings/stage.rs`.
- **Phone (§3.7, §4.5, §5.8, §7.3):** limiter is `remote::RateLimit`; `/remote/stage` referenced as FEAT-3's row in FEAT-2 §5.11's allowlist table; stream rules per FEAT-2; Stage tab shows the current timer.
- **Keys (§3.3, §3.5, §7.2, §9, decision 16):** N → Alt+N; Ctrl/Cmd+Shift+J in the capture set; "F1–F6"; bindings reference FEAT-1 §7's key table; test → `alt_n_opens_note_prompt_only_outside_text_fields`.
- **Layout budget (§7.2, §3.4, decision 23):** Stage strip is one 32 px row above Up next, collapsing to a single Stage… button at ≤ 1250 px; Item edit fields sit in its Stage section.
