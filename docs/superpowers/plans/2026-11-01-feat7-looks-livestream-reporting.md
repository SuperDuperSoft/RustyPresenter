# FEAT-7 — Screen Views, Livestream Output, Song Use Report, Planning Center: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each output (room, livestream) shows a chosen *screen view*. The words go to OBS as a transparent lower third (browser source) or to a keyed stream window. The app keeps a CCLI song use log and exports a report. A Planning Center Services plan can be imported as a service (phase D, Cargo feature `planning-center`).

**Architecture:** The plan has two parts, because another session is rewriting the shared files right now (Plan 3: `src/app.rs`, `src/ui/**`, `ui/**`, `src/server.rs`, `src/bin/native.rs`, `src/video`; Plan 4: nearly everything, including `store.rs`, `platform`, and removing Tauri/web). **Part A** builds every piece of FEAT-7 logic that is pure: data types with serde defaults, screen-view resolution and masking, the usage session rule, reports and exports, the stream render gate, the frame store, the compositor's blend and PNG encoding, monitor rules, PKCE and Planning Center mapping. It lives only in **new files**, under a single staging root module `crate::feat7` (one `pub mod feat7;` line in `src/lib.rs`) that pulls each module in from its final file path with `#[path]`, the same pattern FEAT-2 (`crate::automation`) and FEAT-3/FEAT-5 use. Task B1 promotes each module to its spec path (`crate::screen_view`, `crate::stream`, `crate::usage`, `crate::pco`, `crate::render::compose`, `crate::strings::{screen_view,stream,usage,pco}`) without moving a file. Types the spec puts on existing structs (`Settings`, `Data`, `Service`, `ServiceItem`, `Frame`, `Command`, `LiveView`) are defined as standalone types and functions in the new modules. Anything that needs a type that FEAT-1/2/4/5 have not built yet (`OverlaySlot`, `SavedTheme`, `SongRef`, `SectionKind`, `scripture::input`) takes a closure or a plain value instead. **Part B** wires these modules into model, live, app, store, room, render, server, the Slint UI and the phone remote, against Plan 3's file layout. **Part B is blocked until the `native-foundation` branch has Plans 3 and 4 merged** (and, per task, the FEAT specs it names).

**Tech Stack:** Rust 2024; existing crates only in Part A: `serde`/`serde_json`, `uuid` (`v4`, `serde`), `chrono` 0.4 (`clock` only, **no `serde` feature**), `sha2` 0.10, `image` 0.25 (`png`), `cosmic-text`/`tiny-skia` through `crate::render`. Part B, phase D only, adds `ureq` (rustls), `keyring` and `open` behind the `planning-center` feature, with exact versions pinned when that task runs.

**Spec:** `docs/superpowers/specs/2026-11-01-feat7-looks-livestream-reporting-design.md` (FEAT-7, rev 2). Contracts it relies on: FEAT-1 `docs/superpowers/specs/2026-11-01-feat1-themes-backgrounds-props-design.md` §4.5 (overlay names), §4.7 (`Data::after_load`), §6 (`render::overlay::draw`, `render::background::background_pixels`); FEAT-2 (`Change::Silent`, `Update::Timers`, `MacroStep::ScreenView`, §5.11 allowlist); FEAT-4 (`LiveItem.song: Option<SongRef>`, `SectionKind::from_label`, `src/strings/<area>.rs`); FEAT-5 (`scripture::input`, `scripture::fetch`, parallel `second`); FEAT-6 (practice engine routing §5.5, `src/report_page.rs`). Plan 3: `docs/superpowers/plans/2026-11-01-native-operator.md` (dialogs, `Ops`, `Theme` global, element ids, `Ops::inline` UI tests).

## Global Constraints

- **Test command (Part A).** In PowerShell: `& "C:\Users\jakes\AppData\Local\Temp\claude\C--Users-jakes-OneDrive-Documents-RustyPresenter\cfa9fbaf-d5ee-4157-9a05-a69be9a8108a\scratchpad\feat-cargo.cmd" test --release --lib <module path>`. Below it is written as `& $cargo test --release --lib <path>`, with `$cargo` set to that full path.
- **Part A file rule.** Only new files, plus exactly **one** line in an existing file: `pub mod feat7;` at the end of `src/lib.rs`, after a `// FEAT-7 (staging root; Task B1 promotes its modules)` comment. No edits to `src/render/mod.rs`, `src/strings/mod.rs` (FEAT-4 owns it; Part A never creates it), `src/app.rs`, `src/live.rs`, `src/model.rs`, `src/store.rs`, `src/room.rs`, `src/slides.rs`, `src/server.rs`, `src/ui/**`, `ui/**`, `web/**`, `src/bible*` or `Cargo.toml`. Part A may *read* existing public types that the spec keeps (`model::Theme`, `model::Song`, `model::Background`, `live::Frame`, `live::SlideLayer`, `render::Rendered`, `render::Fonts`, `render::render_text`).
- **Staging root `src/feat7/mod.rs`** (new file, created in Task A1, grown by later tasks). Its whole final content in Part A:
  ```rust
  //! FEAT-7 staging root. Each module lives at its final path; Task B1 declares them
  //! at their spec paths and turns this file into re-exports.
  #[path = "../screen_view.rs"] pub mod screen_view;
  #[path = "../stream/mod.rs"] pub mod stream;
  #[path = "../usage/mod.rs"] pub mod usage;
  #[path = "../pco/mod.rs"] pub mod pco;
  #[path = "../render/compose.rs"] pub mod compose;
  ```
  `#[path]` in a non-inline module is relative to the directory of the file that holds it (`src/feat7/`). A `#[path]` file named `mod.rs` keeps its children in its own directory, so `src/stream/mod.rs`'s plain `pub mod settings;` finds `src/stream/settings.rs`. So in Part A the crate paths are `crate::feat7::{screen_view, stream, usage, pco, compose}`.
- **Strings per module.** Each FEAT-7 module includes its strings file under the name `strings`, as FEAT-3/FEAT-5 do: `src/screen_view.rs` has `#[path = "strings/screen_view.rs"] pub mod strings;` (relative to `src/`), `src/stream/mod.rs` has `#[path = "../strings/stream.rs"] pub mod strings;`, and likewise `src/usage/mod.rs` → `../strings/usage.rs`, `src/pco/mod.rs` → `../strings/pco.rs`. Call sites write `screen_view::strings::X`, `usage::strings::fmt_day`, and so on. Task B1 adds the four `pub mod <area>;` lines to `src/strings/mod.rs` and replaces each `#[path]` line with `pub use crate::strings::<area> as strings;`, so these call sites never change.
- **Sibling references.** Part A code names a sibling FEAT-7 module as `crate::feat7::<module>` (for example `crate::feat7::screen_view::LOWER_THIRD`). Task B1 rewrites `crate::feat7::` to `crate::` across these files (one mechanical replace) and keeps `src/feat7/mod.rs` as `pub use` re-exports until Task B15 deletes it.
- **Persisted enums never block loading.** Every new persisted unit enum that may grow (`DeckSlides`, `StreamMode`, `KeyColor`, `PcoAuth`) puts `#[default] #[serde(other)]` on its default variant, per the FEAT-1 §4.7 / FEAT-2 §4.1 cross-spec rule. A `data.json` written by a later version (`"key_color": "blue"`) then loads as the default in this one, and `SUPPORTED_FORMAT` never needs a bump. `OutputRole` is not persisted as a free value (it is a command argument), so it stays strict. Each module that defines such an enum has a test `unknown_enum_values_load_as_default`.
- **No `Cargo.toml` change in Part A.** Decisions this forces, each recorded here once:
  - chrono has no `serde` feature in this tree. `usage` serialises dates with its own `#[serde(with = …)]` helpers: `NaiveDate` as `"YYYY-MM-DD"`, `DateTime<Utc>` as RFC 3339 (`to_rfc3339_opts(SecondsFormat::Secs, true)`, e.g. `"2026-11-01T14:05:00Z"`). If Plan 4 or FEAT-2 turns on `chrono/serde`, the helpers stay: same wire format, no migration.
  - No `csv` crate. Writing a CSV needs about 15 lines (RFC 4180 quoting). The spec's `csv` dependency is dropped.
  - No `uuid` macro feature. The fixed ids are `Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0701)` (a `const fn`), which is the same value as the spec's `uuid!("00000000-0000-4000-8000-000000000701")`.
  - No `base64` crate. PKCE base64url (no padding) is about 20 lines in `pco::pkce`.
- **Dates and times:** chrono only (`NaiveDate`, `DateTime<Utc>`); `jiff` is never added. Instants in pure functions are parameters (`now: DateTime<Utc>`, `today: NaiveDate`). Pure code never calls `Utc::now()`.
- **Strings:** every user-facing text from spec §3, §5.5 and §8 lives in `src/strings/{screen_view,stream,usage,pco}.rs` (Part A paths `crate::feat7::<module>::strings`) as `pub const` or a small `pub fn` for text with values. Core errors and Slint labels use these constants and never inline copies. Curly quotes “ ” and the ellipsis … are copied exactly as in the spec. UI words: "screen view", "theme", "overlay". "Look" and "Props" never appear in UI text or new identifiers.
- **Dates shown to people** use `fmt_day(d) = d.format("%b %-d, %Y")` ("Nov 1, 2026"), defined once in `usage::strings` (final path `strings::usage`). File names and CSV cells use ISO `YYYY-MM-DD`.
- **Fixed ids:** `EVERYTHING …0701`, `WORDS_ONLY …0702`, `LOWER_THIRD …0703`, `NO_WORDS …0704`, `STREAM_LOWER_THIRD_THEME …0711` (all `00000000-0000-4000-8000-000000000XXX`).
- **Numbers from the spec:** usage session gap 45 minutes; log kept 3 years (36 months); stream render cap 10 per second (100 ms); keep the latest 2 frames; crossfade 400 ms; stream size default 1920×1080; key colours Black `#000000`, Green `#00b140`; stream key 32 lowercase hex chars; OAuth callback `http://127.0.0.1:4318/pco/callback`; token refresh within 5 minutes of expiry; sign-in timeout 5 minutes; Planning Center HTTP timeout 20 s, max 3 `Retry-After` waits; plans: next 8 + 4 most recent; `room_preparing` wait at most 2 s; image cache 64 MB.
- **Part B rules.** Each Part B task names its prerequisite. No Part B task starts before `native-foundation` has Plans 3 and 4 merged and this branch is rebased on it. Part B follows Plan 3's constraints (Theme global, element ids, `Ops`, UI tests with `Ops::inline`, no blocking on the Slint thread, files under about 400 lines) and runs the whole suite (`cargo test` through the same wrapper without `--lib <path>`).
- Commit after every task, with the message given. Never `--no-verify`.

## Review Focus

1. **A CSV cell that starts with `=`, `+`, `-` or `@`** (a song title from SongSelect or Planning Center such as `=HYPERLINK(...)`). Expected: Excel shows it as text and does not run a formula, so the cell gets a leading `'`. Test `csv_neutralises_formula_cells` in Task A9.
2. **A log entry stamped later than `now`** (the clock went back after a time-zone fix or a dead CMOS battery). Expected: the usage session rule neither panics nor logs every slide again. A negative gap counts as "within 45 minutes". Test `future_entry_counts_as_same_session` in Task A8.
3. **"Reported through" is today or later** (marked on the day, or with a wrong clock). Expected: the default From date is never after To, so the dialog opens on a valid one-day range and never shows the "Pick a From date…" error by itself. Test `default_range_never_inverted` in Task A8.
4. **A custom screen view saved under a built-in's name** ("Lower third", any case), or with only spaces. Expected: refused with the duplicate-name or empty-name message, so the pickers never show two identical names. Test `save_rejects_builtin_name_and_blank` in Task A2.
5. **A browser source URL with a wrong or empty key while the stream key is still empty** (stream just enabled, settings not normalised yet). Expected: 404. An empty stored key never matches anything, including an empty path key. Test `empty_key_never_matches` in Task A6.

---

# PART A — Core, buildable now

### Task A1: Staging root and FEAT-7 strings

**Files:**
- Create: `src/feat7/mod.rs` (the doc comment and the first four `#[path]` lines from Global Constraints; `compose` is added in Task A7), `src/strings/screen_view.rs`, `src/strings/stream.rs`, `src/strings/usage.rs`, `src/strings/pco.rs`, and the four module root files with only a doc comment and their strings line: `src/screen_view.rs` (`#[path = "strings/screen_view.rs"] pub mod strings;`), `src/stream/mod.rs` (`#[path = "../strings/stream.rs"] pub mod strings;`), `src/usage/mod.rs` (`#[path = "../strings/usage.rs"] pub mod strings;`), `src/pco/mod.rs` (`#[path = "../strings/pco.rs"] pub mod strings;`). Later tasks fill these root files; they are new files, so that is allowed.
- Modify: `src/lib.rs` (the single `pub mod feat7;` line after the `// FEAT-7` comment). **Do not** create `src/strings/mod.rs`.

