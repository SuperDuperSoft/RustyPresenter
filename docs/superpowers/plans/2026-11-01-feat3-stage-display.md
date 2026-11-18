# FEAT-3 Stage Display Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stage layouts (five built-ins, then the church's own), stage messages from the laptop, phones and macros, the latched "current timer", per-item layouts and per-slide notes, all drawn per box by the Rust compositor, with the old stage reproduced exactly as "Classic".

**Architecture:** All logic lives in a new core module `src/stage/` (no UI, no `App`): stored types, presets, runtime state, the view builder, the notes resolver, layout references, editor geometry, the cell renderer and the cell swap/diff state machine. The functions take **explicit inputs** (`LiveFacts`, `RefSources`, `SlideKeyFacts`, `Instant`s), not `LiveView`, `Data` or `Slide`, so Part A builds and tests now without touching any file that Plans 3 and 4 are rewriting. Part B adds thin adapters from the real types (`LiveView`, `Data`, `Slide` with FEAT-4/5 fields) and wires state, actions, the stage window, the operator strip, dialogs and the phone remote into Plan 3's layout.

**Tech Stack:** Rust 2024; existing `serde`/`serde_json`, `uuid`, `chrono` (features `clock` only, so `DateTime<Utc>` gets a hand-written RFC 3339 serde module, no Cargo change), `cosmic-text 0.19.0` (`Attrs::letter_spacing` in em), `tiny-skia 0.12.0`, `image 0.25.10`; `crate::render::{Fonts, Rendered, parse_color}`; FEAT-2's crate-root modules `crate::lenient::Lenient` and `crate::timers::{TimersView, TimerSnapshot, TimerState, TimerKind, text}` (FEAT-2 plan, Plan decision 17 and Global Constraints "Final paths now": both are declared in `src/lib.rs` by FEAT-2 Tasks A1 and A2, not as `#[path]` children of `automation`). Part B: Slint 1.18 (Plan 3 `Theme`, `Ops`, dialogs, `keys.rs`), axum (phone routes).

**Spec:** `docs/superpowers/specs/2026-11-01-feat3-stage-display-design.md` (FEAT-3, rev 3). Also read where named: FEAT-2 spec §4.1, §4.4, §5.11 (`Lenient`, timers, phone allowlist) and FEAT-2 plan Global Constraints; FEAT-4 §4.1 (`SectionRef`, `color_for_label`, strings convention); FEAT-5 §4 (`VerseId`, `SlideSource`); FEAT-6 (`App::display_view()`, `Update::Practice`); FEAT-1 §6.4 (`CacheKey`), §7 (key table, Settings tabs, Live-column budget), FEAT-1 plan `Data::after_load`; Plan 3 (`docs/superpowers/plans/2026-11-01-native-operator.md`); Plan 4 Task 9 (`recovery::state::LiveState`).

**Prerequisites:**
- **Part A, before Task 1:** FEAT-2 **Task A1** is merged on this branch and `crate::lenient::Lenient` resolves (`pub enum Lenient<T> { Known(T), Unknown(serde_json::Value) }`, `Clone, Debug, PartialEq`). Check by building, not by looking for the file: `& $FC test --release --lib lenient::` must report `test result: ok` with at least 4 passed, and `src/lib.rs` must contain `pub mod lenient;`. A `src/lenient.rs` that exists but is mounted elsewhere (for example as a `#[path]` child of `automation`) fails this check: stop and reconcile the paths with FEAT-2's plan before going on.
- **Part A, before Task 8:** FEAT-2 **Tasks A2 and A3** are merged and `crate::timers::{TimersView, TimerSnapshot, TimerState, TimerKind, text}` resolve. Check: `& $FC test --release --lib timers::` passes, `src/lib.rs` contains `pub mod timers;` and `src/timers.rs` contains `pub struct TimersView`. Tasks 1–7 need only `lenient`.
- If a check fails, stop and run the FEAT-2 task that provides it; do not create `lenient.rs` or `timers.rs` here (FEAT-2 owns both).
- **Part B:** blocked until the native-foundation branch has **Plans 3 and 4** merged, plus the FEAT-2 core (`App::timers`, `Update::Timers`, macros), FEAT-4 (`SectionRef`, `sections::color_for_label`, notes-only adoption), FEAT-5 (`VerseId`, `SlideSource`), FEAT-6 (`App::display_view`, `Update::Practice`) and FEAT-1 (`CacheKey`, `Data::after_load`, key table, merge-by-key saves) pieces each task names.

## Global Constraints

- **Part A touches only new files** under `src/stage/`, `src/strings/stage.rs` and `tests/golden/stage_*.png`, plus exactly one line `pub mod stage;` in `src/lib.rs`. No edits to `src/app.rs`, `live.rs`, `model.rs`, `store.rs`, `room.rs`, `slides.rs`, `server.rs`, `src/render/**`, `src/automation/**`, `src/lenient.rs`, `src/timers.rs`, `src/ui/**`, `ui/**`, `web/**`, `src/bible*` or `Cargo.toml`.
- **FEAT-2 paths (decided with FEAT-2, its Plan decision 17).** Stage code names FEAT-2's types only as `crate::lenient::Lenient` and `crate::timers::…`. These paths are final from FEAT-2 Part A on, so Part B changes no import. Never `crate::automation::lenient` or `crate::automation::timers`.
- **Strings:** every user-visible string is a `pub const` (or a small `pub fn` for templated text) in `src/strings/stage.rs`. Until `src/strings/mod.rs` exists, `src/stage/mod.rs` includes it with `#[path = "../strings/stage.rs"] pub mod strings;`; Part B Task B1 swaps that for `pub use crate::strings::stage as strings;`. Tone: short and plain, "box", never "element"; the user-visible name is always **"Stage layout"**. Each Part B UI task lists the strings it adds to this file.
- **Forward compatibility:** every stored stage enum sits inside `crate::lenient::Lenient`. Each `StageElement` in a layout and each `SlideNote` is a `Lenient<…>`, and every such `Vec` is read through `stage::lossless::lenient_vec` (deviation 7), so a value that parses but would lose a field on write-back (an unknown field anywhere in it) is kept `Unknown`. `Unknown` round-trips meaning (equal `serde_json::Value`), not bytes. An `Unknown` element draws nothing; an `Unknown` note is kept, never shown, never an orphan. Runtime-only `MsgSource` has `#[serde(other)] Unknown`.
- **Limits (exact):** message 120 characters (counted with `chars()`, after trim); quick messages max 12, each ≤ 120; auto-hide minutes ∈ {0, 1, 5, 10}; layout name 1..=60 characters after trim; boxes per layout ≤ 24; `Text` box ≤ 200 characters; note ≤ 1000 characters; duplicate memory 256 ids; undo 50 steps; rate limit one phone `show` per second per phone.
- **Geometry (exact):** rects in % of the window, `w, h ∈ [2, 100]`, `x ∈ [0, 100 − w]`, `y ∈ [0, 100 − h]`; `max_size`/`min_size ∈ [2, 40]` with `min ≤ max` (also after kind defaults are applied, Task 2); grid 2.5 %; guide tolerance 1.0 %; minimum box 5 %; a new box is 30 % × 15 % centred.
- **Rendering (exact):** padding 1.2 % of window height; line height 1.15; weight 650 bold (the bundled 600 face) / 500; fit sizes `max − 0.4·k` (computed by multiplication, never by repeated subtraction) down to `min_size`, never dropping words, clipping at the floor; caption 2.2 % of height, letter spacing 0.12 em, `#8b93a7`, uppercase; dim alpha 0.35; red `#ff4f55`; fill radius 1 % of height; picture border 2 px `#2a303b`; message fill `#f5c66b`, text `#1a1405`, bold; stage background `#07080a`; fallback banner covers the top 16 %; layout-switch swap deadline 300 ms.
- **chrono is the only time crate.** `StageMessage.shown_at: chrono::DateTime<chrono::Utc>` serialises as RFC 3339 through `stage::rfc3339` (`to_rfc3339` / `DateTime::parse_from_rfc3339`). Monotonic deadlines (rate limit, swap) take `std::time::Instant` arguments; nothing in Part A reads a clock.
- **Nothing in FEAT-3 changes the room output**, the output version, command ids or `base`.
- **No empty tests.** Every test this plan names has concrete inputs and exact assertions: either written out in a code block, or stated in the parenthetical after its name (that parenthetical is the assertion to write). A test whose body is empty or only a comment is not allowed. Each new test must fail (or fail to compile) before its implementation exists; a test that passes at Step 2 is wrong and is fixed before Step 3.
- **Part A test command** (PowerShell). Define once per shell: `$FC = "C:\Users\jakes\AppData\Local\Temp\claude\C--Users-jakes-OneDrive-Documents-RustyPresenter\cfa9fbaf-d5ee-4157-9a05-a69be9a8108a\scratchpad\feat-cargo.cmd"`, then run `& $FC test --release --lib stage::<module>`. Every command below is written in this full `& $FC …` form. "PASS" means `test result: ok` with 0 failed.
- **Part B** uses `cargo test` (whole crate; several filters go after `--`) and, for UI tasks, Plan 3's `ui_test_init()` and element ids.

### Deviations from the spec (decided here; each is the smallest change that keeps the spec's meaning)