**Interfaces:** module paths below are the final ones (`strings::screen_view`); in Part A they are `crate::feat7::screen_view::strings`, `crate::feat7::stream::strings`, `crate::feat7::usage::strings`, `crate::feat7::pco::strings`.
- Produces, `strings::screen_view`: `NOT_FOUND` "That screen view no longer exists."; `BUILTIN_LOCKED` "Built-in views can't be changed. Duplicate it to make your own."; `NAME_EMPTY` "Give the view a name."; `fn name_taken(name) -> String` "There's already a view called “N”."; `ON_SCREEN` "That view is on a screen. Pick another view for that screen first."; `fn delete_confirm(name)` "Delete the screen view “N”?"; `DELETE_MACRO_NOTE` "Macros that use it won't run until you edit them."; `fn update_screen_ask(name, on_room: bool)` "“N” is on the room screen now. Update the screen now?" / "…on the livestream now…"; `fn theme_update_ask(name, on_room: bool)` "“N” uses this theme and is on the livestream now. Update the screen now?" (room: "…is on the room screen now…"); `UPDATE_NOW` "Update now"; `LATER` "Later"; `fn toast_shows(on_room: bool, name)` "Room screen now shows: N" / "Livestream now shows: N"; `fn chip(name)` "Room shows: N"; `CHIP_CHANGE` "Change"; `ROOM_RESETS` "Goes back to Everything when RustyPresenter starts."; `CHANGES_NOW` "Changes the screen right away."; `LOGO_HIDDEN` "The room's screen view hides the logo."; `THEME_DELETED` "Theme was deleted — using the item's theme"; `TEXT_STYLE_SAME` "Same as the song or item"; `TEXT_STYLE_NOTE` "Replaces each song's own style on this screen."; `BUILT_IN` "Built in"; `NAME_EVERYTHING` "Everything", `NAME_WORDS_ONLY` "Words only", `NAME_LOWER_THIRD` "Lower third", `NAME_NO_WORDS` "No words"; checkbox labels `SHOW_BACKGROUND` "Show background", `SHOW_MEDIA` "Show videos and pictures", `SHOW_SLIDE` "Show words and slides", `SHOW_LOGO` "Show the logo screen (F5)", `OVERLAYS` "Overlays", `OVERLAYS_SAVED` "Logo bug, clock and other overlays", `OVERLAYS_TIMERS` "Timers", `OVERLAYS_MESSAGES` "Messages", `DECK_SLIDES` "Deck slides", `SHOW` "Show", `HIDE` "Hide", `MORE_OPTIONS` "More options", `TEXT_STYLE` "Text style"; `fn macro_view_deleted(macro_name, step: usize)` "Couldn't run “M”: step N's screen view was deleted. Edit the macro.".
- Produces, `strings::stream`: card text `INTRO` "Words for your livestream, as a lower third or full screen. Off until you turn it on."; `HOW` "How does your stream get it?"; `MODE_BROWSER` "OBS / streaming app (browser source)"; `MODE_SCREEN` "A screen or capture card"; `OBS_STEPS` "1. In OBS, add a Browser source. 2. Paste this address. 3. Set width 1920 and height 1080."; `LOCAL_NOTE` "Use this one when OBS runs on this computer."; `NEW_ADDRESS` "New address"; `NEW_ADDRESS_CONFIRM` "The old address stops working. Continue?"; `SHOW_ON_SCREEN` "Show on this screen"; `TURN_OFF` "Turn off"; `TEST_WINDOW` "Test in a window"; `BACKGROUND` "Background"; `BG_BLACK` "Black"; `BG_GREEN` "Green"; `fn monitor_used_by(role: &str)` "That screen is used by the room. Pick another." / "That screen is used by the stage. Pick another."; `CONNECTED` "Connected"; `WAITING` "Waiting for OBS"; `SHOWING` "Showing"; `STARTING` "Starting…"; `OFF` "Off"; `VIDEO_NOTE` "Videos aren't sent to the livestream output. Send video to your stream from your streaming app."; `TURN_ON_FIRST` "Turn on the livestream output first."; `FIREWALL_NOTE` "OBS on another computer can't connect? Allow RustyPresenter through this computer's firewall."; `RENDER_FAILED` "The livestream picture couldn't update."; `SAME_AS_ROOM` "The livestream screen is the same as the room screen. Pick another in Screens."; `PICK_A_SCREEN` "Pick a screen"; `CARD_TITLE` "Livestream (optional)"; `SHOWS` "Shows".
- Produces, `strings::usage`: `fn fmt_day(d: NaiveDate) -> String`; `LOG_CHECKBOX` "Keep a song use log for CCLI reporting"; `MORE` "More"; `MENU_ITEM` "Song use report…"; `TITLE` "Song use report"; `SAVE_FIRST_ASK` "Save your settings changes and open the report?"; `SAVE_AND_OPEN` "Save and open"; `CANCEL` "Cancel"; `FROM` "From"; `TO` "To"; quick buttons `SINCE_LAST` "Since last report", `LAST_3` "Last 3 months", `LAST_6` "Last 6 months", `THIS_YEAR` "This year"; `INCLUDE_NO_CCLI` "Include songs without a CCLI number"; `PD_NOTE` "Public domain songs don't need reporting."; columns `COL_SONG` "Song", `COL_CCLI` "CCLI #", `COL_SHOWN` "Times shown", `COL_STREAMED` "Times streamed", `COL_LAST` "Last used"; `fn license_header(n)` "CCLI license #: N"; `NO_LICENSE` "Add your church's CCLI license number in Settings → Songs."; `SAVE_CSV` "Save as spreadsheet (CSV)…"; `OPEN_PRINTABLE` "Open printable page"; `fn mark_button(to)` "Mark as reported through Nov 1, 2026"; `fn mark_confirm(to)` "Next time, the report starts the day after Nov 1, 2026."; `DETAILS` "Details"; `REMOVE` "Remove"; `REMOVE_CONFIRM` "Remove this entry? Use this for rehearsals that shouldn't count."; `ENTRY_REMOVED` "Entry removed"; `STREAMED` "streamed"; `FOOTER` (the §3.3 step 6 text, verbatim); `BAD_RANGE` "Pick a From date before the To date."; `fn export_failed(reason)` "Couldn't save the report: R."; `fn csv_file_name(from, to)` "song-use-2026-08-01-to-2026-11-01.csv".
- Produces, `strings::pco`: every §3.4 and §8 Planning Center string, including `fn plan_row_use_song(title)` "Uses your song “T” (order from Planning Center)", `ROW_NEW_SONG` "Adds new song from Planning Center", `ROW_NEW_SONG_NO_LYRICS` "Adds new song — lyrics missing, paste them in the song editor", `fn row_scripture(reference, translation)` "Scripture: John 3:16-18 (KJV)", `fn cant_show(translation)` "KJV can't show this passage", `SOME_NOT_FOUND` "Some passages couldn't be found. Check the list and press Import again.", `ROW_TEXT` "Announcement slide with the title", `ROW_HEADING` "Skipped (heading)", `ROW_MEDIA` "Skipped — add the file yourself", `fn imported_toast(name)` "Imported “N”. Open it from Services when you're ready.", `fn reimport_ask(service)` "This plan was imported before as “S”.", `IMPORT_AS_NEW` "Import as a new service", `UPDATE_THAT` "Update that service…", `fn update_confirm(replaced: usize, kept: &[String])` (exact text below), `NO_ITEMS` "This plan has no items.", `OFFLINE` "Can't reach Planning Center. Check the internet connection.", `SIGNED_OUT` "Planning Center signed you out. Connect again.", `FORBIDDEN` (§8 403 text), `TOKEN_WRONG` "Planning Center didn't accept that Application ID and Secret.", `PORT_BUSY` "Planning Center sign-in needs port 4318, which is busy. Use a personal access token instead.", `TIMED_OUT` "Planning Center sign-in timed out. Try again.", `SAVE_FAILED` "Couldn't save the Planning Center sign-in on this computer.", `CONNECTED_PAGE` "RustyPresenter is connected. You can close this tab.", `fn connected_to(org)` "Connected to O", tab text, buttons and the "Saved on this computer only." note.

- [ ] **Step 1: Write the failing tests** (in each file's `#[cfg(test)] mod tests`)

```rust
// src/strings/screen_view.rs
#[test] fn name_taken_uses_curly_quotes() { assert_eq!(name_taken("Kids"), "There's already a view called “Kids”."); }
#[test] fn update_ask_names_the_screen() {
    assert_eq!(update_screen_ask("Words", true), "“Words” is on the room screen now. Update the screen now?");
    assert_eq!(update_screen_ask("Words", false), "“Words” is on the livestream now. Update the screen now?");
}
#[test] fn macro_view_deleted_text() {
    assert_eq!(macro_view_deleted("Pre-service", 3), "Couldn't run “Pre-service”: step 3's screen view was deleted. Edit the macro.");
}
// src/strings/stream.rs
#[test] fn monitor_used_by_room_and_stage() {
    assert_eq!(monitor_used_by("room"), "That screen is used by the room. Pick another.");
    assert_eq!(monitor_used_by("stage"), "That screen is used by the stage. Pick another.");
}
// src/strings/usage.rs
#[test] fn day_format_and_mark_texts() {
    let d = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    assert_eq!(fmt_day(d), "Nov 1, 2026");
    assert_eq!(mark_confirm(d), "Next time, the report starts the day after Nov 1, 2026.");
    assert_eq!(csv_file_name(NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(), d), "song-use-2026-08-01-to-2026-11-01.csv");
}
// src/strings/pco.rs
#[test] fn update_confirm_lists_kept_items() {
    assert_eq!(update_confirm(6, &["Welcome video".into(), "Announcements".into()]),
        "6 items from Planning Center will be replaced. Changes you made to them here are lost. 2 items you added yourself stay: Welcome video, Announcements.");
    assert_eq!(update_confirm(1, &[]), "1 item from Planning Center will be replaced. Changes you made to it here are lost.");
}
```

- [ ] **Step 2: Create** `src/feat7/mod.rs`, the four module root files and the `pub mod feat7;` line, with the four strings files holding only the tests. **Run** `& $cargo test --release --lib feat7::` → FAIL (does not compile: functions missing).
- [ ] **Step 3: Implement** the four strings files with every constant listed in Interfaces. `usage.rs` imports `chrono::NaiveDate` for `fmt_day`, `mark_button`, `mark_confirm` and `csv_file_name`.
- [ ] **Step 4: Run** `& $cargo test --release --lib feat7::` → PASS (6 tests). `git diff --stat -- src/lib.rs` shows 2 added lines (comment + `pub mod feat7;`) and nothing else outside new files.
- [ ] **Step 5: Commit** `feat(feat7): staging root and FEAT-7 screen view, livestream, song use and Planning Center texts`

### Task A2: Screen view types, built-ins, validation, activation

**Files:**
- Modify: `src/screen_view.rs` (new in A1; Part A path `crate::feat7::screen_view`)

**Interfaces:**
- Produces the spec §4 types exactly: `OutputRole { Room, Stream }` (snake_case), `OverlayMask { saved, timers, messages }` (`#[serde(default)]`, all true), `LayerMask { background, media, slide, logo, overlays }` (`#[serde(default)]`, all true), `DeckSlides { #[default] #[serde(other)] Show, Hide }` (`rename_all = "snake_case"`; an unknown value loads as `Show`), `ScreenView { id, name, layers, text_theme: Option<Uuid>, deck_slides }`, `ActiveView { id, name, layers, deck_slides, text_style: Option<crate::model::Theme> }` (`Serialize, Clone, Debug, PartialEq`), `OverlayClass { Saved, Timer, Message }` (`Clone, Copy, Debug, PartialEq, Eq`).
- `pub const EVERYTHING, WORDS_ONLY, LOWER_THIRD, NO_WORDS, STREAM_LOWER_THIRD_THEME: Uuid` (Global Constraints values).
- `pub fn builtins() -> [ScreenView; 4]`: the §3.1 table rows. Words only: slide + overlays.saved only. Lower third: as Words only, `text_theme: Some(STREAM_LOWER_THIRD_THEME)`, `deck_slides: Hide`. No words: everything except slide, `deck_slides: Hide`. Names come from `strings::NAME_*` (this module's `strings`).
- `pub fn is_builtin(id: Uuid) -> bool`
- `pub fn all_from(custom: &[ScreenView]) -> Vec<ScreenView>` (built-ins first, then custom in stored order); `pub fn find_in(custom: &[ScreenView], id: Uuid) -> Option<ScreenView>`. Part B wraps them as the spec's `all(data)` / `find(data, id)`.
- `pub fn activate_with(v: &ScreenView, theme_style: impl Fn(Uuid) -> Option<Theme>) -> ActiveView`: `text_style = v.text_theme.and_then(theme_style)` (a missing theme → `None`, the item's own). Part B: `activate(data, v) = activate_with(v, |id| theme::find(data, id).map(|t| t.style))`.
- `pub fn everything() -> ActiveView`
- `pub fn after_load_views(custom: &mut Vec<ScreenView>) -> bool`: drops views whose id is a built-in id; true when something was dropped.
- `pub fn validate_save(custom: &[ScreenView], v: &ScreenView) -> Result<(), String>`: built-in id → `BUILTIN_LOCKED`; name trimmed empty → `NAME_EMPTY`; name equal ignoring case and surrounding spaces to any *other* view in `all_from(custom)` (built-ins included) → `name_taken(trimmed)`.
- `pub fn validate_delete(id: Uuid, on_screens: &[Uuid]) -> Result<(), &'static str>`: built-in → `BUILTIN_LOCKED`; in `on_screens` → `ON_SCREEN`.
- `pub fn screens_showing(id: Uuid, room: Uuid, stream: Option<Uuid>) -> Vec<OutputRole>` (stream is `None` when the livestream is off). It feeds `screen-view/save`'s `on_screens` and the "Update now?" ask.

- [ ] **Step 1: Write the failing tests** in `src/screen_view.rs`

```rust
#[test] fn builtins_have_fixed_ids_and_cannot_be_saved_as_custom() {
    let ids: Vec<Uuid> = builtins().iter().map(|v| v.id).collect();
    assert_eq!(ids, [EVERYTHING, WORDS_ONLY, LOWER_THIRD, NO_WORDS]);
    assert_eq!(EVERYTHING.to_string(), "00000000-0000-4000-8000-000000000701");
    assert_eq!(STREAM_LOWER_THIRD_THEME.to_string(), "00000000-0000-4000-8000-000000000711");
    let mut copy = builtins()[2].clone(); copy.name = "Mine".into();
    assert_eq!(validate_save(&[], &copy).unwrap_err(), strings::BUILTIN_LOCKED);
}
#[test] fn find_unknown_id_is_none() { assert!(find_in(&[], Uuid::new_v4()).is_none()); assert!(find_in(&[], NO_WORDS).is_some()); }
#[test] fn lower_third_hides_messages_and_timers_by_default() {
    let lt = &builtins()[2];
    assert_eq!(lt.layers.overlays, OverlayMask { saved: true, timers: false, messages: false });
    assert_eq!((lt.layers.background, lt.layers.media, lt.layers.slide, lt.layers.logo), (false, false, true, false));
    assert_eq!((lt.text_theme, lt.deck_slides), (Some(STREAM_LOWER_THIRD_THEME), DeckSlides::Hide));
}
#[test] fn everything_shows_all_overlays() { let e = everything(); assert_eq!(e.layers, LayerMask::default()); assert_eq!(e.text_style, None); assert_eq!(e.id, EVERYTHING); }
#[test] fn activate_with_deleted_theme_uses_item_theme() {
    let v = ScreenView { text_theme: Some(Uuid::new_v4()), ..builtins()[1].clone() };
    assert_eq!(activate_with(&v, |_| None).text_style, None);
}
#[test] fn active_view_with_deleted_theme_keeps_its_copy() {
    let t = Theme { size: 60, position: "lower".into(), ..Theme::default() };
    let theme_id = Uuid::new_v4();
    let mut themes: std::collections::HashMap<Uuid, Theme> = [(theme_id, t.clone())].into();
    let v = ScreenView { id: Uuid::new_v4(), name: "Kids".into(), layers: LayerMask::default(), text_theme: Some(theme_id), deck_slides: DeckSlides::Show };
    let active = activate_with(&v, |id| themes.get(&id).cloned());
    themes.remove(&theme_id); // the theme is deleted while the view is on a screen
    assert_eq!(active.text_style, Some(t), "the engine's copy is unchanged");
    assert_eq!(activate_with(&v, |id| themes.get(&id).cloned()).text_style, None, "a fresh activation falls back to the item's theme");
}
#[test] fn after_load_drops_custom_views_with_builtin_ids() {
    let mut custom = vec![ScreenView { id: LOWER_THIRD, name: "Hack".into(), layers: LayerMask::default(), text_theme: None, deck_slides: DeckSlides::Show }];
    assert!(after_load_views(&mut custom)); assert!(custom.is_empty()); assert!(!after_load_views(&mut custom));
}
#[test] fn save_rejects_builtin_name_and_blank() {
    let mut v = ScreenView { id: Uuid::new_v4(), name: "  lower THIRD ".into(), layers: LayerMask::default(), text_theme: None, deck_slides: DeckSlides::Show };
    assert_eq!(validate_save(&[], &v).unwrap_err(), "There's already a view called “lower THIRD”.");
    v.name = "   ".into(); assert_eq!(validate_save(&[], &v).unwrap_err(), strings::NAME_EMPTY);
    v.name = "Kids".into(); assert!(validate_save(&[v.clone()], &v).is_ok(), "renaming to its own name is fine");
}
#[test] fn delete_refused_for_builtin_and_on_screen() {
    assert_eq!(validate_delete(WORDS_ONLY, &[]), Err(strings::BUILTIN_LOCKED));
    let id = Uuid::new_v4(); assert_eq!(validate_delete(id, &[id]), Err(strings::ON_SCREEN)); assert!(validate_delete(id, &[]).is_ok());
}
#[test] fn unknown_enum_values_load_as_default() {
    let v: ScreenView = serde_json::from_str(r#"{"id":"00000000-0000-4000-8000-000000000999","name":"A","deck_slides":"fit"}"#).unwrap();
    assert_eq!(v.deck_slides, DeckSlides::Show);
}
#[test] fn old_screen_view_json_fills_defaults() {
    let v: ScreenView = serde_json::from_str(r#"{"id":"00000000-0000-4000-8000-000000000999","name":"A"}"#).unwrap();
    assert_eq!((v.layers, v.text_theme, v.deck_slides), (LayerMask::default(), None, DeckSlides::Show));
    let m: LayerMask = serde_json::from_str(r#"{"slide":false,"overlays":{"timers":false}}"#).unwrap();
    assert_eq!((m.background, m.slide, m.overlays.saved, m.overlays.timers), (true, false, true, false));
    assert_eq!(serde_json::to_string(&DeckSlides::Hide).unwrap(), r#""hide""#);
    assert_eq!(serde_json::to_string(&OutputRole::Stream).unwrap(), r#""stream""#);
}
#[test] fn screens_showing_lists_room_and_stream() {
    let id = Uuid::new_v4();
    assert_eq!(screens_showing(id, id, Some(id)), [OutputRole::Room, OutputRole::Stream]);
    assert!(screens_showing(id, EVERYTHING, None).is_empty());
}
```

- [ ] **Step 2: Run** `& $cargo test --release --lib screen_view` → FAIL (module missing).
- [ ] **Step 3: Implement** the types and functions in `src/screen_view.rs` with the signatures above (below the existing `#[path]` strings line). No other file changes.
- [ ] **Step 4: Run** `& $cargo test --release --lib screen_view` → PASS (12 tests, plus the 3 strings tests the filter also matches).
- [ ] **Step 5: Commit** `feat(screen_view): screen view types, built-ins, validation and activation`

### Task A3: Applying a view to a frame

**Files:**
- Modify: `src/screen_view.rs` (new in A2)

**Interfaces:**
- Consumes: `live::Frame { background, media, slide, logo, audio }`, `live::SlideLayer::{Text, Image}`, `model::Background` (read only).
- Produces:
  - `impl OverlayMask { pub fn shows(&self, c: OverlayClass) -> bool }`
  - `pub fn apply(frame: &Frame, v: &ActiveView, slide_is_deck: bool) -> Frame`: background `None` unless `layers.background`; media `None` unless `layers.media`; slide `None` unless `layers.slide`, and also `None` when `slide_is_deck && deck_slides == Hide`; `logo &&= layers.logo`; **audio is always kept** (hidden media keeps playing, spec §5.2). Part B Task B1 adds one line that filters `frame.overlays` with `v.layers.overlays.shows(class_of(slot))`.
  - `pub fn shows_words(v: &ActiveView) -> bool` (`layers.slide`), used by the usage rule.
  - `pub fn hides_logo(v: &ActiveView) -> bool` (`!layers.logo`). It drives the "The room's screen view hides the logo." toast.

- [ ] **Step 1: Write the failing tests** (`media_layer(inst)`, `audio_layer(inst)` and `some_asset()` are test helpers built the way `src/live.rs` tests build those layers)

```rust
fn full_frame() -> Frame {
    Frame { background: Some(Inst { inst: 1, v: Background::Color { color: "#123456".into() } }),
            media: Some(/* an Inst<MediaLayer> built like live.rs tests do */ media_layer(2)),
            slide: Some(SlideLayer::Text { text: "Amazing grace".into(), footer: None, reference: None, scripture: false }),
            logo: true, audio: Some(audio_layer(3)) }
}
#[test] fn apply_hides_masked_layers() {
    let f = apply(&full_frame(), &activate_with(&builtins()[3], |_| None), false); // No words
    assert!(f.slide.is_none()); assert!(f.background.is_some() && f.media.is_some() && f.logo);
    let f = apply(&full_frame(), &activate_with(&builtins()[1], |_| None), false); // Words only
    assert!(f.background.is_none() && f.media.is_none() && !f.logo && f.slide.is_some());
    assert!(f.audio.is_some(), "audio is never affected");
}
#[test] fn apply_hides_deck_image_when_deck_slides_hide() {
    let mut fr = full_frame(); fr.slide = Some(SlideLayer::Image { asset: some_asset() });
    assert!(apply(&fr, &activate_with(&builtins()[2], |_| None), true).slide.is_none());
    assert!(apply(&fr, &everything(), true).slide.is_some());
}
#[test] fn apply_keeps_text_slide_when_deck_slides_hide() {
    assert!(apply(&full_frame(), &activate_with(&builtins()[2], |_| None), false).slide.is_some());
}
#[test] fn apply_everything_is_identity() { assert_eq!(apply(&full_frame(), &everything(), false), full_frame()); }
#[test] fn overlay_mask_shows_by_class() {
    let m = builtins()[2].layers.overlays;
    assert!(m.shows(OverlayClass::Saved) && !m.shows(OverlayClass::Timer) && !m.shows(OverlayClass::Message));
}
```

- [ ] **Step 2: Run** `& $cargo test --release --lib screen_view` → FAIL (`apply` missing).
- [ ] **Step 3: Implement** `apply`, `OverlayMask::shows`, `shows_words` and `hides_logo`. `apply` clones the frame and clears fields, so any field added to `Frame` later (FEAT-1 `overlays`) passes through unchanged until B1 filters it.
- [ ] **Step 4: Run** `& $cargo test --release --lib screen_view` → PASS.
- [ ] **Step 5: Commit** `feat(screen_view): apply a view's layer mask to a frame`

### Task A4: Stream settings and monitor rules

**Files:**
- Create: `src/stream/settings.rs`, `src/stream/monitor.rs`
- Modify: `src/stream/mod.rs` (new in A1: add `pub mod settings; pub mod monitor;`; later tasks add their own lines here)

**Interfaces:** (Part A path `crate::feat7::stream`; `LOWER_THIRD` is `crate::feat7::screen_view::LOWER_THIRD`)
- Produces (`stream::settings`): `StreamSettings { enabled, mode: StreamMode, key: String, key_color: KeyColor, size: (u32, u32), view: Uuid }` with `#[serde(default)]` and the spec's explicit `Default` (view `LOWER_THIRD`, size `(1920, 1080)`); `StreamMode { #[default] #[serde(other)] BrowserSource, Screen }` and `KeyColor { #[default] #[serde(other)] Black, Green }` (`rename_all = "snake_case"`, `Clone, Copy, Debug, PartialEq, Eq`); `impl KeyColor { pub fn hex(self) -> &'static str }`; `UsageSettings { enabled: bool }` (`#[serde(default)]`, `Default` → `true`) is defined in `usage` (Task A8), not here.
  - `pub fn normalize(s: &mut StreamSettings, view_exists: impl Fn(Uuid) -> bool, make_key: impl FnOnce() -> String) -> bool`: unknown `view` → `LOWER_THIRD`; `enabled && key.is_empty()` → `key = make_key()`; true when changed. Part B passes `|| crate::app::random_token()[..32].to_string()` (32 lowercase hex chars).
  - `pub fn is_valid_key(k: &str) -> bool` (32 chars, `[0-9a-f]`).
- Produces (`stream::monitor`):
  - `pub fn check_pick(pick: &str, room: Option<&str>, stage: Option<&str>) -> Result<(), String>`: equal to room → `monitor_used_by("room")`, to stage → `monitor_used_by("stage")`.
  - `pub fn default_pick(monitors: &[String], room: Option<&str>, stage: Option<&str>, operator: Option<&str>) -> Option<String>`: the first monitor that is not room, stage or operator.
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub enum StartupPick { Open(String), SameAsRoom, NotConnected, Nothing }` (the tests `assert_eq!` on it) and `pub fn startup_pick(s: &StreamSettings, saved: Option<&str>, connected: &[String], room: Option<&str>) -> StartupPick`: `Open` only when `enabled && mode == Screen` and `saved` is connected and is not the room's; `SameAsRoom` raises the `SAME_AS_ROOM` notice; `NotConnected` waits for the monitor like the room.

- [ ] **Step 1: Write the failing tests**

```rust
// settings.rs
#[test] fn empty_json_deserialises_to_stream_settings_default() {
    let s: StreamSettings = serde_json::from_str("{}").unwrap();
    assert_eq!((s.view, s.size, s.key_color, s.mode, s.enabled), (LOWER_THIRD, (1920, 1080), KeyColor::Black, StreamMode::BrowserSource, false));
    assert_eq!(serde_json::to_value(&s).unwrap()["mode"], "browser_source");
    assert_eq!((KeyColor::Black.hex(), KeyColor::Green.hex()), ("#000000", "#00b140"));
}
#[test] fn normalize_fixes_unknown_view_and_makes_key_only_when_enabled() {
    let mut s = StreamSettings { view: Uuid::new_v4(), ..Default::default() };
    assert!(normalize(&mut s, |_| false, || panic!("no key while off")));
    assert_eq!((s.view, s.key.as_str()), (LOWER_THIRD, ""));
    s.enabled = true;
    assert!(normalize(&mut s, |_| true, || "0123456789abcdef0123456789abcdef".into()));
    assert!(is_valid_key(&s.key));
    assert!(!normalize(&mut s, |_| true, || panic!("key kept")));
}
#[test] fn unknown_enum_values_load_as_default() {
    let s: StreamSettings = serde_json::from_str(r#"{"key_color":"blue","mode":"ndi"}"#).unwrap();
    assert_eq!((s.key_color, s.mode), (KeyColor::Black, StreamMode::BrowserSource));
}
// monitor.rs
#[test] fn stream_refuses_room_monitor() { assert_eq!(check_pick("D2", Some("D2"), None).unwrap_err(), "That screen is used by the room. Pick another."); }
#[test] fn stream_refuses_stage_monitor() { assert_eq!(check_pick("D3", Some("D2"), Some("D3")).unwrap_err(), "That screen is used by the stage. Pick another."); }
#[test] fn stream_default_pick_skips_operator_monitor() {
    let m = vec!["D1".to_string(), "D2".into(), "D3".into()];
    assert_eq!(default_pick(&m, Some("D2"), None, Some("D1")), Some("D3".into()));
    assert_eq!(default_pick(&m[..2], Some("D2"), None, Some("D1")), None, "only the operator's monitor left: never chosen automatically");
}
#[test] fn stream_window_reopens_at_startup_when_enabled() {
    let s = StreamSettings { enabled: true, mode: StreamMode::Screen, ..Default::default() };
    assert_eq!(startup_pick(&s, Some("D3"), &["D1".into(), "D3".into()], Some("D2")), StartupPick::Open("D3".into()));
    assert_eq!(startup_pick(&StreamSettings::default(), Some("D3"), &["D3".into()], None), StartupPick::Nothing);
    assert_eq!(startup_pick(&s, Some("D3"), &["D1".into()], None), StartupPick::NotConnected);
}
#[test] fn saved_stream_monitor_equal_to_room_is_ignored() {
    let s = StreamSettings { enabled: true, mode: StreamMode::Screen, ..Default::default() };
    assert_eq!(startup_pick(&s, Some("D2"), &["D2".into()], Some("D2")), StartupPick::SameAsRoom);
}
```

- [ ] **Step 2: Run** `& $cargo test --release --lib stream::` → FAIL.
- [ ] **Step 3: Implement** both files and add their two `pub mod` lines to `src/stream/mod.rs`. No other file changes.
- [ ] **Step 4: Run** `& $cargo test --release --lib stream::` → PASS.
- [ ] **Step 5: Commit** `feat(stream): livestream settings, normalisation and monitor rules`

### Task A5: Stream input selection and render gate

**Files:**
- Create: `src/stream/input.rs`, `src/stream/gate.rs`
- Modify: `src/stream/mod.rs` (`pub mod input; pub mod gate;`)

**Interfaces:**
- Consumes: `crate::feat7::screen_view::{apply, ActiveView, LOWER_THIRD}`, `live::Frame`, `model::Theme`, `serde::Serialize`.
- Produces (`stream::input`):
  - `pub fn source_frame<'a>(room_connected: bool, pending: bool, committed: &'a Frame, requested: &'a Frame) -> Option<&'a Frame>`: room connected and pending → `None` (keep the last picture); room connected → `committed`; no room → `requested`.
  - `pub fn strip_for_stream(f: &Frame) -> Frame`: drops `media`, `audio` and a `Background::Video` background. Colour and image backgrounds stay.
  - `pub fn stream_frame(f: &Frame, v: &ActiveView, slide_is_deck: bool) -> Frame` = `strip_for_stream(&apply(f, v, slide_is_deck))`.
  - `pub fn primary_only(v: &ActiveView) -> bool` (`v.id == LOWER_THIRD`). Only the built-in Lower third drops FEAT-5's second translation.
  - `#[derive(Clone, Debug, PartialEq, Serialize)] pub struct StreamInput { pub frame: Frame, pub text_style: Option<Theme>, pub primary_only: bool, pub overlay_texts: Vec<(String, String)> }` (`overlay_texts` = slot key → bound text, filled in B6). No `Hash` derive: `live::Frame`, `Background`, `MediaLayer` and `AudioLayer` do not derive `Hash`, and Part A cannot add it.
  - `pub fn input_hash(i: &StreamInput) -> u64`: a `std::collections::hash_map::DefaultHasher` over `serde_json::to_vec(i).expect("StreamInput serialises")` (every field is plain data, so serialisation cannot fail). The hash only has to be stable within one run.
- Produces (`stream::gate`):
  - `pub enum Gate { Render, WaitFor(Duration), Skip }`
  - `pub struct RenderGate { last_hash: Option<u64>, last_render: Option<Instant> }`, `RenderGate::new()`, `pub fn decide(&mut self, hash: u64, active: bool, now: Instant) -> Gate`: not active → `Skip` and forget `last_hash` (the first connection renders the current picture); same hash as the last render → `Skip`; less than 100 ms since the last render → `WaitFor(rest)`; else `Render` (records hash and time).

- [ ] **Step 1: Write the failing tests**