1. **`NoteKey::Verse` is a struct variant** `Verse { verse: VerseKey }` (JSON `{"on":"verse","verse":[42,3,16]}`), not `Verse(VerseId)`. Serde cannot write an internally tagged (`tag = "on"`) newtype variant whose content is a tuple struct (it fails at run time). Until FEAT-5 lands, `VerseKey(pub u8, pub u16, pub u16)` is a stage-local type with exactly `VerseId`'s JSON (`[book, chapter, verse]`); Task B1 makes it `pub type VerseKey = crate::scripture::VerseId;`.
2. **The cell renderer lives in `src/stage/draw.rs`**, not `src/render/stage.rs`, so Part A needs one `mod` line only. `render_cell` takes the decoded picture as an argument (`Option<&image::RgbaImage>`); Part B supplies it from the existing decode-at-display-size path. `render_view` (composite of a whole `StageView`) is added for goldens and the strip's 120 px layout thumbnails. The §6 swap rule and the cell diff are pure logic in `src/stage/swap.rs` (Task 11), driven by the stage window in B6.
3. **Shared-rect rule:** in a rect shared by several boxes, a `StageMessage` box with content always wins; otherwise the **first** non-empty box in layout order wins and the others become `Empty`. This is what spec §4.3 needs for both Classic pairs (media over timer; message over notes, §3.1).
4. **`StageState` gains two runtime fields:** `last_seq: u64` (so `seq` keeps growing after a hide) and `timer_idle: HashMap<Uuid, bool>` (the previous state of each candidate timer, for the latch).
5. **Colour validity** (`sanitise`): `render::parse_color` returns opaque white both for white and for bad input (and for `transparent`), so `color_ok(s)` is: `parse_color(s) != [255, 255, 255, 255]`, **or** `s` is a white spelling by a local check (`is_white_spelling`): the name `white`; hex `#fff`, `#ffff`, `#ffffff`, `#ffffffff` (any case); or `rgb()/rgba()/hsl()/hsla()` in comma or space syntax (optional `/ alpha`) with 3 or 4 finite numeric arguments where every rgb channel is ≥ 255 or ≥ 100 %, hsl lightness is ≥ 100 %, and alpha (if any) is ≥ 1 or ≥ 100 %. Non-opaque white (`#FFFFFF80`) already passes the first test. `transparent` is invalid and becomes `None`, which means "transparent" for `fill` anyway. No edit to `src/render`.
6. **`layout_refs`, `plan_replace` and notes take fact structs** (`RefSources`, `SlideKeyFacts`), not `&Data`/`&[Slide]`; Part B builds them.
7. **Lossless check for `Lenient` vectors.** FEAT-2's `Lenient` deserialiser makes a value `Known` whenever it parses, and serde ignores unknown fields, so a newer build's extra field (`style.shadow`, `CurrentSlide { chords }`) would parse `Known` and be dropped by the next autosave. `#[serde(deny_unknown_fields)]` is not used, because it does not cover unit variants of internally tagged enums (`{"type":"notes","x":1}` still parses). Instead `stage::lossless::lenient_vec` (a `deserialize_with` helper that still produces FEAT-2's unchanged `Lenient` type) parses each item, re-serialises the `Known` value and keeps it `Known` only if every non-null key of the input survives (recursively); otherwise the item is `Unknown(input)`. Scalar values are not compared (so `3` vs `3.0` and default-filled keys are fine). Layout-level and settings-level unknown fields are out of scope (spec §4.1 names only boxes and notes as lenient containers).
8. **Note page after a page-count round trip.** Note keys are never rewritten. A note saved on page 2 of a 2-page chorus shows on the occurrence's first page while the page count differs, and on page 2 again when the count goes back to 2. A note saved on page 1 stays on page 1 either way, which is the case spec §9.1's `note_survives_lines_per_slide_change` text describes; the test covers both.
9. **Sanitising returns whether it changed anything** (`sanitise_layout`/`sanitise_settings` → `bool`), so Part B's `Data::after_load` step (FEAT-1) can ask for one save. This is a sanitise step, not a format migration; the data format number stays unchanged (spec §4.5).
10. **`active_layout_checked`** returns the layout and the id it fell back from, so the operator can toast `layout_deleted` once (spec §5.9); `active_layout` is a thin wrapper.

### Product defaults chosen in this plan (beyond the spec)

- Box kind names and Add ▾ descriptions (listed in Task 1's strings); sample words for the editor preview (Task 1's strings).
- Delete-confirm wording for cases other than the spec's example (Task 5).
- Chip from a phone reads "On stage · 3 min · from Jake's phone".
- A for-now layout picked with nothing live clears when anything goes live (spec §5.1), and `TooFast` replies are not remembered as duplicates, so a retry after a second works.
- Duplicate layout names are checked against the built-in names too ("Classic" cannot be reused).
- **Classic's Timer box has `caption: Some(false)`.** The box is 6 % high; with the Timer kind's default caption, padding (2 × 1.2 %) and caption (2.2 %) would leave 1.4 % for text, below the 3 % floor, so a "Show on stage" timer in the default layout would clip. Band's Timer (11 %) keeps its caption (6.4 % left for text); Speaker and Clock and countdown keep theirs. Tested in Task 7 (`classic_header_cells_fit_at_min_size`).
- Duplicating a box (Ctrl/Cmd+D) offsets the copy by one grid step right and down, clamped inside the window; Tab order is layout order, wrapping.

## Review Focus

1. **A message sent while Classic shows notes.** Notes and StageMessage share Classic's rect (3,63,94,11). Expected: the message shows and the notes come back when it is hidden. Test in Task 10 (`classic_message_wins_notes_rect`).
2. **Emoji or accented text at the limits.** A 120-character message of "é" or "🙏", or a hand-edited 300-character name. Expected: counted in characters, never in bytes; truncation never splits a character or panics. Tests in Task 3 (`message_limit_counts_characters_not_bytes`) and Task 2 (`sanitise_truncates_on_char_boundaries`).
3. **A phone retries the same request inside one second.** Expected: the original result comes back (nothing re-flashes); it is never answered "Too fast — try again.". Test in Task 3 (`duplicate_checked_before_rate_limit`).
4. **A hand-edited or newer `data.json`.** NaN or negative rects, `min_size > max_size`, a colour "blurple", a box `{"type":"hologram"}`, a known box with an extra field (`{"type":"clock","blink":true}`, `style.shadow`), a note `{"on":"bar"}`. Expected: the file loads, bad numbers are clamped, the unknown or extended box and note are kept and written back with the same meaning. Tests in Task 1 (`unknown_field_in_known_box_kept_as_unknown`) and Task 2.
5. **A zero-size or tiny box** (2 % × 2 % on a 640×360 stage, or a window reported as 0×0 while minimised). Expected: rendering returns an image of at least 1×1 and never panics. Test in Task 7 (`tiny_and_zero_boxes_render_without_panic`).
6. **A "Show on stage" timer in Classic.** Expected: it takes the media-time spot, text at least 3 % of height, not clipped. Test in Task 7 (`classic_header_cells_fit_at_min_size`).

---

## PART A — Core, buildable now

### Task 1: Stage module, strings and stored types

**Files:**
- Create: `src/stage/mod.rs`, `src/stage/lossless.rs`, `src/stage/presets.rs` (id constants only in this task), `src/strings/stage.rs`
- Modify: `src/lib.rs` (add `pub mod stage;`, the only edit to an existing file in Part A)

**Interfaces:**
- Consumes: `crate::lenient::Lenient` (FEAT-2 Task A1).
- Produces, in `src/stage/mod.rs` (all `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]` unless noted), exactly as spec §4.2, §4.4, §4.5: `StageLayout` (`#[serde(default = "default_stage_bg")]` → `"#07080a"`; `elements` is `#[serde(default, deserialize_with = "lossless::lenient_vec")]`), `StageElement`, `PctRect` (`Copy`), `ElementStyle` (`Default`, `#[serde(default)]`, every field `skip_serializing_if = "Option::is_none"`), `Align { Left, Center, Right }` and `VAlign { Top, Middle, Bottom }` (`Copy, Eq, Hash`, `rename_all = "snake_case"`), `ElementKind` (`tag = "type"`, snake_case), `TimerRef` (`tag = "by"`), `StageSettings` (`#[serde(default)]`, `Default` = `{ layout: presets::CLASSIC, quick_messages: strings::DEFAULT_QUICK (six), flash: true, auto_hide_mins: 0 }`), `SlideNote`, `NoteKey` (`Eq, Hash`, `tag = "on"`: `Song { section: String, occurrence: u16, page: u16, pages: u16 }`, `Verse { verse: VerseKey }`, `Index { slide: u16 }`), `VerseKey(pub u8, pub u16, pub u16)` (`Copy, Eq, Ord, Hash`), `StageMessage { seq: u64, text: String, from: MsgSource, #[serde(with = "rfc3339")] shown_at: DateTime<Utc> }`, `MsgSource { Operator, Phone(String), Macro, #[serde(other)] Unknown }` (if serde rejects `other` on this externally tagged enum at compile time, write `Deserialize` by hand from a `Value`: the three known shapes, anything else `Unknown`), `pub mod rfc3339 { serialize, deserialize }`.
- `lossless.rs` (deviation 7): `pub fn keeps_fields(input: &serde_json::Value, output: &serde_json::Value) -> bool` (objects: every key of `input` whose value is not `null` is present in `output` and `keeps_fields` holds for it; arrays: same length and pairwise; anything else: `true`) and `pub fn lenient_vec<'de, D, T>(d: D) -> Result<Vec<Lenient<T>>, D::Error> where D: Deserializer<'de>, T: DeserializeOwned + Serialize` (reads `Vec<serde_json::Value>`; per item `Known(t)` when `from_value::<T>` succeeds and `keeps_fields(&v, &to_value(&t))`, else `Unknown(v)`; a non-array is an error, as for a plain `Vec`).
- `presets.rs`: `pub const CLASSIC: Uuid = Uuid::from_u128(0x7270_5354_0000_4000_8000_0000_0000_0001);` then `LYRICS_ONLY` …`0002`, `BAND` …`0003`, `SPEAKER` …`0004`, `CLOCK_COUNTDOWN` …`0005`.
- `src/strings/stage.rs` holds every string the Part A tasks use. Exact copy:
  - `STAGE_LAYOUT = "Stage layout"`; preset names `CLASSIC = "Classic"`, `LYRICS_ONLY = "Lyrics only"`, `BAND = "Band"`, `SPEAKER = "Speaker"`, `CLOCK_COUNTDOWN = "Clock and countdown"`.
  - Labels: `SLIDE_PICTURE = "(slide picture)"`, `PICTURE = "(picture)"`, `VIDEO = "(video)"`, `AUDIO = "(audio)"`, `LOGO = "(logo)"`, `END_OF_SERVICE = "End of service"`, `NOTHING_LIVE = "Nothing live"`, `TIMER_REMOVED = "Timer removed"`, `CAPTION_NEXT = "NEXT"`, `CAPTION_NEXT_ITEM = "NEXT ITEM"`, `PAUSED = "paused"`, `fn service_position(n, of) -> "Item {n} of {of}"`, `fn next_in(title) -> "NEXT · {title}"`.
  - Errors: `LAYOUT_GONE = "That stage layout is gone."`, `TYPE_FIRST = "Type a message first."`, `MESSAGE_TOO_LONG = "Messages can be at most 120 characters."`, `AUTO_HIDE_CHOICES = "Choose Off, 1, 5 or 10 minutes."`, `ITEM_GONE = "That item is gone."`, `NOTE_TOO_LONG = "Notes can be at most 1000 characters."`, `BUILTIN_LOCKED = "Built-in stage layouts can't be changed. Make a copy first."`, `NAME_EMPTY = "Give the stage layout a name."`, `fn name_taken(n) -> "There is already a stage layout called “{n}”."`, `TOO_MANY_BOXES = "A stage layout can have at most 24 boxes."`, `TOO_FAST = "Too fast — try again."`, `NOT_FROM_PHONE = "Not allowed from the phone."`.
  - Toasts and chips: `fn for_now(name) -> "Stage: {name} until the next item."`, `STAGE_OFF_SHOW = "The stage display is off. The message will show when it's turned on."`, `fn layout_deleted(fallback) -> "The stage layout was deleted; showing {fallback}."`, `STAGE_DISPLAY_OFF = "Stage display is off"`, `ON_STAGE = "On stage"`, `JUST_NOW = "just now"`, `fn minutes(n) -> "{n} min"`, `fn from_phone(name) -> "from {name}"`, `fn set_default_question(name) -> "Use “{name}” as the stage layout for every service?"`, `NEEDS_NEWER = "Needs a newer RustyPresenter"`, `TEXT_MAY_NOT_FIT = "Text may not fit"`.
  - `DEFAULT_QUICK: [&str; 6] = ["Last chorus", "Repeat the chorus", "Go to the bridge", "Please wrap up", "5 minutes left", "Check your microphone"]`.
  - Box kinds, `KIND_NAMES`/`KIND_HELP` in `ElementKind` order: Current slide — "the words on screen now"; Next slide — "the words coming next"; Section — "Verse, Chorus, Bridge…"; Item title — "the live item's name"; Next item — "the item after this one"; Position — "“Item 3 of 8”"; Notes — "the slide's or item's stage notes"; Clock — "the time of day"; Timer — "a countdown or count-up"; Video time left — "time left on the video or audio"; Message — "messages sent from the back"; Text — "words you type once".
  - Sample words for the editor preview when nothing is live (Task 9 `sample_facts`): `SAMPLE_TITLE = "Amazing Grace"`, `SAMPLE_SECTION = "Verse 1"`, `SAMPLE_LYRIC = "Amazing grace, how sweet the sound\nThat saved a wretch like me"`, `SAMPLE_NEXT = "I once was lost, but now am found"`, `SAMPLE_NEXT_ITEM = "How Great Thou Art"`, `SAMPLE_NOTES = "Build on the last line"`, `SAMPLE_TEXT = "Welcome"`.

- [ ] **Step 1: Write the failing tests** in `src/stage/mod.rs` (`ID`, `EL` are fixed uuid strings; `R = json!({"x":0,"y":0,"w":10,"h":10})`):

```rust
#[test] fn known_value_round_trips() {
    let l = StageLayout { id: Uuid::new_v4(), name: "Band".into(), background: "#07080a".into(), font: String::new(),
        elements: vec![Lenient::Known(StageElement { id: Uuid::new_v4(), kind: ElementKind::Timer { timer: TimerRef::Current },
            rect: PctRect { x: 3.0, y: 2.0, w: 40.0, h: 8.0 }, style: ElementStyle { align: Some(Align::Right), ..Default::default() } })] };
    assert_eq!(serde_json::from_str::<StageLayout>(&serde_json::to_string(&l).unwrap()).unwrap(), l);
}
#[test] fn unknown_element_kind_kept_as_unknown_and_written_back() {
    let raw = json!({"id": ID, "name": "X", "elements": [{"id": EL, "kind": {"type": "hologram", "beam": 3}, "rect": R}]});
    let l: StageLayout = serde_json::from_value(raw.clone()).unwrap();
    assert!(matches!(l.elements[0], Lenient::Unknown(_)));
    assert_eq!(serde_json::to_value(&l).unwrap()["elements"], raw["elements"]);
}
#[test] fn unknown_align_or_timer_ref_makes_element_unknown_not_load_error() {
    for el in [json!({"id": EL, "kind": {"type": "clock"}, "rect": R, "style": {"align": "justify"}}),
               json!({"id": EL, "kind": {"type": "timer", "timer": {"by": "next"}}, "rect": R})] {
        let l: StageLayout = serde_json::from_value(json!({"id": ID, "name": "X", "elements": [el.clone()]})).unwrap();
        assert_eq!(l.elements, vec![Lenient::Unknown(el)]);
    }
}
#[test] fn unknown_field_in_known_box_kept_as_unknown() {
    let extended = [
        json!({"id": EL, "kind": {"type": "clock", "seconds": true, "blink": true}, "rect": R}),      // struct variant + field
        json!({"id": EL, "kind": {"type": "notes", "x": 1}, "rect": R}),                              // unit variant + field
        json!({"id": EL, "kind": {"type": "item_title"}, "rect": R, "style": {"shadow": 1}}),         // ElementStyle field
        json!({"id": EL, "kind": {"type": "item_title"}, "rect": {"x":0,"y":0,"w":10,"h":10,"r":4}}), // PctRect field
        json!({"id": EL, "kind": {"type": "item_title"}, "rect": R, "locked": true}),                 // StageElement field
    ];
    for el in extended {
        let l: StageLayout = serde_json::from_value(json!({"id": ID, "name": "X", "elements": [el.clone()]})).unwrap();
        assert_eq!(l.elements, vec![Lenient::Unknown(el.clone())], "{el}");
        assert_eq!(serde_json::to_value(&l).unwrap()["elements"][0], el);
    }
    // no false positives: null style fields and integer rects stay Known
    let plain = json!({"id": EL, "kind": {"type": "item_title"}, "rect": {"x":3,"y":2,"w":44,"h":6}, "style": {"color": null}});
    let l: StageLayout = serde_json::from_value(json!({"id": ID, "name": "X", "elements": [plain]})).unwrap();
    assert!(matches!(l.elements[0], Lenient::Known(_)));
}
#[test] fn future_box_and_note_survive_unrelated_change_with_same_meaning() {
    #[derive(Serialize, Deserialize)]
    struct Doc { layouts: Vec<StageLayout>, #[serde(deserialize_with = "lossless::lenient_vec")] notes: Vec<Lenient<SlideNote>> }
    let holo = json!({"id": EL, "kind": {"type": "hologram", "beam": 3}, "rect": R});
    let bar = json!({"key": {"on": "bar", "x": 1}, "text": "later"});
    let raw = json!({"layouts": [{"id": ID, "name": "Old", "elements": [holo.clone()]}],
                     "notes": [bar.clone(), {"key": {"on": "index", "slide": 0}, "text": "hi"}]});
    let mut d: Doc = serde_json::from_value(raw).unwrap();
    d.layouts[0].name = "New".into();
    let out = serde_json::to_value(&d).unwrap();
    assert_eq!(out["layouts"][0]["name"], "New");
    assert_eq!(out["layouts"][0]["elements"][0], holo);
    assert_eq!(out["notes"][0], bar);
    assert!(matches!(d.notes[1], Lenient::Known(_)));
}
#[test] fn keeps_fields_ignores_nulls_and_scalar_differences() {
    assert!(lossless::keeps_fields(&json!({"a": 3, "b": null}), &json!({"a": 3.0, "c": 1})));
    assert!(!lossless::keeps_fields(&json!({"a": {"x": 1}}), &json!({"a": {}})));
    assert!(!lossless::keeps_fields(&json!([1, 2]), &json!([1])));
}
#[test] fn stage_settings_defaults_from_empty_object() {
    let s: StageSettings = serde_json::from_str("{}").unwrap();
    assert_eq!((s.layout, s.flash, s.auto_hide_mins, s.quick_messages.len()), (presets::CLASSIC, true, 0, 6));
}
#[test] fn note_key_json_shapes() {
    assert_eq!(serde_json::to_value(NoteKey::Verse { verse: VerseKey(42, 3, 16) }).unwrap(), json!({"on":"verse","verse":[42,3,16]}));
    assert_eq!(serde_json::to_value(NoteKey::Song { section: "chorus".into(), occurrence: 2, page: 1, pages: 2 }).unwrap(),
               json!({"on":"song","section":"chorus","occurrence":2,"page":1,"pages":2}));
    assert_eq!(serde_json::to_value(NoteKey::Index { slide: 4 }).unwrap(), json!({"on":"index","slide":4}));
}
#[test] fn message_shown_at_is_rfc3339_and_unknown_source_loads() {
    let m = StageMessage { seq: 1, text: "Hi".into(), from: MsgSource::Phone("Jake's phone".into()),
                           shown_at: Utc.with_ymd_and_hms(2026, 11, 1, 9, 5, 30).unwrap() };
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v["shown_at"], "2026-11-01T09:05:30+00:00");
    assert_eq!(serde_json::from_value::<StageMessage>(v).unwrap(), m);
    assert_eq!(serde_json::from_value::<MsgSource>(json!("Satellite")).unwrap(), MsgSource::Unknown);
}
#[test] fn empty_style_serialises_as_empty_object() { assert_eq!(serde_json::to_value(ElementStyle::default()).unwrap(), json!({})); }
```

- [ ] **Step 2: Run to verify they fail.** Run: `& $FC test --release --lib stage::tests`. Expected: FAIL to compile (`stage` module missing).
- [ ] **Step 3: Implement** the module, the types, `lossless`, `rfc3339`, the five id constants and the strings file; add `pub mod stage;` to `src/lib.rs`. `mod.rs` declares `pub mod lossless; pub mod presets;` and the `#[path]` strings module; later tasks add their own `pub mod` lines here (new file, so no conflict).
- [ ] **Step 4: Run to verify they pass.** Run: `& $FC test --release --lib stage::tests`, then `& $FC test --release --lib stage::lossless`. Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/stage/mod.rs src/stage/lossless.rs src/stage/presets.rs src/strings/stage.rs
git commit -m "feat(stage): stored stage types with forward-compatible boxes and notes"
```

### Task 2: Built-in layouts, style defaults, cell types and sanitising

**Files:**
- Create: `src/stage/cells.rs`, `src/stage/geom.rs` (sanitising only in this task)
- Modify: `src/stage/presets.rs`, `src/stage/mod.rs` (`pub mod cells; pub mod geom;`)

**Interfaces:**
- Consumes: Task 1 types and ids.
- Produces:
  - `cells.rs`: `#[derive(Clone, Debug, PartialEq)] pub struct ResolvedStyle { pub color: String, pub max_size: f32, pub min_size: f32, pub align: Align, pub valign: VAlign, pub bold: bool, pub caption: bool, pub caption_text: String, pub fill: Option<String> }`; `#[derive(Clone, Debug, PartialEq, Eq, Hash)] pub enum CellContent { Empty, Text { text: String, dim: bool, caption: Option<String>, red: bool }, Picture { asset: String, caption: Option<String> }, Message { text: String, seq: u64 } }`; `pub struct Cell { pub element: Uuid, pub rect: PctRect, pub style: ResolvedStyle, pub content: CellContent }` and `pub struct StageView { pub layout_id: Uuid, pub background: String, pub font: String, pub cells: Vec<Cell>, pub message_fallback: Option<StageMessage> }` (both `Clone, Debug, PartialEq`).
  - `presets.rs`: `pub fn builtins() -> &'static [StageLayout]` (a `LazyLock`, order Classic, Lyrics only, Band, Speaker, Clock and countdown; element ids `Uuid::from_u128(0x7270_5354_0000_4000_8001_0000_0000_0000 | (layout_no << 8) | element_no)`, both 1-based); `pub fn is_builtin(id: Uuid) -> bool`; `pub fn find_layout<'a>(church: &'a [StageLayout], id: Uuid) -> Option<&'a StageLayout>` (built-ins first; a church layout carrying a built-in id is ignored); `pub fn all_layouts(church: &[StageLayout]) -> impl Iterator<Item = &StageLayout>` (built-ins, then church layouts whose id is not built-in); `pub fn kind_defaults(kind: &ElementKind) -> ResolvedStyle`; `pub fn resolve_style(kind: &ElementKind, style: &ElementStyle) -> ResolvedStyle` (overrides each `Some` field, then `min_size = min_size.min(max_size)`, so a box with only `max_size: 2` resolves to min 2, max 2).
  - Kind defaults (spec §4.2): colour `#ffffff`, Center, Middle, bold true, min 3, fill None, caption off; `max_size` 9 CurrentSlide, 6 NextSlide, 4 ItemTitle/NextItemTitle/SectionName/ServicePosition/Notes/MediaTimeLeft, 8 Clock, 10 Timer, 6 StageMessage, 5 Text; caption on for NextSlide (`"NEXT"`), NextItemTitle (`"NEXT ITEM"`), Timer (`""`: the view fills the timer's name). Other kinds' `caption_text` default is their `KIND_NAMES` entry.
  - Preset boxes: the spec §4.3 table, in its order, with its colours, alignments and sizes. Classic: ItemTitle (3,2,44,6) `#9aa3b5` Left · SectionName (48,2,15,6) colour None, Left · MediaTimeLeft (64,2,13,6) `#ff6b6b` Right · Timer Current (64,2,13,6) `#ff6b6b` Right **`caption: Some(false)`** (Product defaults) · Clock (78,1,19,8) Right · CurrentSlide (3,10,94,52) max 9 min 3, `pictures: false, keep_after_clear: false` · Notes (3,63,94,11) `#cfe3ff` fill `#12151b` Left · StageMessage (3,63,94,11) · NextSlide (3,76,94,22) `#f5c66b` caption on "NEXT" Left Top. Speaker's CurrentSlide has `pictures: true`. Band's Timer keeps the kind default caption. All presets: background `#07080a`, font `""`.
  - `geom.rs`: `pub fn sanitise_layout(l: &mut StageLayout) -> bool`, `pub fn sanitise_settings(s: &mut StageSettings) -> bool` (both return whether anything changed, deviation 9), `pub fn color_ok(s: &str) -> bool` and `fn is_white_spelling(s: &str) -> bool` (deviation 5), `pub fn truncate_chars(s: &mut String, max: usize) -> bool`. Rules (spec §5.10 and Global Constraints): NaN/inf in `x`/`y` → 0, in `w`/`h` → 30/15; clamp `w,h` first, then `x,y`; sizes clamp to `[2, 40]`, then `min = min.min(max)` when both are set; bad `color`/`fill` → `None`; bad `background` → `"#07080a"`; `name` 60, `font` 100, `caption_text` 40, `Text.text` 200 characters; more than 24 elements → keep the first 24; `Unknown` elements untouched. Settings: unknown `auto_hide_mins` → 0, `quick_messages` → `normalize_quick` (`pub fn normalize_quick(v: Vec<String>) -> Vec<String>`: trim, drop empty, 120 chars each, first 12; Task 3 re-exports it).

- [ ] **Step 1: Write the failing tests** (test helpers: `rect_of(id, pred)` and `pos_of(id, pred)` find the first `Known` element of a built-in matching `pred`; `known_at(id, i)` returns that built-in's `i`-th element as `&StageElement`; `all_kinds()` returns one `ElementKind` of each variant in declaration order; `layout(id, name)` builds an empty church layout; `layout_with(elements)`; `el(rect, style)` and `el_kind(kind)` build a `Known` ItemTitle/`kind` box; `known(&l, i)`; `R10 = PctRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }`; `L1` a fixed uuid):

```rust
// presets.rs
#[test] fn builtin_ids_are_stable() {
    let ids: Vec<_> = builtins().iter().map(|l| l.id.as_u128()).collect();
    assert_eq!(ids, (1..=5).map(|n| 0x7270_5354_0000_4000_8000_0000_0000_0000u128 | n).collect::<Vec<_>>());
}
#[test] fn builtin_names_are_unique() {
    let names: Vec<String> = builtins().iter().map(|l| l.name.to_lowercase()).collect();
    assert_eq!(names, ["classic", "lyrics only", "band", "speaker", "clock and countdown"]);
    assert_eq!(names.iter().collect::<std::collections::HashSet<_>>().len(), 5);
    assert_eq!([builtins()[0].name.as_str(), builtins()[4].name.as_str()], [strings::CLASSIC, strings::CLOCK_COUNTDOWN]);
}
#[test] fn every_builtin_passes_sanitise_unchanged() {
    for l in builtins() { let mut c = l.clone(); assert!(!geom::sanitise_layout(&mut c), "{}", l.name); assert_eq!(&c, l); }
}
#[test] fn classic_matches_todays_stage_regions() {
    let r = |k: fn(&ElementKind) -> bool| rect_of(presets::CLASSIC, k);
    assert_eq!(r(|k| matches!(k, ElementKind::CurrentSlide { .. })), PctRect { x: 3.0, y: 10.0, w: 94.0, h: 52.0 });
    assert_eq!(r(|k| matches!(k, ElementKind::Notes)), PctRect { x: 3.0, y: 63.0, w: 94.0, h: 11.0 });
    assert_eq!(r(|k| matches!(k, ElementKind::NextSlide { .. })), PctRect { x: 3.0, y: 76.0, w: 94.0, h: 22.0 });
    assert_eq!(r(|k| matches!(k, ElementKind::ItemTitle)), PctRect { x: 3.0, y: 2.0, w: 44.0, h: 6.0 });
    assert_eq!(r(|k| matches!(k, ElementKind::SectionName)), PctRect { x: 48.0, y: 2.0, w: 15.0, h: 6.0 });
}
#[test] fn classic_lists_media_before_timer_and_notes_before_message() {
    let p = |k: fn(&ElementKind) -> bool| pos_of(presets::CLASSIC, k);
    assert!(p(|k| matches!(k, ElementKind::MediaTimeLeft)) < p(|k| matches!(k, ElementKind::Timer { .. })));
    assert!(p(|k| matches!(k, ElementKind::Notes)) < p(|k| matches!(k, ElementKind::StageMessage)));
    let timer = known_at(presets::CLASSIC, p(|k| matches!(k, ElementKind::Timer { .. })));
    assert_eq!(timer.style.caption, Some(false));
    assert_eq!(timer.kind, ElementKind::Timer { timer: TimerRef::Current });
}
#[test] fn kind_defaults_table() {
    let kinds = all_kinds();
    let max: Vec<f32> = kinds.iter().map(|k| kind_defaults(k).max_size).collect();
    assert_eq!(max, [9.0, 6.0, 4.0, 4.0, 4.0, 4.0, 4.0, 8.0, 10.0, 4.0, 6.0, 5.0]);
    for k in &kinds {
        let d = kind_defaults(k);
        assert_eq!((d.min_size, d.color.as_str(), d.align, d.valign, d.bold, d.fill.clone()),
                   (3.0, "#ffffff", Align::Center, VAlign::Middle, true, None));
    }
    let cap = |i: usize| { let d = kind_defaults(&kinds[i]); (d.caption, d.caption_text) };
    assert_eq!(cap(1), (true, "NEXT".to_string()));       // NextSlide
    assert_eq!(cap(4), (true, "NEXT ITEM".to_string()));  // NextItemTitle
    assert_eq!(cap(8), (true, String::new()));            // Timer
    assert_eq!(cap(7), (false, "Clock".to_string()));     // Clock
}
#[test] fn resolve_style_overrides_only_set_fields() {
    let r = resolve_style(&ElementKind::ItemTitle, &ElementStyle { color: Some("#f00".into()), max_size: Some(12.0), ..Default::default() });
    assert_eq!(r, ResolvedStyle { color: "#f00".into(), max_size: 12.0, ..kind_defaults(&ElementKind::ItemTitle) });
}
#[test] fn resolve_style_keeps_min_at_or_below_max() {
    let r = resolve_style(&ElementKind::ItemTitle, &ElementStyle { max_size: Some(2.0), ..Default::default() });
    assert_eq!((r.min_size, r.max_size), (2.0, 2.0));
    let r = resolve_style(&ElementKind::ItemTitle, &ElementStyle { min_size: Some(8.0), ..Default::default() });
    assert_eq!((r.min_size, r.max_size), (4.0, 4.0));
}
#[test] fn find_layout_prefers_builtins_then_church() {
    let fake = layout(presets::CLASSIC, "Fake");
    let ours = layout(L1, "Ours");
    let church = [fake, ours.clone()];
    assert_eq!(find_layout(&church, presets::CLASSIC).unwrap().name, "Classic");
    assert_eq!(find_layout(&church, L1), Some(&ours));
    assert_eq!(find_layout(&church, Uuid::from_u128(77)), None);
    assert_eq!(all_layouts(&church).count(), 6);
}
// geom.rs
#[test] fn sanitise_clamps_and_fixes_nan() {
    let mut l = layout_with(vec![
        el(PctRect { x: f32::NAN, y: -5.0, w: 150.0, h: 1.0 }, ElementStyle { max_size: Some(80.0), min_size: Some(50.0), ..Default::default() }),
        el(PctRect { x: 90.0, y: 0.0, w: 20.0, h: 10.0 }, ElementStyle { max_size: Some(1.0), ..Default::default() }),
        el(PctRect { x: 0.0, y: 0.0, w: f32::INFINITY, h: f32::NAN }, ElementStyle::default())]);
    assert!(sanitise_layout(&mut l));
    assert_eq!(known(&l, 0).rect, PctRect { x: 0.0, y: 0.0, w: 100.0, h: 2.0 });
    assert_eq!((known(&l, 0).style.max_size, known(&l, 0).style.min_size), (Some(40.0), Some(40.0)));
    assert_eq!(known(&l, 1).rect.x, 80.0);
    assert_eq!(known(&l, 1).style.max_size, Some(2.0));
    assert_eq!((known(&l, 2).rect.w, known(&l, 2).rect.h), (30.0, 15.0));
}
#[test] fn sanitise_drops_bad_colours_keeps_white() {
    for ok in ["#FFF", "white", "WHITE", "#ffffff", "#FFFFFF80", "rgb(255, 255, 255)", "rgb(255 255 255)", "rgb(255 255 255 / 100%)",
               "rgba(255,255,255,1)", "rgb(100%,100%,100%)", "hsl(0,0%,100%)", "hsl(0 0% 100%)", "hsla(120, 50%, 100%, 1)", "#9aa3b5"] {
        assert!(color_ok(ok), "{ok}");
    }
    for bad in ["blurple", "transparent", "", "rgb(255,255)", "rgb(a,b,c)", "#ggg", "hsl(0,0%,100%"] {
        assert!(!color_ok(bad), "{bad}");
    }
    let mut l = layout_with(vec![el(R10, ElementStyle { color: Some("blurple".into()), fill: Some("white".into()), ..Default::default() })]);
    l.background = "nope".into();
    assert!(sanitise_layout(&mut l));
    assert_eq!((known(&l, 0).style.color.clone(), known(&l, 0).style.fill.as_deref(), l.background.as_str()), (None, Some("white"), "#07080a"));
}
#[test] fn sanitise_truncates_on_char_boundaries() {
    let mut l = layout_with(vec![el_kind(ElementKind::Text { text: "🙏".repeat(250) })]);
    l.name = "é".repeat(300);
    assert!(sanitise_layout(&mut l));
    assert_eq!(l.name.chars().count(), 60);
    let ElementKind::Text { text } = &known(&l, 0).kind else { panic!("kind changed") };
    assert_eq!(text.chars().count(), 200);
}
#[test] fn sanitise_keeps_first_24_boxes_and_leaves_unknown_untouched() {
    let holo = json!({"id": EL, "kind": {"type": "hologram"}, "rect": {"x": 0, "y": 0, "w": 500, "h": 10}});
    let mut els = vec![Lenient::Unknown(holo.clone())];
    els.extend((0..29).map(|_| el(PctRect { x: 0.0, y: 0.0, w: f32::NAN, h: 10.0 }, ElementStyle::default())));
    let mut l = layout_with(els);
    assert!(sanitise_layout(&mut l));
    assert_eq!(l.elements.len(), 24);
    assert_eq!(l.elements[0], Lenient::Unknown(holo));
    assert_eq!(known(&l, 1).rect.w, 30.0);
}
#[test] fn sanitise_settings_fixes_auto_hide_and_quick_list() {
    let mut q = vec![" One ".to_string(), String::new(), "   ".into(), "é".repeat(130)];
    q.extend((2..=13).map(|n| format!("Msg {n}")));
    let mut s = StageSettings { auto_hide_mins: 7, quick_messages: q, ..Default::default() };
    assert!(sanitise_settings(&mut s));
    assert_eq!(s.auto_hide_mins, 0);
    assert_eq!(s.quick_messages.len(), 12);
    assert_eq!(s.quick_messages[0], "One");
    assert_eq!(s.quick_messages[1].chars().count(), 120);
    assert_eq!(s.quick_messages[11], "Msg 11");
    let mut ok = StageSettings { auto_hide_mins: 5, ..Default::default() };
    assert!(!sanitise_settings(&mut ok));
    assert_eq!(ok.auto_hide_mins, 5);
}
```

- [ ] **Step 2:** Run `& $FC test --release --lib stage::presets` and `& $FC test --release --lib stage::geom`. Expected: FAIL (missing functions).
- [ ] **Step 3: Implement** `cells.rs`, the presets, `resolve_style` and the sanitiser.
- [ ] **Step 4:** Run both commands. Expected: PASS.
- [ ] **Step 5: Commit** `feat(stage): five built-in stage layouts, style defaults and sanitising` (`git add src/stage/`).

### Task 3: Runtime state: messages, for-now layout, phone gate

**Files:**
- Create: `src/stage/state.rs`, `src/stage/rate_limit.rs`
- Modify: `src/stage/mod.rs` (`pub mod state; pub mod rate_limit; pub use state::{StageState, ManualLayout, StageOutcome, active_layout};`)

**Interfaces:**
- Consumes: Task 1 types and strings; `presets::{find_layout, builtins}`, `geom::normalize_quick`.
- Produces (`state.rs`):
  - `#[derive(Clone, Debug, Default)] pub struct StageState { pub message: Option<StageMessage>, pub manual: Option<ManualLayout>, pub current_timer: Option<Uuid>, pub seen: VecDeque<(Uuid, StageOutcome)>, pub last_seq: u64 }`; `#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)] pub struct ManualLayout { pub layout: Uuid, pub item_id: Option<Uuid> }`; `#[derive(Clone, Debug, PartialEq, Serialize)] pub enum StageOutcome { Shown { seq: u64 }, Hidden, Refused(String) }`.
  - `pub fn validate_message(text: &str) -> Result<String, String>` (trimmed text, or `TYPE_FIRST` / `MESSAGE_TOO_LONG`).
  - `impl StageState`:
    - `pub fn show_message(&mut self, text: &str, from: MsgSource, now: DateTime<Utc>) -> Result<u64, String>` (`last_seq += 1`, replaces any message, returns the seq);
    - `pub fn hide_message(&mut self, if_seq: Option<u64>) -> bool` (true when something was hidden; never an error);
    - `pub fn use_layout(&mut self, church: &[StageLayout], id: Uuid, live_item: Option<Uuid>) -> Result<(), String>` (`LAYOUT_GONE` for an unknown id);
    - `pub fn auto_layout(&mut self) -> bool`;
    - `pub fn live_item_changed(&mut self, item: Option<Uuid>) -> bool` (clears `manual` when `item != manual.item_id`; true when cleared);
    - `pub fn service_switched(&mut self) -> bool` (hides the message);
    - `pub fn auto_hide_due(&self, mins: u8, now: DateTime<Utc>) -> Option<u64>` (the seq to hide when `mins > 0` and the message is at least `mins` minutes old);
    - `pub fn remember(&mut self, req: Uuid, outcome: StageOutcome)` (cap 256, oldest dropped) and `pub fn recall(&self, req: Uuid) -> Option<&StageOutcome>`.
  - `pub fn set_default(settings: &mut StageSettings, state: &mut StageState, church: &[StageLayout], id: Uuid) -> Result<(), String>` (sets the default and clears `manual`).
  - `pub fn active_layout_checked<'a>(church: &'a [StageLayout], settings: &StageSettings, state: &StageState, item_layout: Option<Uuid>) -> (&'a StageLayout, Option<Uuid>)` — spec §5.1: manual if it exists, else the live item's layout if it exists, else the default if it exists, else Classic. The second value is the first requested id (manual, then item, then default) that was not found (deviation 10). `pub fn active_layout(…) -> &'a StageLayout` returns `.0`.
  - `pub fn validate_auto_hide(mins: u8) -> Result<(), String>`; `pub use crate::stage::geom::normalize_quick;`.
  - `pub fn chip_text(msg: &StageMessage, now: DateTime<Utc>) -> String`: `"On stage · just now"` under 60 s, else `"On stage · {n} min"` (whole minutes, rounded down); a `Phone(name)` source appends `" · from {name}"`.
  - `pub enum PhoneOp { Show(String), Hide }`, `#[derive(Debug, PartialEq)] pub enum Gate { Duplicate(StageOutcome), TooFast, Go }`, `pub fn phone_gate(state: &StageState, limit: &mut RateLimit, phone: Uuid, req: Uuid, op: &PhoneOp, now: Instant) -> Gate`: (1) `recall(req)` → `Duplicate`; (2) `Hide` → `Go`; (3) `Show` → `limit.check(phone, now)` ? `Go` : `TooFast`. Callers `remember` every outcome except `TooFast`.
- Produces (`rate_limit.rs`): `pub struct RateLimit` with `pub fn new(every: Duration) -> Self` and `pub fn check(&mut self, who: Uuid, now: Instant) -> bool` (true = allowed and recorded; a refused try does not move the window; entries older than `every` are pruned once the map holds more than 64), plus `#[cfg(test)] pub fn len(&self) -> usize`. Part B exposes it as `remote::RateLimit` (FEAT-2 shares it).

- [ ] **Step 1: Write the failing tests** (`t0()` = `Utc.with_ymd_and_hms(2026, 11, 1, 9, 0, 0).unwrap()`; `A`, `B`, `L1`, `P`, `P2` fixed uuids; `secs(n)` = `chrono::Duration::seconds(n)`; `ms(n)` = `std::time::Duration::from_millis(n)`; `layout(id, name)` as in Task 2):

```rust
// state.rs
#[test] fn stage_message_show_trims_limits_and_bumps_seq() {
    let mut s = StageState::default();
    assert_eq!(s.show_message("  Last chorus ", MsgSource::Operator, t0()), Ok(1));
    assert_eq!(s.message.as_ref().unwrap().text, "Last chorus");
    assert_eq!(s.show_message("Last chorus", MsgSource::Operator, t0()), Ok(2)); // same text re-flashes
    assert_eq!(s.show_message("   ", MsgSource::Operator, t0()), Err("Type a message first.".into()));
    assert_eq!(s.show_message(&"a".repeat(121), MsgSource::Operator, t0()), Err("Messages can be at most 120 characters.".into()));
    assert_eq!(s.message.as_ref().unwrap().seq, 2, "a refused show leaves the message");
}
#[test] fn message_limit_counts_characters_not_bytes() {
    assert_eq!(validate_message(&"🙏".repeat(120)), Ok("🙏".repeat(120)));
    assert_eq!(validate_message(&"é".repeat(121)), Err(strings::MESSAGE_TOO_LONG.into()));
}
#[test] fn stage_message_hide_is_idempotent_and_if_seq_guards() {
    let mut s = StageState::default();
    s.show_message("One", MsgSource::Operator, t0()).unwrap();
    s.show_message("Two", MsgSource::Operator, t0()).unwrap();
    assert!(!s.hide_message(Some(1)));
    assert_eq!(s.message.as_ref().unwrap().text, "Two");
    assert!(s.hide_message(None));
    assert!(s.message.is_none());
    assert!(!s.hide_message(None));
    assert!(!s.hide_message(Some(2)));
}
#[test] fn seq_keeps_growing_after_hide() {
    let mut s = StageState::default();
    assert_eq!(s.show_message("A", MsgSource::Operator, t0()), Ok(1));
    assert!(s.hide_message(None));
    assert_eq!(s.show_message("B", MsgSource::Operator, t0()), Ok(2));
}
#[test] fn stage_message_show_from_macro_records_source() {
    let mut s = StageState::default();
    s.show_message("Pray", MsgSource::Macro, t0()).unwrap();
    let m = s.message.as_ref().unwrap();
    assert_eq!((m.from.clone(), m.shown_at), (MsgSource::Macro, t0()));
    assert_eq!(chip_text(m, t0() + secs(10)), "On stage · just now");
}
#[test] fn stage_layout_use_is_for_now_and_default_is_separate() {
    let (mut settings, mut s) = (StageSettings::default(), StageState::default());
    s.use_layout(&[], presets::SPEAKER, Some(A)).unwrap();
    assert_eq!(s.manual, Some(ManualLayout { layout: presets::SPEAKER, item_id: Some(A) }));
    assert_eq!(settings.layout, presets::CLASSIC);
    set_default(&mut settings, &mut s, &[], presets::BAND).unwrap();
    assert_eq!((settings.layout, s.manual), (presets::BAND, None));
}
#[test] fn stage_layout_use_unknown_is_error() {
    let (mut settings, mut s) = (StageSettings::default(), StageState::default());
    assert_eq!(s.use_layout(&[], L1, None), Err("That stage layout is gone.".into()));
    assert_eq!(set_default(&mut settings, &mut s, &[], L1), Err("That stage layout is gone.".into()));
    assert_eq!((settings.layout, s.manual), (presets::CLASSIC, None));
}
#[test] fn manual_layout_ends_when_live_item_changes() {
    let mut s = StageState::default();
    s.use_layout(&[], presets::SPEAKER, Some(A)).unwrap();
    assert!(!s.live_item_changed(Some(A)));
    assert!(s.manual.is_some());
    assert!(s.live_item_changed(Some(B)));
    assert_eq!(s.manual, None);
    assert!(!s.live_item_changed(Some(A)), "nothing left to clear");
}
#[test] fn manual_with_nothing_live_ends_when_something_goes_live() {
    let mut s = StageState::default();
    s.use_layout(&[], presets::SPEAKER, None).unwrap();
    assert!(!s.live_item_changed(None));
    assert_eq!(s.manual.unwrap().item_id, None);
    assert!(s.live_item_changed(Some(A)));
    assert_eq!(s.manual, None);
}
#[test] fn active_layout_manual_then_item_then_default_then_classic() {
    let church = vec![layout(L1, "Ours")];
    let (mut settings, mut s) = (StageSettings::default(), StageState::default());
    assert_eq!(active_layout(&church, &settings, &s, None).id, presets::CLASSIC);
    settings.layout = L1;
    assert_eq!(active_layout(&church, &settings, &s, None).id, L1);
    assert_eq!(active_layout(&church, &settings, &s, Some(presets::BAND)).id, presets::BAND);
    s.use_layout(&church, presets::SPEAKER, None).unwrap();
    assert_eq!(active_layout(&church, &settings, &s, Some(presets::BAND)).id, presets::SPEAKER);
}
#[test] fn deleted_layout_falls_back() {
    let gone = Uuid::from_u128(404);
    let s = StageState { manual: Some(ManualLayout { layout: gone, item_id: None }), ..Default::default() };
    let settings = StageSettings { layout: gone, ..Default::default() };
    assert_eq!(active_layout(&[], &settings, &s, Some(gone)).id, presets::CLASSIC);
    let settings = StageSettings { layout: presets::BAND, ..Default::default() };
    assert_eq!(active_layout(&[], &settings, &s, Some(gone)).id, presets::BAND);
}
#[test] fn active_layout_reports_missing_id() {
    let gone = Uuid::from_u128(404);
    let s = StageState { manual: Some(ManualLayout { layout: gone, item_id: None }), ..Default::default() };
    let (l, missing) = active_layout_checked(&[], &StageSettings::default(), &s, None);
    assert_eq!((l.id, missing), (presets::CLASSIC, Some(gone)));
    let (_, missing) = active_layout_checked(&[], &StageSettings::default(), &StageState::default(), None);
    assert_eq!(missing, None);
}
#[test] fn service_switch_hides_stage_message() {
    let mut s = StageState::default();
    s.show_message("Hi", MsgSource::Operator, t0()).unwrap();
    assert!(s.service_switched());
    assert!(s.message.is_none());
    assert!(!s.service_switched());
    assert_eq!(s.show_message("Again", MsgSource::Operator, t0()), Ok(2));
}
#[test] fn auto_hide_due_after_setting_only() {
    let mut s = StageState::default();
    s.show_message("Hi", MsgSource::Operator, t0()).unwrap();
    assert_eq!(s.auto_hide_due(0, t0() + secs(3600)), None);
    assert_eq!(s.auto_hide_due(5, t0() + secs(299)), None);
    assert_eq!(s.auto_hide_due(5, t0() + secs(300)), Some(1));
    s.hide_message(None);
    assert_eq!(s.auto_hide_due(5, t0() + secs(900)), None);
}
#[test] fn quick_and_auto_hide_validation() {
    for m in [0, 1, 5, 10] { assert_eq!(validate_auto_hide(m), Ok(())); }
    assert_eq!(validate_auto_hide(2), Err("Choose Off, 1, 5 or 10 minutes.".into()));
    assert_eq!(normalize_quick(vec![" a ".into(), "".into()]), vec!["a".to_string()]);
}
#[test] fn chip_counts_minutes_and_names_the_phone() {
    let mut s = StageState::default();
    s.show_message("Hi", MsgSource::Operator, t0()).unwrap();
    assert_eq!(chip_text(s.message.as_ref().unwrap(), t0() + secs(20)), "On stage · just now");
    assert_eq!(chip_text(s.message.as_ref().unwrap(), t0() + secs(760)), "On stage · 12 min");
    s.show_message("Hi", MsgSource::Phone("Jake's phone".into()), t0()).unwrap();
    assert_eq!(chip_text(s.message.as_ref().unwrap(), t0() + secs(760)), "On stage · 12 min · from Jake's phone");
}
#[test] fn duplicate_memory_caps_at_256() {
    let mut s = StageState::default();
    let ids: Vec<Uuid> = (1..=257u128).map(Uuid::from_u128).collect();
    for (i, id) in ids.iter().enumerate() { s.remember(*id, StageOutcome::Shown { seq: i as u64 + 1 }); }
    assert_eq!(s.seen.len(), 256);
    assert_eq!(s.recall(ids[0]), None);
    assert_eq!(s.recall(ids[1]), Some(&StageOutcome::Shown { seq: 2 }));
    assert_eq!(s.recall(ids[256]), Some(&StageOutcome::Shown { seq: 257 }));
}
#[test] fn duplicate_checked_before_rate_limit() {
    let mut s = StageState::default();
    let mut lim = RateLimit::new(ms(1000));
    let (r1, r2, now) = (Uuid::from_u128(1), Uuid::from_u128(2), Instant::now());
    let show = PhoneOp::Show("Last chorus".into());
    assert_eq!(phone_gate(&s, &mut lim, P, r1, &show, now), Gate::Go);
    s.remember(r1, StageOutcome::Shown { seq: 1 });
    assert_eq!(phone_gate(&s, &mut lim, P, r1, &show, now + ms(300)), Gate::Duplicate(StageOutcome::Shown { seq: 1 }));
    assert_eq!(phone_gate(&s, &mut lim, P, r2, &show, now + ms(400)), Gate::TooFast);
    assert_eq!(s.recall(r2), None, "TooFast is not remembered");
    assert_eq!(phone_gate(&s, &mut lim, P, r2, &show, now + ms(1000)), Gate::Go);
}
#[test] fn hide_never_rate_limited() {
    let s = StageState::default();
    let mut lim = RateLimit::new(ms(1000));
    let now = Instant::now();
    assert_eq!(phone_gate(&s, &mut lim, P, Uuid::from_u128(1), &PhoneOp::Show("A".into()), now), Gate::Go);
    for k in 0..5u64 {
        assert_eq!(phone_gate(&s, &mut lim, P, Uuid::from_u128(10 + k as u128), &PhoneOp::Hide, now + ms(10 * k)), Gate::Go);
    }
    assert_eq!(phone_gate(&s, &mut lim, P, Uuid::from_u128(2), &PhoneOp::Show("B".into()), now + ms(500)), Gate::TooFast);
}
// rate_limit.rs
#[test] fn one_per_second_per_phone() {
    let mut lim = RateLimit::new(ms(1000));
    let t0 = Instant::now();
    assert!(lim.check(P, t0));
    assert!(!lim.check(P, t0 + ms(999)));
    assert!(lim.check(P, t0 + ms(1000)));
    assert!(!lim.check(P, t0 + ms(1500)));
}
#[test] fn phones_are_independent() {
    let mut lim = RateLimit::new(ms(1000));
    let t0 = Instant::now();
    assert!(lim.check(P, t0));
    assert!(lim.check(P2, t0));
    assert!(!lim.check(P, t0 + ms(10)));
    assert!(!lim.check(P2, t0 + ms(10)));
}
#[test] fn refused_try_does_not_extend_window() {
    let mut lim = RateLimit::new(ms(1000));
    let t0 = Instant::now();
    assert!(lim.check(P, t0));
    assert!(!lim.check(P, t0 + ms(500)));
    assert!(lim.check(P, t0 + ms(1000)), "the refused try at +500 ms did not push the window to +1500 ms");
}
#[test] fn old_entries_pruned_past_64() {
    let mut lim = RateLimit::new(ms(1000));
    let t0 = Instant::now();
    for n in 0..70u128 { assert!(lim.check(Uuid::from_u128(1000 + n), t0)); }
    assert!(lim.check(P, t0 + ms(2000)));
    assert!(lim.len() <= 2, "entries older than the window are pruned once the map passes 64");
}
```

- [ ] **Step 2:** Run `& $FC test --release --lib stage::state` and `& $FC test --release --lib stage::rate_limit`. Expected: FAIL.
- [ ] **Step 3: Implement** both files.
- [ ] **Step 4:** Run both. Expected: PASS.
- [ ] **Step 5: Commit** `feat(stage): stage message, for-now layout and phone gate state`.

### Task 4: Slide notes resolver

**Files:**
- Create: `src/stage/notes.rs`
- Modify: `src/stage/mod.rs` (`pub mod notes;`)

**Interfaces:**
- Consumes: `NoteKey`, `SlideNote`, `VerseKey`, `crate::lenient::Lenient`.
- Produces:
  - `#[derive(Clone, Debug, PartialEq)] pub struct SongPos { pub label: String, pub ordinal: u16, pub page: u16, pub pages: u16 }` (FEAT-4 `SectionRef` fields, 0-based as there);
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct VerseSpan { pub first: VerseKey, pub last: VerseKey, pub part: u8 }` (FEAT-5 `SlideSource`);
  - `#[derive(Clone, Debug, Default, PartialEq)] pub struct SlideKeyFacts { pub song: Option<SongPos>, pub verses: Option<VerseSpan> }`;
  - `pub fn key_of(f: &SlideKeyFacts) -> Option<NoteKey>`: `Song { section: label.to_lowercase(), occurrence: ordinal + 1, page: page + 1, pages }`, else `Verse { verse: first }`, else `None`;
  - `pub fn key_at(slides: &[SlideKeyFacts], i: usize) -> NoteKey` (`key_of` or `Index { slide: i as u16 }`);
  - `pub fn resolve(notes: &[Lenient<SlideNote>], slides: &[SlideKeyFacts]) -> Vec<Option<String>>` (one entry per slide; `Some` where at least one note landed);
  - `pub fn orphans<'a>(notes: &'a [Lenient<SlideNote>], slides: &[SlideKeyFacts]) -> Vec<&'a SlideNote>`;
  - `pub fn set_note(notes: &mut Vec<Lenient<SlideNote>>, key: NoteKey, text: &str) -> Result<(), String>` (trims; empty removes the `Known` note with that key; over 1000 characters → `NOTE_TOO_LONG`; replaces an existing note with the same key; `Unknown` notes are never touched).
- Matching (spec §4.4), per `Known` note: **Song** — slides whose `song` has the same lower-cased label and `ordinal + 1 == occurrence`; if that occurrence now has `pages == key.pages`, the slide with `page + 1 == key.page`, else the occurrence's first page. **Verse** — the slide whose `first..=last` contains the key, preferring `part == 0`. **Index** — that slide, if it exists. Several notes on one slide are joined with `"\n"`, sorted by the note key's `page` (non-song notes count as page 0), stable for ties. Keys are never rewritten (deviation 8).

- [ ] **Step 1: Write the failing tests.** Helpers: `song(label, ordinal, pages)` → `pages` facts for one occurrence; `order(&[(&str, u16)])` → one 1-page fact per (label, ordinal); `span(first, last, part)`; `note(key, text)` → `Lenient::Known(SlideNote)`; `song_key(section, occ, page, pages)`.
  - `key_of_derives_song_key_from_section_ref` (label "Chorus", ordinal 2, page 0, pages 2 → `Some(Song{"chorus",3,1,2})`);
  - `key_of_derives_verse_key_from_source` (`span(VerseKey(42,3,14), VerseKey(42,3,17), 0)` → `Some(Verse { verse: VerseKey(42,3,14) })`; a fact with both `song` and `verses` → the `Song` key; neither → `None`);
  - `key_at_falls_back_to_index` (three default facts, i = 2 → `Index { slide: 2 }`);
  - `note_exact_match` (`song("Verse", 0, 2)` followed by `song("Chorus", 0, 2)` as 4 slides, note `Song{"chorus",1,2,2}` "x" → `[None, None, None, Some("x")]`);
  - `note_survives_lines_per_slide_change` (below);
  - `note_stays_on_its_chorus_occurrence` (order Chorus#0, Verse#0, Chorus#1, Chorus#2; note on chorus occurrence 3 → only index 3 is `Some`; with the third chorus removed → `resolve` all `None` and the note is in `orphans`);
  - `note_follows_arrangement_reorder` (V#0, C#0, B#0, C#1 with a note on chorus occurrence 2 → index 3; reordered to V#0, C#0, C#1, B#0 → index 2);
  - `removed_occurrence_orphans_note` (note on chorus occurrence 2, order V#0, C#0 → `resolve` all `None`, `orphans` returns exactly that note);
  - `verse_note_survives_translation_switch_and_rebreak` (`Verse{VerseKey(42,3,16)}` on slides [3:14–3:17] → index 0; on slides [3:15–3:15, 3:16–3:16 part 0, 3:16–3:16 part 1] → index 1 only);
  - `index_note_for_decks` (`Index{2}` on 3 default facts → `[None, None, Some]`; `Index{5}` → `resolve` all `None` and in `orphans`);
  - `merged_notes_join_in_page_order` (notes `Song{"chorus",1,2,3}` "second" then `Song{"chorus",1,1,3}` "first"; chorus now 1 page → `[Some("first\nsecond")]`);
  - `unknown_note_kept_never_shown_never_orphan` (`Lenient::Unknown(json!({"key":{"on":"bar"},"text":"x"}))` → `resolve` all `None`, `orphans` empty; `set_note(Index{0}, "hi")` leaves the Unknown note equal and adds one Known note, length 2);
  - `set_note_empty_removes_and_long_is_error` (`set_note(Index{0}, "  hi ")` stores `"hi"`; the same key with `"yo"` replaces it (length 1, text `"yo"`); `"   "` removes it (length 0); `"x".repeat(1001)` → `Err("Notes can be at most 1000 characters.")` and nothing stored; `"é".repeat(1000)` → `Ok`).

```rust
#[test] fn note_survives_lines_per_slide_change() {
    // Spec §9.1 reading: a note on page 1 stays on page 1 whichever way the page count changes.
    let p1 = vec![note(song_key("chorus", 1, 1, 2), "start soft")];
    assert_eq!(resolve(&p1, &song("Chorus", 0, 1)), vec![Some("start soft".into())]);        // 2 → 4 lines per slide: 1 page
    assert_eq!(resolve(&p1, &song("Chorus", 0, 2)), vec![Some("start soft".into()), None]);  // back to 2 pages
    // Deviation 8: a page-2 note goes to the first page while the count differs, and back to page 2 when it is restored.
    let p2 = vec![note(song_key("chorus", 1, 2, 2), "build")];
    assert_eq!(resolve(&p2, &song("Chorus", 0, 1)), vec![Some("build".into())]);
    assert_eq!(resolve(&p2, &song("Chorus", 0, 4)), vec![Some("build".into()), None, None, None]);
    assert_eq!(resolve(&p2, &song("Chorus", 0, 2)), vec![None, Some("build".into())]);
}
```

- [ ] **Step 2:** Run `& $FC test --release --lib stage::notes`. Expected: FAIL.
- [ ] **Step 3: Implement** `notes.rs`.
- [ ] **Step 4:** Run it. Expected: PASS.
- [ ] **Step 5: Commit** `feat(stage): slide notes keyed by section occurrence, verse or index`.

### Task 5: Layout references and the all-or-nothing replace

**Files:**
- Create: `src/stage/refs.rs`
- Modify: `src/stage/mod.rs` (`pub mod refs;`)

**Interfaces:**
- Consumes: `presets::{is_builtin, builtins}`, `geom::sanitise_layout`, strings.
- Produces:
  - `#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(tag = "kind", rename_all = "snake_case")] pub enum LayoutRef { StageDefault, ServiceItem { service: Uuid, item: Uuid, title: String }, MacroStep { macro_id: Uuid, name: String } }` (spec §5.6; FEAT-6 later adds `Template`).
  - `pub struct ItemUse { pub service: Uuid, pub item: Uuid, pub title: String, pub layout: Option<Uuid> }`, `pub struct MacroUse { pub macro_id: Uuid, pub name: String, pub layouts: Vec<Uuid> }`, `pub struct RefSources<'a> { pub stage_default: Uuid, pub items: &'a [ItemUse], pub macros: &'a [MacroUse] }`.
  - `pub fn layout_refs(src: &RefSources, id: Uuid) -> Vec<LayoutRef>`: default first, then items in the given order (every service), then one `MacroStep` per macro that names the layout at least once.
  - `pub fn delete_question(name: &str, refs: &[LayoutRef]) -> String`. No refs → `Delete “{name}”?`. Else `Delete “{name}”? It is used by {parts}. {tails}.` where parts are, in order and only when present, `1 service item` / `N service items`, `the stage default`, `1 macro` / `N macros`, joined `a, b and c`; tails are joined with `; `: items and default → `Items and the default will use Classic`; items only → `That item will use the stage default` / `Those items will use the stage default`; default only → `The default will use Classic`; macros → `the macro won't run until it is edited` / `the macros won't run until they are edited` (first letter upper-cased when it starts the tails).
  - `pub fn plan_replace(church: &[StageLayout], layouts: Vec<StageLayout>, deleted: &[Uuid]) -> Result<Vec<StageLayout>, String>`: returns the new church set or an error with nothing applied. Checks in this order: a built-in id in either list → `BUILTIN_LOCKED`; any layout with more than 24 elements → `TOO_MANY_BOXES`; then remove `deleted`, upsert `layouts` (by id, new ones appended), sanitise each; an empty trimmed name → `NAME_EMPTY`; a name equal (case-insensitive, trimmed) to another in the resulting set or to a built-in → `name_taken(name)` (the trimmed name as sent). `Unknown` elements are kept as sent.
  - `pub fn clear_deleted(default: &mut Uuid, item_layouts: &mut [&mut Option<Uuid>], deleted: &[Uuid]) -> usize`: default → `CLASSIC`, matching item overrides → `None`; returns how many were cleared (the action's `{cleared}`).

- [ ] **Step 1: Write the failing tests:**
  - `delete_question_matches_spec_example` (3 items, default, 1 macro → exactly `Delete “Band”? It is used by 3 service items, the stage default and 1 macro. Items and the default will use Classic; the macro won't run until it is edited.`);
  - `delete_question_without_refs` (`[]` → `Delete “Band”?`);
  - `delete_question_items_only_singular` (1 item → `Delete “Band”? It is used by 1 service item. That item will use the stage default.`; 2 macros only → `Delete “Band”? It is used by 2 macros. The macros won't run until they are edited.`);
  - `deleting_layout_reports_macro_uses` (macro naming the layout twice → exactly one `MacroStep { macro_id, name }`);
  - `layout_refs_counts_items_in_every_service` (items in two services, 2 + 1 using L1, one using L2 → 3 `ServiceItem` refs in input order; with `stage_default: L1` the first ref is `StageDefault`);
  - `layouts_replace_rejects_builtin_empty_duplicate_and_too_many` (each error string exact: a layout with `CLASSIC`'s id → `BUILTIN_LOCKED`; `deleted: [BAND]` → `BUILTIN_LOCKED`; name `"  "` → `"Give the stage layout a name."`; two layouts "Ours" and " ours " → `"There is already a stage layout called “ours”."`; name `"classic"` → `"There is already a stage layout called “classic”."`; 25 elements → `"A stage layout can have at most 24 boxes."`);
  - `layouts_replace_is_all_or_nothing` (a valid rename plus an invalid layout → `Err`, input `church` equal to a clone taken before);
  - `rename_into_deleted_name_succeeds` (delete "Band" and rename "Band copy" to "Band" in one call → `Ok` with exactly one layout named "Band");
  - `replace_keeps_unknown_boxes_as_sent` (a layout whose `elements[0]` is `Unknown(v)` → the result's `elements[0] == Unknown(v)`);
  - `replace_sanitises_rects` (`w: 150` → `100`, NaN `x` → `0`);
  - `clear_deleted_resets_default_and_items` (default L1, items `[Some(L1), Some(L2), None]`, deleted `[L1]` → default `CLASSIC`, items `[None, Some(L2), None]`, returns 2).
- [ ] **Step 2:** Run `& $FC test --release --lib stage::refs`. Expected: FAIL.
- [ ] **Step 3: Implement** `refs.rs`.
- [ ] **Step 4:** Run it. Expected: PASS.
- [ ] **Step 5: Commit** `feat(stage): stage layout references and all-or-nothing replace`.

### Task 6: Editor geometry, box order and undo

**Files:**
- Modify: `src/stage/geom.rs`

**Interfaces:**
- Consumes: `PctRect`, `StageElement`, `crate::lenient::Lenient`, strings.
- Produces: `pub const GRID: f32 = 2.5; pub const GUIDE_TOL: f32 = 1.0; pub const MIN_BOX: f32 = 5.0;`
  - `pub fn snap(v: f32, step: f32) -> f32` (nearest multiple);
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub enum Guide { V(f32), H(f32) }` and `pub fn snap_to_guides(r: PctRect, others: &[PctRect], tol: f32) -> (PctRect, Vec<Guide>)`: aligns r's left, centre or right to any other's left, centre or right (and the window's 0/50/100) within `tol`, the same for top/middle/bottom; each axis snaps to its closest candidate; returns one guide per snapped axis;
  - `pub fn move_by(r: PctRect, dx: f32, dy: f32) -> PctRect` (stays inside the window);
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Handle { N, NE, E, SE, S, SW, W, NW }` and `pub fn resize(r: PctRect, h: Handle, dx: f32, dy: f32, min: f32) -> PctRect` (the opposite edge stays put; never below `min`; never outside the window);
  - `pub fn element_id(e: &Lenient<StageElement>) -> Option<Uuid>` (an `Unknown` box: its JSON `"id"` if it parses);
  - `pub fn hit_test(p: (f32, f32), elements: &[Lenient<StageElement>]) -> Option<Uuid>`: topmost (last) box containing p; an `Unknown` box hits when its JSON has a parsable `"id"` and `"rect"`;
  - `#[derive(Clone, Copy, Debug)] pub enum Nudge { Left, Right, Up, Down }` and `pub fn nudge(r: PctRect, key: Nudge, shift: bool) -> PctRect` (one `GRID` step; Shift resizes the right/bottom edge, minimum `MIN_BOX`);
  - `pub fn new_box_rect() -> PctRect` (`{35, 42.5, 30, 15}`);
  - Box order and copies (spec §3.6 keys and right-click menu): `pub fn bring_to_front(els: &mut Vec<Lenient<StageElement>>, id: Uuid) -> bool` and `pub fn send_to_back(els: &mut Vec<Lenient<StageElement>>, id: Uuid) -> bool` (move the `Known` box to the end / start; false when not found, already there, or `Unknown`); `pub fn duplicate_box(els: &mut Vec<Lenient<StageElement>>, id: Uuid, new_id: Uuid) -> Result<Uuid, String>` (a `Known` box only: the copy gets `new_id` and the rect `move_by(r, GRID, GRID)`, so it stays inside the window, and is inserted right after the original; at 24 boxes → `Err(TOO_MANY_BOXES)`; an `Unknown` or missing id → `Err(String::new())`, which the UI never shows because its menu item is disabled for those); `pub fn next_box(els: &[Lenient<StageElement>], sel: Option<Uuid>, back: bool) -> Option<Uuid>` (Tab / Shift+Tab: layout order over every box with an `element_id`, `Unknown` included so it can be deleted, wrapping; `sel: None` → first, or last when `back`);
  - `pub struct UndoStack<T: Clone> { … }` with `pub fn new(cap: usize) -> Self`, `pub fn push(&mut self, before: T)`, `pub fn undo(&mut self, current: T) -> Option<T>`, `pub fn redo(&mut self, current: T) -> Option<T>`, `pub fn can_undo/can_redo(&self) -> bool`; a push clears redo; more than `cap` drops the oldest. The editor uses `UndoStack<Vec<StageLayout>>::new(50)`.

- [ ] **Step 1: Write the failing tests:**
  - `snap_to_grid` (`snap(3.7, 2.5) == 2.5`, `snap(3.8, 2.5) == 5.0`, `snap(0.0, 2.5) == 0.0`);
  - `snap_to_other_box_edges_and_centres_returns_guides` (r `{10.6, 60, 20, 10}` next to a box `{10, 0, 30, 10}` → x 10.0 with `[Guide::V(10.0)]` and y unchanged; r at x 11.5 (centre 21.5, right 31.5, nothing within 1.0) → unchanged, no guides);
  - `snaps_to_window_centre` (`{34.4, 70, 30, 10}` with no other boxes → x 35.0 with `[Guide::V(50.0)]`);
  - `move_by_stays_inside` (`{70,70,20,20}` by (+50, +50) → `{80,80,20,20}`; by (−200, 0) → x 0);
  - `resize_keeps_minimum_size_and_inside_window` (NW handle of `{10,10,20,20}` dragged by (+40, +40) → `{25,25,5,5}`, a 5 × 5 box at the old SE corner (30, 30); SE handle dragged (+500, +500) → right and bottom edges at 100);
  - `hit_test_returns_topmost` (A then B at the same rect → `Some(B)`; a point outside both → `None`);
  - `hit_test_finds_unknown_box` (`Unknown(json!({"id": EL, "rect": {"x":0,"y":0,"w":50,"h":50}, "kind": {"type":"hologram"}}))` hit at (10, 10) → `Some(EL)`; an `Unknown` without `rect` → `None`);
  - `nudge_and_shift_nudge` (`{10,10,20,20}` Right → x 12.5; Shift+Left → w 17.5; Shift+Up on h 5 → stays 5; Left at x 0 → stays 0);
  - `bring_to_front_and_send_to_back` (A, B, C: front(A) → B, C, A and true; front(A) again → false; back(C) → C, B, A; an `Unknown` box's id → false and order unchanged);
  - `duplicate_box_offsets_by_one_grid_step_and_clamps` (A at `{10,10,20,20}` → copy at `{12.5,12.5,20,20}` with `new_id`, at index 1; a box at `{80,85,20,15}` → copy at `{80,85,20,15}`; with 24 boxes → `Err("A stage layout can have at most 24 boxes.")` and length still 24; an `Unknown` id → `Err`, nothing added);
  - `next_box_tabs_in_layout_order_and_wraps` (A, Unknown U, B: `None` → A; A → U; U → B; B → A; back from A → B; back from `None` → B);
  - `undo_redo_cap_50` (60 pushes of `0..60` → 50 `undo` calls return `59, 58, …, 10`, the 51st returns `None`; after one undo, a push makes `can_redo()` false).
- [ ] **Step 2:** Run `& $FC test --release --lib stage::geom`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** Run it. Expected: PASS (Task 2's tests included).
- [ ] **Step 5: Commit** `feat(stage): stage layout editor geometry, box order and undo stack`.

### Task 7: Cell renderer

**Files:**
- Create: `src/stage/draw.rs`, `tests/golden/stage_classic_lyric.png`, `tests/golden/stage_speaker_timer_notes.png`, `tests/golden/stage_message_banner.png` (generated with `UPDATE_GOLDENS=1`, then looked at)
- Modify: `src/stage/mod.rs` (`pub mod draw;`)

**Interfaces:**
- Consumes: `crate::render::{Fonts, Rendered, parse_color}` (read only; `Fonts.sys` is `pub(crate)`), `cells::{CellContent, ResolvedStyle, StageView}`, `presets`.
- Produces:
  - `pub fn fit_sizes(max: f32, min: f32) -> impl Iterator<Item = f32>`: for `k = 0, 1, …` yields `max - 0.4 * k as f32` while that value is `> min + 1e-4`, then yields exactly `min` once (today's `fit_size` scheme; computed by multiplication, never by repeated subtraction, so there is no f32 drift). If `max <= min + 1e-4` it yields only `min`. For 9 → 3: 16 sizes, the last exactly `3.0`.
  - `pub fn render_cell(fonts: &mut Fonts, content: &CellContent, style: &ResolvedStyle, font: &str, px: (u32, u32), window_h: u32, picture: Option<&image::RgbaImage>) -> Rendered`. `px` is clamped with `render::layout::clamp_canvas`. Text: family `font` if `fonts.has_family`, else Noto Sans (`font_fallback`); sizes in % of `window_h`; padding, line height, weights, caption, dim, red, fill as in Global Constraints; vertical/horizontal alignment from the style; the first size from `fit_sizes` whose shaped height and widest line fit the padded box, else `min_size`, clipped at the box. `Rendered.font_px` = chosen size in pixels. `Picture` with an image: letterboxed, 2 px border; without one: drawn as text `"(slide picture)"`. `Message`: fill `#f5c66b`, text `#1a1405` bold, ignoring style colour and fill. `Empty`: fully transparent. Rasterise glyphs with `cosmic_text::SwashCache` the way `render::raster::coverage_of` does (write a local copy; do not edit `src/render`).
  - `pub fn text_fits(fonts: &mut Fonts, content: &CellContent, style: &ResolvedStyle, font: &str, px: (u32, u32), window_h: u32) -> bool`: false when even `min_size` does not fit (the editor's "Text may not fit" mark); true for non-text content.
  - `pub fn render_view(fonts: &mut Fonts, view: &StageView, window: (u32, u32), picture: &dyn Fn(&str) -> Option<image::RgbaImage>) -> Rendered`: background, then each cell at its rect, then `message_fallback` as a message cell over the top 16 % (full width). Used by goldens and Part B's layout thumbnails.
  - `#[derive(Clone, Debug, PartialEq, Eq, Hash)] pub struct StageKey { pub content: u64, pub style: u64, pub font: String, pub px: (u32, u32) }` and `pub fn stage_key(content: &CellContent, style: &ResolvedStyle, font: &str, px: (u32, u32)) -> StageKey` (`f32`s hashed by `to_bits`). Task B6 replaces FEAT-1's placeholder `StageKey` with this type.

- [ ] **Step 1: Write the failing tests** (in `draw.rs`; goldens use `Fonts::bundled_only()` and the tolerance helper copied from `tests/render_golden.rs`: mean ≤ 2, ≤ 0.5 % of pixels off by > 32; path `tests/golden/stage_*.png`; `px_of(rect, (w, h))` converts a `PctRect` to pixels):
  - `fit_sizes_match_old_fit_size` (`fit_sizes(9.0, 3.0).collect::<Vec<_>>()` has length 16, equals `(0..=15).map(|k| 9.0 - 0.4 * k as f32)` within 1e-4, and its last value is exactly `3.0`; `fit_sizes(3.0, 3.0)` is `[3.0]`; `fit_sizes(4.0, 3.0)` is `[4.0, 3.6, 3.2, 3.0]` within 1e-4);
  - `classic_fit_sizes_equal_old_fit_size` (20 sample lyrics of 1–12 lines in Classic's CurrentSlide box at 1920×1080: the chosen size is in `fit_sizes(9,3)`, the text fits at that size and does not fit at the next larger one);
  - `long_text_shrinks_to_floor_and_clips` (2 000 characters in a 1880×560 box, window 1080 → `font_px == 32.4`, image size 1880×560, `text_fits` false);
  - `short_text_uses_max_size` ("Amen", same box → `font_px == 97.2`, `text_fits` true);
  - `caption_reduces_text_area` (a 6-line text that fits at 9 % in a 1880×560 box without a caption gets a smaller `font_px` with `caption: Some("NOW")`);
  - `classic_header_cells_fit_at_min_size` (at 1920×1080, with Classic's resolved styles at Classic's rects: Timer `Text { "12:34", caption: None }`, MediaTimeLeft `"−1:20"`, ItemTitle `"Amazing Grace"`, SectionName `"Verse 1"` → each `font_px >= 32.4` and `text_fits` true; Band's Timer `Text { "12:34", caption: Some("SERMON") }` with its kind-default caption at (68,74,29,11) → `font_px >= 32.4`, `text_fits` true);
  - `dim_text_is_35_percent_alpha` (max alpha of a dim white "Amen" ≈ 89 ± 2);
  - `red_text_is_ff4f55` (the most common fully opaque pixel of a red `"0:00"` is `#ff4f55` ± 2 per channel);
  - `message_cell_has_amber_fill` (a pixel 2 px inside the fill's top-left corner radius is `#f5c66b`);
  - `picture_without_image_says_slide_picture` (the cell renders pixel-identical to `Text { "(slide picture)", dim: false, caption: None, red: false }` with the same style);
  - `tiny_and_zero_boxes_render_without_panic` (px (0,0), (1,1), (12,7) with window 0 and 360: no panic, every image at least 1×1);
  - `stage_key_changes_with_content_style_font_px` (changing each of the four inputs alone gives a different key; the same inputs give equal keys);
  - goldens at 1280×720: `golden_classic_lyric` (Classic: title "Amazing Grace", section "Verse 1", clock "9:05 AM", two-line lyric, next line), `golden_speaker_with_timer_and_notes` (Speaker: clock, timer "12:34" with caption "SERMON", notes, next item), `golden_message_banner` (Lyrics only without its message box → `message_fallback` "Please wrap up"). Build the `StageView`s by hand from `presets::builtins()` (the view builder comes later).
- [ ] **Step 2:** Run `& $FC test --release --lib stage::draw`. Expected: FAIL.
- [ ] **Step 3: Implement** `draw.rs`. Generate goldens (PowerShell, `$FC` defined as in Global Constraints):

```powershell
$env:UPDATE_GOLDENS = "1"; & $FC test --release --lib stage::draw::tests::golden_; Remove-Item Env:UPDATE_GOLDENS
```

  Then open each PNG and check it looks like the spec's stage (Classic: same regions as today's stage window).
- [ ] **Step 4:** Run `& $FC test --release --lib stage::draw`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(stage): per-box stage renderer with fitting, captions and goldens` (`git add src/stage/draw.rs src/stage/mod.rs tests/golden/stage_*.png`).

### Task 8: Current-timer latch (needs FEAT-2 Tasks A2–A3)

**Files:**
- Create: `src/stage/latch.rs`
- Modify: `src/stage/state.rs` (add `pub timer_idle: HashMap<Uuid, bool>` to `StageState`), `src/stage/mod.rs` (`pub mod latch; pub use latch::{track_timers, current_timer};`)

**Interfaces:**
- Consumes: `crate::timers::{TimersView, TimerSnapshot, TimerState, TimerKind}`.
- Produces:
  - `pub fn track_timers(state: &mut StageState, view: &TimersView)`: for each `on_stage` snapshot, `idle = matches!(state, Stopped | Finished)`; a timer that is now `Running` and was recorded idle becomes `current_timer`; the latched timer is released when it is `Stopped`, is gone, or is no longer `on_stage`; `timer_idle` is rewritten from this view (timers not in it are dropped). A timer seen for the first time while running is recorded but does not latch.
  - `pub fn current_timer<'a>(state: &StageState, view: &'a TimersView) -> Option<&'a TimerSnapshot>`: the latched one if present and `on_stage`, else the first `on_stage` snapshot (FEAT-2 order) whose state is not `Stopped`, else `None`.
- Test snapshots: `snap(id, state, on_stage)` = `TimerSnapshot { id, name: "T".into(), kind: TimerKind::Duration { secs: 300 }, state, value_ms: 300_000, overrun: false, on_stage, on_remote: false }`, `TimersView { timers, auto: None }`.

- [ ] **Step 1: Write the failing tests** (each calls `track_timers` once per view in sequence, then checks `current_timer(..).map(|t| t.id)`):
  - `current_timer_latches_most_recent_start` (views: [A Running, B Stopped] → [A Running, B Running] ⇒ `Some(B)`, `state.current_timer == Some(B)`);
  - `current_timer_stays_when_paused` (after that latch, [A Running, B Paused] ⇒ `Some(B)`);
  - `current_timer_stays_when_finished_and_overrun` ([A Running, B Finished with `overrun: true`] ⇒ `Some(B)`);
  - `current_timer_released_on_reset_to_stopped` (then [A Running, B Stopped] ⇒ `Some(A)` and `state.current_timer == None`);
  - `current_timer_ignores_not_on_stage` (C Stopped → Running with `on_stage: false` ⇒ `state.current_timer == None`; a view with only C ⇒ `None`);
  - `nothing_latched_falls_back_to_first_not_stopped` (fresh state, view [B Paused, A Running] ⇒ `Some(B)`);
  - `first_sighting_while_running_does_not_latch` (fresh state, [A Running] ⇒ `state.current_timer == None` and `current_timer` ⇒ `Some(A)` by fallback; then [A Running, B Running] with B new ⇒ `state.current_timer` still `None`);
  - `latched_timer_gone_is_released` (latched B, next view without B ⇒ `state.current_timer == None` and `timer_idle` has no key B).
- [ ] **Step 2:** Run `& $FC test --release --lib stage::latch`. Expected: FAIL.
- [ ] **Step 3: Implement** `latch.rs` (match on `&TimerState`; no derives on FEAT-2's types are required).
- [ ] **Step 4:** Run it, then `& $FC test --release --lib stage::state`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(stage): latch the most recently started on-stage timer`.

### Task 9: View builder — slides, titles, sections, notes

**Files:**
- Create: `src/stage/view.rs`
- Modify: `src/stage/mod.rs` (`pub mod view;`)

**Interfaces:**
- Consumes: `cells::*`, `presets::resolve_style`, `StageState`, `crate::timers::TimersView`, strings.
- Produces (facts filled by Part B's adapter from `LiveView` of the **displayed** engine and `Data`):

```rust
#[derive(Clone, Debug, PartialEq)] pub enum SlideFace { Text(String), Picture { asset: String, still: bool }, Video, Audio }
#[derive(Clone, Debug, PartialEq, Default)] pub enum OnAir { Slide(SlideFace), Logo, ClearedSlide, #[default] ClearedAll }
#[derive(Clone, Debug, PartialEq)] pub struct NextFacts { pub face: SlideFace, pub other_item: Option<String> }
#[derive(Clone, Debug, PartialEq)] pub struct ItemFacts { pub id: Uuid, pub title: String }
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LiveFacts {
    pub item: Option<ItemFacts>,          // the cursor's item, kept after Clear Slide / Logo
    pub on_air: OnAir,
    pub cursor_face: Option<SlideFace>,   // the cursor slide (for keep_after_clear)
    pub section: Option<String>, pub section_color: Option<String>,
    pub notes: String,                    // the snapshot slide's Slide.notes
    pub next: Option<NextFacts>,          // LiveView.next; None = end of service
    pub next_item_title: Option<String>, pub first_item_title: Option<String>,
    pub service_pos: Option<(usize, usize)>,
    pub media_left: Option<f64>,          // seconds; Some only when video, else audio, has duration > 0
}
pub struct StageInputs<'a> { pub live: &'a LiveFacts, pub state: &'a StageState, pub timers: &'a TimersView, pub now: chrono::NaiveTime }
pub fn build(inp: &StageInputs, layout: &StageLayout) -> StageView
pub fn face_text(face: &SlideFace) -> &str   // Text → its text, Picture{still:false} "(slide picture)", still "(picture)", "(video)", "(audio)"
pub fn sample_facts() -> LiveFacts           // the editor preview when nothing is live (spec §3.6): SAMPLE_* strings
```

- In this task `build` fills CurrentSlide, NextSlide, SectionName, ItemTitle, NextItemTitle, ServicePosition, Notes and Text per spec §5.2, and leaves the other kinds `Empty` (Task 10). One `Cell` per `Known` element in layout order, `Unknown` elements skipped. CurrentSlide: `Slide(face)` → text, or `Picture` when `pictures` and the face is a picture; `Logo` → `"(logo)"` dim; `ClearedSlide` → empty and dim, or the cursor face's text dim when `keep_after_clear`; `ClearedAll` → empty, dim. NextSlide: `face_text` (or picture), caption `"NEXT"` or `"NEXT · {other_item}"` when the caption is on; `None` → `"End of service"`. SectionName: label or `Empty`; when the element's own `style.color` is `None`, colour = `section_color` or `#9aa3b5`. ItemTitle: title or `"Nothing live"`. NextItemTitle: next item title, or the first item's title when nothing is live, else `"End of service"`. ServicePosition: `"Item 3 of 8"` or `Empty`. Notes: text or `Empty`. Text: its text or `Empty`. Text cells carry `caption: Some(caption_text)` only when the style's caption is on. `sample_facts()`: item `SAMPLE_TITLE`, `on_air: Slide(Text(SAMPLE_LYRIC))`, section `SAMPLE_SECTION`, notes `SAMPLE_NOTES`, next `Text(SAMPLE_NEXT)`, next item `SAMPLE_NEXT_ITEM`, `service_pos: Some((3, 8))`, `media_left: None`.

- [ ] **Step 1: Write the failing tests** (helpers: `inputs(&live)` with an empty `StageState`, empty `TimersView`, `now` 9:05:30; `classic()`/`speaker()` from `presets::builtins()`; `item(title)`; `content_of(&view, pred)` / `cell_of(&view, pred)` find the cell of the first element whose kind matches; `is_current`, `is_next`, … predicates):
  - `classic_current_slide_empty_and_dim_after_clear_slide` (below; port of `stage_text_dims_when_cleared`);
  - `keep_after_clear_shows_dim_words_after_clear_slide_only` (a CurrentSlide box with `keep_after_clear: true`, `ClearedSlide` with cursor face "Amazing grace" → `Text { "Amazing grace", dim: true }`; with `Logo` → `Text { "(logo)", dim: true }`);
  - `clear_all_always_empties_current_slide` (`ClearedAll` with `keep_after_clear: true` and a cursor face → `Text { "", dim: true }`);
  - `current_slide_shows_logo_label` (`Logo` → `Text { "(logo)", dim: true, caption: None, red: false }`);
  - `classic_picture_slide_shows_label` (`Picture { still: false }` → text `"(slide picture)"`; `still: true` → `"(picture)"`, `dim: false`);
  - `pictures_option_shows_picture` (Speaker, `Picture { asset: "a.png", still: false }` → `Picture { asset: "a.png", caption: None }`);
  - `video_and_audio_labels` (`Video` → `"(video)"`, `Audio` → `"(audio)"`);
  - `next_slide_matches_live_next_and_names_next_item` (`next: Some(NextFacts { face: Text("Line 2"), other_item: None })` → `Text { "Line 2", caption: Some("NEXT") }`; `other_item: Some("Offering")` → caption `Some("NEXT · Offering")`);
  - `end_of_service_text` (`next: None` → `Text { "End of service" }`);
  - `section_name_from_section_ref_songs_only` (`section: Some("Chorus")` → `"Chorus"`; `None` → `Empty`);
  - `section_name_uses_color_for_label` (`section_color: Some("#4aa3ff")` → the cell's `style.color == "#4aa3ff"`; `None` → `"#9aa3b5"`; a box whose own `style.color` is `"#00ff00"` keeps `"#00ff00"`);
  - `classic_shows_section_label_line` (Classic with section "Verse 1" → its SectionName cell text `"Verse 1"` at rect (48,2,15,6));
  - `item_titles_when_nothing_live` (`item: None`, `first_item_title: Some("Welcome")` → ItemTitle `"Nothing live"`, NextItemTitle `"Welcome"`; with no first item → `"End of service"`);
  - `service_position_counts_items` (`Some((3, 8))` → `"Item 3 of 8"`; `None` → `Empty`);
  - `notes_cell_shows_snapshot_notes` (`notes: "Build"` → `Text { "Build" }`; `""` → `Empty`);
  - `unknown_element_draws_nothing` (a layout of [Known ItemTitle, Unknown, Known Clock] → 2 cells with the two Known boxes' element ids);
  - `cells_follow_layout_order_with_resolved_styles` (Classic → cell element ids equal Classic's element ids in order; each cell's `style == resolve_style(kind, &element.style)` except SectionName's colour);
  - `sample_facts_fill_every_text_kind` (`build` over Classic with `sample_facts()` → the CurrentSlide, NextSlide, ItemTitle, SectionName, Notes cells are not `Empty`, and the CurrentSlide text equals `SAMPLE_LYRIC`).

```rust
#[test] fn classic_current_slide_empty_and_dim_after_clear_slide() {
    let live = LiveFacts { item: Some(item("Amazing Grace")), on_air: OnAir::ClearedSlide,
                           cursor_face: Some(SlideFace::Text("Amazing grace".into())), ..Default::default() };
    let v = build(&inputs(&live), classic());
    assert_eq!(content_of(&v, is_current), CellContent::Text { text: String::new(), dim: true, caption: None, red: false });
}
```

- [ ] **Step 2:** Run `& $FC test --release --lib stage::view`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** Run it. Expected: PASS.
- [ ] **Step 5: Commit** `feat(stage): stage view builder for slides, titles, sections and notes`.

### Task 10: View builder — clock, timers, media, messages, shared rects

**Files:**
- Modify: `src/stage/view.rs`

**Interfaces:**
- Consumes: Task 8 `current_timer`, Task 9 `build`, `crate::timers::text`.
- Produces:
  - `pub fn clock_text(t: chrono::NaiveTime, seconds: bool, h24: bool) -> String` (`%-I:%M %p`, `%-I:%M:%S %p`, `%H:%M`, `%H:%M:%S`);
  - `pub fn countdown(left: f64) -> String` (`"−m:ss"`, U+2212, rounded, never below 0);
  - `pub fn timer_cell(t: &TimerSnapshot, style: &ResolvedStyle) -> CellContent`: text `timers::text(t.value_ms)`; `red` when `overrun` or `Finished`; `Paused` → `dim` and caption `"{name} · paused"`; caption = name when the style's caption is on;
  - `pub fn shared_rect_priority(cells: &mut [Cell], kinds: &[&ElementKind])` (deviation 3; cells with bit-identical rects form a group);
  - `build` now fills Clock, Timer (`TimerRef::Id` not found → `"Timer removed"` dim; `Current` → `current_timer` or `Empty`), MediaTimeLeft (`countdown` or `Empty`), StageMessage (`Message { text, seq }` or `Empty`), applies `shared_rect_priority`, and sets `message_fallback` when a message is up and the layout has no `Known` StageMessage box.

- [ ] **Step 1: Write the failing tests:**
  - `clock_formats_12h_24h_seconds` (9:05:30 → `"9:05 AM"`, `"9:05:30 AM"`, `"09:05"`, `"09:05:30"`; 18:40 → `"6:40 PM"`);
  - `media_time_left_matches_todays_countdown` (port: 79.6 → `"−1:20"`, 5.0 → `"−0:05"`, −5 → `"−0:00"`, 125 → `"−2:05"`);
  - `timer_text_comes_from_timers_text` (equals `crate::timers::text(value_ms)` for 0, 59_001, 3_600_000, −65_000);
  - `current_timer_shows_overrun_red` (latched timer with `overrun: true`, `Running` → `red: true`);
  - `current_timer_shows_finished_red` (`Finished`, `overrun: false` → `red: true`);
  - `timer_paused_is_dim_with_caption` (Speaker timer box, name "Sermon", `Paused` → `dim: true`, caption `Some("Sermon · paused")`);
  - `missing_timer_says_removed` (`TimerRef::Id { id: gone }` → `Text { "Timer removed", dim: true }`);
  - `classic_media_wins_shared_rect_over_timer` (media playing (`media_left: Some(80.0)`) and a running on-stage timer → MediaTimeLeft `"−1:20"`, Timer cell `Empty`; `media_left: None` → Timer shows the timer's text with `caption: None`);
  - `classic_message_wins_notes_rect` (notes "Build" and a message "Pray" → Notes cell `Empty`, StageMessage cell `Message { "Pray", seq: 1 }`; after hide → Notes `"Build"`, StageMessage `Empty`);
  - `message_fills_message_cell` (Band with message seq 4 "Wrap up" → `Message { "Wrap up", 4 }`, `message_fallback: None`);
  - `message_without_message_box_sets_fallback` (a layout without a StageMessage box → `message_fallback == state.message`);
  - `clears_never_touch_message_timer_clock_notes` (the same inputs with `on_air` = `Slide(Text)`, `ClearedSlide` and `ClearedAll`: the views differ only in the CurrentSlide cell).
- [ ] **Step 2:** Run `& $FC test --release --lib stage::view`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** Run `& $FC test --release --lib stage::`. Expected: PASS for every stage module.
- [ ] **Step 5: Commit** `feat(stage): clock, timers, media time, messages and shared boxes on stage`.

### Task 11: Cell swap and diff state machine

**Files:**
- Create: `src/stage/swap.rs`
- Modify: `src/stage/mod.rs` (`pub mod swap;`)

**Interfaces:**
- Consumes: `cells::Cell`, `draw::StageKey`.
- Produces (spec §6 "Swaps" and "Cost", §8 "Cell render fails"; times are explicit `Instant`s):
  - `pub fn changed_cells(old: &[Cell], new: &[Cell]) -> Vec<Uuid>`: element ids of cells in `new` whose `(element, content, rect, style)` differs from the cell with the same element id in `old`, or that are not in `old`, in `new`'s order.
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Shown { Nothing, Old, New }` and `pub const SWAP_DEADLINE: Duration = Duration::from_millis(300);`
  - `pub struct Swapper { … }` with:
    - `pub fn new() -> Self` (window just opened: no old frame);
    - `pub fn update(&mut self, new: &[Cell], current_slide: &[Uuid], layout_changed: bool, slide_on_air: bool, now: Instant) -> Vec<Uuid>`: the element ids to render, CurrentSlide ids first. Within a layout (`layout_changed == false` and a frame is up) only `changed_cells`; on a layout switch or the first call, every cell, and a pending whole-frame swap starts at `now`.
    - `pub fn rendered(&mut self, element: Uuid)`; `pub fn poll(&mut self, now: Instant) -> bool` (true when the visible frame changed);
    - `pub fn shown(&self, element: Uuid) -> Shown` (which image the window shows for that cell);
    - Rules: a changed cell within a layout keeps `Old` until `rendered`, then `New`. A pending switch completes when every cell is rendered, or when `SWAP_DEADLINE` has passed **and** every CurrentSlide cell is rendered (when `slide_on_air`; with nothing on air the deadline alone is enough); until then every cell shows `Old`; after it completes, late cells show `Nothing` until rendered. On open (no old frame) every cell shows `Nothing` (background only) until the CurrentSlide cells are rendered, then the ready cells show `New`.
    - `pub fn render_failed(&mut self, key: &StageKey) -> bool`: true only the first time a key fails (log once per key); the cell keeps what it showed (`shown` unchanged).

- [ ] **Step 1: Write the failing tests** (cells built by hand; `CS` = the CurrentSlide element id, `CL` = a clock cell; `t0 = Instant::now()`, `ms(n)`):
  - `only_changed_cells_rerender` (old and new views equal except the clock text → `changed_cells == [CL]`; a rect change on CS → `[CS]`; a new element → its id; identical views → `[]`; with a frame up, `update(.., layout_changed: false, ..)` returns `[CL]` for the clock change);
  - `current_slide_rendered_first` (`update` on a layout switch with cells in order [CL, CS] → `[CS, CL]`);
  - `swap_waits_for_all_cells_or_300ms` (frame up, switch at t0 with CS and CL, `slide_on_air: true`: `rendered(CS)` → `poll(t0+ms(50))` false and `shown(CS) == Old`; `poll(t0+ms(299))` false; `poll(t0+ms(300))` true, `shown(CS) == New`, `shown(CL) == Nothing`; `rendered(CL)` → `poll(t0+ms(310))` true and `shown(CL) == New`. Separately: both rendered by t0+20 ms → `poll(t0+ms(20))` true before the deadline);
  - `never_swaps_without_current_slide_while_on_air` (switch at t0 with `slide_on_air: true`, only CL rendered: `poll(t0+ms(1000))` false and `shown(CS) == Old`; `rendered(CS)` → `poll(t0+ms(1001))` true. With `slide_on_air: false` and only CL rendered: `poll(t0+ms(300))` true);
  - `open_shows_background_until_current_slide` (`Swapper::new()`, first `update`: `shown(CS) == Nothing`, `shown(CL) == Nothing`; `rendered(CL)` and `poll` → `shown(CL)` still `Nothing`; `rendered(CS)` → `poll` true, both `New`);
  - `changed_cell_keeps_old_image_until_ready` (frame up; clock change → `shown(CL) == Old` until `rendered(CL)`, then `New`; `shown(CS) == New` throughout);
  - `failed_render_logged_once_per_key` (`render_failed(&k)` → true, again → false, another key → true; `shown` of that cell unchanged).
- [ ] **Step 2:** Run `& $FC test --release --lib stage::swap`. Expected: FAIL.
- [ ] **Step 3: Implement** `swap.rs`.
- [ ] **Step 4:** Run it, then `& $FC test --release --lib stage::`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(stage): cell diff and layout-switch swap rule`.

---

## PART B — Integration after Plans 3 and 4 merge

> **BLOCKED** until the native-foundation branch has **Plan 3 and Plan 4** merged (and the FEAT-1/2/4/5/6 pieces named per task). File paths below are Plan 3's and Plan 4's layout (`src/ui/operator/*`, `src/ui/dialogs/*`, `ui/operator/*`, `ui/dialogs/*`, `src/recovery/state.rs`; `server.rs` keeps only `/remote*` and `/api/do`). Before each task, re-read the files it modifies: if Plan 3/4 moved a function, follow it, keep this plan's names and tests. Run `cargo build` and `cargo test` after every task. Each UI task lists the strings it adds to `src/strings/stage.rs`; add them there (never inline in `.slint` or glue) and pass them to Slint through Plan 3's string route.

### Task B1: Stored fields, module hand-over and sanitise on load

**Files:**
- Modify: `src/model.rs`, the `Data::after_load` site (FEAT-1), `src/stage/mod.rs`, `src/strings/mod.rs`, `src/stage/notes.rs`

**Interfaces:**
- `Settings { …, #[serde(default)] pub stage: StageSettings }`; `Data { …, #[serde(default)] pub stage_layouts: Vec<StageLayout> }` (each layout's `elements` already go through `lenient_vec`); `ServiceItem { …, #[serde(default, skip_serializing_if = "Option::is_none")] pub stage_layout: Option<Uuid>, #[serde(default, skip_serializing_if = "Vec::is_empty", deserialize_with = "crate::stage::lossless::lenient_vec")] pub slide_notes: Vec<Lenient<SlideNote>> }` (spec §4.4). No format bump.
- **Sanitise on load:** in FEAT-1's `Data::after_load(&mut self) -> bool`, add one step (a sanitise step, not a format migration; deviation 9): `changed |= geom::sanitise_settings(&mut self.settings.stage); for l in &mut self.stage_layouts { changed |= geom::sanitise_layout(l); }`. When `after_load` returns true FEAT-1's store saves once, as for its own fix-ups. Saves from `stage/layouts/replace` are sanitised by `plan_replace`.
- `src/stage/mod.rs`: the `#[path]` strings module becomes `pub use crate::strings::stage as strings;`, and `src/strings/mod.rs` gains `pub mod stage;`. `crate::lenient` and `crate::timers` imports are unchanged (already final).
- `pub type VerseKey = crate::scripture::VerseId;` (FEAT-5) replaces the stage-local struct; JSON unchanged.
- Merge-by-key (FEAT-1): `settings/save` and `service/update` keep `stage`, `stage_layout`, `slide_notes` when their keys are missing; an explicit `null` clears `stage_layout`.

- [ ] **Step 1: Failing tests:** `old_data_json_loads_with_stage_defaults` (today's fixture → Classic, six quick messages, no overrides, and the stage step reports no change), `hand_edited_stage_layout_sanitised_on_load` (data.json with a church layout holding a NaN rect `x`, colour `"blurple"`, `min_size: 30, max_size: 10`, and settings `auto_hide_mins: 7` → after load: `x` 0, colour `None`, sizes (10, 10), auto-hide 0, and the file is saved exactly once), `future_box_survives_unrelated_edit_with_same_meaning` (spec §9.1: data.json with a `hologram` box, a `{"type":"clock","blink":true}` box and an `{"on":"bar"}` note; `App::edit` renames a service; read the file back; all three parse to equal `serde_json::Value`s), `service_update_without_stage_fields_keeps_them`, `service_update_null_clears_stage_layout`, `verse_key_json_unchanged_after_alias` (`[42,3,16]`).
- [ ] **Step 2:** `cargo test -- model:: app:: stage::` → FAIL. **Step 3:** implement. **Step 4:** `cargo test` passes.
- [ ] **Step 5: Commit** `feat(stage): stage settings, church layouts, item layout and slide notes in data`.

### Task B2: Adapters from live and slide types; notes into `build_item`

**Files:**
- Create: `src/stage/adapt.rs`
- Modify: `src/slides.rs` (`build_item`), `src/stage/mod.rs`

**Interfaces:**
- `pub fn live_facts(view: &LiveView, data: &Data) -> LiveFacts` (FEAT-6: callers pass `App::display_view()`): `on_air` from `view.frame` (slide or media on air → `Slide(face of view.slide)`; else `frame.logo` → `Logo`; else a background still in the frame → `ClearedSlide`; else `ClearedAll`), `cursor_face` from `view.slide`, `section`/`section_color` from `view.slide.section` and `sections::color_for_label` (FEAT-4), `notes` from `view.slide.notes`, `next` from `view.next` (`other_item` when its title differs from the live item), `next_item_title`/`first_item_title`/`service_pos` from the current service order, `media_left` by today's rule (video, else audio, `duration > 0`, `duration − position`).
- **Rich text flattened (spec §6):** `SlideFace::Text` takes the slide's plain `content` string only; FEAT-1's `styles` spans are ignored on stage, for the current and the next slide.
- `pub fn key_facts(slide: &Slide) -> SlideKeyFacts` from `slide.section: Option<SectionRef>` and `slide.source: Option<SlideSource>`.
- `pub fn item_layout(data: &Data, item_id: Uuid) -> Option<Uuid>`: searches the current service first, then every service (spec §5.1).
- `pub fn ref_sources(data: &Data, automation: &Automation) -> (Vec<ItemUse>, Vec<MacroUse>)` (FEAT-2 `MacroStep::StageLayout { layout }` steps).
- `slides::build_item`: after building, `notes::resolve(&item.slide_notes, &facts)` writes each `Some` into `Slide.notes`; today's item-notes fill (`slides.rs:234`) then fills only slides still empty.

- [ ] **Step 1: Failing tests:** port every `stage_text_*` test from `src/ui/stage.rs` to `adapt.rs` as `live_facts` + `view::build(Classic)` assertions (same values); `notes_prefer_slide_note_over_item_note`; `build_item_puts_slide_note_into_slide_notes`; `override_survives_switching_current_service`; `key_facts_from_section_ref_and_source`; `rich_text_flattened_to_plain_on_stage` (a text slide with bold and colour spans → the CurrentSlide cell text equals the plain content, and its `StageKey` equals that of the same slide without spans).
- [ ] **Step 2–4:** FAIL → implement → `cargo test` passes.
- [ ] **Step 5: Commit** `feat(stage): adapters from live view, data and slides to stage facts`.

### Task B3: App state, stage actions and updates (Phase A)

**Files:**
- Modify: `src/app.rs`

**Interfaces:**
- `App.stage: Mutex<StageState>`, `App.stage_limit: Mutex<RateLimit>` (1 s), `App.stage_missing_reported: Mutex<Option<Uuid>>`; `Update::Stage`.
- Actions (spec §5.5, all through `do_action`): `stage/layout/use {id}`, `stage/layout/auto {}`, `stage/layout/default {id}` (through `App::edit`), `stage/message/show {text} → {seq}`, `stage/message/hide {if_seq?}`, `stage/quick/save {messages}`, `stage/settings {flash, auto_hide_mins}`. Data-changing ones publish `Update::Data` + `Update::Stage`; runtime ones `Update::Stage`.
- `pub fn stage_message_show(&self, text: &str, from: MsgSource) -> Result<u64, String>` (spec §5.5); `pub fn stage_view(&self) -> StageView` (builds from `display_view()`, `timers()`, `Local::now().time()`, through `active_layout_checked`); `pub fn take_stage_fallback_notice(&self) -> Option<String>`: when `active_layout_checked` reports a missing id that differs from `stage_missing_reported`, returns `layout_deleted(<fallback layout name>)` once and records the id; a later build with nothing missing resets the record.
- Hooks: on every `Update::Live` and `Update::Practice` publish, `state.live_item_changed(display_view().cursor item)`; where FEAT-2 publishes `Update::Timers`, `latch::track_timers` runs first; a `current_service` change calls `service_switched()` in the same call; when a layout on stage disappears the next build falls back (§5.9) and the operator shows the notice (B7a).
- `pub mod remote { pub use crate::stage::rate_limit::RateLimit; }` wherever Plan 4 put the remote code, so FEAT-2 can share it.

- [ ] **Step 1: Failing tests** (spec §9.1 app [A]): `stage_layout_use_is_for_now_and_default_is_separate`, `stage_layout_use_unknown_is_error`, `stage_message_show_trims_limits_and_bumps_seq`, `stage_message_hide_is_idempotent_and_if_seq_guards`, `service_switch_hides_stage_message`, `stage_actions_do_not_change_output_version` (every stage action; `live_view().version` unchanged), `stage_message_show_from_macro_records_source`, `macro_with_deleted_layout_refuses_to_run` (FEAT-2 resolver; notice `Couldn't run “<macro>”: step N's stage layout was deleted. Edit the macro.`), `stage_follows_practice_engine_via_display_view`, `manual_layout_ends_on_practice_item_change`, `stage_settings_rejects_bad_minutes`, `deleted_active_layout_toasts_fallback` (for-now layout "Ours" on stage; `stage/layouts/replace` deletes it → `stage_view().layout_id == CLASSIC`, `take_stage_fallback_notice() == Some("The stage layout was deleted; showing Classic.")`, a second call → `None`; with the default set to Band → "…showing Band.").
- [ ] **Step 2–4:** FAIL → implement → `cargo test` passes.
- [ ] **Step 5: Commit** `feat(stage): stage actions, state and updates in the app core`.

### Task B4: Notes and item layouts in the core (Phase B)

**Files:**
- Modify: `src/app.rs`

**Interfaces:**
- `stage/note {item_id, key, text}` → `notes::set_note` through `App::edit` (`ITEM_GONE`, `NOTE_TOO_LONG`); `service/update` accepts `stage_layout` (unknown id → `LAYOUT_GONE`). FEAT-4's notes-only adoption carries note edits into the live snapshot with `Change::None`.

- [ ] **Step 1: Failing tests:** `stage_note_set_and_clear`, `service_update_rejects_unknown_stage_layout`, `live_note_edit_reaches_stage_via_snapshot_after_unretaken_edit` (spec §9.1 [B], output version unchanged), `stage_window_switches_layout_on_item_override` (core half: `stage_view().layout_id`).
- [ ] **Step 2–5:** FAIL → implement → `cargo test` → commit `feat(stage): slide notes and per-item stage layouts in the core`.

### Task B5: Layout replace and references in the core (Phase C)

**Files:**
- Modify: `src/app.rs`; FEAT-2's macro resolver (add the `LayoutRef::MacroStep` source)

**Interfaces:**
- `stage/layout/refs {id} → {refs}` (`refs::layout_refs`); `stage/layouts/replace {layouts, deleted} → {cleared}` in one `App::edit`: `plan_replace`, then `clear_deleted` over the default and every item, then store. Any error → nothing changes.

- [ ] **Step 1: Failing tests:** `layouts_replace_is_all_or_nothing`, `rename_into_deleted_name_succeeds`, `layouts_replace_clears_item_and_default_refs`, `layouts_replace_rejects_builtin_empty_duplicate_and_too_many`, `deleting_layout_reports_macro_uses` (through the action).
- [ ] **Step 2–5:** FAIL → implement → `cargo test` → commit `feat(stage): replace stage layouts as one action and report references`.

### Task B6: Stage window on the cell model

**Files:**
- Modify: `ui/stage.slint` (rewritten), `src/ui/stage.rs`, the renderer cache file (FEAT-1 `CacheKey`)

**Interfaces:**
- **`StageKey` hand-over:** FEAT-1's placeholder unit struct `StageKey` in the cache file is deleted and replaced by `pub use crate::stage::draw::StageKey;`, so `CacheKey::Stage(StageKey)` (FEAT-1 §6.4) carries the real key. Cells render off the UI thread through the shared `Renderer` LRU.
- `ui/stage.slint`: `in property <color> bg; in property <[StageCellVm]> cells; in property <string> banner; in property <int> banner-seq; in property <bool> connected; in property <bool> practice;`, `StageCellVm { x, y, w, h: float, image: image, flash-seq: int, is-message: bool }` placed in %; banner; "Not connected" dot; PRACTICE tag top-right while `practice`; cursor hidden; 4 px accent border pulsing 3× over 1.5 s on a new `flash-seq` when `StageSettings.flash`.
- `src/ui/stage.rs`: `stage_text`/`fit_size` removed; `follow` listens to `Update::{Live, Practice, Data, Stage, Timers}`, keeps the newest of each, rebuilds `App::stage_view()` on the event loop (one queued redraw); the 1 s clock timer rebuilds only clock cells. **All swap and diff decisions go through `stage::swap::Swapper`** (Task 11): `update` gives the cells to render (CurrentSlide first), finished renders call `rendered`, a 16 ms poll while a switch is pending calls `poll`, and each `StageCellVm.image` is chosen by `shown`. Resize re-renders all cells (debounced 100 ms) as a `layout_changed` update. **A failed cell render** keeps the cell's previous image; when `swap.render_failed(&key)` returns true, log one warning with the key through the crate's existing logger.
- Strings added to `src/strings/stage.rs`: `PRACTICE = "PRACTICE"`, `NOT_CONNECTED = "Not connected"`.

- [ ] **Step 1: Failing tests:** `stage_window_places_cells_from_layout`, `layout_switch_never_shows_empty_current_slide` (mock time), `stage_window_shows_practice_tag_in_practice_mode`, `stage_cells_cached_under_cache_key_stage` (the LRU holds `CacheKey::Stage(stage::draw::StageKey { .. })` entries after one frame), `failed_cell_keeps_previous_image_logged_once` (an injected renderer failure for the clock cell: the window keeps the previous clock image, and after three failures the log has exactly one line for that key), the ported `now_text_element_shows_the_live_lyric`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test` passes. Manual: spec §9.3 checks **1** (upgrade with an existing data.json: side-by-side photo with the old build, including Clear Slide and a PowerPoint slide), **2** (readability from 5 m on a 32" TV for each of the five presets) and **3** (old laptop, software renderer: CPU under 5 % with clock + running timer + a lyric change every 5 s; a layout switch shows no blank lyrics).
- [ ] **Step 5: Commit** `feat(stage): stage window draws stage layouts box by box`.

### Task B7a: Operator Stage strip, popover, chip and toasts (Phase A)

**Files:**
- Create: `ui/operator/stage-strip.slint`, `src/ui/operator/stage_strip.rs`
- Modify: `ui/operator/live.slint` (strip below FEAT-2's automation strip, directly above "Up next"), `ui/theme.slint` (`stage-chip-bg: #f5c66b26`)

**Interfaces:**
- One 32 px row: layout button "Stage: Classic ▾" / "Stage: Speaker (for now)" with a menu (thumbnails via `draw::render_view` at 120 px, cached per layout; check on the active one; "Set “X” as the stage default…" → Plan 3 confirm `set_default_question`; "Back to automatic" only while for-now; "Edit stage layouts…" hidden until B10a), `stage-msg` `LineEdit` (stops at 120, counter turns amber), ▾ quick messages + "Edit quick messages…" (opens B7c's dialog; until B7c lands the item is hidden), `stage-show-btn`, `stage-hide-btn`, `stage-chip` (`state::chip_text`, 1 s timer that also sends `stage/message/hide {if_seq}` when `auto_hide_due`). At ≤ 1250 px: a single `stage-strip-btn` "Stage…" (with the chip's dot) opening a popover with the same controls. Toasts: `for_now`, `STAGE_OFF_SHOW` when the stage window is off, and `App::take_stage_fallback_notice()` checked after every `Update::Stage`/`Update::Data`.
- A message from a phone or macro never touches the `stage-msg` text (spec §5.9); only the chip changes.
- Strings added to `src/strings/stage.rs`: `fn strip_layout(name) -> "Stage: {name}"`, `FOR_NOW = "(for now)"`, `fn set_default_menu(name) -> "Set “{name}” as the stage default…"`, `BACK_TO_AUTOMATIC = "Back to automatic"`, `EDIT_LAYOUTS = "Edit stage layouts…"`, `EDIT_QUICK = "Edit quick messages…"`, `SHOW = "Show"`, `HIDE = "Hide"`, `STAGE_BUTTON = "Stage…"`, `MESSAGE_PLACEHOLDER = "Message to the stage"`.

- [ ] **Step 1: Failing tests (UI):** `strip_pick_is_for_now_until_next_item`, `strip_set_default_asks_then_sets`, `strip_show_message_reaches_stage`, `strip_hide_removes_message`, `strip_is_one_row_and_collapses_to_stage_button_at_1250`, `chip_shows_elapsed_minutes`, `auto_hide_after_setting`, `quick_message_click_shows_it`, `stage_off_toast_when_showing_without_window`, `layout_deleted_toast_shown_once`, `phone_message_keeps_typed_text_and_names_phone` (type "Pray for" in `stage-msg`; `App::stage_message_show("Last chorus", MsgSource::Phone("Jake's phone".into()))` → `stage-msg` still reads "Pray for", the chip reads "On stage · just now · from Jake's phone").
- [ ] **Step 2–4:** FAIL → implement → `cargo test` passes.
- [ ] **Step 5: Commit** `feat(ui): operator Stage strip with messages and layout picker`.

### Task B7b: Stage keys, OS-reserved check and Help (Phase A)

**Files:**
- Modify: `src/ui/operator/keys.rs`, the Help rows (Plan 3's Help dialog source)

**Interfaces:**
- Keys (FEAT-1 table rows): Ctrl/Cmd+J focuses the message box (global, not over dialogs; opens the popover when narrow); Ctrl/Cmd+Shift+J hides (capture set; consumed only while a message is up); `const OS_RESERVED: &[KeyCombo]` (macOS Cmd+M/H/Q/W/Tab/Space/`,`; Windows Alt+F4, Alt+Tab, Win+any; Linux Super+any), against which every binding in the table is checked.
- Strings added to `src/strings/stage.rs`: `HELP_FOCUS_MESSAGE = "Type a stage message"`, `HELP_HIDE_MESSAGE = "Hide the stage message"`.

- [ ] **Step 1: Failing tests:** keys `stage_shortcuts_avoid_os_reserved`, `ctrl_shift_j_is_in_capture_set`, `help_lists_every_bound_key`; UI `ctrl_j_focuses_message_box` (also opens the popover at 1200 px wide), `ctrl_shift_j_hides_only_when_message_up` (with no message the key is not consumed and reaches the focused field), `f1_in_message_box_clears_and_types_nothing`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test` passes.
- [ ] **Step 5: Commit** `feat(ui): stage message shortcuts and OS-reserved key check`.

### Task B7c: Settings Stage tab, Screens default picker, quick-messages editor (Phase A)

**Files:**
- Create: `ui/dialogs/quick-messages.slint`, `src/ui/dialogs/quick_messages.rs`
- Modify: `ui/dialogs/settings.slint` + `src/ui/dialogs/settings.rs` (**Stage** tab), `ui/dialogs/screens.slint` + `src/ui/dialogs/screens.rs` (stage card "Stage layout: Classic" with a default picker), `src/ui/operator/stage_strip.rs` (show "Edit quick messages…")

**Interfaces:**
- Settings → Stage: "Flash new messages", "Hide stage messages after" (Off / 1 / 5 / 10 minutes) → `stage/settings` only (merge by key).
- Screens stage card: label `stage_layout_label(name)` and a `ComboBox` over `presets::all_layouts` → `stage/layout/default` (no confirm here; this is the settings place).
- Quick messages dialog: a list with Add, remove, drag to reorder; each row a `LineEdit` capped at 120; Add disabled at 12 with `QUICK_MAX` shown; Save → `stage/quick/save` (the core normalises); Cancel discards.
- Strings added to `src/strings/stage.rs`: `TAB_STAGE = "Stage"`, `FLASH_NEW = "Flash new messages"`, `HIDE_AFTER = "Hide stage messages after"`, `OFF = "Off"`, `fn stage_layout_label(name) -> "Stage layout: {name}"`, `QUICK_TITLE = "Quick messages"`, `QUICK_ADD = "Add"`, `QUICK_MAX = "You can have at most 12 quick messages."`.

- [ ] **Step 1: Failing tests (UI):** `settings_stage_tab_sends_only_stage_fields` (toggle flash → exactly one `stage/settings` with `flash` and `auto_hide_mins`, no other settings keys), `screens_stage_card_sets_default` (pick Band → `settings.stage.layout == BAND`, `stage.manual == None`), `quick_messages_editor_reorders_and_caps_at_12` (open with the six defaults, add six → Add disabled and `QUICK_MAX` visible; drag row 7 to the top; Save → `settings.stage.quick_messages[0]` is the dragged text and the length is 12; reopen, delete one, Cancel → still 12).
- [ ] **Step 2–4:** FAIL → implement → `cargo test` passes.
- [ ] **Step 5: Commit** `feat(ui): Stage settings tab, stage default picker and quick messages editor`.

### Task B8: Phone remote Stage tab (Phase A)

**Files:**
- Modify: `src/server.rs` (or wherever Plan 4 put the phone stream), `web/remote.html`, `web/remote.js` (strings in its `STR` object)

**Interfaces:**
- `remote_view` gains `"stage": { "message": {seq,text,from}|null, "quick": [..], "notes": "..", "clock_h24": bool, "timer_id": uuid|null, "timer": {name,text,over}|null, "media_left": ".." }` (`timer_id` = the stage's current timer from `latch::current_timer`; `timer` its value when `remote_view` was built); the phone stream resends `remote_view` on `Update::Stage`.
- **The timer line stays live.** `remote_view` is not resent on `Update::Timers`, so where the phone stream forwards FEAT-2's `{type:'timers'}` message it adds one key after calling FEAT-2's `timers_message(view)` (that function and its exact-JSON test stay unchanged): `"stage_timer": {"id","name","text","over"}|null`, computed from `latch::current_timer(&app.stage.lock(), view)` after `track_timers` ran (B3). This covers a current timer that is not `on_remote`, which FEAT-2's `timers` list leaves out. `remote.js` updates the Stage tab's timer line from `stage_timer` on every timers message, and from `stage.timer` on a full `remote_view`.
- `POST /remote/stage` `{id, op: "show", text}` / `{id, op: "hide"}`: session/origin/JSON checks as `/remote/command`, then `state::phone_gate`, then `App::stage_message_show(text, MsgSource::Phone(name))` or hide, then `remember`. Other ops → `NOT_FROM_PHONE`. This is FEAT-3's row in FEAT-2 §5.11's allowlist; nothing is added to `/remote/command`.
- "Stage" tab beside "Control": message box, quick buttons, Show, Hide, "On stage: … (from Jake's phone)", the current timer.

- [ ] **Step 1: Failing tests:** `remote_stage_show_requires_pairing_and_same_origin`, `remote_stage_duplicate_id_does_not_reflash`, `retry_within_rate_window_returns_duplicate_not_too_fast`, `hide_never_rate_limited`, `remote_stage_rate_limited_per_phone`, `remote_stage_rejects_other_ops`, `remote_command_with_stage_body_is_bad_command`, `remote_view_includes_stage_block` (includes `timer_id`), `phone_stage_timer_updates_every_second` (a running 5:00 on-stage countdown with `on_remote: false`; two ticks one second apart → two `{type:'timers'}` stream messages whose `timers` list is empty and whose `stage_timer.text` is `"4:59"` then `"4:58"`; with no current timer → `"stage_timer": null`).
- [ ] **Step 2–4:** FAIL → implement → `cargo test` passes. Manual: spec §9.3 check 4 (iPhone and Android).
- [ ] **Step 5: Commit** `feat(remote): Stage tab and stage messages from paired phones`.

### Task B9: Item edit layout, slide notes UI (Phase B)

**Files:**
- Modify: `ui/dialogs/item-edit.slint`, `src/ui/dialogs/item_edit.rs` (Stage section: "Stage layout" `ComboBox` "Use the stage default" + every layout; "Notes for slides that changed" with Delete), `ui/operator/slides.slint`, `src/ui/operator/slides.rs` (note mark; context item "Stage note…" → Plan 3 prompt "Note for this slide (stage only)" → `stage/note` with `notes::key_at`), `ui/operator/service.slint` (stage icon, tooltip "Stage layout: Speaker"), `src/ui/operator/keys.rs` (Alt+N, presentation scope)

**Interfaces:**
- Strings added to `src/strings/stage.rs`: `USE_STAGE_DEFAULT = "Use the stage default"`, `NOTES_CHANGED = "Notes for slides that changed"`, `STAGE_NOTE_MENU = "Stage note…"`, `NOTE_PROMPT = "Note for this slide (stage only)"`, `DELETE = "Delete"`, `HELP_STAGE_NOTE = "Stage note for the selected slide"`; the tooltip reuses `stage_layout_label` (B7c).

- [ ] **Step 1: Failing tests:** `stage_window_switches_layout_on_item_override`, `item_edit_stage_layout_round_trips`, `slide_note_prompt_saves_and_marks_thumbnail`, `alt_n_opens_note_prompt_only_outside_text_fields`, `orphaned_notes_listed_and_deletable`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test` passes. Manual: spec §9.3 check 6.
- [ ] **Step 5: Commit** `feat(ui): per-item stage layout and slide stage notes`.

### Task B10a: Stage layout editor — list pane and footer (Phase C)

**Files:**
- Create: `ui/dialogs/stage-layouts.slint` (shell, list, footer), `src/ui/dialogs/stage_layouts.rs` (glue)
- Modify: `ui/dialogs/screens.slint`/`screens.rs` ("Stage layouts…"), `src/ui/operator/stage_strip.rs` (show "Edit stage layouts…")

**Interfaces:**
- Glue holds an edited clone of the church layouts, deleted ids and `UndoStack::new(50)`; it is not rebuilt by `dialog_refresh`.
- List: built-ins with a lock and `BUILTIN_HINT`; **New** (name `NEW_LAYOUT_NAME`, made unique with " 2", " 3"…; no boxes; selected), **Copy** (`copy_name(name)` made unique the same way, a new layout id and new element ids), **Rename** (Plan 3 prompt; name errors appear on Save, per spec), **Delete** (confirm text `refs::delete_question` from `stage/layout/refs`; disabled for built-ins; adds to the deleted ids), **Use on stage now** (sends `stage/layout/use` for the selected layout; disabled for an unsaved layout, tooltip `SAVE_FIRST`).
- Footer: Cancel asks `DISCARD_QUESTION` only when the edited set differs from the stored one; Save = one `stage/layouts/replace`, errors shown under the footer, the dialog stays open with the changes.
- Strings added to `src/strings/stage.rs`: `EDITOR_TITLE = "Stage layouts"`, `BUILTIN_HINT = "Built-in — make a copy to change it"`, `NEW = "New"`, `COPY = "Copy"`, `RENAME = "Rename"`, `USE_NOW = "Use on stage now"`, `SAVE_FIRST = "Save first"`, `NEW_LAYOUT_NAME = "New stage layout"`, `fn copy_name(name) -> "{name} copy"`, `DISCARD_QUESTION = "Discard your changes to the stage layouts?"`, `CANCEL = "Cancel"`, `SAVE = "Save"`.

- [ ] **Step 1: Failing tests (UI):** `editor_new_copy_rename_delete` (New → "New stage layout"; Copy of Classic → "Classic copy" with fresh layout and element ids; Rename it to "Ours"; Delete of the unsaved New layout asks `Delete “New stage layout”?`; Save → one replace whose set is exactly ["Ours"]), `editor_use_on_stage_now_is_for_now` (saved "Ours" → `stage.manual.layout == Ours`), `editor_cancel_discards`, `editor_cancel_asks_only_when_changed`, `editor_save_updates_stage_live`, `editor_save_error_keeps_dialog_and_changes`, `editor_builtin_is_read_only_copy_works`, `editor_delete_in_use_counts_all_refs`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test` passes.
- [ ] **Step 5: Commit** `feat(ui): stage layout editor list and save`.

### Task B10b: Stage layout editor — canvas and box settings (Phase C)

**Files:**
- Create: `ui/dialogs/stage-canvas.slint` (boxes, handles, guides; pointer → callbacks in %), `ui/dialogs/stage-props.slint`
- Modify: `src/ui/dialogs/stage_layouts.rs`, `ui/dialogs/stage-layouts.slint` (mount the canvas and the properties)

**Interfaces:**
- Canvas: the stage monitor's aspect (or 16:9), showing `view::build` over the live `display_view()` facts, or `view::sample_facts()` when nothing is live; refreshes on `Update::Live`. Every box shows its kind name (`KIND_NAMES`) in a corner tab. Click selects (accent outline, eight handles). Pointer events arrive in % and go through `geom::{snap, snap_to_guides, move_by, resize, hit_test}`; **Alt held = no snapping** (grid and guides off). Each edit pushes the previous set on the `UndoStack`.
- Keys while the canvas has focus: arrows / Shift+arrows → `geom::nudge`; Delete/Backspace removes the selected box (`Unknown` included); Ctrl/Cmd+Z and Ctrl/Cmd+Y (and Ctrl/Cmd+Shift+Z) undo/redo; Ctrl/Cmd+D → `geom::duplicate_box` (selects the copy); Tab / Shift+Tab → `geom::next_box`.
- Right-click on a box: Bring to front (`bring_to_front`), Send to back (`send_to_back`), Duplicate, Delete; for an `Unknown` box only Delete is enabled.
- Properties (spec §3.6 item 5): what it shows, text colour, biggest text size, alignment, caption on/off and its words, background, per-kind options (which timer, 24-hour clock, seconds, show pictures, keep the words after Clear). Add ▾ uses `KIND_NAMES`/`KIND_HELP` and `new_box_rect`, selecting the new box. The layout's font field shows `font_missing(name)` when `Fonts::has_family` is false (reuse FEAT-1's Theme-editor string if it exists with the same meaning; otherwise this one). "Text may not fit" (`TEXT_MAY_NOT_FIT`) on a box when `draw::text_fits` is false.
- `Unknown` boxes drawn grey hatched with `NEEDS_NEWER`: selectable, deletable, never moved or changed.
- Strings added to `src/strings/stage.rs`: `ADD = "Add"`, `BRING_TO_FRONT = "Bring to front"`, `SEND_TO_BACK = "Send to back"`, `DUPLICATE = "Duplicate"`, `fn font_missing(name) -> "“{name}” isn't installed on this computer. The stage will use Noto Sans."`, `PROP_SHOWS = "Shows"`, `PROP_COLOR = "Text colour"`, `PROP_MAX = "Biggest text"`, `PROP_ALIGN = "Alignment"`, `PROP_CAPTION = "Caption"`, `PROP_FILL = "Background"`, `PROP_TIMER = "Timer"`, `CURRENT_TIMER = "Current timer"`, `PROP_H24 = "24-hour clock"`, `PROP_SECONDS = "Seconds"`, `PROP_PICTURES = "Show pictures"`, `PROP_KEEP = "Keep the words after Clear"`.

- [ ] **Step 1: Failing tests (UI):** `editor_drag_moves_box_snapped`, `editor_alt_drag_moves_freely` (Alt-drag by 0.7 % → x moves 0.7, no guide shown), `editor_resize_handle_respects_minimum`, `editor_ctrl_d_duplicates` (copy at +2.5/+2.5 and selected; Ctrl+Z removes it), `editor_context_menu_z_order` (Bring to front / Send to back change the element order; on an `Unknown` box only Delete is enabled), `editor_tab_selects_next_box`, `editor_delete_key_removes_box`, `editor_unknown_box_locked_but_deletable`, `editor_shows_sample_words_when_nothing_live`, `editor_box_name_tabs` (each box's tab text equals its kind name), `editor_shows_font_not_installed_note`, `editor_text_may_not_fit_mark`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test` passes. Manual: spec §9.3 check 7 (mouse and trackpad: drag, resize, guides, undo).
- [ ] **Step 5: Commit** `feat(ui): stage layout editor canvas and box settings`.

### Task B11: Tablet stage view (Phase C)

**Files:**
- Modify: `src/app.rs` (`Phone` gains `#[serde(default)] role: PhoneRole { #[default] Remote, StageView }`), `src/server.rs` (`/remote/stage-view`; a StageView session is accepted only there and on `/remote/events`; 403 on `/remote/command`, `/remote/stage`, `/remote`), Screens stage card ("Pair a tablet as a stage screen…")
- Create: `web/stage-view.html` (one fixed layout: clock and timer top, current slide with the 0.4 vh step fit, notes, next, message banner; no links or gestures to controls)

**Interfaces:**
- Strings added to `src/strings/stage.rs`: `PAIR_TABLET = "Pair a tablet as a stage screen…"`.

- [ ] **Step 1: Failing tests:** `stage_view_session_cannot_send_commands`, `old_phones_json_loads_as_remote_role`, `stage_view_pairing_counts_against_five_devices`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test` passes. Manual: spec §9.3 check 7 (tablet half).
- [ ] **Step 5: Commit** `feat(remote): view-only tablet stage screen`.

### Task B12: Crash restore of stage state (Plan 4 recovery)

**Files:**
- Modify: `src/recovery/state.rs` (`LiveState` gains `#[serde(default)] stage: Option<StageCrashState { message: Option<StageMessage>, manual: Option<ManualLayout> }>`), `src/app.rs` (write atomically on every message show/hide and for-now change; read only on `--restore`; the current timer is recomputed from FEAT-2's restored timers), `docs/superpowers/checks/2026-native-checks.md`

- [ ] **Step 1: Failing tests** (spec §9.1, gated on Plan 4): `stage_state_restored_from_live_state`, `stage_state_written_on_message_show_hide`; remove their `#[ignore]`.
- [ ] **Step 2–4:** FAIL → implement → `cargo test` passes. Manual: spec §9.3 checks 5 and 8.
- [ ] **Step 5:** Record the §9.3 results in the checks doc. Commit `feat(stage): stage message and for-now layout survive a crash`.