```rust
// input.rs
#[test] fn stream_follows_committed_not_requested_when_room_connected() {
    let (c, r) = (frame_with_text("old"), frame_with_text("new"));
    assert_eq!(source_frame(true, false, &c, &r), Some(&c));
    assert_eq!(source_frame(true, true, &c, &r), None, "while pending the stream keeps its last picture");
}
#[test] fn stream_uses_requested_when_no_room() { let (c, r) = (frame_with_text("a"), frame_with_text("b")); assert_eq!(source_frame(false, true, &c, &r), Some(&r)); }
#[test] fn stream_never_contains_media_or_video_background() {
    let mut f = frame_with_text("x"); f.background = Some(Inst { inst: 1, v: Background::Video { asset: some_asset() } }); f.media = Some(media_layer(2));
    let s = stream_frame(&f, &everything(), false);
    assert!(s.background.is_none() && s.media.is_none() && s.audio.is_none() && s.slide.is_some());
}
#[test] fn primary_only_for_builtin_lower_third_only() {
    assert!(primary_only(&activate_with(&builtins()[2], |_| None)));
    let custom = ScreenView { id: Uuid::new_v4(), text_theme: Some(STREAM_LOWER_THIRD_THEME), ..builtins()[2].clone() };
    assert!(!primary_only(&activate_with(&custom, |_| None)));
}
#[test] fn input_hash_changes_with_text_and_style() {
    let base = StreamInput { frame: frame_with_text("Amazing grace"), text_style: None, primary_only: true, overlay_texts: vec![] };
    assert_eq!(input_hash(&base), input_hash(&base.clone()), "equal inputs hash equally");
    let other_text = StreamInput { frame: frame_with_text("How great"), ..base.clone() };
    assert_ne!(input_hash(&base), input_hash(&other_text));
    let other_style = StreamInput { text_style: Some(Theme { size: 60, ..Theme::default() }), ..base.clone() };
    assert_ne!(input_hash(&base), input_hash(&other_style));
}
// gate.rs
#[test] fn stream_renders_only_on_input_change() {
    let (mut g, t0) = (RenderGate::new(), Instant::now());
    assert_eq!(g.decide(1, true, t0), Gate::Render);
    assert_eq!(g.decide(1, true, t0 + ms(500)), Gate::Skip);
    assert_eq!(g.decide(2, true, t0 + ms(600)), Gate::Render);
}
#[test] fn stream_render_rate_capped_at_ten_per_second() {
    let (mut g, t0) = (RenderGate::new(), Instant::now());
    g.decide(1, true, t0);
    assert_eq!(g.decide(2, true, t0 + ms(30)), Gate::WaitFor(ms(70)));
    assert_eq!(g.decide(2, true, t0 + ms(100)), Gate::Render);
}
#[test] fn stream_renders_nothing_while_inactive() {
    let (mut g, t0) = (RenderGate::new(), Instant::now());
    assert_eq!(g.decide(1, false, t0), Gate::Skip);
    assert_eq!(g.decide(1, true, t0 + ms(1)), Gate::Render, "first connection renders the current picture");
    g.decide(1, false, t0 + ms(500));
    assert_eq!(g.decide(1, true, t0 + ms(900)), Gate::Render, "reconnecting renders again even if unchanged");
}
```

- [ ] **Step 2: Run** `& $cargo test --release --lib stream::` → FAIL.
- [ ] **Step 3: Implement** `input.rs` and `gate.rs` (derive `PartialEq, Debug` on `Gate`).
- [ ] **Step 4: Run** `& $cargo test --release --lib stream::` → PASS.
- [ ] **Step 5: Commit** `feat(stream): stream input selection and 10 fps render gate`

### Task A6: Frame store, connection count, key check, sink seam, browser source page

**Files:**
- Create: `src/stream/frames.rs`, `src/stream/sink.rs`, `src/stream/page.rs`, `src/stream/page.html`
- Modify: `src/stream/mod.rs` (`pub mod frames; pub mod sink; pub mod page;`)

**Interfaces:**
- Produces (`stream::frames`):
  - `pub struct StreamFrame { pub n: u64, pub png: Arc<[u8]>, pub rgba: Arc<Rendered> }`; `pub struct FrameStore` (holds at most 2), `pub fn push(&mut self, png: Arc<[u8]>, rgba: Arc<Rendered>) -> u64` (numbers start at 1), `pub fn get(&self, n: u64) -> Option<StreamFrame>` (unknown `n` → newest; empty → `None`), `pub fn newest(&self) -> Option<StreamFrame>`.
  - `#[derive(Clone, Default)] pub struct StreamConns(Arc<AtomicUsize>)`, `pub fn guard(&self) -> ConnGuard` (decrements on drop), `pub fn count(&self) -> usize`.
  - `pub fn key_matches(expected: &str, given: &str) -> bool`: constant time over the longer length; `false` whenever `expected` is empty.
- Produces (`stream::sink`): the spec trait, with `&Rendered` from `crate::render`: `pub trait StreamSink: Send + Sync { fn frame(&self, n: u64, rgba: &Rendered, png: &Arc<[u8]>); fn active(&self) -> bool; }`. Part B implements `BrowserSourceSink` (Task B6) and `WindowSink` (Task B9a).
- Produces (`stream::page`): `pub const PAGE: &str = include_str!("page.html");` (under 60 lines: transparent `html, body`, two absolutely positioned `<img>` with a 400 ms opacity transition, `new EventSource(location.pathname + "/events")`, each message `{"n":N}` loads `location.pathname + "/frame/" + n + ".png"` into the hidden image and swaps faces on `load`. The page is served at `/stream/<key>` with no trailing slash, so a relative `frame/N.png` would resolve to `/stream/frame/N.png` and 404; every URL is built from `location.pathname`); `pub fn sse_frame(n: u64) -> String` = `"data: {\"n\":N}\n\n"`; `pub const SSE_PING: &str = ": ping\n\n";`, `pub const PING_SECS: u64 = 15;`. The spec's `web/stream.html` path changes to `src/stream/page.html` because Plan 4 is reorganising `web/`.

- [ ] **Step 1: Write the failing tests**

```rust
// frames.rs
#[test] fn stream_frame_returns_newest_for_unknown_number() {
    let mut s = FrameStore::default();
    assert!(s.get(1).is_none());
    for _ in 0..3 { s.push(png(b"x"), rgba()); }
    assert_eq!(s.get(2).unwrap().n, 2);
    assert_eq!(s.get(1).unwrap().n, 3, "frame 1 was dropped: newest is served");
    assert_eq!(s.get(99).unwrap().n, 3);
}
#[test] fn conns_count_while_guards_live() {
    let c = StreamConns::default(); let a = c.guard(); let b = c.guard();
    assert_eq!(c.count(), 2); drop(a); assert_eq!(c.count(), 1); drop(b); assert_eq!(c.count(), 0);
}
#[test] fn empty_key_never_matches() {
    assert!(!key_matches("", "")); assert!(!key_matches("", "abc"));
    let k = "0123456789abcdef0123456789abcdef";
    assert!(key_matches(k, k)); assert!(!key_matches(k, &k[..31])); assert!(!key_matches(k, "0123456789abcdef0123456789abcdeg"));
}
// page.rs
#[test] fn page_is_small_transparent_and_local_only() {
    assert!(PAGE.lines().count() < 60);
    assert!(PAGE.contains("EventSource") && PAGE.contains("transparent") && PAGE.contains("400ms"));
    assert!(!PAGE.contains("http://") && !PAGE.contains("https://"), "no remote URLs");
    assert!(PAGE.contains("location.pathname + \"/frame/\"") && PAGE.contains("location.pathname + \"/events\""), "absolute paths: the page URL has no trailing slash");
    assert_eq!(sse_frame(7), "data: {\"n\":7}\n\n");
}
```

- [ ] **Step 2: Run** `& $cargo test --release --lib stream::` → FAIL.
- [ ] **Step 3: Implement** the four files.
- [ ] **Step 4: Run** `& $cargo test --release --lib stream::` → PASS.
- [ ] **Step 5: Commit** `feat(stream): frame store, connection guard, key check, sink trait and browser source page`

### Task A7: Compositor blend and fast PNG

**Files:**
- Create: `src/render/compose.rs`
- Modify: `src/feat7/mod.rs` (new in A1: add `#[path = "../render/compose.rs"] pub mod compose;`). **Do not** touch `src/render/mod.rs`; Task B1 adds `pub mod compose;` there.

**Interfaces:** (Part A path `crate::feat7::compose`; it names render items as `crate::render::…`, which keeps working after B1 moves it under `render`)
- Consumes: `render::{Rendered, Fonts, TextSlide, render_text}`, `model::Theme`, `image::codecs::png::{PngEncoder, CompressionType, FilterType}`.
- Produces:
  - `pub struct Layer<'a> { pub x: i32, pub y: i32, pub img: &'a Rendered }` (FEAT-1's `Placed { x, y, img }` converts to it in Part B).
  - `pub fn blend(canvas: (u32, u32), layers: &[Layer]) -> Rendered`: a transparent premultiplied canvas, each layer source-over in slice order (bottom first), clipped to the canvas, `font_px: 0.0`, `font_fallback: false`. Part B's `compose(input, canvas, fonts, images)` builds the layer list (background, still, slide, logo, overlays) and calls this.
  - `pub fn to_png(r: &Rendered) -> Vec<u8>`: un-premultiplies to straight RGBA8 and encodes with `PngEncoder::new_with_quality(w, CompressionType::Fast, FilterType::Sub)`.

- [ ] **Step 1: Write the failing tests**

```rust
fn solid(w: u32, h: u32, px: [u8; 4]) -> Rendered { Rendered { width: w, height: h, rgba_premul: px.repeat((w * h) as usize), font_px: 0.0, font_fallback: false } }
#[test] fn blend_keeps_uncovered_pixels_transparent() {
    let r = blend((4, 4), &[Layer { x: 1, y: 1, img: &solid(1, 1, [255, 0, 0, 255]) }]);
    assert_eq!(&r.rgba_premul[0..4], &[0, 0, 0, 0]);
    assert_eq!(px(&r, 1, 1), [255, 0, 0, 255]);
}
#[test] fn blend_draws_layers_bottom_to_top() {
    let (red, blue) = (solid(2, 2, [255, 0, 0, 255]), solid(2, 2, [0, 0, 255, 255]));
    assert_eq!(px(&blend((2, 2), &[Layer { x: 0, y: 0, img: &red }, Layer { x: 0, y: 0, img: &blue }]), 0, 0), [0, 0, 255, 255]);
}
#[test] fn blend_half_alpha_source_over_premultiplied() {
    let (white, half_black) = (solid(1, 1, [255, 255, 255, 255]), solid(1, 1, [0, 0, 0, 128]));
    assert_eq!(px(&blend((1, 1), &[Layer { x: 0, y: 0, img: &white }, Layer { x: 0, y: 0, img: &half_black }]), 0, 0), [127, 127, 127, 255]);
}
#[test] fn blend_clips_layers_outside_canvas() {
    let r = blend((2, 2), &[Layer { x: -1, y: 1, img: &solid(3, 3, [0, 255, 0, 255]) }]);
    assert_eq!(px(&r, 0, 0), [0, 0, 0, 0]); assert_eq!(px(&r, 1, 1), [0, 255, 0, 255]);
}
#[test] fn png_keeps_alpha_and_unpremultiplies() {
    let png = to_png(&solid(1, 1, [64, 0, 0, 128]));
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(0, 0).0, [128, 0, 0, 128]);
}
#[test] fn lower_third_words_over_transparent() {
    let theme = Theme { position: "lower".into(), size: 60, scripture_size: 46, shadow: true, ..Theme::default() };
    let text = render_text(&mut Fonts::bundled_only(), &TextSlide { text: "Amazing grace", footer: None, reference: None, scripture: false }, &theme, (1920, 1080));
    let r = blend((1920, 1080), &[Layer { x: 0, y: 0, img: &text }]);
    // render_text draws the lower shade (raster.rs LOWER_SHADE = 0.55) from row 1080 * 0.45 = 486 down,
    // so only rows above it are fully transparent.
    let shade_top = (1080.0 * (1.0 - 0.55)) as u32 - 6; // 480
    assert!((0..shade_top).all(|y| (0..1920).step_by(16).all(|x| px(&r, x, y)[3] == 0)), "everything above the shade stays transparent");
    assert!(px(&r, 960, 1079)[3] > px(&r, 960, 500)[3], "the shade gets darker toward the bottom");
    assert!((810..1080).any(|y| (0..1920).any(|x| px(&r, x, y)[3] > 0)), "words in the lower third");
}
```

- [ ] **Step 2: Run** `& $cargo test --release --lib feat7::compose` → FAIL.
- [ ] **Step 3: Implement** `blend` (integer premultiplied source-over: `dst = src + dst * (255 - src_a) / 255`, rounding down) and `to_png` (`(c * 255 + a / 2) / a`, 0 when `a == 0`).
- [ ] **Step 4: Run** `& $cargo test --release --lib feat7::compose` → PASS.
- [ ] **Step 5: Commit** `feat(render): stream compositor blend and fast PNG encoding`

### Task A8: Song use log, usage session and date ranges

**Files:**
- Modify: `src/usage/mod.rs` (new in A1; Part A path `crate::feat7::usage`)

**Interfaces:**
- Produces: `UsageLog { entries: Vec<UsageEntry>, reported_through: Option<NaiveDate> }` (`#[serde(default)]`, `Default`); `UsageEntry { id, at: DateTime<Utc>, local_date: NaiveDate, song_id, title, ccli, writers, copyright, service_id, service_name, #[serde(default)] streamed }`; `UsageSettings { enabled }` (`#[serde(default)]`, `Default` → `enabled: true`); private serde helper modules `date_iso` and `rfc3339` (formats in Global Constraints), also used for `Option<NaiveDate>`.
- `pub const SESSION_GAP_MIN: i64 = 45; pub const KEEP_MONTHS: u32 = 36;`
- `pub fn should_log(entries: &[UsageEntry], song_id: Uuid, service_id: Uuid, now: DateTime<Utc>) -> bool`: walk this service's entries newest first, starting from `prev = now`; stop at the first entry more than 45 minutes older than `prev` (a negative gap counts as within); `false` if the song appears before stopping; each visited entry becomes `prev`.
- `pub struct SongInfo { pub id: Uuid, pub title: String, pub ccli: String, pub writers: String, pub copyright: String }` (FEAT-4's `SongRef` maps to it in Part B; `ccli: None` → `""`).
- `pub struct HitCheck { pub enabled: bool, pub is_song_text: bool, pub room_shows_words: bool, pub stream_enabled: bool, pub stream_active: bool, pub stream_shows_words: bool }` and `pub fn hit(c: &HitCheck) -> Option<bool /*streamed*/>`: `None` unless enabled, a song with a text slide, and words visible in the room or on an enabled, active stream that shows words; `Some(streamed)` where `streamed = stream_enabled && stream_active && stream_shows_words`.
- `pub struct UsageHit { pub song: SongInfo, pub service_id: Uuid, pub service_name: String, pub streamed: bool, pub at: DateTime<Utc>, pub local_date: NaiveDate }` and `pub fn record(log: &mut UsageLog, h: UsageHit) -> bool`, which re-checks `should_log` and appends with a new id. This is the `usage-log` thread's write in B8.
- `pub fn remove(log: &mut UsageLog, id: Uuid)` (unknown id is fine); `pub fn mark_reported(log: &mut UsageLog, through: NaiveDate)`.
- `pub fn prune(log: &mut UsageLog, today: NaiveDate) -> bool` keeps `local_date >= today - 36 months`; `pub fn after_load(log: &mut UsageLog, today: NaiveDate) -> bool` = `prune`. **Deliberate deviation from spec §4:** the spec's hook is `usage::after_load(&mut UsageLog) -> bool`; this one also takes `today` so it stays pure (pure code never calls the clock). FEAT-1's `Data::after_load` passes `Local::now().date_naive()` (Task B1).
- Ranges: `pub fn default_range(reported_through: Option<NaiveDate>, today: NaiveDate) -> (NaiveDate, NaiveDate)` (From = day after `reported_through`, capped at `today`; else `today − 3 months`; To = today); `pub fn last_months(today, n: u32) -> (NaiveDate, NaiveDate)`; `pub fn this_year(today) -> (NaiveDate, NaiveDate)`; `pub fn validate_range(from, to) -> Result<(), &'static str>` (`from > to` → `BAD_RANGE`).

- [ ] **Step 1: Write the failing tests** (helper `entry(song, service, "2026-11-01T09:40:00Z")`)

```rust
#[test] fn usage_dates_serialise_as_naive_date_and_rfc3339() {
    let e = entry(A, S, "2026-11-01T09:40:00Z");
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!((v["at"].as_str(), v["local_date"].as_str()), (Some("2026-11-01T09:40:00Z"), Some("2026-11-01")));
    let back: UsageEntry = serde_json::from_value(v).unwrap(); assert_eq!(back, e);
    let log: UsageLog = serde_json::from_str(r#"{"reported_through":"2026-07-31"}"#).unwrap();
    assert_eq!(log.reported_through, NaiveDate::from_ymd_opt(2026, 7, 31));
    assert!(serde_json::from_str::<UsageLog>("{}").unwrap().entries.is_empty());
    assert!(serde_json::from_str::<UsageSettings>("{}").unwrap().enabled);
}
#[test] fn logs_once_per_song_per_session() { let l = vec![entry(A, S, "2026-11-01T09:00:00Z")]; assert!(!should_log(&l, A, S, t("2026-11-01T09:05:00Z"))); assert!(should_log(&l, B, S, t("2026-11-01T09:05:00Z"))); }
#[test] fn refresh_and_jumps_do_not_log_twice() { let mut log = UsageLog::default(); for m in [0, 1, 2] { record(&mut log, hit_at(A, S, m)); } assert_eq!(log.entries.len(), 1); }
#[test] fn same_service_reused_after_gap_logs_again() { let l = vec![entry(A, S, "2026-11-01T09:40:00Z")]; assert!(should_log(&l, A, S, t("2026-11-01T11:20:00Z"))); }
#[test] fn reprise_within_session_not_logged() {
    let l = vec![entry(A, S, "2026-11-01T09:00:00Z"), entry(B, S, "2026-11-01T09:10:00Z"), entry(C, S, "2026-11-01T09:40:00Z")];
    assert!(!should_log(&l, A, S, t("2026-11-01T10:20:00Z")), "chain 10:20→9:40→9:10→9:00 has no gap over 45 min");
    assert!(should_log(&l, A, S2, t("2026-11-01T10:20:00Z")), "another service has its own session");
}
#[test] fn session_survives_restart() { let l: Vec<UsageEntry> = serde_json::from_str(&serde_json::to_string(&vec![entry(A, S, "2026-11-01T09:00:00Z")]).unwrap()).unwrap(); assert!(!should_log(&l, A, S, t("2026-11-01T09:30:00Z"))); }
#[test] fn future_entry_counts_as_same_session() { let l = vec![entry(A, S, "2026-11-01T12:00:00Z")]; assert!(!should_log(&l, A, S, t("2026-11-01T11:00:00Z"))); }
#[test] fn does_not_log_when_words_hidden_everywhere() { assert_eq!(hit(&HitCheck { room_shows_words: false, stream_enabled: true, stream_active: true, stream_shows_words: false, ..song_check() }), None); }
#[test] fn does_not_log_non_song_items() { assert_eq!(hit(&HitCheck { is_song_text: false, ..song_check() }), None); assert_eq!(hit(&HitCheck { enabled: false, ..song_check() }), None); }
#[test] fn streamed_flag_only_when_stream_active_and_showing_words() {
    assert_eq!(hit(&song_check()), Some(false));
    assert_eq!(hit(&HitCheck { stream_enabled: true, stream_active: true, stream_shows_words: true, ..song_check() }), Some(true));
    assert_eq!(hit(&HitCheck { stream_enabled: true, stream_active: false, stream_shows_words: true, ..song_check() }), Some(false));
    assert_eq!(hit(&HitCheck { room_shows_words: false, stream_enabled: true, stream_active: true, stream_shows_words: true, ..song_check() }), Some(true), "stream only still counts");
}
#[test] fn prune_keeps_three_years() {
    let mut log = UsageLog { entries: vec![entry_on(A, "2023-10-31"), entry_on(B, "2023-11-01")], ..Default::default() };
    assert!(prune(&mut log, d("2026-11-01"))); assert_eq!(log.entries.len(), 1); assert_eq!(log.entries[0].song_id, B);
}
#[test] fn mark_reported_moves_default_from_date() {
    let mut log = UsageLog::default();
    assert_eq!(default_range(log.reported_through, d("2026-11-01")), (d("2026-08-01"), d("2026-11-01")));
    mark_reported(&mut log, d("2026-09-30"));
    assert_eq!(default_range(log.reported_through, d("2026-11-01")), (d("2026-10-01"), d("2026-11-01")));
}
#[test] fn default_range_never_inverted() {
    assert_eq!(default_range(Some(d("2026-11-01")), d("2026-11-01")), (d("2026-11-01"), d("2026-11-01")));
    assert_eq!(default_range(Some(d("2027-01-01")), d("2026-11-01")), (d("2026-11-01"), d("2026-11-01")));
    assert_eq!(validate_range(d("2026-11-02"), d("2026-11-01")), Err(strings::BAD_RANGE));
    assert_eq!(this_year(d("2026-11-01")).0, d("2026-01-01"));
    assert_eq!(last_months(d("2026-08-31"), 6).0, d("2026-02-28"));
}
#[test] fn remove_unknown_id_is_ok() { let mut log = UsageLog::default(); remove(&mut log, Uuid::new_v4()); }
```

- [ ] **Step 2: Run** `& $cargo test --release --lib usage` → FAIL.
- [ ] **Step 3: Implement** `src/usage/mod.rs` below its `#[path]` strings line. Use `NaiveDate::checked_sub_months(Months::new(n))` (it clamps to the month's end). No other file changes.
- [ ] **Step 4: Run** `& $cargo test --release --lib usage` → PASS.
- [ ] **Step 5: Commit** `feat(usage): song use log, 45-minute usage session and report ranges`

### Task A9: Report rows, CSV and printable HTML

**Files:**
- Create: `src/usage/export.rs`
- Modify: `src/usage/mod.rs` (`mod export; pub use export::{report, ReportRow, to_csv, to_html};`)

**Interfaces:**
- Produces:
  - `#[derive(Serialize, Clone, Debug, PartialEq)] pub struct ReportRow { pub title, pub ccli, pub writers: String, pub shown: u32, pub streamed: u32, pub last_used: NaiveDate }`
  - `pub fn report(log: &[UsageEntry], from: NaiveDate, to: NaiveDate, include_no_ccli: bool) -> Vec<ReportRow>`: entries with `from <= local_date <= to`; group key is the trimmed `ccli` when non-empty, else `song_id`; title, ccli and writers come from the group's newest entry; `shown` = entries, `streamed` = entries with `streamed`; `last_used` = max `local_date`; rows with empty ccli are dropped unless `include_no_ccli`; sorted by lower-cased title, then ccli.
  - `pub fn to_csv(rows: &[ReportRow]) -> String`: UTF-8 BOM, then the header `Title,CCLI Song #,Writers,Times shown,Times streamed,Last used` as the very first line (spec §5.4; no license preamble, so "first row is header" imports in Excel, LibreOffice and CCLI tooling work; the license number is on the printable page and in the dialog header), one line per row, `\r\n` line ends, dates ISO. A cell is quoted when it contains `,` `"` `\r` `\n` or starts or ends with a space; `"` doubles. A text cell starting with `=`, `+`, `-` or `@` gets a leading `'` first.
  - `pub fn to_html(rows: &[ReportRow], license: &str, range: (NaiveDate, NaiveDate)) -> String`: a full HTML page with the title "Song use report", range line `fmt_day(from) – fmt_day(to)`, the license header or `NO_LICENSE`, a table with the five §3.3 columns, the `FOOTER` text and a print stylesheet. No `<script>` and no remote URLs. Every text is HTML-escaped (`& < > " '`). Part B passes it to FEAT-6's `report_page::write_and_open`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn report_groups_by_ccli_and_excludes_no_ccli_by_default() {
    // helper e(song, title, ccli, date, streamed) sets writers "John Newton"
    let l = vec![e(A, "Amazing Grace", "22025", "2026-10-04", false), e(A2, "Amazing Grace (My Chains)", "22025", "2026-10-11", true),
                 e(B, "Doxology", "", "2026-10-11", false), e(C, "Old", "999", "2026-01-01", false)];
    let rows = report(&l, d("2026-10-01"), d("2026-10-31"), false);
    assert_eq!(rows, vec![ReportRow { title: "Amazing Grace (My Chains)".into(), ccli: "22025".into(), writers: "John Newton".into(), shown: 2, streamed: 1, last_used: d("2026-10-11") }]);
    assert_eq!(report(&l, d("2026-10-01"), d("2026-10-31"), true).len(), 2);
}
#[test] fn csv_quotes_commas_and_quotes() {
    let csv = to_csv(&[row("Holy, Holy, Holy", "1156", "He said \"Holy\"")]);
    assert_eq!(csv, "\u{feff}Title,CCLI Song #,Writers,Times shown,Times streamed,Last used\r\n\"Holy, Holy, Holy\",1156,\"He said \"\"Holy\"\"\",1,0,2026-10-04\r\n");
}
#[test] fn csv_neutralises_formula_cells() {
    assert!(to_csv(&[row("=HYPERLINK(\"x\")", "1", "@me")]).contains("\"'=HYPERLINK(\"\"x\"\")\",1,'@me,"));
}
#[test] fn html_escapes_and_has_no_script_or_remote_urls() {
    let h = to_html(&[row("<b>Grace</b> & Truth", "1", "")], "", (d("2026-10-01"), d("2026-10-31")));
    assert!(h.contains("&lt;b&gt;Grace&lt;/b&gt; &amp; Truth") && h.contains("Oct 1, 2026 – Oct 31, 2026"));
    assert!(h.contains(strings::NO_LICENSE));
    assert!(to_html(&[], "123456", (d("2026-10-01"), d("2026-10-31"))).contains("CCLI license #: 123456"));
    assert!(!h.contains("<script") && !h.contains("http://") && !h.contains("https://"));
}
```

- [ ] **Step 2: Run** `& $cargo test --release --lib usage` → FAIL.
- [ ] **Step 3: Implement** `export.rs` (it names texts as `super::strings::…`; its tests `use super::super::strings;`).
- [ ] **Step 4: Run** `& $cargo test --release --lib usage` → PASS.
- [ ] **Step 5: Commit** `feat(usage): CCLI report rows, CSV and printable HTML`

### Task A10: Planning Center data, plan parsing and PKCE

**Files:**
- Create: `src/pco/model.rs`, `src/pco/pkce.rs`, `tests/fixtures/pco/plan_items.json` (hand-written in the JSON:API shape below; Task B13b replaces it with a recorded, anonymised real response)
- Modify: `src/pco/mod.rs` (new in A1: add `pub mod model; pub mod pkce;` and the settings types; Part A path `crate::feat7::pco`). The module is pure and uses no phase-D crate, so it is not behind the feature yet. B13b puts `auth`/`api` behind `#[cfg(feature = "planning-center")]`.

**Interfaces:**
- Produces (`pco`): `PcoAuth { #[default] #[serde(other)] None, OAuth, PersonalToken }` (`rename_all = "snake_case"`, `Clone, Copy, Debug, PartialEq, Eq`; an unknown value loads as `None`, which only means "not connected"); `PcoSettings { auth, organization: Option<String>, service_type_id: Option<String> }` (`#[serde(default)]`, derived `Default`; it has no secret fields); `PcoLink { service_type_id: String, plan_id: String, imported_at: DateTime<Utc> }` (uses `crate::feat7::usage`'s `rfc3339` helper, made `pub(crate)`). `PcoPlan` derives `Clone, Debug, PartialEq`.
- Produces (`pco::model`): `PcoItemKind { Song, Header, Media, Item }` (from `attributes.item_type`; anything unknown → `Item`); `PcoSong { title, ccli: Option<String>, author: Option<String>, copyright: Option<String> }`; `PcoArrangement { lyrics: Option<String>, sequence: Vec<String> }`; `PcoItem { id: String, title: String, kind: PcoItemKind, description: Option<String>, notes: Vec<String>, song: Option<PcoSong>, arrangement: Option<PcoArrangement> }`; `PcoPlan { id: String, title: Option<String>, date: NaiveDate }`;
  - `pub fn parse_plan_items(doc: &serde_json::Value) -> Vec<PcoItem>`: `data[]` items of `type: "Item"` in order, resolving `relationships.song.data.id` / `relationships.arrangement.data.id` / `relationships.item_notes.data[]` against `included[]` by `(type, id)`; `Song.attributes { title, ccli_number (number or string), author, copyright }`, `Arrangement.attributes { lyrics, sequence: [String] }`, `ItemNote.attributes { content }`. A missing include gives `None` and never fails.
  - `pub fn next_link(doc: &serde_json::Value) -> Option<String>` (`links.next`).
  - `pub fn pick_plans(future: Vec<PcoPlan>, past: Vec<PcoPlan>) -> Vec<PcoPlan>`: the 8 soonest future plans ascending, then the 4 most recent past plans, newest first.
- Produces (`pco::pkce`): `pub const REDIRECT_URI: &str = "http://127.0.0.1:4318/pco/callback";` `pub fn base64url(bytes: &[u8]) -> String` (no padding); `pub fn verifier_from(entropy: &[u8; 32]) -> String` (43 chars); `pub fn challenge_s256(verifier: &str) -> String` (`sha2::Sha256`); `pub fn authorize_url(client_id: &str, state: &str, challenge: &str) -> String` (`https://api.planningcenteronline.com/oauth/authorize?client_id=…&redirect_uri=<percent-encoded>&response_type=code&scope=services&state=…&code_challenge=…&code_challenge_method=S256`); `pub fn callback_code(expected_state: &str, query: &str) -> Option<String>` (`Some(code)` only when `state` matches in constant time and `code` is non-empty); `pub fn needs_refresh(expires_at: DateTime<Utc>, now: DateTime<Utc>) -> bool` (within 5 minutes).

- [ ] **Step 1: Write the failing tests**

```rust
// pkce.rs
#[test] fn pkce_challenge_matches_rfc7636_example() {
    assert_eq!(challenge_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    assert_eq!(verifier_from(&[0u8; 32]).len(), 43);
    assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
}
#[test] fn authorize_url_has_pkce_and_encoded_redirect() {
    let u = authorize_url("cid", "st", "ch");
    assert!(u.starts_with("https://api.planningcenteronline.com/oauth/authorize?"));
    for p in ["client_id=cid", "redirect_uri=http%3A%2F%2F127.0.0.1%3A4318%2Fpco%2Fcallback", "response_type=code", "scope=services", "state=st", "code_challenge=ch", "code_challenge_method=S256"] { assert!(u.contains(p), "{p}"); }
}
#[test] fn callback_with_wrong_state_is_rejected() {
    assert_eq!(callback_code("abc", "code=xyz&state=abc"), Some("xyz".into()));
    assert_eq!(callback_code("abc", "code=xyz&state=abd"), None);
    assert_eq!(callback_code("abc", "state=abc"), None);
}
#[test] fn refresh_within_five_minutes() { assert!(needs_refresh(t("2026-11-01T10:04:00Z"), t("2026-11-01T10:00:00Z"))); assert!(!needs_refresh(t("2026-11-01T10:06:00Z"), t("2026-11-01T10:00:00Z"))); }
// model.rs
#[test] fn parses_items_with_song_arrangement_and_notes() {
    let doc: serde_json::Value = serde_json::from_str(include_str!("../../tests/fixtures/pco/plan_items.json")).unwrap();
    let items = parse_plan_items(&doc);
    assert_eq!(items.iter().map(|i| i.kind).collect::<Vec<_>>(), [PcoItemKind::Header, PcoItemKind::Song, PcoItemKind::Item, PcoItemKind::Media]);
    let s = &items[1];
    assert_eq!(s.song.as_ref().unwrap().ccli.as_deref(), Some("22025"));
    assert_eq!(s.arrangement.as_ref().unwrap().sequence, ["Verse 1", "Chorus", "Verse 2", "Chorus"]);
    assert_eq!(items[2].notes, ["Read slowly"]);
}
#[test] fn pick_plans_next_eight_then_four_recent() {
    let plan = |n: u32, date: &str| PcoPlan { id: format!("p{n}"), title: None, date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap() };
    // 10 future plans given out of order (Nov 8 .. Jan 10); 6 past plans oldest first (Sep 27 .. Nov 1); all dates distinct
    let future_dates = ["2027-01-10", "2026-11-08", "2026-12-27", "2026-11-15", "2027-01-03", "2026-11-22", "2026-12-20", "2026-11-29", "2026-12-13", "2026-12-06"];
    let past_dates = ["2026-09-27", "2026-10-04", "2026-10-11", "2026-10-18", "2026-10-25", "2026-11-01"];
    let future: Vec<_> = future_dates.iter().enumerate().map(|(i, d)| plan(i as u32, d)).collect();
    let past: Vec<_> = past_dates.iter().enumerate().map(|(i, d)| plan(100 + i as u32, d)).collect();
    let picked: Vec<String> = pick_plans(future, past).iter().map(|p| p.date.to_string()).collect();
    assert_eq!(picked, [
        "2026-11-08", "2026-11-15", "2026-11-22", "2026-11-29", "2026-12-06", "2026-12-13", "2026-12-20", "2026-12-27",
        "2026-11-01", "2026-10-25", "2026-10-18", "2026-10-11",
    ]);
}
// mod.rs
#[test] fn pco_settings_default_and_no_secret_fields() {
    let s: PcoSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(s.auth, PcoAuth::None);
    let j = serde_json::to_string(&s).unwrap(); assert!(!j.contains("secret") && !j.contains("token\""));
}
#[test] fn unknown_enum_values_load_as_default() {
    let s: PcoSettings = serde_json::from_str(r#"{"auth":"saml","organization":"Grace First"}"#).unwrap();
    assert_eq!((s.auth, s.organization.as_deref()), (PcoAuth::None, Some("Grace First")));
}
```

- [ ] **Step 2: Run** `& $cargo test --release --lib pco` → FAIL.
- [ ] **Step 3: Implement** `model.rs`, `pkce.rs`, the additions to `pco/mod.rs` and the fixture: one header, one song item with `include`d Song (`ccli_number: 22025`) and Arrangement (lyrics with `Verse 1`, `Chorus`, `Verse 2` sections and the sequence above), one plain item "John 3:16-18" with one ItemNote "Read slowly", one media item. Percent-encode with a 10-line `pct()` that keeps `A-Z a-z 0-9 - . _ ~`.
- [ ] **Step 4: Run** `& $cargo test --release --lib pco` → PASS.
- [ ] **Step 5: Commit** `feat(pco): Planning Center plan parsing, settings types and PKCE`

### Task A11: Planning Center mapping, preview and re-import order

**Files:**
- Create: `src/pco/map.rs`
- Modify: `src/pco/mod.rs` (`pub mod map;`)

**Interfaces:**
- Consumes: `pco::model::*`, `model::Song` (read only), `super::strings` (`strings::pco`), `crate::feat7::usage::strings::fmt_day`.
- Produces:
  - `pub fn ccli_digits(s: &str) -> Option<String>` (trimmed, all ASCII digits, non-empty); `pub fn title_key(s: &str) -> String` (lowercase, letters and digits only, words joined by one space).
  - `pub fn match_song(s: &PcoSong, lib: &[Song]) -> Option<Uuid>`: CCLI first (`ccli_digits` on both sides), then `title_key` equality, first library match wins.
  - `pub struct MapCtx<'a> { pub reference: &'a dyn Fn(&str) -> Option<String>, pub labels: &'a dyn Fn(&str) -> Vec<String>, pub label_key: &'a dyn Fn(&str) -> (String, Option<u8>) }`. `reference` returns the normalised reference only when the text has a book **and** a chapter (Part B: FEAT-5 `scripture::input`). `labels` lists a lyrics text's section labels (Part B: `slides::parse_sections`). `label_key` returns kind and number (Part B: FEAT-4 `SectionKind::from_label`).
  - `pub fn map_sequence(seq: &[String], labels: &[String], key: &dyn Fn(&str) -> (String, Option<u8>)) -> Option<Vec<String>>`: each entry → the song's own label, by exact case-insensitive match, else by equal `(kind, number)`; unmatched entries are dropped; `None` when nothing matched or `seq` is empty.
  - `#[derive(Debug, Clone)] pub enum Planned { UseSong { id: Uuid, arrangement: Option<Vec<String>> }, NewSong { song: Song, arrangement: Option<Vec<String>> }, Scripture { reference: String, translation: String }, Text { title: String }, Skip(String) }`; `#[derive(Debug, Clone)] pub struct PreviewRow { pub pco_item_id: String, pub title: String, pub action: Planned, pub checked: bool, pub enabled: bool, pub warning: Option<String> }`; `pub fn row_label(r: &PreviewRow) -> String` (the §3.4 texts from `strings::pco`). Neither derives `PartialEq`: `model::Song` does not (src/model.rs), and Part A cannot add it. Tests compare with `matches!` and field checks.
  - `pub fn preview_with(items: &[PcoItem], lib: &[Song], translation: &str, ctx: &MapCtx) -> Vec<PreviewRow>`, with the rules of spec §3.4 step 2 and §5.6. A new song takes title, author → writers, copyright, CCLI number and lyrics, with its own `arrangement` empty. Song without lyrics → `NewSong` checked with warning `ROW_NEW_SONG_NO_LYRICS`. A song item with no song record, and a plain item, → `reference(title)` then `reference(description)` → `Scripture` checked; else `Text` unchecked. Header → `Skip(ROW_HEADING)`; media → `Skip(ROW_MEDIA)`; both unchecked with `enabled: false`. The preview never fetches. Part B wraps it as the spec's `preview(items, lib, translation)`.
  - `pub fn mark_missing(rows: &mut [PreviewRow], missing: &[String], translation: &str)`: those rows get `checked: false` and warning `cant_show(translation)` (the import-time fetch failure path).
  - `pub fn merge_update<T: Clone>(old: &[T], imported: Vec<T>, pco_id: impl Fn(&T) -> Option<&str>) -> Vec<T>`: the imported items in order, plus every old item without a pco id, each placed right after the imported item with the same pco id as the nearest pco item before it in `old`; at the top when it preceded all pco items; at the end when that anchor is not in `imported`. Hand items that shared an anchor keep their old relative order.
  - `pub fn update_summary<T>(old: &[T], pco_id: impl Fn(&T) -> Option<&str>, title: impl Fn(&T) -> String) -> (usize, Vec<String>)` (replaced count, kept titles) for `strings::update_confirm` (final path `strings::pco::update_confirm`).
  - `pub fn service_name(plan: &PcoPlan, service_type: &str) -> String` = `"{fmt_day(date)} {title or service_type}"`.

- [ ] **Step 1: Write the failing tests** (fake `MapCtx`: `reference` accepts `"<Book> <n>[:<v>[-<v>]]"` for the books John, Mark, Acts, Job; `labels` uses lines that end with a digit or equal "Chorus"; `label_key` lowercases the first word and parses a trailing number)

```rust
#[test] fn matches_song_by_ccli_then_title() {
    let lib = vec![song("Amazing Grace", "22025"), song("How Great Thou Art", "")];
    assert_eq!(match_song(&pso("Amazing Grace (My Chains Are Gone)", Some("22025")), &lib), Some(lib[0].id));
    assert_eq!(match_song(&pso("how great, thou art!", None), &lib), Some(lib[1].id));
    assert_eq!(match_song(&pso("Unknown", Some("abc")), &lib), None);
}
#[test] fn new_song_takes_lyrics() {
    let rows = preview_with(&[song_item("New One", Some("Verse 1\nHello"), &[])], &[], "KJV", &ctx());
    let Planned::NewSong { song, .. } = &rows[0].action else { panic!() };
    assert_eq!((song.lyrics.as_str(), song.arrangement.len(), rows[0].checked), ("Verse 1\nHello", 0, true));
}
#[test] fn existing_song_gets_item_arrangement_not_library_change() {
    let lib = vec![song_with_lyrics("Amazing Grace", "22025", "Verse 1\na\n\nChorus\nb\n\nVerse 2\nc")];
    let rows = preview_with(&[song_item_ccli("Amazing Grace", "22025", &["Verse 1", "Chorus", "Verse 2", "Chorus"])], &lib, "KJV", &ctx());
    assert!(matches!(&rows[0].action, Planned::UseSong { id, arrangement: Some(a) } if *id == lib[0].id && a == &["Verse 1", "Chorus", "Verse 2", "Chorus"]));
    assert!(lib[0].arrangement.is_empty());
}
#[test] fn sequence_labels_matched_by_from_label() { assert_eq!(map_sequence(&["verse 1".into(), "CHORUS".into(), "V1".into()], &["Verse 1".into(), "Chorus".into()], &key()), Some(vec!["Verse 1".into(), "Chorus".into()])); }
#[test] fn unmatched_sequence_gives_none() { assert_eq!(map_sequence(&["Bridge".into()], &["Verse 1".into()], &key()), None); }
#[test] fn song_without_lyrics_warns() { let r = &preview_with(&[song_item("X", None, &[])], &[], "KJV", &ctx())[0]; assert_eq!((r.checked, r.warning.as_deref()), (true, Some(strings::ROW_NEW_SONG_NO_LYRICS))); }
#[test] fn bible_reference_item_becomes_scripture() {
    let r = &preview_with(&[plain_item("Reading", Some("John 3:16-18"))], &[], "KJV", &ctx())[0];
    assert!(matches!(&r.action, Planned::Scripture { reference, translation } if reference == "John 3:16-18" && translation == "KJV") && r.checked);
    assert_eq!(row_label(r), "Scripture: John 3:16-18 (KJV)");
}
#[test] fn book_name_without_chapter_is_not_scripture() {
    for t in ["Acts of Service", "Mark's welcome", "Job fair"] { let r = &preview_with(&[plain_item(t, None)], &[], "KJV", &ctx())[0]; assert!(matches!(r.action, Planned::Text { .. }) && !r.checked, "{t}"); }
}
#[test] fn preview_checks_reference_syntax_only() {
    let mut rows = preview_with(&[plain_item("John 99", None)], &[], "KJV", &ctx());
    assert!(rows[0].checked && rows[0].warning.is_none(), "existence is checked at Import, not here");
    mark_missing(&mut rows, &[rows[0].pco_item_id.clone()], "KJV");
    assert_eq!((rows[0].checked, rows[0].warning.as_deref()), (false, Some("KJV can't show this passage")));
}
#[test] fn headers_and_media_skipped() {
    let rows = preview_with(&[header("Worship"), media("Intro video")], &[], "KJV", &ctx());
    assert!(rows.iter().all(|r| !r.checked && !r.enabled && matches!(r.action, Planned::Skip(_))));
    assert_eq!(row_label(&rows[1]), "Skipped — add the file yourself");
}
#[test] fn hand_added_item_keeps_its_place_after_update() {
    // old: [Welcome(hand), A(p1), Video(hand), B(p2), Notes(hand after p3)] p3 gone
    let old = vec![h("Welcome"), p("A", "p1"), h("Video"), p("B", "p2"), p("C", "p3"), h("Notes")];
    let new = vec![p("B2", "p2"), p("A2", "p1")];
    let titles: Vec<_> = merge_update(&old, new, |x| x.1.as_deref()).into_iter().map(|x| x.0).collect();
    assert_eq!(titles, ["Welcome", "B2", "A2", "Video", "Notes"]);
    assert_eq!(update_summary(&old, |x| x.1.as_deref(), |x| x.0.clone()), (3, vec!["Welcome".into(), "Video".into(), "Notes".into()]));
}
```

- [ ] **Step 2: Run** `& $cargo test --release --lib pco::map` → FAIL.
- [ ] **Step 3: Implement** `map.rs` (texts as `super::strings::…`; tests `use super::super::strings;`).
- [ ] **Step 4: Run** `& $cargo test --release --lib pco` → PASS.
- [ ] **Step 5: Commit** `feat(pco): plan preview mapping, song matching and re-import ordering`

---

# PART B — Integration after Plan 3 and Plan 4 merge

> **BLOCKED** until the `native-foundation` branch has Plans 3 and 4 merged and this branch is rebased on it. Each task also names the FEAT work it needs. Where a FEAT dependency is missing when the task runs, build the task without that part (each task says which part) and leave a `// FEAT-n:` TODO with the test marked `#[ignore = "needs FEAT-n"]`. File paths are Plan 3's layout. Run the whole suite at Step 4: `& $cargo test --release`.

### Task B1: Promote modules, model fields, load fix-ups and overlay classes

**Prerequisites:** Plans 3+4; FEAT-1 (`Data::after_load`, `OverlaySlot`, `theme::find`, `theme::builtin`). FEAT-4 for `SongRef` and `src/strings/mod.rs`.

**Files:** Modify `src/lib.rs`, `src/render/mod.rs`, `src/strings/mod.rs` (create it if FEAT-4 has not), `src/feat7/mod.rs`, `src/screen_view.rs`, `src/stream/mod.rs`, `src/usage/mod.rs`, `src/pco/mod.rs`, every FEAT-7 file that names `crate::feat7::`, `src/model.rs`, `src/store.rs` (test fixture only); Test `src/model.rs`, `src/screen_view.rs`.

**Interfaces:**
- **Promotion (no file moves).** `src/lib.rs`: add `pub mod screen_view; pub mod stream; pub mod usage; pub mod pco;` next to `pub mod feat7;` (which stays until B15, see below). `src/render/mod.rs`: `pub mod compose;`. `src/strings/mod.rs`: `pub mod screen_view; pub mod stream; pub mod usage; pub mod pco;`. In the four module roots, replace the `#[path = …strings…] pub mod strings;` line with `pub use crate::strings::<area> as strings;`. Replace `crate::feat7::` with `crate::` in every FEAT-7 file (`compose` becomes `crate::render::compose`). `src/feat7/mod.rs` becomes `pub use crate::{screen_view, stream, usage, pco}; pub use crate::render::compose;` so any stray old path still compiles; Task B15 deletes it. The Part A tests then run under the spec paths unchanged.
- **Spec hook names for FEAT-1.** `screen_view::after_load(data: &mut Data) -> bool` wraps `after_load_views(&mut data.screen_views)`, the name spec §4 gives FEAT-1's `Data::after_load`. `usage::after_load(log: &mut UsageLog, today: NaiveDate) -> bool` keeps its extra `today` parameter (deliberate deviation for purity, see Task A8); the FEAT-1 hook author passes `Local::now().date_naive()`.
- `Data.screen_views: Vec<ScreenView>` and `Data.usage: UsageLog` (`#[serde(default)]`); `Settings.stream: StreamSettings`, `.usage: UsageSettings`, `.planning_center: PcoSettings`; `DisplayRoles.stream: Option<String>`; `Service.planning_center: Option<PcoLink>` and `ServiceItem.pco_item_id: Option<String>`, both `#[serde(default, skip_serializing_if = "Option::is_none")]`. `SUPPORTED_FORMAT` unchanged.
- `screen_view::{all(data), find(data, id), activate(data, v)}` wrap A2. `pub fn class_of(slot: &OverlaySlot) -> OverlayClass` (`Timer{..}` → Timer, `Message{..}` → Message, everything else including `Unknown` → Saved). `apply` filters `frame.overlays` by `v.layers.overlays.shows(class_of(s))`.
- `Data::after_load` calls `screen_view::after_load(self)`, `usage::after_load(&mut self.usage, Local::now().date_naive())` and `Settings::normalize`, which calls `stream::settings::normalize(&mut self.stream, |id| …, || random_token()[..32].into())`.

- [ ] **Step 1: Failing tests:** `old_data_json_loads_without_screen_views_or_usage` (a pre-FEAT-7 `data.json` fixture loads; `screen_views` empty, `usage.entries` empty, `settings.stream == StreamSettings::default()`, `usage.enabled`), `after_load_drops_custom_views_with_builtin_ids` (through `Data::after_load`, returns true), `apply_filters_overlays_by_class` (frame with saved, timer and message slots + Lower third → only the saved slot), `class_of_maps_unknown_source_to_saved`, `stream_lower_third_theme_is_builtin_not_in_data_themes` (`theme::builtin()` contains `STREAM_LOWER_THIRD_THEME` with position "lower", size 60, scripture 46, shadow, band opacity 55; `Data::default().themes` does not), `normalize_makes_32_hex_key_when_enabled`, `pco_item_id_absent_is_not_serialised`, `feat7_modules_reachable_at_spec_paths` (names `crate::screen_view::LOWER_THIRD`, `crate::strings::usage::fmt_day`, `crate::render::compose::blend` and `crate::feat7::screen_view::LOWER_THIRD`, so the promotion and the re-export both compile).
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Do the promotion first and run `& $cargo test --release` (all Part A tests pass at the new paths), then implement the rest of Interfaces.
- [ ] **Step 4:** `& $cargo test --release` → PASS.
- [ ] **Step 5:** Commit `feat(model): promote FEAT-7 modules; fields, load fix-ups and overlay classes`

### Task B2: Live engine — room and stream views

**Prerequisites:** B1; FEAT-2 (`commit_change`, `Change::Silent`); FEAT-6 (practice engine routing) for one test.

**Files:** Modify `src/live.rs`; Test `src/live.rs`.

**Interfaces:**
- `Live.room_view: ActiveView` (in `Snapshot` and `version`), `Live.stream_view: ActiveView`, `Live.stream_rev: u64` (outside `version`, snapshots and history); `LiveView { room_view, stream_view, stream_rev }`; `Command::SetScreenView { screen: OutputRole, view: Uuid }` (no `base`); `trait Plan { fn view(&self, id: Uuid) -> Option<ActiveView>; }` (`DataPlan`: `find` + `activate`).
- Behaviour exactly as spec §5.1 steps 1–3. `Live::new` takes the startup stream view; the room view starts as `everything()`.
- "Same view" (no bump) means an equal `ActiveView` after activation (id **and** activated `text_style`/layers), not just an equal id. So re-sending a view's id after its theme or mask was edited applies the edit; that is how B10's "Update now" works. Test `resending_view_after_theme_edit_applies_new_style`.

- [ ] **Step 1: Failing tests** (all spec §9 `live` names): `room_view_change_bumps_version_without_touching_frame_or_cursor`, `room_view_change_is_silent_and_does_not_rearm_auto`, `set_screen_view_targets_real_engine_during_practice`, `stream_view_change_does_not_stale_phone_base`, `stream_view_change_bumps_stream_rev_only`, `same_view_twice_is_ok_without_bump`, `unknown_view_id_fails_with_message` ("That screen view no longer exists."), `failed_reveal_reverts_room_view_with_frame`, `room_failure_does_not_revert_stream_view`, `room_view_resets_to_everything_on_startup`, `stream_view_restored_from_settings_on_startup`, `clear_all_with_words_hidden_still_clears_frame`, `next_with_words_hidden_advances_cursor`, `set_screen_view_has_no_base_and_dedupes_by_id`, `resending_view_after_theme_edit_applies_new_style`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `& $cargo test --release` → PASS.
- [ ] **Step 5:** Commit `feat(live): room and stream screen views with SetScreenView`

### Task B3: Screen view and stream actions in `App`

**Prerequisites:** B2; FEAT-1 (`theme/save` returns `{id, screen_views}`); FEAT-2 (macro references) for the delete confirm hint.

**Files:** Modify `src/app.rs`; Test `src/app.rs`.

**Interfaces:**
- Actions from spec §5.5: `screen-view/set` (routes to the real engine also during practice), `screen-view/save` → `{id, on_screens}` (validation from A2), `screen-view/delete` (refused while on a screen), `stream/set` → `{stream}` (enabling runs `normalize`), `stream/new-key` (`TURN_ON_FIRST` when off; bumps a `stream_key_gen: AtomicU64` that open SSE streams watch), `display/open|close` with role `"stream"` (uses `stream::monitor::check_pick`). The stream view change saves `settings.stream.view` through `edit` after the command applies. `theme/save` fills `screen_views` with views whose `text_theme` is that id. `screen-view/delete` returns `{ok, used_by_macros: bool}` for the confirm text.
- `App::new` sets the startup views (spec §5.1 "Startup").

- [ ] **Step 1: Failing tests:** `screen_view_delete_refused_while_on_screen`, `screen_view_save_reports_on_screens`, `stream_view_set_persists_settings_view`, `stream_new_key_refused_when_off`, `display_open_stream_refuses_room_monitor`, `theme_save_lists_screen_views_using_it`, `settings_save_merges_and_keeps_stream_changed_meanwhile`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `& $cargo test --release` → PASS.
- [ ] **Step 5:** Commit `feat(app): screen view and livestream actions`

### Task B4: Room output honours the room view

**Prerequisites:** B2; FEAT-1 room `Kind::Overlays`.

**Files:** Modify `src/room.rs`, `src/ui/room.rs`, `ui/room.slint` (`in property <bool> media-visible`); Test `src/room.rs`.

**Interfaces:** `RoomState::plan` reads `screen_view::apply(&view.frame, &view.room_view, slide_is_deck)`. `room::Layers::of` includes the mask and `text_style` in its keys. Hidden media keeps its player; `media-visible: false` hides the mpv underlay (GL) or `video-frame` (software). `pub room_preparing: Arc<AtomicBool>` is set around prepare.

- [ ] **Step 1: Failing tests:** `stream_view_change_prepares_nothing_in_room`, `room_view_hiding_media_keeps_player_and_reports`, `room_view_reveal_prepares_only_revealed_layer`, `room_prepare_not_delayed_by_stream_render`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `& $cargo test --release` → PASS. Manual: room to No words during a video; the sound continues.
- [ ] **Step 5:** Commit `feat(room): apply the room screen view; hidden media keeps playing`

### Task B5: Full compositor with goldens

**Prerequisites:** A7; FEAT-1 (`render::background::background_pixels`, `render::overlay::draw`, slide theme rendering); FEAT-5 (`second`) for the primary-only rule.

**Files:** Modify `src/render/compose.rs`; Create `src/render/image_cache.rs` (64 MB LRU of decoded assets at canvas size); golden PNGs under `tests/golden/compose/`.

**Interfaces:** `pub fn compose(input: &StreamInput, canvas: (u32, u32), fonts: &mut Fonts, images: &ImageCache) -> Rendered`. It builds `Layer`s in the room's order (background, still, slide, logo, overlays) from the shared per-layer functions and calls `blend`. With `input.primary_only` it renders the slide without FEAT-5's `second`.

- [ ] **Step 1: Failing tests** (spec goldens): `compose_lower_third_transparent`, `compose_full_view_with_image_background`, `compose_words_only_over_transparent`, `compose_message_over_logo`, `compose_matches_room_layers_for_same_frame`, `lower_third_stream_drops_second_translation`, `custom_lower_style_view_keeps_both_translations`.
- [ ] **Step 2:** Run → FAIL. Generate goldens once with `RP_BLESS=1`, inspect them by eye, then commit them.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `& $cargo test --release` → PASS. Add an `#[ignore]` timing test that prints the 1080p compose + `to_png` time (target under 60 ms on the oldest laptop; manual check).
- [ ] **Step 5:** Commit `feat(render): stream compositor from the room's layer functions`

### Task B6: `StreamOutput` thread

**Prerequisites:** B2, B4, B5; FEAT-2 `Update::Timers`; FEAT-6 (practice updates are separate).

**Files:** Modify `src/stream/mod.rs` (add `pub mod output;` and re-export `StreamOutput`), `src/stream/sink.rs` (add `BrowserSourceSink`); Create `src/stream/output.rs`; Modify `src/app.rs` (start it in `App::new` with the `BrowserSourceSink` and, once B9a lands, the `WindowSink`; expose `stream_conns`, `stream_frames: Arc<Mutex<FrameStore>>` and the sink for B7's routes). Test `src/stream/sink.rs`, `src/stream/output.rs`.

**Interfaces:**
- `StreamOutput::start(app: &Arc<App>, sinks: Vec<Arc<dyn StreamSink>>) -> StreamOutput` subscribes to `App::subscribe()`, builds `StreamInput` with `source_frame` + `stream_frame` + timer texts, runs `RenderGate` with `active = sinks.iter().any(|s| s.active())`, waits on `room_preparing` (at most 2 s), then renders on the `stream-render` thread (`compose` + `to_png`), numbers the frame (`n` from 1, one per render) and calls `frame(n, …)` on each sink. It never touches `FrameStore` itself. A render failure is logged; it keeps the last frame and toasts `RENDER_FAILED` at most once a minute.
- Spec §6.4 `BrowserSourceSink { frames: Arc<Mutex<FrameStore>>, conns: StreamConns, notify: broadcast::Sender<u64> }` implements `StreamSink`: `frame(n, rgba, png)` pushes into the `FrameStore` (debug-asserting that the returned number equals `n`) and sends `n` on the broadcast channel; `active()` = `conns.count() > 0`; `subscribe() -> broadcast::Receiver<u64>` is what B7's `/events` route reads. Use `tokio::sync::broadcast` if Plan 4's server runs on tokio, else its own channel type; the contract is "one push, every open stream sees `n`".

- [ ] **Step 1: Failing tests:** `browser_source_sink_pushes_and_notifies` (`active()` false with no guard and true while a `ConnGuard` lives; after `frame(1, …)` a subscriber receives `1` and `frames.newest().n == 1`), `stream_ignores_practice_updates`, `stream_timer_text_follows_update_timers`, `stream_renders_once_per_change_end_to_end` (fake sink counts frames), `render_failure_keeps_last_frame`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `& $cargo test --release` → PASS.
- [ ] **Step 5:** Commit `feat(stream): stream render thread following the real room`

### Task B7: Browser source routes

**Prerequisites:** B3, B6.

**Files:** Modify `src/server.rs`; Test `src/server.rs`.

**Interfaces:**
- The routes read the `FrameStore` and `BrowserSourceSink::subscribe()` that B6 put on `App`.
- `GET /stream/{key}` → `stream::page::PAGE`; `GET /stream/{key}/events` → SSE (`sse_frame`, `SSE_PING` every 15 s, holds a `ConnGuard`, closes when `stream_key_gen` changes); `GET /stream/{key}/frame/{n}.png` (`Cache-Control: no-store`, unknown `n` → newest). Outside `local_ok`; 404 unless `stream.enabled && key_matches`. `GET /pco/callback` (behind `local_ok`) is added in B13b, branch (a) only.

- [ ] **Step 1: Failing tests:** `stream_routes_404_when_disabled_or_wrong_key`, `stream_frame_returns_newest_for_unknown_number` (HTTP level), `stream_routes_accept_no_commands`, `connected_while_sse_open_without_new_frames`, `new_key_closes_open_streams`, `remote_cannot_send_set_screen_view`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `& $cargo test --release` → PASS. Manual: an OBS browser source on the same computer and over Wi-Fi shows a transparent lower third; "Connected" stays during a 30-minute single slide.
- [ ] **Step 5:** Commit `feat(server): livestream browser source routes`

### Task B8: Usage log writer and report actions

**Prerequisites:** B1, B2; FEAT-4 `LiveItem.song`; FEAT-6 `src/report_page.rs` (build it here if FEAT-6 has not landed: `write_and_open(store, stem, html) -> Result<PathBuf, String>`, as the FEAT-6 spec's `report_page` section defines it).

**Files:** Modify `src/app.rs`; Create `src/usage/writer.rs` (the `usage-log` thread).

**Interfaces:** `App::room_event` (after an accepted `Committed(v)`, or after a command with no room) builds `HitCheck` + `UsageHit` and sends it on an `std::sync::mpsc` channel. The `usage-log` thread calls `usage::record` under the data lock through `App::edit`. A failed save keeps the hit in memory and retries on the next save. Actions `usage/report`, `usage/remove`, `usage/mark-reported`, `usage/export` (`csv` with path → `to_csv` then `write_atomic`; `html` without path → `report_page::write_and_open`; errors `export_failed(reason)`), all off the command path.

- [ ] **Step 1: Failing tests:** `usage_write_happens_off_the_command_path` (a blocked data lock does not delay `command`), `logs_on_commit_not_on_request`, `usage_report_action_rejects_inverted_range`, `usage_export_csv_writes_file`, `usage_export_failure_message`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `& $cargo test --release` → PASS.
- [ ] **Step 5:** Commit `feat(usage): log song use off the command path; report and export actions`

### Task B9a: Stream window, `WindowSink` and the `stream` screen role

**Prerequisites:** B3, B6; Plan 3's `NativeScreens` and room window.

**Files:** Create `ui/stream.slint` (`StreamWindow`: `key-color` background, two `Image` faces, 400 ms crossfade, no cursor), `src/ui/stream.rs` (`WindowSink`); Modify the `NativeScreens` file from Plan 3 (the `"stream"` role), `src/app.rs` / `src/bin/native.rs` startup (pass the `WindowSink` to `StreamOutput::start`; reopen at startup). Test `src/ui/stream.rs`, `tests/ui_operator_flows.rs`.

**Interfaces:**
- `WindowSink` implements `StreamSink`: `frame` hands the `Rendered` to the Slint thread with `slint::invoke_from_event_loop` and sets the hidden face with `Image::from_rgba8_premultiplied`, then flips faces; `active()` = the stream window is open. It never blocks the stream thread on the Slint thread.
- `NativeScreens` role `"stream"`: open on a monitor through `display/open` (A4's `check_pick` refuses the room or stage monitor); closes when the monitor is unplugged and reopens when it returns, like the room.
- Startup: `stream::monitor::startup_pick(&settings.stream, saved, connected, room)`: `Open(m)` opens the window, `SameAsRoom` raises the `SAME_AS_ROOM` notice and opens nothing, `NotConnected` waits for the monitor, `Nothing` does nothing.

- [ ] **Step 1: Failing tests:** `window_sink_inactive_until_window_opens` (unit), UI `stream_window_opens_on_picked_monitor_with_key_color`, `stream_window_reopens_at_startup_when_enabled` (fake monitor list), `stream_window_same_as_room_raises_notice`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `& $cargo test --release` → PASS. Manual: an ATEM Mini over HDMI with a luma key on black; unplug the stream monitor and plug it back.
- [ ] **Step 5:** Commit `feat(ui): stream window and the stream screen role`

### Task B9b: Screens dialog — Shows pickers and Livestream card

**Prerequisites:** B3, B7, B9a, **B10** (run B10 before this task: the Edit screen views… button opens B10's dialog); Plan 3 Task 19 (Screens dialog).

**Files:** Modify `ui/dialogs/screens.slint`, `src/ui/dialogs/screens.rs`, `ui/theme.slint` (tokens `chip-warn-bg`, `chip-warn-text`); Create `ui/dialogs/screens-stream.slint`. Test `tests/ui_operator_flows.rs`.

**Interfaces:** element ids `ScreensDialog::room-shows`, `::stream-shows`, `::edit-screen-views`, `::stream-mode`, `::stream-copy`, `::stream-copy-local`, `::stream-new-key`, `::stream-monitor`, `::stream-key-color`, `::stream-status`. `default_pick`/`check_pick` come from A4. All labels come from `strings::stream` and `strings::screen_view`. The status chip shows "Connected" when `stream_conns > 0`. `FIREWALL_NOTE` appears when nothing has connected 2 minutes after enabling. The **Edit screen views…** button (spec §3.1) opens the Screen views dialog from Task B10 (`DialogKind::ScreenViews`).

- [ ] **Step 1: Failing UI tests:** `screens_dialog_picks_room_view_and_chip_appears`, `livestream_card_copy_address_and_new_address_confirm`, `livestream_card_refuses_room_monitor`, `edit_screen_views_button_opens_dialog`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `& $cargo test --release` → PASS.
- [ ] **Step 5:** Commit `feat(ui): livestream card and Shows pickers in Screens`

### Task B10: Live chip, Screen views dialog, preview applies the room view

**Prerequisites:** B3; Plan 3 live column and `src/ui/preview.rs`; FEAT-1's Themes dialog for the theme-edit ask (build the rest without it and mark that test `#[ignore = "needs FEAT-1"]`).

**Files:** Modify `ui/operator/live.slint`, `src/ui/operator/live.rs`, `src/ui/operator/view.rs` (`pub fn room_view_chip(view: &LiveView) -> Option<String>`, `pub fn theme_update_targets(screen_views: &[(Uuid, String)], live: &LiveView, stream_on: bool) -> Vec<(String, OutputRole)>`), `src/ui/preview.rs`, `src/ui/dialogs/mod.rs` (`DialogKind::ScreenViews`), FEAT-1's Themes dialog controller (`src/ui/dialogs/themes.rs` or wherever FEAT-1 put it); Create `ui/dialogs/screen-views.slint`, `src/ui/dialogs/screen_views.rs`.

**Interfaces:**
- The chip sits inside the monitor's top edge (no extra height) and uses the `notice` colours when the view hides everything. The "Change" menu sends `screen-view/set`.
- The Screen views dialog has New, Duplicate, Rename and Delete (confirm + `DELETE_MACRO_NOTE` when `used_by_macros`), the checkboxes, More options → Text style, a 16:9 sample preview, and Save → "Update now?" (`update_screen_ask`) when `on_screens` is not empty. When a view's `text_theme` names a theme that no longer exists, the Text style row shows the `THEME_DELETED` note (spec §5.8).
- **Theme edited while on a screen (spec §5.8).** After a `theme/save` in FEAT-1's Themes dialog, `theme_update_targets` keeps the returned `screen_views` that `screens_showing` puts on a screen. For each one the dialog asks `theme_update_ask(name, on_room)` with [`UPDATE_NOW`] [`LATER`]; Update now sends `screen-view/set` for that screen with the same view id, which re-activates it with the edited theme. Later changes nothing.
- Logo F5 with a logo-hiding room view toasts `LOGO_HIDDEN`.

- [ ] **Step 1: Failing tests:** `room_view_chip_hidden_for_everything` (pure, `view.rs`), `theme_update_targets_only_views_on_screens` (pure, `view.rs`), UI `chip_change_menu_sets_everything_and_chip_hides`, `editing_active_view_asks_before_updating_screen`, `editing_theme_used_on_screen_asks_before_updating` (Update now sends `screen-view/set` and the stream's `text_style` changes; Later leaves it), `text_style_hidden_under_more_options`, `deleted_text_theme_shows_note`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `& $cargo test --release` → PASS.
- [ ] **Step 5:** Commit `feat(ui): room view chip, Screen views dialog, preview applies the view`

### Task B11: More menu, Song use report dialog, Settings → Songs

**Prerequisites:** B8; Plan 3 Task 20 (Settings) and the top bar; FEAT-1 Settings merge rule.

**Files:** Modify `ui/operator/topbar.slint` (`more-btn`, icon-only below 1500 px), `ui/dialogs/settings.slint`, `src/ui/dialogs/settings.rs`, `src/ui/dialogs/mod.rs` (`DialogKind::Usage`); Create `ui/dialogs/usage.slint`, `src/ui/dialogs/usage.rs`.

**Interfaces:** element ids `OperatorWindow::more-btn`, `UsageDialog::from`, `::to`, `::since-last`, `::include-no-ccli`, `::export-csv`, `::open-printable`, `::mark-reported`, `::details`. The CSV goes through an `rfd` save dialog with `csv_file_name`. Settings Save sends only `usage.enabled` and `church_ccli` from the Songs tab. The report link asks `SAVE_FIRST_ASK` when Settings has unsaved changes.

- [ ] **Step 1: Failing UI tests:** `usage_dialog_defaults_since_last_report`, `usage_remove_entry_asks_then_removes`, `settings_edits_survive_opening_usage_report`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `& $cargo test --release` → PASS. Manual: the CSV opens in Excel and LibreOffice; the printable page prints.
- [ ] **Step 5:** Commit `feat(ui): song use report dialog and More menu`

### Task B12: Phone remote and macro step

**Prerequisites:** B2; Plan 4's remote; FEAT-2 (`MacroStep::ScreenView`, §5.11).

**Files:** Modify `src/app.rs` (`remote_view` gains `"room_view": name | null`), the phone remote page that Plan 4 keeps, and FEAT-2's macro runner (map `MacroStep::ScreenView { screen, view }` → `SetScreenView`; resolve before running, `macro_view_deleted` when gone).

- [ ] **Step 1: Failing tests:** `remote_view_shows_room_view_name_or_null`, `macro_with_deleted_view_runs_nothing` (FEAT-2), `phone_macro_screen_view_step_obeys_base_checks` (FEAT-2).
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `& $cargo test --release` → PASS.
- [ ] **Step 5:** Commit `feat(remote): show the room view on the phone; macro screen view steps`

### Task B13a: Feasibility check P1 (human: Jake) — gate for phase D

**Prerequisites:** none in code; it can run any time before B13b. **A subagent cannot do this task**: it needs a Planning Center developer app registration and sign-in runs against a real church account on three machines.

**Files:** Create `docs/superpowers/checks/2026-xx-xx-feat7-p1-planning-center-oauth.md`.

- [ ] **Step 1:** Jake registers a Planning Center developer OAuth app (api.planningcenteronline.com/oauth/applications) with redirect `http://127.0.0.1:4318/pco/callback` and scope `services`, and notes whether the app can be a public client (no secret).
- [ ] **Step 2:** On Windows, macOS and Linux, run an authorization-code + PKCE (S256) sign-in without a client secret (a throwaway script, or `curl` for the token exchange using `pco::pkce` values). Record per OS: did the browser return to the loopback URL; did the token exchange succeed without a secret; did the refresh token work.
- [ ] **Step 3:** Record the outcome in the checks file as exactly one of: **(a) OAuth works** (PKCE public client, loopback redirect accepted on all three) or **(b) personal tokens only** (any of the above fails). Include the date and the Planning Center docs version read.
- [ ] **Step 4:** Commit `docs(checks): FEAT-7 P1 Planning Center OAuth feasibility`

### Task B13b: Planning Center connection (phase D, feature `planning-center`)

**Prerequisites:** B1, A10, **B13a recorded**. The task has two branches; take the one B13a recorded.

**Files (both branches):** Modify `Cargo.toml` (`[features] planning-center = [...]` with each crate pinned `=x.y.z` at task time: branch (a) `["dep:ureq", "dep:keyring", "dep:open"]`, branch (b) `["dep:ureq", "dep:keyring"]`; `ureq` with rustls), `src/pco/mod.rs` (`#[cfg(feature = "planning-center")] pub mod auth; pub mod api;`), `src/app.rs` (actions `pco/token`, `pco/disconnect`, `pco/service-types`, `pco/plans`, `pco/preview`; branch (a) also `pco/connect`); Create `src/pco/auth.rs` (token storage: keyring service `RustyPresenter`, account `planning-center`; fallback file `secrets/planning-center.json`, owner-only), `src/pco/api.rs` (`trait PcoApi`, `UreqApi`, 20 s timeout, `links.next`, 429 `Retry-After` up to 3 times), and replace `tests/fixtures/pco/plan_items.json` with a recorded, anonymised response (the include list becomes `song,arrangement,item_notes`; spec §5.6 names only `song,arrangement`, but item notes need `item_notes`). **Branch (a) only:** `src/server.rs` (`GET /pco/callback`, behind `local_ok`), the OAuth flow in `auth.rs` (PKCE from `pco::pkce`, state check, 5-minute timeout, token refresh within 5 minutes of expiry).

- [ ] **Step 1: Failing tests (both branches):** `secrets_never_serialized_into_data_json` (connect with a fake keyring; `data.json`, a backup and `state_json` contain no token), `token_verify_failure_message`, `api_follows_next_links_and_retries_429`, `disconnect_removes_stored_secret`. **Branch (a) adds:** `pco_callback_rejects_non_loopback_and_wrong_state`, `refresh_failure_signs_out`, `port_busy_message_when_not_on_4318`, `sign_in_times_out_after_five_minutes`.
- [ ] **Step 2:** Run with `--features planning-center` → FAIL.
- [ ] **Step 3:** Implement the branch's scope. In branch (b), `PcoAuth::OAuth` stays in the enum (data written by a later build still loads), but nothing sets it, and B14b shows no Connect button.
- [ ] **Step 4:** `& $cargo test --release --features planning-center` and `& $cargo test --release` (feature off) both → PASS.
- [ ] **Step 5:** Commit `feat(pco): Planning Center connection behind the planning-center feature`

### Task B14a: `service/import-pco` action

**Prerequisites:** B13b, A11; FEAT-4 (`ItemKind::Song.arrangement`, `SectionKind::from_label`); FEAT-5 (`scripture::input`, `scripture::fetch`).

**Files:** Modify `src/app.rs` (`service/import-pco`), `src/pco/map.rs` (`pub fn preview(items, lib, translation)` = `preview_with` with the real `MapCtx`). Test `src/app.rs`.

**Interfaces:** `service/import-pco {service_type_id, plan_id, rows, target: "new" | {"update": id}}`: first fetches every checked scripture row with `scripture::fetch` on the blocking pool; any failure → `mark_missing` + `SOME_NOT_FOUND`, nothing saved. Otherwise one `edit` creates the new songs and the service (named by `service_name`) or updates it with `merge_update`, and stores `Service.planning_center` (`PcoLink`). Every item gets `pco_item_id` and notes. It also remembers `settings.planning_center.service_type_id` = the request's `service_type_id` (spec §3.4), in the same `edit`. The service opens only when nothing is live; otherwise `imported_toast`. A plan with no importable rows is refused with `NO_ITEMS`.

- [ ] **Step 1: Failing tests:** `import_pco_creates_service_and_songs_in_one_save`, `update_import_keeps_service_id_and_hand_added_items`, `hand_added_item_keeps_its_place_after_update` (app level), `import_while_live_keeps_current_service`, `import_with_missing_passage_imports_nothing_and_unchecks_row`, `import_remembers_service_type`, `import_of_empty_plan_is_refused`.
- [ ] **Step 2:** Run with `--features planning-center` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Both feature builds → PASS.
- [ ] **Step 5:** Commit `feat(pco): import a Planning Center plan as a service`

### Task B14b: Planning Center tab, import dialog and Services button

**Prerequisites:** B14a; Plan 3 Task 20 (Settings) and the Services dialog.

**Files:** Modify `ui/dialogs/services.slint` + `src/ui/dialogs/services.rs` (Import from Planning Center…), `ui/dialogs/settings.slint` (Planning Center tab), `src/ui/dialogs/mod.rs` (`DialogKind::PcoImport`); Create `ui/dialogs/settings-pco.slint`, `src/ui/dialogs/settings_pco.rs`, `ui/dialogs/pco-import.slint`, `src/ui/dialogs/pco_import.rs` (`PcoImportSession`), all `#[cfg(feature = "planning-center")]`. Test `tests/ui_operator_flows.rs` (fake `PcoApi`).

**Interfaces:**
- Settings → Planning Center tab: Connect (branch (a) of B13b only), personal token fields, Disconnect, `connected_to(org)`, the "Saved on this computer only." note. All texts from `strings::pco`.
- Services → **Import from Planning Center…**: when not connected it opens Settings on the Planning Center tab instead of the import dialog (spec §3.4).
- The import dialog preselects `settings.planning_center.service_type_id` when it is still in the service-type list; lists plans with `pick_plans`; shows `row_label` rows with checkboxes; a plan with no items shows `NO_ITEMS` with Import disabled (spec §5.8); re-import shows `reimport_ask` with Import as a new service as the primary button.
- `PcoImportSession` owns the in-flight fetches; closing the dialog drops it, which cancels them, and late results are ignored (spec §5.6).

- [ ] **Step 1: Failing UI tests:** `reimport_primary_button_is_new_service`, `pco_import_preview_checkboxes_follow_defaults`, `import_when_not_connected_opens_planning_center_tab`, `import_dialog_preselects_remembered_service_type`, `plan_with_no_items_disables_import`, `closing_import_dialog_cancels_session` (a late fake response after close changes nothing).
- [ ] **Step 2:** Run with `--features planning-center` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Both feature builds → PASS. Manual: connect, import, update and disconnect against a real Planning Center account.
- [ ] **Step 5:** Commit `feat(ui): Planning Center tab and import dialog`

### Task B15: Manual checks, docs and staging-root removal

**Prerequisites:** B1–B12 (B13a–B14b when phase D ships).

**Files:** `docs/superpowers/checks/` (new FEAT-7 checklist from spec §9 "Manual checks"), `PROJECT_PLAN.md` §10b FEAT-7 wording ("screen views", not "looks"), the FEAT-1, FEAT-2, FEAT-3 and FEAT-5 specs (spec §10 Naming: every "look"/"looks" that means a FEAT-7 screen view becomes "screen view"), `THIRD-PARTY-LICENSES` (phase D crates, through Plan 4's `cargo-about` flow); delete `src/feat7/mod.rs` and its `pub mod feat7;` line in `src/lib.rs`.

- [ ] **Step 1:** `rg -n "crate::feat7" src tests` → no hits, then delete `src/feat7/mod.rs` and the `pub mod feat7;` line. `& $cargo test --release` → PASS.
- [ ] **Step 2:** Run every spec §9 manual check on Windows, macOS and Linux, and record the results.
- [ ] **Step 3:** Update the docs listed above. Check: `rg -n -i "\blooks?\b" docs/superpowers/specs/2026-11-01-feat{1,2,3,5}-*.md PROJECT_PLAN.md` shows no hit that means a screen view (hits for the ordinary verb "looks like" stay).
- [ ] **Step 4:** Commit `docs: FEAT-7 manual checks and screen view wording; remove the feat7 staging root`
